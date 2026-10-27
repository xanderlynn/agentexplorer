//! Links sessions to the commits they produced, using the `git` CLI.

use std::collections::HashMap;
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
    /// Paths the commit touched.
    pub paths: Vec<String>,
    /// The sha this commit reverts, from git's "This reverts commit …".
    pub reverts: Option<String>,
}

const RECORD: char = '\u{1e}';
const FIELD: char = '\u{1f}';
const END: char = '\u{1d}';

/// A later commit counts as rework of an earlier one within this window.
pub const REWORK_WINDOW_MS: i64 = 7 * 24 * 3_600_000;

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
            "--numstat",
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
            // --numstat: "<added>\t<deleted>\t<path>"; binary files show "-".
            let (mut insertions, mut deletions) = (0, 0);
            let mut paths = vec![];
            for line in stats.lines() {
                let mut cols = line.splitn(3, '\t');
                let (Some(a), Some(d), Some(p)) = (cols.next(), cols.next(), cols.next()) else {
                    continue;
                };
                insertions += a.trim().parse::<i64>().unwrap_or(0);
                deletions += d.trim().parse::<i64>().unwrap_or(0);
                paths.push(p.trim().to_string());
            }
            let reverts = body
                .split("this reverts commit ")
                .nth(1)
                .map(|rest| {
                    rest.chars()
                        .take_while(|c| c.is_ascii_hexdigit())
                        .collect::<String>()
                })
                .filter(|s| s.len() >= 7);
            Some(Commit {
                sha,
                committed_at,
                author_email,
                subject,
                insertions,
                deletions,
                files: paths.len() as i64,
                ai_trailer: is_ai_commit(&body),
                paths,
                reverts,
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

/// Subjects that mark a follow-up correction rather than new work.
fn is_fix_subject(subject: &str) -> bool {
    let words: Vec<String> = subject
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    words.iter().any(|w| {
        matches!(
            w.as_str(),
            "fix"
                | "fixes"
                | "fixed"
                | "hotfix"
                | "revert"
                | "oops"
                | "typo"
                | "regression"
                | "broken"
                | "followup"
        )
    }) || subject.to_lowercase().contains("follow-up")
}

/// For each commit, the later commit that reworked it: a revert of it, or a
/// fix-like commit touching one of its files within `REWORK_WINDOW_MS`, made
/// outside the session that produced it (iterating inside a session is not
/// rework). `owner` maps a sha to its session.
pub fn find_rework<'a>(
    commits: &'a [Commit],
    owner: &dyn Fn(&str) -> Option<&'a str>,
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for c in commits {
        let Some(c_session) = owner(&c.sha) else {
            continue;
        };
        let hit = commits.iter().find(|l| {
            l.sha != c.sha
                && l.committed_at > c.committed_at
                && l.committed_at <= c.committed_at + REWORK_WINDOW_MS
                && owner(&l.sha) != Some(c_session)
                && (l
                    .reverts
                    .as_deref()
                    .is_some_and(|r| c.sha.starts_with(r) || r.starts_with(&c.sha))
                    || (is_fix_subject(&l.subject) && l.paths.iter().any(|p| c.paths.contains(p))))
        });
        if let Some(l) = hit {
            out.insert(c.sha.clone(), l.sha.clone());
        }
    }
    out
}

/// The repository's shared git directory. Worktrees of one repository have
/// different top-levels but the same common dir.
pub fn common_dir(repo: &str) -> Option<String> {
    git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// Recomputes commit links for every session in `roots`, which must be the
/// worktrees of one repository. Each commit by the configured author goes to
/// the most recent session (in any of those worktrees) that started before it
/// and was active within `COMMIT_WINDOW_MS`, so a commit is counted once.
pub fn link_commits(conn: &Connection, roots: &[String]) -> Result<usize> {
    let mut sessions: Vec<(String, i64, i64)> = vec![];
    let mut root_of: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, started_at, ended_at FROM sessions
             WHERE repo_root = ?1 AND started_at IS NOT NULL AND ended_at IS NOT NULL",
        )?;
        for root in roots {
            for row in stmt.query_map([root], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))? {
                let row: (String, i64, i64) = row?;
                root_of.insert(row.0.clone(), root.clone());
                sessions.push(row);
            }
        }
    }
    sessions.sort_by_key(|s| s.1);

    // Run git before taking the write lock; it can take seconds on big repos.
    let commits = match (roots.first(), sessions.first()) {
        (Some(repo), Some(first)) => read_commits(repo, first.1)?,
        _ => vec![],
    };
    let email = roots
        .first()
        .and_then(|r| git(r, &["config", "user.email"]).ok())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();

    // Decide ownership first so rework can look across all commits.
    let owners: HashMap<String, String> = commits
        .iter()
        .filter(|c| email.is_empty() || c.author_email == email)
        .filter_map(|c| {
            owning_session(&sessions, c.committed_at).map(|o| (c.sha.clone(), o.to_string()))
        })
        .collect();
    let rework = find_rework(&commits, &|sha| owners.get(sha).map(String::as_str));

    let tx = conn.unchecked_transaction()?;
    for root in roots {
        tx.execute("DELETE FROM session_commits WHERE repo_root = ?1", [root])?;
    }
    let mut linked = 0;
    {
        let mut insert = tx.prepare(
            "INSERT OR IGNORE INTO session_commits
             (session_id, repo_root, sha, committed_at, subject, insertions, deletions, files, ai_trailer,
              reworked, rework_sha)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for c in &commits {
            if let Some(owner) = owners.get(&c.sha).map(String::as_str) {
                insert.execute(params![
                    owner,
                    root_of[owner],
                    c.sha,
                    c.committed_at,
                    c.subject,
                    c.insertions,
                    c.deletions,
                    c.files,
                    c.ai_trailer,
                    rework.contains_key(&c.sha),
                    rework.get(&c.sha)
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
            "{RECORD}abc{FIELD}1700000000{FIELD}Me@X.com{FIELD}Add login{FIELD}body\n\nCo-Authored-By: Claude Opus <x>\n{END}\n\n7\t1\tsrc/a.rs\n3\t0\tsrc/b.rs\n-\t-\tlogo.png\n\
             {RECORD}def{FIELD}1700000100{FIELD}me@x.com{FIELD}Revert \"Add login\"{FIELD}This reverts commit abc1234.{END}\n\n0\t7\tsrc/a.rs\n"
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
            (3, 10, 1)
        );
        assert!(commits[0].ai_trailer);
        assert_eq!(commits[0].paths, ["src/a.rs", "src/b.rs", "logo.png"]);
        assert_eq!(commits[1].reverts.as_deref(), Some("abc1234"));
    }

    fn commit(sha: &str, t: i64, subject: &str, paths: &[&str]) -> Commit {
        Commit {
            sha: sha.into(),
            committed_at: t,
            author_email: "me@x.com".into(),
            subject: subject.into(),
            insertions: 1,
            deletions: 0,
            files: paths.len() as i64,
            ai_trailer: false,
            paths: paths.iter().map(|p| p.to_string()).collect(),
            reverts: None,
        }
    }

    #[test]
    fn finds_rework_outside_the_session() {
        let day = 86_400_000;
        let commits = vec![
            commit("a1", 0, "Add export", &["src/export.rs"]),
            commit("a2", 1000, "Fix export header", &["src/export.rs"]),
            commit(
                "b1",
                2 * day,
                "Fix export for empty reports",
                &["src/export.rs"],
            ),
            commit("c1", 3 * day, "Add docs", &["README.md"]),
            commit("c2", 20 * day, "Fix docs typo", &["README.md"]),
        ];
        let owner = |sha: &str| -> Option<&str> {
            match sha {
                "a1" | "a2" => Some("s1"),
                "b1" => Some("s2"),
                "c1" => Some("s3"),
                _ => None,
            }
        };
        let rework = find_rework(&commits, &owner);
        // a2 is iteration inside s1, but b1 (another session, 2 days later)
        // fixes the same file.
        assert_eq!(rework.get("a1").map(String::as_str), Some("b1"));
        // c2 is outside the 7-day window.
        assert!(!rework.contains_key("c1"));
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
