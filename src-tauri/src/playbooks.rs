//! Playbooks are the instructions you give agents: instruction files
//! (CLAUDE.md, AGENTS.md, copilot-instructions.md) and saved prompt templates.
//! Each distinct content is a version, and sessions are linked to the version
//! that was in effect when they ran, so outcomes can be compared per version.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::git;
use crate::model::Source;
use crate::util::{file_mtime_ms, home, now_ms, path_key};

/// Instruction files looked for at each project root, and their kind.
const INSTRUCTION_FILES: &[(&str, &str)] = &[
    ("CLAUDE.md", "claude_md"),
    (".claude/CLAUDE.md", "claude_md"),
    ("AGENTS.md", "agents_md"),
    (".github/copilot-instructions.md", "copilot_instructions"),
];

/// Versions read back from git history per file.
const MAX_GIT_VERSIONS: usize = 30;

/// Prompt templates match a session when its first prompt contains this many
/// leading characters of the template.
const PROMPT_MATCH_CHARS: usize = 60;

pub fn applies_to(kind: &str, source: &str) -> bool {
    match kind {
        "claude_md" => source == Source::ClaudeCode.as_str(),
        "agents_md" | "copilot_instructions" => source != Source::ClaudeCode.as_str(),
        _ => true,
    }
}

/// Git checks files out with CRLF on Windows (`core.autocrlf`) but `git show`
/// returns LF; normalize so the same text is one version.
fn normalize_eol(content: &str) -> String {
    content.replace("\r\n", "\n")
}

fn hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

fn ensure_playbook(
    conn: &Connection,
    kind: &str,
    name: &str,
    path: &str,
    project_root: Option<&str>,
) -> Result<i64> {
    if let Some(id) = conn
        .query_row("SELECT id FROM playbooks WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO playbooks (kind, name, path, project_root, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind, name, path, project_root, now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

fn add_version(
    conn: &Connection,
    playbook_id: i64,
    content: &str,
    first_seen: i64,
    git_sha: Option<&str>,
) -> Result<()> {
    let content = normalize_eol(content);
    let version_id: i64 = conn.query_row(
        "INSERT INTO playbook_versions (playbook_id, hash, content, first_seen, git_sha)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(playbook_id, hash) DO UPDATE SET
           first_seen = MIN(first_seen, excluded.first_seen),
           git_sha = COALESCE(git_sha, excluded.git_sha)
         RETURNING id",
        params![playbook_id, hash(&content), content, first_seen, git_sha],
        |r| r.get(0),
    )?;
    // Content that comes back (a revert) gets another sighting, so it is in
    // effect again from that point.
    conn.execute(
        "INSERT OR IGNORE INTO playbook_sightings (version_id, seen_at) VALUES (?1, ?2)",
        params![version_id, first_seen],
    )?;
    Ok(())
}

/// Finds instruction files under each project root and records new versions,
/// including past versions from git history.
pub fn sync(conn: &Connection, roots: &BTreeSet<String>) -> Result<()> {
    let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"));
    let global = claude_home.join("CLAUDE.md");
    if let Ok(content) = std::fs::read_to_string(&global) {
        let path = global.to_string_lossy();
        let id = ensure_playbook(conn, "claude_md", "Global CLAUDE.md", &path, None)?;
        add_version(
            conn,
            id,
            &content,
            file_mtime_ms(&global).unwrap_or_else(now_ms),
            None,
        )?;
    }

    for root in roots {
        let is_repo = git::repo_root(root).is_some_and(|r| path_key(&r) == path_key(root));
        for (rel, kind) in INSTRUCTION_FILES {
            let file = Path::new(root).join(rel);
            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };
            let project = Path::new(root)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.clone());
            let id = ensure_playbook(
                conn,
                kind,
                &format!("{project}/{rel}"),
                &file.to_string_lossy(),
                Some(root),
            )?;
            if is_repo {
                sync_git_history(conn, id, root, rel)?;
            }
            add_version(
                conn,
                id,
                &content,
                file_mtime_ms(&file).unwrap_or_else(now_ms),
                None,
            )?;
        }
    }
    Ok(())
}

fn sync_git_history(conn: &Connection, playbook_id: i64, repo: &str, rel: &str) -> Result<()> {
    let n = format!("-n{MAX_GIT_VERSIONS}");
    let Ok(log) = git::git(repo, &["log", &n, "--format=%H %ct", "--", rel]) else {
        return Ok(());
    };
    for line in log.lines() {
        let Some((sha, ct)) = line.split_once(' ') else {
            continue;
        };
        let known: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM playbook_versions WHERE playbook_id = ?1 AND git_sha = ?2)",
            params![playbook_id, sha],
            |r| r.get(0),
        )?;
        if known {
            continue;
        }
        let Ok(ct) = ct.trim().parse::<i64>() else {
            continue;
        };
        // A commit that deleted the file has nothing to show.
        if let Ok(content) = git::git(repo, &["show", &format!("{sha}:{rel}")]) {
            add_version(conn, playbook_id, &content, ct * 1000, Some(sha))?;
        }
    }
    Ok(())
}

pub fn create_prompt_template(conn: &Connection, name: &str, content: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO playbooks (kind, name, path, project_root, created_at) VALUES ('prompt', ?1, NULL, NULL, ?2)",
        params![name, now_ms()],
    )?;
    let id = conn.last_insert_rowid();
    add_version(conn, id, content, now_ms(), None)?;
    relink(conn, None)?;
    Ok(id)
}

pub fn update_prompt_template(conn: &Connection, id: i64, name: &str, content: &str) -> Result<()> {
    conn.execute(
        "UPDATE playbooks SET name = ?1 WHERE id = ?2 AND kind = 'prompt'",
        params![name, id],
    )?;
    add_version(conn, id, content, now_ms(), None)?;
    relink(conn, None)
}

fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// id, source, started_at, project root, first prompt
type SessionLinkRow = (String, String, Option<i64>, Option<String>, Option<String>);
/// id, kind, name, path, project_root
type PlaybookRow = (i64, String, String, Option<String>, Option<String>);

/// A point from which a version was in effect.
struct Sighting {
    version_id: i64,
    seen_at: i64,
}

struct VersionHistory {
    sightings: Vec<Sighting>,
    from_git: bool,
}

/// Re-derives the automatic session links. `None` means every session.
pub fn relink(conn: &Connection, sessions: Option<&[String]>) -> Result<()> {
    let playbooks: Vec<(i64, String, Option<String>)> = {
        let mut stmt = conn.prepare("SELECT id, kind, project_root FROM playbooks")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut history = std::collections::HashMap::new();
    let mut snippets = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT s.version_id, s.seen_at, v.git_sha IS NOT NULL, v.content
             FROM playbook_sightings s JOIN playbook_versions v ON v.id = s.version_id
             WHERE v.playbook_id = ?1 ORDER BY s.seen_at, s.version_id",
        )?;
        for (pid, kind, _) in &playbooks {
            let mut h = VersionHistory {
                sightings: vec![],
                from_git: false,
            };
            let mut latest_content = String::new();
            for row in stmt.query_map([pid], |r| {
                Ok((
                    Sighting {
                        version_id: r.get(0)?,
                        seen_at: r.get(1)?,
                    },
                    r.get::<_, bool>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })? {
                let (sighting, from_git, content) = row?;
                h.from_git |= from_git;
                latest_content = content;
                h.sightings.push(sighting);
            }
            if kind == "prompt" {
                let snippet: String = normalize(&latest_content)
                    .chars()
                    .take(PROMPT_MATCH_CHARS)
                    .collect();
                if snippet.chars().count() >= 20 {
                    snippets.insert(*pid, snippet);
                }
            }
            history.insert(*pid, h);
        }
    }

    let rows: Vec<SessionLinkRow> = {
        let mut stmt = conn.prepare("SELECT id, source, started_at, COALESCE(repo_root, project_path), first_prompt FROM sessions")?;
        stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let wanted: Option<BTreeSet<&str>> =
        sessions.map(|ids| ids.iter().map(String::as_str).collect());

    let tx = conn.unchecked_transaction()?;
    {
        let mut delete =
            tx.prepare("DELETE FROM session_playbooks WHERE session_id = ?1 AND link = 'auto'")?;
        let mut insert = tx.prepare("INSERT OR IGNORE INTO session_playbooks (session_id, version_id, link) VALUES (?1, ?2, 'auto')")?;
        for (sid, source, started, root, first_prompt) in &rows {
            if wanted.as_ref().is_some_and(|w| !w.contains(sid.as_str())) {
                continue;
            }
            delete.execute([sid])?;
            let started = started.unwrap_or(0);
            let prompt = first_prompt.as_deref().map(normalize).unwrap_or_default();
            for (pid, kind, proot) in &playbooks {
                if !applies_to(kind, source) {
                    continue;
                }
                let matches = if kind == "prompt" {
                    snippets
                        .get(pid)
                        .is_some_and(|snip| prompt.contains(snip.as_str()))
                } else {
                    match (proot, root) {
                        (None, _) => true,
                        (Some(p), Some(r)) => path_key(p) == path_key(r),
                        _ => false,
                    }
                };
                if !matches {
                    continue;
                }
                if let Some(v) = version_at(&history[pid], started) {
                    insert.execute(params![sid, v])?;
                }
            }
        }
    }
    tx.commit()?;
    Ok(())
}

/// The version in effect at `t`: the one sighted most recently before it.
/// Without git history the first version we saw is the best guess for older
/// sessions; with history, a session from before the file existed gets none.
fn version_at(h: &VersionHistory, t: i64) -> Option<i64> {
    let in_effect = h
        .sightings
        .iter()
        .filter(|s| s.seen_at <= t)
        .max_by_key(|s| s.seen_at);
    match in_effect {
        Some(s) => Some(s.version_id),
        None if h.from_git => None,
        None => h
            .sightings
            .iter()
            .min_by_key(|s| s.seen_at)
            .map(|s| s.version_id),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionStats {
    pub id: i64,
    pub first_seen: i64,
    /// When this content was last put in effect; the newest is current.
    pub last_seen: i64,
    pub git_sha: Option<String>,
    pub content: String,
    pub sessions: i64,
    pub rated: i64,
    pub success_rate: Option<f64>,
    pub avg_rating: Option<f64>,
    pub avg_turns: Option<f64>,
    pub avg_interruptions: Option<f64>,
    pub tool_error_rate: Option<f64>,
    pub commits_per_session: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybookStats {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub path: Option<String>,
    pub project_root: Option<String>,
    pub versions: Vec<VersionStats>,
}

pub fn list(conn: &Connection) -> Result<Vec<PlaybookStats>> {
    let mut out = vec![];
    let mut stmt = conn
        .prepare("SELECT id, kind, name, path, project_root FROM playbooks ORDER BY kind, name")?;
    let pbs: Vec<PlaybookRow> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut vstmt = conn.prepare(
        "SELECT v.id, v.first_seen,
                (SELECT MAX(seen_at) FROM playbook_sightings ps WHERE ps.version_id = v.id),
                v.git_sha, v.content,
                COUNT(s.id),
                SUM(s.outcome IS NOT NULL),
                AVG(CASE s.outcome WHEN 'shipped' THEN 1.0 WHEN 'partial' THEN 0.5
                                   WHEN 'abandoned' THEN 0.0 WHEN 'redo' THEN 0.0 END),
                AVG(s.rating),
                AVG(s.user_turns),
                AVG(s.interruptions),
                CAST(SUM(s.tool_errors) AS REAL) / NULLIF(SUM(s.tool_calls), 0),
                CASE WHEN COUNT(s.id) = 0 THEN NULL
                     ELSE AVG((SELECT COUNT(*) FROM session_commits c WHERE c.session_id = s.id)) END
         FROM playbook_versions v
         LEFT JOIN session_playbooks sp ON sp.version_id = v.id
         LEFT JOIN sessions s ON s.id = sp.session_id
         WHERE v.playbook_id = ?1
         GROUP BY v.id
         ORDER BY 3 DESC, v.id DESC",
    )?;
    for (id, kind, name, path, project_root) in pbs {
        let versions = vstmt
            .query_map([id], |r| {
                Ok(VersionStats {
                    id: r.get(0)?,
                    first_seen: r.get(1)?,
                    last_seen: r.get(2)?,
                    git_sha: r.get(3)?,
                    content: r.get(4)?,
                    sessions: r.get(5)?,
                    rated: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    success_rate: r.get(7)?,
                    avg_rating: r.get(8)?,
                    avg_turns: r.get(9)?,
                    avg_interruptions: r.get(10)?,
                    tool_error_rate: r.get(11)?,
                    commits_per_session: r.get(12)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        out.push(PlaybookStats {
            id,
            kind,
            name,
            path,
            project_root,
            versions,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history(sightings: &[(i64, i64)], from_git: bool) -> VersionHistory {
        VersionHistory {
            sightings: sightings
                .iter()
                .map(|&(version_id, seen_at)| Sighting {
                    version_id,
                    seen_at,
                })
                .collect(),
            from_git,
        }
    }

    #[test]
    fn picks_version_in_effect() {
        let h = history(&[(1, 100), (2, 200)], true);
        assert_eq!(version_at(&h, 150), Some(1));
        assert_eq!(version_at(&h, 250), Some(2));
        assert_eq!(version_at(&h, 50), None);
        // Without git history, older sessions fall back to the first version.
        assert_eq!(version_at(&history(&[(3, 100)], false), 50), Some(3));
    }

    #[test]
    fn reverted_content_is_in_effect_again() {
        let conn = crate::db::open_in_memory().unwrap();
        let id =
            create_prompt_template(&conn, "T", "version A of the prompt template text").unwrap();
        conn.execute("UPDATE playbook_sightings SET seen_at = 100", [])
            .unwrap();
        add_version(
            &conn,
            id,
            "version B of the prompt template text",
            200,
            None,
        )
        .unwrap();
        add_version(
            &conn,
            id,
            "version A of the prompt template text",
            300,
            None,
        )
        .unwrap();
        let count = || -> i64 {
            conn.query_row("SELECT COUNT(*) FROM playbook_versions", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(), 2);
        let listed = &list(&conn).unwrap()[0];
        assert!(listed.versions[0].content.starts_with("version A"));
        assert_eq!(listed.versions[0].last_seen, 300);

        // CRLF content (a Windows checkout) is the same version as LF content.
        add_version(&conn, id, "line one\nline two", 400, None).unwrap();
        add_version(&conn, id, "line one\r\nline two", 400, None).unwrap();
        assert_eq!(count(), 3);
    }

    #[test]
    fn prompt_templates_link_by_first_prompt() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, first_prompt, started_at, ingested_at)
             VALUES ('claude_code:a', 'claude_code', 'a', '/a', 'Please   REVIEW this diff for correctness bugs and missing tests. Thanks', 10, 0),
                    ('claude_code:b', 'claude_code', 'b', '/b', 'something else entirely', 10, 0)",
            [],
        )
        .unwrap();
        create_prompt_template(
            &conn,
            "Review",
            "Review this diff for correctness bugs and missing tests.",
        )
        .unwrap();
        let linked: Vec<String> = conn
            .prepare("SELECT session_id FROM session_playbooks")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(linked, ["claude_code:a"]);
    }
}
