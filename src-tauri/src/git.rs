//! Links sessions to the commits they produced, using the `git` CLI.

use std::path::Path;

use anyhow::{Result, bail};
use rusqlite::{Connection, params};

use crate::util::quiet_command;

/// Commits this long after a session's last event still count toward it.
pub const COMMIT_WINDOW_MS: i64 = 30 * 60 * 1000;

pub fn git(repo: &str, args: &[&str]) -> Result<String> {
    let out = quiet_command("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn repo_root(path: &str) -> Option<String> {
    if !Path::new(path).is_dir() {
        return None;
    }
    git(path, &["rev-parse", "--show-toplevel"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub committed_at: i64,
    pub author_email: String,
    pub subject: String,
    pub insertions: i64,
    pub deletions: i64,
    pub files: i64,
    pub ai_trailer: bool,
}

const RECORD: char = '\u{1e}';
const FIELD: char = '\u{1f}';
const END: char = '\u{1d}';

fn read_commits(repo: &str, since_ms: i64) -> Result<Vec<Commit>> {
    let since = format!("--since=@{}", since_ms / 1000);
    let out = git(
        repo,
        &[
            "log",
            "--all",
            "--no-merges",
            &since,
            "--format=%x1e%H%x1f%ct%x1f%ae%x1f%s%x1f%b%x1d",
            "--shortstat",
        ],
    )?;
    Ok(parse_log(&out))
}

fn parse_log(out: &str) -> Vec<Commit> {
    out.split(RECORD)
        .filter(|r| !r.trim().is_empty())
        .filter_map(|rec| {
            let (header, stats) = rec.split_once(END).unwrap_or((rec, ""));
            let mut f = header.splitn(5, FIELD);
            let sha = f.next()?.trim().to_string();
            let committed_at = f.next()?.trim().parse::<i64>().ok()? * 1000;
            let author_email = f.next()?.trim().to_lowercase();
            let subject = f.next()?.trim().to_string();
            let body = f.next().unwrap_or("").to_lowercase();
            let (files, insertions, deletions) = parse_shortstat(stats);
            Some(Commit {
                sha,
                committed_at,
                author_email,
                subject,
                insertions,
                deletions,
                files,
                ai_trailer: is_ai_commit(&body),
            })
        })
        .collect()
}

fn is_ai_commit(body_lower: &str) -> bool {
    [
        "co-authored-by: claude",
        "co-authored-by: copilot",
        "generated with [claude code]",
    ]
    .iter()
    .any(|m| body_lower.contains(m))
}

/// " 3 files changed, 10 insertions(+), 2 deletions(-)"
fn parse_shortstat(s: &str) -> (i64, i64, i64) {
    let (mut files, mut ins, mut del) = (0, 0, 0);
    for part in s.split(',') {
        let part = part.trim();
        let n = part
            .split_whitespace()
            .next()
            .and_then(|x| x.parse::<i64>().ok())
            .unwrap_or(0);
        if part.contains("file") {
            files = n;
        } else if part.contains("insertion") {
            ins = n;
        } else if part.contains("deletion") {
            del = n;
        }
    }
    (files, ins, del)
}

/// Recomputes commit links for every session in `repo`. Each commit by the
/// repo's configured author goes to the most recent session that started
/// before it and was active within `COMMIT_WINDOW_MS`.
pub fn link_commits(conn: &Connection, repo: &str) -> Result<usize> {
    let sessions: Vec<(String, i64, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT id, started_at, ended_at FROM sessions
             WHERE repo_root = ?1 AND started_at IS NOT NULL AND ended_at IS NOT NULL
             ORDER BY started_at",
        )?;
        stmt.query_map([repo], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM session_commits WHERE repo_root = ?1", [repo])?;
    let Some(since) = sessions.first().map(|s| s.1) else {
        tx.commit()?;
        return Ok(0);
    };

    let email = git(repo, &["config", "user.email"])
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    let commits = read_commits(repo, since)?;

    let mut linked = 0;
    {
        let mut insert = tx.prepare(
            "INSERT OR IGNORE INTO session_commits
             (session_id, repo_root, sha, committed_at, subject, insertions, deletions, files, ai_trailer)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for c in &commits {
            if !email.is_empty() && c.author_email != email {
                continue;
            }
            if let Some(owner) = owning_session(&sessions, c.committed_at) {
                insert.execute(params![
                    owner,
                    repo,
                    c.sha,
                    c.committed_at,
                    c.subject,
                    c.insertions,
                    c.deletions,
                    c.files,
                    c.ai_trailer
                ])?;
                linked += 1;
            }
        }
    }
    tx.commit()?;
    Ok(linked)
}

/// `sessions` must be sorted by start time.
fn owning_session(sessions: &[(String, i64, i64)], t: i64) -> Option<&str> {
    sessions
        .iter()
        .rev()
        .find(|(_, start, end)| *start <= t && t <= end + COMMIT_WINDOW_MS)
        .map(|(id, _, _)| id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_log_records() {
        let out = format!(
            "{RECORD}abc{FIELD}1700000000{FIELD}Me@X.com{FIELD}Add login{FIELD}body\n\nCo-Authored-By: Claude Opus <x>\n{END}\n\n 2 files changed, 10 insertions(+), 1 deletion(-)\n\
             {RECORD}def{FIELD}1700000100{FIELD}me@x.com{FIELD}Docs{FIELD}{END}\n\n 1 file changed, 3 insertions(+)\n"
        );
        let commits = parse_log(&out);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "abc");
        assert_eq!(commits[0].committed_at, 1_700_000_000_000);
        assert_eq!(commits[0].author_email, "me@x.com");
        assert_eq!(
            (
                commits[0].files,
                commits[0].insertions,
                commits[0].deletions
            ),
            (2, 10, 1)
        );
        assert!(commits[0].ai_trailer);
        assert!(!commits[1].ai_trailer);
        assert_eq!(commits[1].insertions, 3);
    }

    #[test]
    fn commit_goes_to_latest_overlapping_session() {
        let s = vec![("a".to_string(), 0, 100), ("b".to_string(), 50, 60)];
        assert_eq!(owning_session(&s, 55), Some("b"));
        assert_eq!(owning_session(&s, 60 + COMMIT_WINDOW_MS), Some("b"));
        assert_eq!(owning_session(&s, 100 + COMMIT_WINDOW_MS + 1), None);
        assert_eq!(owning_session(&s, -1), None);
    }
}
