//! Turns source files into rows: detect changed units, parse them in
//! parallel, write them in one transaction, then run the linkers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Result;
use rayon::prelude::*;
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::model::{ParsedSession, Source};
use crate::sources::{self, Root, Unit};
use crate::util::{now_ms, path_within, truncate};
use crate::{db, git, signals};

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestReport {
    pub scanned: usize,
    pub updated: usize,
    pub failed: usize,
    pub errors: Vec<String>,
    pub duration_ms: u64,
    /// Whether any row changed, so the UI can skip refetching after an idle scan.
    pub changed: bool,
}

/// Sessions not touched by the watcher for this long drop their parse
/// state; the next change reads them whole once.
const RESUME_TTL: Duration = Duration::from_secs(10 * 60);

pub struct Ingestor {
    pub data_dir: PathBuf,
    /// Parse state for sessions the watcher saw change, so the next change
    /// reads only the lines appended since. Keyed by unit.
    resume: Mutex<HashMap<String, (Instant, sources::Resume)>>,
}

impl Ingestor {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            resume: Mutex::new(HashMap::new()),
        }
    }

    /// Where the retro runs `claude -p`; its own sessions are not tracked.
    pub fn retro_workdir(&self) -> PathBuf {
        self.data_dir.join("retro-runs")
    }

    /// Retro runs from the current data folder and the ones used before each
    /// rename, whose transcripts still carry the old path.
    fn retro_workdirs(&self) -> Vec<String> {
        std::iter::once(self.retro_workdir())
            .chain(
                crate::LEGACY_IDENTIFIERS
                    .iter()
                    .map(|id| self.data_dir.with_file_name(id).join("retro-runs")),
            )
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    pub fn enabled_roots(&self, conn: &Connection) -> Vec<Root> {
        sources::roots()
            .into_iter()
            .filter(|r| db::setting_bool(conn, &source_setting(r.source), true))
            .collect()
    }

    pub fn ingest_all(&self, conn: &mut Connection, force: bool) -> Result<IngestReport> {
        let writes_before = conn.total_changes();
        let units: Vec<Unit> = self
            .enabled_roots(conn)
            .iter()
            .flat_map(sources::discover)
            .collect();
        let (mut report, _) = self.ingest_units(conn, &units, force, false)?;
        // Sessions parsed before the current signals existed, whose original
        // transcript has since been deleted, are re-read from the archive.
        let stale: Vec<(String, String)> = {
            let mut stmt =
                conn.prepare("SELECT source, file_path FROM sessions WHERE signals_version < ?1")?;
            stmt.query_map([SIGNALS_VERSION], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        let from_archive: Vec<Unit> = stale
            .iter()
            .filter(|(_, path)| !Path::new(path).exists())
            .filter_map(|(source, path)| {
                let source = Source::from_id(source)?;
                archived_unit(&self.data_dir, source, Path::new(path))
            })
            .collect();
        if !from_archive.is_empty() {
            let (r, _) = self.ingest_units(conn, &from_archive, false, false)?;
            report.updated += r.updated;
            report.errors.extend(r.errors);
        }
        // A full pass re-links every session so new commits, instruction-file
        // edits and intents reach sessions whose transcripts did not change.
        self.postprocess(conn, None)?;
        // Archiving happens here rather than on every file event: copying a
        // large transcript each time an agent appends a line is wasteful, and
        // a few minutes' delay is harmless against a 30-day cleanup.
        if db::setting_bool(conn, "archive.enabled", true) {
            for unit in &units {
                if let Err(e) = archive_unit(&self.data_dir, unit) {
                    report.errors.push(format!("archive {}: {e}", unit.key()));
                }
            }
        }
        report.errors.truncate(50);
        report.changed = conn.total_changes() > writes_before;
        Ok(report)
    }

    pub fn ingest_paths(
        &self,
        conn: &mut Connection,
        changed_paths: &[PathBuf],
    ) -> Result<IngestReport> {
        let roots = self.enabled_roots(conn);
        // FSEvents reports resolved paths, so a symlinked root (say ~/.claude)
        // must also be matched in its canonical form.
        let canonical: Vec<Option<PathBuf>> = roots
            .iter()
            .map(|r| std::fs::canonicalize(&r.path).ok())
            .collect();
        let mut by_key: HashMap<String, Unit> = HashMap::new();
        for p in changed_paths {
            for (root, canon) in roots.iter().zip(&canonical) {
                let path = match canon.as_ref().and_then(|c| p.strip_prefix(c).ok()) {
                    Some(rel) => root.path.join(rel),
                    None if p.starts_with(&root.path) => p.clone(),
                    None => continue,
                };
                if let Some(u) = sources::unit_for_path(root, &path) {
                    by_key.insert(u.key(), u);
                }
            }
        }
        let units: Vec<Unit> = by_key.into_values().collect();
        let (report, changed) = self.ingest_units(conn, &units, false, true)?;
        if !changed.is_empty() {
            self.postprocess(conn, Some(&changed))?;
        }
        Ok(report)
    }

    /// `keep` saves each parse's state for the next pass: worth it for the
    /// few sessions the watcher sees changing, not for a full scan.
    fn ingest_units(
        &self,
        conn: &mut Connection,
        units: &[Unit],
        force: bool,
        keep: bool,
    ) -> Result<(IngestReport, Vec<String>)> {
        let started = Instant::now();
        let known = |key: &str| -> Option<String> {
            conn.prepare_cached("SELECT fingerprint FROM source_files WHERE unit_key = ?1")
                .ok()?
                .query_row([key], |r| r.get(0))
                .ok()
        };
        let mut report = IngestReport {
            scanned: units.len(),
            ..Default::default()
        };

        let todo: Vec<(&Unit, String)> = units
            .iter()
            .filter_map(|u| {
                let fp = u.fingerprint();
                (force || known(&u.key()).as_deref() != Some(fp.as_str())).then_some((u, fp))
            })
            .collect();

        let todo: Vec<_> = {
            let mut saved = self.resume.lock().expect("resume lock");
            saved.retain(|_, (used, _)| used.elapsed() < RESUME_TTL);
            todo.into_iter()
                .map(|(u, fp)| {
                    let resume = if force {
                        None
                    } else {
                        saved.remove(&u.key()).map(|x| x.1)
                    };
                    (u, fp, resume)
                })
                .collect()
        };
        let repo_cache: Mutex<HashMap<String, Option<String>>> = Mutex::new(HashMap::new());
        let parsed: Vec<_> = todo
            .into_par_iter()
            .map(|(unit, fp, resume)| {
                let had_state = resume.is_some();
                let result = unit.source.parse(&unit.paths, resume).map(|(opt, next)| {
                    if let Some(next) = next
                        && (keep || had_state)
                    {
                        self.resume
                            .lock()
                            .expect("resume lock")
                            .insert(unit.key(), (Instant::now(), next));
                    }
                    opt.map(|mut s| {
                        s.close_open_turn();
                        let repo = s
                            .project_path
                            .as_deref()
                            .and_then(|p| cached_repo_root(&repo_cache, p));
                        (s, repo)
                    })
                });
                (unit, fp, result)
            })
            .collect();

        let retro_dirs = self.retro_workdirs();
        let mut changed = vec![];
        let tx = conn.transaction()?;
        for (unit, fp, result) in parsed {
            let session_id = match result {
                Ok(Some((s, _)))
                    if s.project_path
                        .as_deref()
                        .is_some_and(|p| retro_dirs.iter().any(|d| path_within(p, d))) =>
                {
                    None
                }
                Ok(Some((s, repo))) => {
                    let id = upsert_session(&tx, unit.source, unit, &s, repo.as_deref())?;
                    changed.push(id.clone());
                    report.updated += 1;
                    Some(id)
                }
                Ok(None) => None,
                Err(e) => {
                    report.failed += 1;
                    report.errors.push(format!("{}: {e:#}", unit.key()));
                    // Leave the fingerprint unset so it is retried next pass.
                    continue;
                }
            };
            tx.execute(
                "INSERT INTO source_files (unit_key, fingerprint, session_id, ingested_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(unit_key) DO UPDATE SET
                   fingerprint = excluded.fingerprint,
                   session_id = excluded.session_id,
                   ingested_at = excluded.ingested_at",
                params![unit.key(), fp, session_id, now_ms()],
            )?;
        }
        tx.commit()?;
        report.errors.truncate(50);
        report.duration_ms = started.elapsed().as_millis() as u64;
        Ok((report, changed))
    }

    /// Runs the linkers (see `enrich`).
    pub fn postprocess(&self, conn: &Connection, changed: Option<&[String]>) -> Result<()> {
        crate::enrich::run_all(conn, changed)
    }
}

/// Bump when the parsers start recording new per-session signals. It is part
/// of every unit's fingerprint, so a bump re-parses every transcript; rows
/// with a lower version stay out of signal-based statistics until they are.
pub const SIGNALS_VERSION: i64 = 13;

pub fn source_setting(source: Source) -> String {
    format!("source.{}.enabled", source.as_str())
}

fn cached_repo_root(
    cache: &Mutex<HashMap<String, Option<String>>>,
    project: &str,
) -> Option<String> {
    if let Some(hit) = cache.lock().expect("repo cache").get(project) {
        return hit.clone();
    }
    let root = git::repo_root(project);
    cache
        .lock()
        .expect("repo cache")
        .insert(project.to_string(), root.clone());
    root
}

fn upsert_session(
    conn: &Connection,
    source: Source,
    unit: &Unit,
    s: &ParsedSession,
    repo: Option<&str>,
) -> Result<String> {
    let id = format!("{}:{}", source.as_str(), s.external_id);
    let d = s.derive();
    let first_prompt = s.prompts.first().map(|p| truncate(&p.text, 1000));
    let wait_total: i64 = s.wait_samples.iter().sum();
    conn.execute(
        "INSERT INTO sessions (
            id, source, external_id, file_path, project_path, repo_root, git_branch, title,
            first_prompt, entrypoint, started_at, ended_at, active_ms, model, user_turns,
            assistant_turns, tool_calls, tool_errors, interruptions, permission_requests,
            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, cost_usd,
            cost_is_estimate, premium_requests, lines_added, lines_removed, last_event,
            wait_ms_total, wait_count, ingested_at, tool_counts, reads, edits, shell_calls,
            edits_without_read, re_edits, compactions, subagents, corrections, category,
            last_message, signals_version, ask_score, tool_errors_by, tool_vetoes, agent_ms,
            timeline, mode_ms, planned, peak_context, last_context, team, toolkit, pending_tool,
            turn_started_at, failed, tasks, prs
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                   ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33,
                   ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57, ?58, ?59, ?60, ?61)
         ON CONFLICT(id) DO UPDATE SET
            file_path = excluded.file_path,
            project_path = COALESCE(excluded.project_path, project_path),
            repo_root = COALESCE(excluded.repo_root, repo_root),
            git_branch = COALESCE(excluded.git_branch, git_branch),
            title = COALESCE(excluded.title, title), first_prompt = excluded.first_prompt,
            entrypoint = excluded.entrypoint, started_at = excluded.started_at,
            ended_at = excluded.ended_at, active_ms = excluded.active_ms, model = excluded.model,
            user_turns = excluded.user_turns, assistant_turns = excluded.assistant_turns,
            tool_calls = excluded.tool_calls, tool_errors = excluded.tool_errors,
            interruptions = excluded.interruptions,
            permission_requests = excluded.permission_requests,
            input_tokens = excluded.input_tokens, output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            cache_write_tokens = excluded.cache_write_tokens, cost_usd = excluded.cost_usd,
            cost_is_estimate = excluded.cost_is_estimate,
            premium_requests = excluded.premium_requests, lines_added = excluded.lines_added,
            lines_removed = excluded.lines_removed, last_event = excluded.last_event,
            wait_ms_total = excluded.wait_ms_total, wait_count = excluded.wait_count,
            ingested_at = excluded.ingested_at, tool_counts = excluded.tool_counts,
            reads = excluded.reads, edits = excluded.edits, shell_calls = excluded.shell_calls,
            edits_without_read = excluded.edits_without_read, re_edits = excluded.re_edits,
            compactions = excluded.compactions, subagents = excluded.subagents,
            corrections = excluded.corrections, category = excluded.category,
            last_message = excluded.last_message, signals_version = excluded.signals_version,
            ask_score = excluded.ask_score, tool_errors_by = excluded.tool_errors_by,
            tool_vetoes = excluded.tool_vetoes, agent_ms = excluded.agent_ms,
            timeline = excluded.timeline, mode_ms = excluded.mode_ms,
            planned = excluded.planned, peak_context = excluded.peak_context,
            last_context = excluded.last_context, team = excluded.team,
            toolkit = excluded.toolkit, pending_tool = excluded.pending_tool,
            turn_started_at = excluded.turn_started_at, failed = excluded.failed,
            tasks = excluded.tasks, prs = excluded.prs",
        params![
            id,
            source.as_str(),
            s.external_id,
            unit.key(),
            s.project_path,
            repo,
            s.git_branch,
            s.title,
            first_prompt,
            s.entrypoint,
            d.started_at,
            d.ended_at,
            d.active_ms,
            d.model,
            s.user_turns as i64,
            s.assistant_turns as i64,
            s.tool_calls as i64,
            s.tool_errors as i64,
            s.interruptions as i64,
            s.permission_requests as i64,
            d.tokens.input as i64,
            d.tokens.output as i64,
            d.tokens.cache_read as i64,
            d.tokens.cache_write as i64,
            d.cost_usd,
            d.cost_is_estimate,
            s.premium_requests,
            s.lines_added,
            s.lines_removed,
            s.last_event.map(|e| e.as_str()),
            wait_total,
            s.wait_samples.len() as i64,
            now_ms(),
            serde_json::to_string(&s.tool_counts)?,
            s.reads as i64,
            s.edits as i64,
            s.shell_calls as i64,
            s.edits_without_read as i64,
            s.re_edits as i64,
            s.compactions as i64,
            s.subagents as i64,
            signals::count_corrections(s.prompts.iter().map(|p| p.text.as_str())) as i64,
            signals::categorize(
                s.prompts.first().map(|p| p.text.as_str()).unwrap_or(""),
                s.title.as_deref().unwrap_or("")
            ),
            s.last_message,
            SIGNALS_VERSION,
            signals::check_ask(s.prompts.first().map(|p| p.text.as_str()).unwrap_or("")).score
                as i64,
            serde_json::to_string(&s.tool_errors_by)?,
            serde_json::to_string(&s.tool_vetoes)?,
            s.agent_ms,
            serde_json::to_string(&s.timeline)?,
            serde_json::to_string(&s.mode_ms)?,
            s.planned,
            s.peak_context as i64,
            s.last_context as i64,
            serde_json::to_string(&s.team)?,
            serde_json::json!({ "skills": s.skills, "mcp": s.mcp }).to_string(),
            s.pending_tool(),
            (s.last_event == Some(crate::model::LastEvent::Working))
                .then_some(s.turn_start)
                .flatten(),
            s.failed,
            serde_json::to_string(&s.tasks)?,
            serde_json::to_string(&s.prs)?,
        ],
    )?;

    // Prompts only grow while an agent works, so only new ones are written;
    // if the last stored one differs, the session was re-read and all are.
    let (stored, last): (i64, Option<String>) = conn.query_row(
        "SELECT COUNT(*), (SELECT text FROM prompts WHERE session_id = ?1 ORDER BY idx DESC LIMIT 1)
         FROM prompts WHERE session_id = ?1",
        [&id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut keep = (stored as usize).min(s.prompts.len());
    if keep > 0 && last.as_deref() != Some(s.prompts[keep - 1].text.as_str()) {
        keep = 0;
    }
    conn.execute(
        "DELETE FROM prompts WHERE session_id = ?1 AND idx >= ?2",
        params![id, keep as i64],
    )?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO prompts (session_id, idx, ts, text) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (i, p) in s.prompts.iter().enumerate().skip(keep) {
        stmt.execute(params![id, i as i64, p.ts, p.text])?;
    }
    Ok(id)
}

/// Copies the raw files so history survives the tools' own cleanup (Claude
/// Code deletes transcripts after 30 days by default).
/// Where a session's raw files are archived: one folder per session, named
/// by the id the tool uses (the transcript's file stem, or the session folder
/// for Copilot CLI).
fn archive_dir(data_dir: &Path, source: Source, main: &Path) -> PathBuf {
    let id_source = source.session_id(main);
    let safe_id: String = id_source
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_dir.join("archive").join(source.as_str()).join(safe_id)
}

/// The archived copy of a session whose original transcript is gone.
fn archived_unit(data_dir: &Path, source: Source, original_main: &Path) -> Option<Unit> {
    let dir = archive_dir(data_dir, source, original_main);
    let main = dir.join(original_main.file_name()?);
    if !main.is_file() {
        return None;
    }
    let mut paths = vec![main];
    if let Ok(rd) = std::fs::read_dir(dir.join("sub")) {
        let mut subs: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .collect();
        subs.sort();
        paths.extend(subs);
    }
    Some(Unit { source, paths })
}

fn archive_unit(data_dir: &Path, unit: &Unit) -> Result<()> {
    let dest = archive_dir(data_dir, unit.source, &unit.paths[0]);
    for (i, p) in unit.paths.iter().enumerate() {
        let Ok(meta) = std::fs::metadata(p) else {
            continue;
        };
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = if i == 0 {
            dest.join(&name)
        } else {
            dest.join("sub").join(&name)
        };
        if let Some(parent) = target.parent() {
            crate::util::create_private_dir(parent)?;
        }
        crate::util::restrict_private_if_exists(&target, false)?;
        if i > 0 {
            crate::util::restrict_private_if_exists(&target.with_extension("meta.json"), false)?;
        }
        // Transcripts only grow, so an equal size means nothing new to copy.
        if std::fs::metadata(&target).is_ok_and(|t| t.len() == meta.len()) {
            continue;
        }
        crate::util::copy_private(p, &target)?;
        // A Claude Code sub-agent's role and task sit in a file beside it.
        let side = p.with_extension("meta.json");
        if i > 0 && side.is_file() {
            crate::util::copy_private(&side, &target.with_extension("meta.json"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn archive_is_owner_only() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("session.jsonl");
        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).mode(0o644);
        std::io::Write::write_all(&mut options.open(&source).unwrap(), b"private log").unwrap();

        let unit = Unit {
            source: sources::CLAUDE_CODE,
            paths: vec![source],
        };
        archive_unit(root.path(), &unit).unwrap();
        let archive = archive_dir(root.path(), sources::CLAUDE_CODE, &unit.paths[0]);
        let archived_file = archive.join("session.jsonl");

        assert_eq!(
            std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(archived_file)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            archive.join("session.jsonl"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        archive_unit(root.path(), &unit).unwrap();
        assert_eq!(
            std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(archive.join("session.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    /// Ingests this machine's real logs into a throwaway database and prints
    /// a summary. Run with `cargo test real_logs -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_logs_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        db::set_setting(&conn, "archive.enabled", "false").unwrap();
        let ing = Ingestor::new(dir.path());
        let report = ing.ingest_all(&mut conn, false).unwrap();
        println!("{report:#?}");
        let mut stmt = conn
            .prepare(
                "SELECT source, COUNT(*), SUM(user_turns), SUM(tool_calls), SUM(tool_errors),
                        SUM(interruptions), ROUND(SUM(cost_usd), 2), SUM(active_ms) / 60000,
                        SUM(wait_count), COUNT(repo_root), SUM(input_tokens + output_tokens),
                        SUM(reads), SUM(edits), SUM(edits_without_read), SUM(re_edits),
                        SUM(compactions), SUM(subagents), SUM(corrections),
                        group_concat(DISTINCT category)
                 FROM sessions GROUP BY source",
            )
            .unwrap();
        let rows = stmt
            .query_map([], |r| {
                Ok(format!(
                    "{}: sessions={} prompts={} tools={} tool_err={} interrupts={} cost=${:?} active_min={} waits={} in_repo={} tokens={}",
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<f64>>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, i64>(10)?,
                ) + &format!(
                    " reads={} edits={} blind_edits={} re_edits={} compactions={} subagents={} corrections={} categories={}",
                    r.get::<_, i64>(11)?,
                    r.get::<_, i64>(12)?,
                    r.get::<_, i64>(13)?,
                    r.get::<_, i64>(14)?,
                    r.get::<_, i64>(15)?,
                    r.get::<_, i64>(16)?,
                    r.get::<_, i64>(17)?,
                    r.get::<_, String>(18)?,
                ))
            })
            .unwrap();
        for r in rows {
            println!("{}", r.unwrap());
        }
        drop(stmt);
        let extra: (i64, i64, i64) = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM session_commits), (SELECT COUNT(*) FROM playbooks),
                        (SELECT COUNT(*) FROM session_playbooks)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        println!(
            "commits linked={} playbooks={} playbook links={}",
            extra.0, extra.1, extra.2
        );
        let modes: Vec<String> = conn
            .prepare("SELECT mode_ms FROM sessions")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let mut total: std::collections::BTreeMap<String, i64> = Default::default();
        for m in modes {
            for (k, v) in
                serde_json::from_str::<std::collections::BTreeMap<String, i64>>(&m).unwrap()
            {
                *total.entry(k).or_default() += v;
            }
        }
        println!(
            "agent minutes by mode: {:?}",
            total
                .iter()
                .map(|(k, v)| (k, *v / 60_000))
                .collect::<Vec<_>>()
        );
        // A second pass must find nothing new.
        let again = ing.ingest_all(&mut conn, false).unwrap();
        assert_eq!(again.updated, 0, "unchanged files were re-parsed");
        assert!(!again.changed, "an idle scan wrote to the database");
        println!("second pass: {} ms", again.duration_ms);
    }

    #[test]
    fn upsert_preserves_user_review() {
        let conn = db::open_in_memory().unwrap();
        let unit = Unit {
            source: sources::CLAUDE_CODE,
            paths: vec![PathBuf::from("/x/s.jsonl")],
        };
        let mut s = ParsedSession {
            external_id: "s".into(),
            user_turns: 1,
            ..Default::default()
        };
        let id = upsert_session(&conn, sources::CLAUDE_CODE, &unit, &s, None).unwrap();
        conn.execute(
            "UPDATE sessions SET outcome = 'shipped', rating = 4 WHERE id = ?1",
            [&id],
        )
        .unwrap();

        s.user_turns = 5;
        upsert_session(&conn, sources::CLAUDE_CODE, &unit, &s, None).unwrap();
        let (turns, outcome, rating): (i64, String, i64) = conn
            .query_row(
                "SELECT user_turns, outcome, rating FROM sessions WHERE id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((turns, outcome.as_str(), rating), (5, "shipped", 4));
    }
}
