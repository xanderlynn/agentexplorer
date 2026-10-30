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
    // Marks which sessions carry the signals above; 0 means "parsed before
    // they existed" (for example, a transcript Claude Code already deleted).
    r#"
ALTER TABLE sessions ADD COLUMN signals_version INTEGER NOT NULL DEFAULT 0;
DELETE FROM source_files;
"#,
    // The improvement loop: one-tap reviews, pre-flight intents, experiments,
    // goals and structured retros. ask_score needs a re-parse.
    r#"
ALTER TABLE sessions ADD COLUMN ask_score INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN vs_manual TEXT CHECK (vs_manual IN ('faster','slower','same','unsure'));
ALTER TABLE intents ADD COLUMN done_when TEXT;
ALTER TABLE intents ADD COLUMN context TEXT;
ALTER TABLE retros ADD COLUMN reflection TEXT;
ALTER TABLE retros ADD COLUMN experiments_json TEXT;
ALTER TABLE session_commits ADD COLUMN reworked INTEGER NOT NULL DEFAULT 0;
ALTER TABLE session_commits ADD COLUMN rework_sha TEXT;

CREATE TABLE IF NOT EXISTS experiments (
    id             INTEGER PRIMARY KEY,
    created_at     INTEGER NOT NULL,
    title          TEXT NOT NULL,
    hypothesis     TEXT,
    metric         TEXT NOT NULL,
    category       TEXT,
    source         TEXT,
    start_at       INTEGER NOT NULL,
    end_at         INTEGER NOT NULL,
    baseline_value REAL,
    baseline_n     INTEGER NOT NULL DEFAULT 0,
    status         TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','kept','dropped')),
    verdict_note   TEXT,
    retro_id       INTEGER REFERENCES retros(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS goals (
    id             INTEGER PRIMARY KEY,
    created_at     INTEGER NOT NULL,
    metric         TEXT NOT NULL,
    target         REAL NOT NULL,
    counter_metric TEXT NOT NULL,
    active         INTEGER NOT NULL DEFAULT 1
);

DELETE FROM source_files;
"#,
    // Reality checks: per-tool errors and vetoes, agent working time, and
    // the working / waiting timeline. Needs a re-parse.
    r#"
ALTER TABLE sessions ADD COLUMN tool_errors_by TEXT NOT NULL DEFAULT '{}';
ALTER TABLE sessions ADD COLUMN tool_vetoes TEXT NOT NULL DEFAULT '{}';
ALTER TABLE sessions ADD COLUMN agent_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN timeline TEXT NOT NULL DEFAULT '[]';
DELETE FROM source_files;
"#,
    // Execution style: time per mode, plan use and context size.
    r#"
ALTER TABLE sessions ADD COLUMN mode_ms TEXT NOT NULL DEFAULT '{}';
ALTER TABLE sessions ADD COLUMN planned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN peak_context INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN last_context INTEGER NOT NULL DEFAULT 0;
DELETE FROM source_files;
"#,
    // Live polling and timelines filter on ended_at; Execution and the
    // session list look up intents by session.
    r#"
CREATE INDEX IF NOT EXISTS sessions_ended ON sessions(ended_at);
CREATE INDEX IF NOT EXISTS intents_session ON intents(session_id);
"#,
    // Sub-agents, their messages and scheduled work, as JSON.
    r#"
ALTER TABLE sessions ADD COLUMN team TEXT NOT NULL DEFAULT '{}';
"#,
    // Skills and MCP servers used, as JSON.
    r#"
ALTER TABLE sessions ADD COLUMN toolkit TEXT NOT NULL DEFAULT '{}';
"#,
    // Live status: the tool the lead is waiting on, when its turn began,
    // and whether the latest turn failed.
    r#"
ALTER TABLE sessions ADD COLUMN pending_tool TEXT;
ALTER TABLE sessions ADD COLUMN turn_started_at INTEGER;
ALTER TABLE sessions ADD COLUMN failed INTEGER NOT NULL DEFAULT 0;
"#,
    r#"
ALTER TABLE sessions ADD COLUMN tasks TEXT NOT NULL DEFAULT '{}';
"#,
    r#"
CREATE TABLE task_kinds (
    task_key   TEXT PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('agent','human')),
    updated_at INTEGER NOT NULL
);
"#,
    // Your workstreams: stars, names and the ones you made, the sessions you
    // put in them, and the branches whose sessions join them.
    r#"
CREATE TABLE workstream_prefs (
    ws_key     TEXT PRIMARY KEY,
    title      TEXT,
    starred    INTEGER NOT NULL DEFAULT 0,
    custom     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE TABLE workstream_members (
    session_id TEXT PRIMARY KEY,
    ws_key     TEXT NOT NULL
);
CREATE TABLE workstream_branches (
    repo   TEXT NOT NULL,
    branch TEXT NOT NULL,
    ws_key TEXT NOT NULL,
    PRIMARY KEY (repo, branch)
);
"#,
    // Workstreams imported from an issue tracker's epic, and its items.
    r#"
ALTER TABLE workstream_prefs ADD COLUMN source_url TEXT;
ALTER TABLE workstream_prefs ADD COLUMN synced_at INTEGER;
CREATE TABLE workstream_items (
    ws_key   TEXT NOT NULL,
    item_key TEXT NOT NULL,
    position INTEGER NOT NULL,
    title    TEXT NOT NULL,
    status   TEXT NOT NULL,
    url      TEXT,
    labels   TEXT NOT NULL DEFAULT '[]',
    assignee TEXT,
    PRIMARY KEY (ws_key, item_key)
);
"#,
    // Why an imported workstream's last fetch failed, and when it was tried.
    r#"
ALTER TABLE workstream_prefs ADD COLUMN sync_error TEXT;
ALTER TABLE workstream_prefs ADD COLUMN sync_tried_at INTEGER;
"#,
    // Nested epic items, which are yours, the epic's own title, state and
    // repository, and which sessions worked on which item.
    r#"
ALTER TABLE workstream_items ADD COLUMN parent TEXT;
ALTER TABLE workstream_items ADD COLUMN depth INTEGER NOT NULL DEFAULT 0;
ALTER TABLE workstream_items ADD COLUMN mine INTEGER NOT NULL DEFAULT 0;
ALTER TABLE workstream_prefs ADD COLUMN source_title TEXT;
ALTER TABLE workstream_prefs ADD COLUMN source_closed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE workstream_prefs ADD COLUMN source_repo TEXT;
CREATE TABLE workstream_item_sessions (
    ws_key     TEXT NOT NULL,
    item_key   TEXT NOT NULL,
    session_id TEXT NOT NULL,
    PRIMARY KEY (ws_key, item_key, session_id)
);
"#,
    // Permission prompts Claude Code's hook reported, newest per session.
    r#"
CREATE TABLE hook_events (
    external_id TEXT NOT NULL,
    kind        TEXT NOT NULL,
    detail      TEXT,
    at          INTEGER NOT NULL,
    PRIMARY KEY (external_id, kind)
);
"#,
    // Pull requests each session opened or worked on, as JSON.
    r#"
ALTER TABLE sessions ADD COLUMN prs TEXT NOT NULL DEFAULT '[]';
"#,
    // What was imported: an issue, or a work item's type (Epic, User Story, Task…).
    r#"
ALTER TABLE workstream_prefs ADD COLUMN source_kind TEXT;
"#,
    // Sessions you dismissed from "waiting on you", until they move again.
    r#"
CREATE TABLE dismissed (
    session_id TEXT PRIMARY KEY,
    at         INTEGER NOT NULL
);
"#,
    // Whether each pull request a session opened was merged, from gh.
    r#"
CREATE TABLE pr_states (
    url        TEXT PRIMARY KEY,
    state      TEXT NOT NULL,
    checked_at INTEGER NOT NULL
);
"#,
];

pub fn open(path: &Path) -> Result<Connection> {
    crate::util::prepare_private_file(path)?;
    let conn = Connection::open(path)?;
    configure(&conn)?;
    restrict_sqlite_files(path)?;
    migrate(&conn)?;
    Ok(conn)
}

fn restrict_sqlite_files(path: &Path) -> Result<()> {
    crate::util::restrict_private_if_exists(path, false)?;
    let path = path.as_os_str().to_string_lossy();
    for suffix in ["-wal", "-shm", "-journal"] {
        crate::util::restrict_private_if_exists(Path::new(&format!("{path}{suffix}")), false)?;
    }
    Ok(())
}

/// A fresh database in memory, for tests.
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

/// Columns added by migrations after the first, with their definitions.
/// `repair` adds any that are missing, so a database that ran an earlier
/// draft of a migration (as a development build can) still gets every
/// column. Keep in step with MIGRATIONS.
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    ("sessions", "tool_counts", "TEXT NOT NULL DEFAULT '{}'"),
    ("sessions", "reads", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "edits", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "shell_calls", "INTEGER NOT NULL DEFAULT 0"),
    (
        "sessions",
        "edits_without_read",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    ("sessions", "re_edits", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "compactions", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "subagents", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "corrections", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "category", "TEXT"),
    ("sessions", "last_message", "TEXT"),
    ("sessions", "signals_version", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "ask_score", "INTEGER NOT NULL DEFAULT 0"),
    (
        "sessions",
        "vs_manual",
        "TEXT CHECK (vs_manual IN ('faster','slower','same','unsure'))",
    ),
    ("sessions", "tool_errors_by", "TEXT NOT NULL DEFAULT '{}'"),
    ("sessions", "tool_vetoes", "TEXT NOT NULL DEFAULT '{}'"),
    ("sessions", "agent_ms", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "timeline", "TEXT NOT NULL DEFAULT '[]'"),
    ("sessions", "mode_ms", "TEXT NOT NULL DEFAULT '{}'"),
    ("sessions", "planned", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "peak_context", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "last_context", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "team", "TEXT NOT NULL DEFAULT '{}'"),
    ("sessions", "toolkit", "TEXT NOT NULL DEFAULT '{}'"),
    ("sessions", "pending_tool", "TEXT"),
    ("sessions", "turn_started_at", "INTEGER"),
    ("sessions", "failed", "INTEGER NOT NULL DEFAULT 0"),
    ("sessions", "tasks", "TEXT NOT NULL DEFAULT '{}'"),
    ("sessions", "prs", "TEXT NOT NULL DEFAULT '[]'"),
    ("intents", "done_when", "TEXT"),
    ("intents", "context", "TEXT"),
    ("retros", "reflection", "TEXT"),
    ("retros", "experiments_json", "TEXT"),
    ("session_commits", "reworked", "INTEGER NOT NULL DEFAULT 0"),
    ("session_commits", "rework_sha", "TEXT"),
];

fn repair(conn: &Connection) -> Result<()> {
    let mut added = false;
    for (table, column, decl) in ADDED_COLUMNS {
        if !has_column(conn, table, column)? {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl};"))?;
            added = true;
        }
    }
    if added {
        // New columns are filled by re-parsing the transcripts.
        conn.execute_batch("DELETE FROM source_files;")?;
    }
    Ok(())
}

/// Runs pending migrations statement by statement. An `ALTER TABLE … ADD
/// COLUMN` whose column already exists is skipped, so a database that ran
/// an earlier draft of a migration (as a development build can) upgrades
/// instead of failing with "duplicate column".
fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        for stmt in sql.split(";\n").map(str::trim).filter(|s| !s.is_empty()) {
            if let Some((table, column)) = added_column(stmt)
                && has_column(&tx, table, column)?
            {
                continue;
            }
            tx.execute_batch(stmt)?;
        }
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    repair(conn)
}

/// `ALTER TABLE <table> ADD COLUMN <column> …` → (table, column).
fn added_column(stmt: &str) -> Option<(&str, &str)> {
    let mut words = stmt.split_whitespace();
    let pattern = ["ALTER", "TABLE"];
    if !pattern
        .iter()
        .all(|p| words.next().is_some_and(|w| w.eq_ignore_ascii_case(p)))
    {
        return None;
    }
    let table = words.next()?;
    let add = words.next()?;
    let col_kw = words.next()?;
    if !add.eq_ignore_ascii_case("ADD") || !col_kw.eq_ignore_ascii_case("COLUMN") {
        return None;
    }
    Some((table, words.next()?))
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"),
        [column],
        |r| r.get(0),
    )?)
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
    #[cfg(unix)]
    use std::path::PathBuf;

    #[test]
    fn repair_adds_columns_a_draft_migration_missed() {
        let conn = open_in_memory().unwrap();
        // Simulate a database that ran a draft migration without this column.
        conn.execute_batch("ALTER TABLE session_commits DROP COLUMN rework_sha;")
            .unwrap();
        repair(&conn).unwrap();
        conn.query_row("SELECT rework_sha FROM session_commits LIMIT 1", [], |_| {
            Ok(())
        })
        .ok();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('session_commits') WHERE name = 'rework_sha'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn upgrades_a_database_that_ran_a_draft_migration() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        // Apply migrations up to 5, then add a column migration 6 also adds,
        // as a draft of migration 5 might have.
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN agent_ms INTEGER NOT NULL DEFAULT 0;")
            .unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
        assert!(has_column(&conn, "sessions", "timeline").unwrap());
    }

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

    #[cfg(unix)]
    #[test]
    fn database_and_sidecars_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let _conn = open(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
            if sidecar.exists() {
                assert_eq!(
                    std::fs::metadata(sidecar).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }
}
