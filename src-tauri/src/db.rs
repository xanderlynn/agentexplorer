use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

/// Each entry upgrades the schema by one version. Append only; never edit a
/// migration that has shipped.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY,           -- "<source>:<external id>"
    source              TEXT NOT NULL,
    external_id         TEXT NOT NULL,
    file_path           TEXT NOT NULL,
    project_path        TEXT,
    repo_root           TEXT,
    git_branch          TEXT,
    title               TEXT,
    first_prompt        TEXT,
    entrypoint          TEXT,
    started_at          INTEGER,
    ended_at            INTEGER,
    active_ms           INTEGER NOT NULL DEFAULT 0,
    model               TEXT,
    user_turns          INTEGER NOT NULL DEFAULT 0,
    assistant_turns     INTEGER NOT NULL DEFAULT 0,
    tool_calls          INTEGER NOT NULL DEFAULT 0,
    tool_errors         INTEGER NOT NULL DEFAULT 0,
    interruptions       INTEGER NOT NULL DEFAULT 0,
    permission_requests INTEGER NOT NULL DEFAULT 0,
    input_tokens        INTEGER NOT NULL DEFAULT 0,
    output_tokens       INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens   INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens  INTEGER NOT NULL DEFAULT 0,
    cost_usd            REAL,
    cost_is_estimate    INTEGER NOT NULL DEFAULT 1,
    premium_requests    REAL,
    lines_added         INTEGER,
    lines_removed       INTEGER,
    last_event          TEXT,
    wait_ms_total       INTEGER NOT NULL DEFAULT 0, -- agent waiting on the human
    wait_count          INTEGER NOT NULL DEFAULT 0,
    ingested_at         INTEGER NOT NULL,
    -- Written by the user; ingestion never overwrites these.
    outcome             TEXT CHECK (outcome IN ('shipped','partial','abandoned','redo')),
    rating              INTEGER CHECK (rating BETWEEN 1 AND 5),
    notes               TEXT,
    tags                TEXT NOT NULL DEFAULT '[]',
    reviewed_at         INTEGER
);
CREATE INDEX sessions_started ON sessions(started_at);
CREATE INDEX sessions_repo ON sessions(repo_root);

CREATE TABLE prompts (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    idx        INTEGER NOT NULL,
    ts         INTEGER,
    text       TEXT NOT NULL,
    PRIMARY KEY (session_id, idx)
);

-- Change detection for each source unit (a transcript plus its sub-files).
CREATE TABLE source_files (
    unit_key    TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    session_id  TEXT,
    ingested_at INTEGER NOT NULL
);

CREATE TABLE intents (
    id               INTEGER PRIMARY KEY,
    created_at       INTEGER NOT NULL,
    goal             TEXT NOT NULL,
    task_type        TEXT NOT NULL,
    project_path     TEXT,
    tool             TEXT,
    expected_minutes INTEGER NOT NULL,
    confidence       INTEGER NOT NULL CHECK (confidence BETWEEN 1 AND 5),
    session_id       TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    outcome          TEXT CHECK (outcome IN ('shipped','partial','abandoned','redo')),
    actual_minutes   INTEGER,
    reflection       TEXT,
    closed_at        INTEGER
);

CREATE TABLE playbooks (
    id           INTEGER PRIMARY KEY,
    kind         TEXT NOT NULL,     -- claude_md | agents_md | copilot_instructions | prompt
    name         TEXT NOT NULL,
    path         TEXT UNIQUE,       -- NULL for prompt templates
    project_root TEXT,              -- NULL means it applies everywhere
    created_at   INTEGER NOT NULL
);

CREATE TABLE playbook_versions (
    id          INTEGER PRIMARY KEY,
    playbook_id INTEGER NOT NULL REFERENCES playbooks(id) ON DELETE CASCADE,
    hash        TEXT NOT NULL,
    content     TEXT NOT NULL,
    first_seen  INTEGER NOT NULL,
    git_sha     TEXT,
    UNIQUE (playbook_id, hash)
);

CREATE TABLE session_playbooks (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    version_id INTEGER NOT NULL REFERENCES playbook_versions(id) ON DELETE CASCADE,
    link       TEXT NOT NULL CHECK (link IN ('auto','manual')),
    PRIMARY KEY (session_id, version_id)
);

CREATE TABLE session_commits (
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    repo_root    TEXT NOT NULL,
    sha          TEXT NOT NULL,
    committed_at INTEGER NOT NULL,
    subject      TEXT NOT NULL,
    insertions   INTEGER NOT NULL DEFAULT 0,
    deletions    INTEGER NOT NULL DEFAULT 0,
    files        INTEGER NOT NULL DEFAULT 0,
    ai_trailer   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (session_id, sha)
);

CREATE TABLE retros (
    id           INTEGER PRIMARY KEY,
    created_at   INTEGER NOT NULL,
    period_start INTEGER NOT NULL,
    period_end   INTEGER NOT NULL,
    engine       TEXT NOT NULL,
    model        TEXT,
    stats_json   TEXT NOT NULL,
    content_md   TEXT NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#,
    // A version can be in effect more than once (A, then B, then A again),
    // so record each time a version was seen rather than only the first.
    r#"
CREATE TABLE playbook_sightings (
    version_id INTEGER NOT NULL REFERENCES playbook_versions(id) ON DELETE CASCADE,
    seen_at    INTEGER NOT NULL,
    PRIMARY KEY (version_id, seen_at)
);
INSERT INTO playbook_sightings (version_id, seen_at) SELECT id, first_seen FROM playbook_versions;
"#,
    // Execution-quality signals. Clearing source_files makes the next scan
    // re-parse every transcript so existing sessions get the new columns;
    // reviews and notes are kept because the upsert never touches them.
    r#"
ALTER TABLE sessions ADD COLUMN tool_counts TEXT NOT NULL DEFAULT '{}';
ALTER TABLE sessions ADD COLUMN reads INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN edits INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN shell_calls INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN edits_without_read INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN re_edits INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN compactions INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN subagents INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN corrections INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN category TEXT;
ALTER TABLE sessions ADD COLUMN last_message TEXT;
DELETE FROM source_files;
"#,
];

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(10))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn setting_bool(conn: &Connection, key: &str, default: bool) -> bool {
    match get_setting(conn, key) {
        Ok(Some(v)) => v == "true",
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        open(&path).unwrap();
        let conn = open(&path).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
        set_setting(&conn, "a", "1").unwrap();
        set_setting(&conn, "a", "2").unwrap();
        assert_eq!(get_setting(&conn, "a").unwrap().as_deref(), Some("2"));
    }
}
