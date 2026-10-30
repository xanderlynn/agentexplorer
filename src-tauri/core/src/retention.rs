//! How much history Tilly keeps. A year of a power user's sessions is cheap
//! as summaries but heavy in detail, and the raw transcript archive is far
//! bigger than the database. So:
//! - every session's summary is kept forever (what trends and progress use);
//! - its detail (prompts, team, timeline, task list, last reply) is compacted
//!   after `DETAIL_DAYS_KEY` days, 180 by default;
//! - archived transcripts are compressed once they stop changing and deleted
//!   after `ARCHIVE_DAYS_KEY` days, 90 by default.
//!
//! Log file records (source_files) are never touched, or logs a tool keeps
//! forever (Copilot's) would be read back in.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::stats::DAY_MS;
use crate::util::now_ms;

/// Settings: days of session detail and of archived transcripts to keep;
/// 0 keeps them forever.
pub const DETAIL_DAYS_KEY: &str = "history.detailDays";
pub const ARCHIVE_DAYS_KEY: &str = "archive.days";
pub const DETAIL_DAYS_DEFAULT: i64 = 180;
pub const ARCHIVE_DAYS_DEFAULT: i64 = 90;
/// An archived transcript unchanged this long is compressed.
const COMPRESS_AFTER: Duration = Duration::from_secs(7 * 86_400);

fn days(conn: &Connection, key: &str, default: i64) -> i64 {
    db::get_setting(conn, key)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Drops the detail of sessions that ended over `detail_days` ago, keeping
/// their summary. Returns how many were compacted.
pub fn compact(conn: &Connection, detail_days: i64) -> Result<usize> {
    if detail_days <= 0 {
        return Ok(0);
    }
    let cut = now_ms() - detail_days * DAY_MS;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM prompts WHERE session_id IN (SELECT id FROM sessions WHERE ended_at < ?1)",
        [cut],
    )?;
    let n = tx.execute(
        "UPDATE sessions SET team = '{}', timeline = '[]', tasks = '{}', toolkit = '{}',
             tool_errors_by = '{}', tool_vetoes = '{}', last_message = NULL,
             first_prompt = substr(first_prompt, 1, 200)
         WHERE ended_at < ?1 AND (team != '{}' OR timeline != '[]' OR tasks != '{}'
             OR last_message IS NOT NULL OR length(first_prompt) > 200)",
        [cut],
    )?;
    tx.commit()?;
    Ok(n)
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                files(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

fn age(p: &Path) -> Duration {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .unwrap_or_default()
}

/// Compresses archived transcripts that have stopped changing.
pub fn compress_archive(data_dir: &Path) -> Result<usize> {
    let mut all = vec![];
    files(&data_dir.join("archive"), &mut all);
    let mut n = 0;
    for p in all
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
    {
        if age(p) < COMPRESS_AFTER {
            continue;
        }
        let gz = gz_path(p);
        let mut raw = vec![];
        std::fs::File::open(p)?.read_to_end(&mut raw)?;
        let mut enc = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
        enc.write_all(&raw)?;
        crate::util::write_private(&gz, &enc.finish()?)?;
        std::fs::remove_file(p)?;
        n += 1;
    }
    Ok(n)
}

/// `x.jsonl` → `x.jsonl.gz`.
pub fn gz_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".gz");
    PathBuf::from(s)
}

/// The plain text of a compressed archived transcript.
pub fn read_gz(gz: &Path) -> Result<Vec<u8>> {
    let mut out = vec![];
    flate2::read::GzDecoder::new(std::fs::File::open(gz)?).read_to_end(&mut out)?;
    Ok(out)
}

/// Deletes archived sessions not touched in `archive_days` days.
pub fn prune_archive(data_dir: &Path, archive_days: i64) -> Result<usize> {
    if archive_days <= 0 {
        return Ok(0);
    }
    let keep = Duration::from_secs(archive_days as u64 * 86_400);
    let mut n = 0;
    for source in std::fs::read_dir(data_dir.join("archive"))
        .into_iter()
        .flatten()
        .flatten()
    {
        for session in std::fs::read_dir(source.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let mut fs = vec![];
            files(&session.path(), &mut fs);
            // Its newest file decides: a session still growing stays.
            if fs.iter().map(|f| age(f)).min().unwrap_or_default() > keep {
                std::fs::remove_dir_all(session.path())?;
                n += 1;
            }
        }
    }
    Ok(n)
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub db_bytes: u64,
    pub archive_bytes: u64,
    pub archive_sessions: usize,
    pub sessions: i64,
    /// Sessions whose detail is still kept.
    pub detailed: i64,
    pub detail_days: i64,
    pub archive_days: i64,
    pub archive_enabled: bool,
}

pub fn storage(conn: &Connection, data_dir: &Path, db_path: &Path) -> Result<Storage> {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let mut wal = db_path.as_os_str().to_owned();
    wal.push("-wal");
    let mut all = vec![];
    files(&data_dir.join("archive"), &mut all);
    let archive_sessions = std::fs::read_dir(data_dir.join("archive"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|s| std::fs::read_dir(s.path()).map(|r| r.count()).unwrap_or(0))
        .sum();
    Ok(Storage {
        db_bytes: size(db_path) + size(Path::new(&wal)),
        archive_bytes: all.iter().map(|p| size(p)).sum(),
        archive_sessions,
        sessions: conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?,
        detailed: conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE timeline != '[]' OR team != '{}' OR last_message IS NOT NULL",
            [],
            |r| r.get(0),
        )?,
        detail_days: days(conn, DETAIL_DAYS_KEY, DETAIL_DAYS_DEFAULT),
        archive_days: days(conn, ARCHIVE_DAYS_KEY, ARCHIVE_DAYS_DEFAULT),
        archive_enabled: db::setting_bool(conn, "archive.enabled", true),
    })
}

/// What a maintenance pass did.
#[derive(Debug, Default)]
pub struct Pass {
    pub compacted: usize,
    pub compressed: usize,
    pub pruned: usize,
}

/// The daily pass: compact old detail, compress and prune the archive, and
/// reclaim the space when much was freed.
pub fn maintain(conn: &Connection, data_dir: &Path) -> Result<Pass> {
    let pass = Pass {
        compacted: compact(conn, days(conn, DETAIL_DAYS_KEY, DETAIL_DAYS_DEFAULT))?,
        compressed: compress_archive(data_dir)?,
        pruned: prune_archive(data_dir, days(conn, ARCHIVE_DAYS_KEY, ARCHIVE_DAYS_DEFAULT))?,
    };
    if pass.compacted > 500 {
        conn.execute_batch("VACUUM")?;
    }
    db::set_setting(conn, "retention.lastRun", &now_ms().to_string())?;
    Ok(pass)
}

/// Whether the daily pass is due.
pub fn due(conn: &Connection) -> bool {
    let last: i64 = db::get_setting(conn, "retention.lastRun")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    now_ms() - last > DAY_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn compacts_old_detail_but_keeps_summaries() {
        let conn = crate::db::open_in_memory().unwrap();
        let now = now_ms();
        for (id, age_days) in [("old", 200), ("new", 10)] {
            let t = now - age_days * DAY_MS;
            conn.execute(
                "INSERT INTO sessions (id, source, external_id, file_path, started_at, ended_at, active_ms, user_turns,
                   team, timeline, last_message, first_prompt, cost_usd, outcome, ingested_at)
                 VALUES (?1, 'claude_code', ?1, '/f', ?2, ?2, 60000, 3, '{\"agents\":[1]}', '[[1,2,0]]', 'done',
                   ?3, 2.5, 'shipped', ?2)",
                params![id, t, "x".repeat(900)],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO prompts (session_id, idx, ts, text) VALUES (?1, 0, ?2, 'hi')",
                params![id, t],
            )
            .unwrap();
        }
        assert_eq!(compact(&conn, 180).unwrap(), 1);
        assert_eq!(compact(&conn, 180).unwrap(), 0, "only once");
        let (team, prompt_len, cost, outcome): (String, i64, f64, String) = conn
            .query_row(
                "SELECT team, length(first_prompt), cost_usd, outcome FROM sessions WHERE id = 'old'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (team.as_str(), prompt_len, cost, outcome.as_str()),
            ("{}", 200, 2.5, "shipped")
        );
        let prompts: i64 = conn
            .query_row("SELECT COUNT(*) FROM prompts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(prompts, 1, "the new session's prompt stays");
        assert_eq!(compact(&conn, 0).unwrap(), 0, "0 keeps everything");
    }

    #[test]
    fn compresses_and_prunes_the_archive() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("archive/claude_code/abc");
        std::fs::create_dir_all(&s).unwrap();
        let f = s.join("abc.jsonl");
        std::fs::write(&f, "{\"a\":1}\n".repeat(1000)).unwrap();
        // Fresh: left alone.
        assert_eq!(compress_archive(dir.path()).unwrap(), 0);
        let old = SystemTime::now() - Duration::from_secs(10 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(compress_archive(dir.path()).unwrap(), 1);
        assert!(!f.exists());
        assert_eq!(
            read_gz(&gz_path(&f)).unwrap(),
            "{\"a\":1}\n".repeat(1000).into_bytes()
        );
        // Kept 90 days: a 10-day-old session stays; at 5 days' retention it goes.
        std::fs::File::options()
            .write(true)
            .open(gz_path(&f))
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(prune_archive(dir.path(), 90).unwrap(), 0);
        assert_eq!(prune_archive(dir.path(), 5).unwrap(), 1);
        assert!(!s.exists());
    }
}
