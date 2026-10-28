//! The weekly retro: package the period's numbers, sessions and journal
//! reflections, and ask Claude for a specific, evidence-based review with
//! experiments for next week.
//!
//! Two engines: the local `claude` CLI (uses the person's existing Claude
//! subscription, no key needed) or the Anthropic API with a key kept in the
//! OS keychain.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::analytics::{self, project_name, source_label};
use crate::store::Range;
use crate::util::{home, now_ms, quiet_command, truncate};
use crate::{db, journal};

pub const KEYRING_SERVICE: &str = "tilly";
/// Where the key was stored under earlier names, newest first; moved on first read.
const LEGACY_KEYRING_SERVICES: &[&str] = &["governess", "execution-ledger"];
pub const KEYRING_USER: &str = "anthropic-api-key";
pub const DEFAULT_API_MODEL: &str = "claude-opus-5-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
const MAX_SESSIONS_IN_PROMPT: usize = 60;

const SYSTEM_PROMPT: &str = "You are an execution coach for a developer who does most of their work by delegating to AI coding agents (Claude Code and GitHub Copilot). You review one period of their tracked work and help them get better at delegating, prompting, scoping and closing out tasks.

Ground every claim in the data you are given and cite the numbers. Prefer specific, observed patterns over general advice; if the data is too thin to support a point, say so instead of guessing. Be direct and brief; write for someone who will read this in two minutes.

Respond in Markdown with exactly these sections:
## Summary
Two or three sentences on how the period went.
## What worked
## Where execution leaked
Time, retries, interruptions, waiting, abandoned work: what it cost and the likely cause.
## How you prompt
Patterns in opening prompts and follow-ups, with short quoted examples.
## Calibration
How estimates and confidence compared with results, if journal data exists.
## Experiments so far
Only if experiments are listed: for each, what the measured before/after shows. Cite n, and say \"too early to tell\" when n is below 5 rather than guessing.
## Experiments for next period
Exactly three, each with what to do differently and the metric in this app that will show whether it worked.

If the developer wrote their own reflection, start from it: agree, add evidence, or gently challenge it.

After the Markdown, add one fenced ```json block containing your three experiments, in this shape:
{\"experiments\": [{\"title\": \"short name\", \"hypothesis\": \"if I do X, metric Y improves\", \"metric\": \"<one metric key>\", \"category\": \"<optional task category>\"}]}
Valid metric keys: success_rate, corrections_per_session, interruptions_per_session, re_edit_rate, blind_edit_rate, clear_ask, flow_score, active_minutes, commit_rate, cost_per_session, rework_rate. Valid categories: feature, bugfix, refactor, tests, docs, research, infra, review, other.

Do not use tools. Reply with the Markdown and the JSON block only.";

/// An experiment the retro proposed, ready to adopt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub title: String,
    #[serde(default)]
    pub hypothesis: Option<String>,
    pub metric: String,
    #[serde(default)]
    pub category: Option<String>,
}

/// Splits the model's reply into the Markdown to show and the proposed
/// experiments. Suggestions with unknown metrics are dropped.
pub fn split_reply(reply: &str) -> (String, Vec<Suggestion>) {
    let Some(start) = reply.rfind("```json") else {
        return (reply.trim().to_string(), vec![]);
    };
    let body_start = start + "```json".len();
    let end = reply[body_start..]
        .find("```")
        .map(|e| body_start + e)
        .unwrap_or(reply.len());
    // Parse each suggestion on its own so one malformed entry doesn't
    // drop the others.
    let suggestions = serde_json::from_str::<Value>(reply[body_start..end].trim())
        .ok()
        .and_then(|v| v.get("experiments").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Suggestion>(v).ok())
        .filter(|s| crate::experiments::metric(&s.metric).is_ok() && !s.title.trim().is_empty())
        .take(3)
        .collect();
    let markdown = format!("{}{}", &reply[..start], reply.get(end + 3..).unwrap_or(""));
    (markdown.trim().to_string(), suggestions)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetroRow {
    pub id: i64,
    pub created_at: i64,
    pub period_start: i64,
    pub period_end: i64,
    pub engine: String,
    pub model: Option<String>,
    pub content_md: String,
    pub reflection: Option<String>,
    pub suggestions: Vec<Suggestion>,
}

pub fn list(conn: &Connection) -> Result<Vec<RetroRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, period_start, period_end, engine, model, content_md, reflection,
                experiments_json
         FROM retros ORDER BY created_at DESC LIMIT 100",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(RetroRow {
                id: r.get(0)?,
                created_at: r.get(1)?,
                period_start: r.get(2)?,
                period_end: r.get(3)?,
                engine: r.get(4)?,
                model: r.get(5)?,
                content_md: r.get(6)?,
                reflection: r.get(7)?,
                suggestions: r
                    .get::<_, Option<String>>(8)?
                    .and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM retros WHERE id = ?1", [id])?;
    Ok(())
}

pub struct Prepared {
    pub range: Range,
    pub stats_json: String,
    pub prompt: String,
}

/// Builds the user message. Exposed separately so the UI can preview exactly
/// what will be sent before anything leaves the machine.
pub fn prepare(conn: &Connection, days: i64, reflection: Option<&str>) -> Result<Prepared> {
    let range = Range::last_days(days.max(1));
    let overview = analytics::overview(conn, days.max(1))?;
    let insights = analytics::insights(conn, days.max(1))?;
    let calibration = journal::calibration(conn, range.since)?;
    let sessions = crate::store::load_sessions(conn, range)?;

    let stats = json!({
        "periodDays": days,
        "totals": overview.totals,
        "previousPeriod": overview.previous,
        "byTool": overview.by_source,
        "byProject": overview.by_project,
        "calibration": calibration,
        "ruleBasedInsights": insights,
    });
    let stats_json = serde_json::to_string_pretty(&stats)?;

    let mut prompt = format!(
        "Period: last {days} days, ending {}.\n\n# Aggregate metrics (JSON)\n{stats_json}\n\n# Sessions (newest first, up to {MAX_SESSIONS_IN_PROMPT})\n",
        chrono::Local::now().format("%Y-%m-%d")
    );
    for s in sessions.iter().take(MAX_SESSIONS_IN_PROMPT) {
        let notes: Option<String> = conn
            .query_row("SELECT notes FROM sessions WHERE id = ?1", [&s.id], |r| {
                r.get(0)
            })
            .unwrap_or(None);
        let date = s
            .started_at
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%a %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        prompt.push_str(&format!(
            "- {date} | {} | {} | {} | prompts {} | interruptions {} | tool errors {}/{} | active {} min | commits {} | outcome {} | rating {}\n  opening prompt: \"{}\"\n",
            source_label(&s.source),
            s.repo_root.as_deref().or(s.project_path.as_deref()).map(project_name).unwrap_or_else(|| "-".into()),
            truncate(s.title.as_deref().unwrap_or("untitled"), 80),
            s.user_turns,
            s.interruptions,
            s.tool_errors,
            s.tool_calls,
            s.active_ms / 60_000,
            s.commits,
            s.outcome.as_deref().unwrap_or("unrated"),
            s.rating.map(|r| r.to_string()).unwrap_or_else(|| "-".into()),
            truncate(&s.first_prompt.clone().unwrap_or_default().replace('\n', " "), 300),
        ));
        if let Some(n) = notes {
            prompt.push_str(&format!(
                "  your note: \"{}\"\n",
                truncate(&n.replace('\n', " "), 200)
            ));
        }
    }

    let intents = journal::list(conn)?;
    let closed: Vec<_> = intents
        .iter()
        .filter(|i| i.closed_at.is_some_and(|c| c >= range.since))
        .collect();
    if !closed.is_empty() {
        prompt.push_str("\n# Journal: intents closed this period\n");
        for i in closed.iter().take(40) {
            prompt.push_str(&format!(
                "- [{}] \"{}\" | expected {} min, actual {} | confidence {}/5 | outcome {} | reflection: \"{}\"\n",
                i.task_type,
                truncate(&i.goal, 160),
                i.expected_minutes,
                i.actual_minutes
                    .map(|m| format!("{m} min"))
                    .or_else(|| i.session_active_minutes.map(|m| format!("~{m:.0} active min")))
                    .unwrap_or_else(|| "unknown".into()),
                i.confidence,
                i.outcome.as_deref().unwrap_or("-"),
                truncate(i.reflection.as_deref().unwrap_or(""), 200),
            ));
        }
    }

    let exps = crate::experiments::list(conn)?;
    if !exps.is_empty() {
        prompt.push_str(
            "\n# Experiments (baseline locked at start; current = sessions in the window)\n",
        );
        for e in exps.iter().take(10) {
            prompt.push_str(&format!(
                "- \"{}\" [{}] metric {} ({}): baseline {} (n={}) → current {} (n={}); applied in {}/{} eligible sessions; reading: {}{}\n",
                e.title,
                e.status,
                e.metric.key,
                if e.metric.higher_is_better { "higher is better" } else { "lower is better" },
                e.baseline_value.map(|v| format!("{v:.2}")).unwrap_or_else(|| "none".into()),
                e.baseline_n,
                e.current_value.map(|v| format!("{v:.2}")).unwrap_or_else(|| "none".into()),
                e.current_n,
                e.applied,
                e.eligible,
                e.reading,
                e.verdict_note.as_deref().map(|n| format!("; your note: \"{}\"", truncate(n, 160))).unwrap_or_default(),
            ));
        }
    }
    let clusters = crate::trends::failure_clusters(conn, days.max(1))?;
    if !clusters.is_empty() {
        prompt.push_str("\n# Why sessions went wrong (abandoned, redone or reworked), grouped by likely cause\n");
        for c in &clusters {
            prompt.push_str(&format!("- {}: {} sessions\n", c.cause, c.ids.len()));
        }
    }
    let goals = crate::experiments::goals(conn)?;
    if !goals.is_empty() {
        prompt.push_str("\n# Goals (last 7 days)\n");
        for g in &goals {
            prompt.push_str(&format!(
                "- {} target {:.2}: now {} (n={}); counter-metric {} {} (was {}){}\n",
                g.metric.key,
                g.target,
                g.current
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "none".into()),
                g.n,
                g.counter.key,
                g.counter_current
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "none".into()),
                g.counter_baseline
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "none".into()),
                if g.counter_slipping { ", SLIPPING" } else { "" },
            ));
        }
    }
    if let Some(r) = reflection.map(str::trim).filter(|r| !r.is_empty()) {
        prompt.push_str("\n# The developer's own reflection, written before this review\n");
        prompt.push_str(&truncate(r, 2000));
        prompt.push('\n');
    }

    Ok(Prepared {
        range,
        stats_json,
        prompt,
    })
}

pub fn generate(
    conn: &Connection,
    data_dir: &Path,
    engine: &str,
    days: i64,
    reflection: Option<&str>,
) -> Result<RetroRow> {
    let prepared = prepare(conn, days, reflection)?;
    let (content, model) = match engine {
        "claude_cli" => {
            let model = db::get_setting(conn, "retro.cliModel")?.filter(|m| !m.is_empty());
            let bin = find_claude_cli(conn).ok_or_else(|| {
                anyhow!("Could not find the `claude` CLI. Install Claude Code or set its path in Settings.")
            })?;
            let workdir = data_dir.join("retro-runs");
            tilly_core::util::create_private_dir(&workdir)?;
            (
                run_cli(&bin, &workdir, model.as_deref(), &prepared.prompt)?,
                model,
            )
        }
        "anthropic_api" => {
            let model = db::get_setting(conn, "retro.apiModel")?
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| DEFAULT_API_MODEL.to_string());
            let key =
                api_key()?.ok_or_else(|| anyhow!("Add an Anthropic API key in Settings first."))?;
            (call_api(&key, &model, &prepared.prompt)?, Some(model))
        }
        other => bail!("Unknown retro engine {other}"),
    };

    let (markdown, suggestions) = split_reply(&content);
    conn.execute(
        "INSERT INTO retros (created_at, period_start, period_end, engine, model, stats_json, content_md,
                             reflection, experiments_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            now_ms(),
            prepared.range.since,
            prepared.range.until,
            engine,
            model,
            prepared.stats_json,
            markdown,
            reflection.map(str::trim).filter(|r| !r.is_empty()),
            serde_json::to_string(&suggestions)?
        ],
    )?;
    let id = conn.last_insert_rowid();
    list(conn)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| anyhow!("retro vanished after insert"))
}

/// GUI apps on macOS do not inherit the shell PATH, so check the usual
/// install locations as well as PATH.
pub fn find_claude_cli(conn: &Connection) -> Option<PathBuf> {
    if let Ok(Some(custom)) = db::get_setting(conn, "retro.claudePath") {
        let p = PathBuf::from(custom.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    let h = home();
    let mut candidates: Vec<PathBuf> = if cfg!(windows) {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_default();
        vec![
            h.join(".local").join("bin").join("claude.exe"),
            h.join(".claude").join("local").join("claude.exe"),
            appdata.join("npm").join("claude.cmd"),
        ]
    } else {
        vec![
            h.join(".local/bin/claude"),
            h.join(".claude/local/claude"),
            PathBuf::from("/opt/homebrew/bin/claude"),
            PathBuf::from("/usr/local/bin/claude"),
            h.join(".npm-global/bin/claude"),
        ]
    };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(if cfg!(windows) {
                "claude.exe"
            } else {
                "claude"
            }));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn run_cli(bin: &Path, workdir: &Path, model: Option<&str>, prompt: &str) -> Result<String> {
    let full_prompt = format!("{SYSTEM_PROMPT}\n\n---\n\n{prompt}");
    let mut cmd = quiet_command(bin);
    cmd.current_dir(workdir)
        // The prompt contains transcript text, which is untrusted input, so
        // the model gets no tools at all.
        .args(["-p", "--output-format", "json", "--tools", ""])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("start {}", bin.display()))?;
    child
        .stdin
        .take()
        .context("claude stdin")?
        .write_all(full_prompt.as_bytes())?;
    let out = wait_with_timeout(child, CLI_TIMEOUT)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    match parsed {
        Some(v) if v.get("is_error").and_then(Value::as_bool) != Some(true) => v
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow!("claude returned no result")),
        Some(v) => bail!(
            "claude reported an error: {}",
            v.get("result").and_then(Value::as_str).unwrap_or("unknown")
        ),
        None => bail!(
            "claude exited with {}: {}",
            out.status,
            truncate(String::from_utf8_lossy(&out.stderr).trim(), 500)
        ),
    }
}

/// A retro is one long completion; anything past this is a stuck CLI (an
/// expired login, a stalled network).
const CLI_TIMEOUT: Duration = Duration::from_secs(10 * 60);

fn wait_with_timeout(child: std::process::Child, limit: Duration) -> Result<std::process::Output> {
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(limit) {
        Ok(out) => Ok(out?),
        Err(_) => {
            kill_process(pid);
            bail!(
                "claude did not finish within {} minutes",
                limit.as_secs() / 60
            )
        }
    }
}

fn kill_process(pid: u32) {
    let pid = pid.to_string();
    let _ = if cfg!(windows) {
        quiet_command("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .status()
    } else {
        quiet_command("kill").args(["-9", &pid]).status()
    };
}

/// Server-side refusal fallback is supported on these models only.
fn supports_fallbacks(model: &str) -> bool {
    [
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-fable-5-1",
        "claude-sonnet-5-5",
    ]
    .contains(&model)
}

fn call_api(key: &str, model: &str, prompt: &str) -> Result<String> {
    let mut body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": SYSTEM_PROMPT,
        "messages": [{"role": "user", "content": prompt}],
        "output_config": {"effort": "high"},
    });
    let mut req = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?
        .post(API_URL)
        .header("x-api-key", key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json");
    if supports_fallbacks(model) {
        body["fallbacks"] = json!("default");
        req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
    }
    let resp = req.json(&body).send()?;
    let status = resp.status();
    let v: Value = resp.json().context("read API response")?;
    if !status.is_success() {
        bail!(
            "Anthropic API {status}: {}",
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        );
    }
    if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        bail!("Claude declined to write this retro. Try again, or switch engines in Settings.");
    }
    let text: Vec<&str> = v
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    if text.is_empty() {
        bail!("The API returned no text");
    }
    Ok(text.join("\n\n"))
}

fn keyring_entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)?)
}

pub fn api_key() -> Result<Option<String>> {
    match keyring_entry()?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => migrate_legacy_key(),
        Err(e) => Err(e.into()),
    }
}

fn migrate_legacy_key() -> Result<Option<String>> {
    for service in LEGACY_KEYRING_SERVICES {
        let legacy = keyring::Entry::new(service, KEYRING_USER)?;
        match legacy.get_password() {
            Ok(k) => {
                keyring_entry()?.set_password(&k)?;
                let _ = legacy.delete_credential();
                return Ok(Some(k));
            }
            Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(None)
}

pub fn set_api_key(key: &str) -> Result<()> {
    let key = key.trim();
    if !key.starts_with("sk-ant-") {
        bail!("That does not look like an Anthropic API key (sk-ant-…).");
    }
    keyring_entry()?.set_password(key)?;
    Ok(())
}

pub fn clear_api_key() -> Result<()> {
    match keyring_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_sessions_and_journal() {
        let conn = db::open_in_memory().unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, title, first_prompt, project_path,
                                   started_at, ended_at, active_ms, user_turns, outcome, notes, ingested_at)
             VALUES ('claude_code:a', 'claude_code', 'a', '/f', 'Auth refactor', 'Refactor the auth module',
                     '/work/app', ?1, ?1, 60000, 4, 'redo', 'Went in circles', 0)",
            [now - 3_600_000],
        )
        .unwrap();
        let p = prepare(&conn, 7, Some("I kept steering instead of restarting")).unwrap();
        assert!(p.prompt.contains("kept steering"));
        assert!(p.prompt.contains("Auth refactor"));
        assert!(p.prompt.contains("Refactor the auth module"));
        assert!(p.prompt.contains("Went in circles"));
        assert!(p.prompt.contains("outcome redo"));
        assert!(p.stats_json.contains("\"sessions\": 1"));
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn splits_markdown_from_suggestions() {
        let reply = "## Summary\nGood week.\n\n```json\n{\"experiments\": [{\"title\": \"Test first\", \"metric\": \"success_rate\", \"category\": \"bugfix\"}, {\"title\": \"Bad\", \"metric\": \"sessions\"}, {\"title\": \"No metric\"}]}\n```\n";
        let (md, s) = split_reply(reply);
        assert_eq!(md, "## Summary\nGood week.");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].metric, "success_rate");
        let (md, s) = split_reply("no block");
        assert_eq!((md.as_str(), s.len()), ("no block", 0));
    }
}
