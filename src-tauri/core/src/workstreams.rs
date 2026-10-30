//! Workstreams: larger efforts made of many sessions. Sessions on the same
//! repository and feature branch form one workstream; sessions on a default
//! branch stand alone, and only count when their agents kept a task list or
//! a team. Each workstream merges its sessions' task lists into one tree
//! (session → tasks), marks every task as agent or human work, and gathers
//! the agents' updates.
//!
//! You can also shape them: star one to keep it listed whatever its age,
//! make your own (or import one from an issue tracker's epic, whose items
//! become its tasks), rename them, and put sessions in them. A session you
//! put in a workstream brings its feature branch along, so later sessions on
//! that branch join it too.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::live::{self, Group, LiveStatus};
use crate::tasks::{TaskList, TaskStatus};
use crate::team::Team;
use crate::trackers::Epic;
use crate::util::{NotFound, now_ms};

/// Branches that hold everyone's work, so they don't define an effort.
const DEFAULT_BRANCHES: &[&str] = &["main", "master", "trunk", "develop", "dev", "HEAD"];
/// Updates kept per workstream, newest first.
const MAX_UPDATES: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Agent,
    Human,
}

impl TaskKind {
    pub const ALL: [TaskKind; 2] = [TaskKind::Agent, TaskKind::Human];
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsTask {
    /// `<session id>:<task id>`, or `item:<workstream>:<item>` for an item
    /// from the tracker; the key overrides are stored under.
    pub key: String,
    /// Empty for an item from the tracker.
    pub session_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub kind: TaskKind,
    /// Why it is human work, or "set by you" for an override.
    pub reason: Option<String>,
    pub agent: Option<String>,
    /// Keys of unfinished tasks it waits for.
    pub blocked_by: Vec<String>,
    pub updated_at: Option<i64>,
    /// The tracker item's page.
    pub url: Option<String>,
    /// A tracker item assigned to you.
    pub mine: bool,
    /// The tracker item it sits under, and how deep; none and 0 at the top.
    pub parent: Option<String>,
    pub depth: u8,
    /// Sessions that worked on this tracker item.
    pub sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsSession {
    pub id: String,
    pub source: String,
    pub entrypoint: Option<String>,
    pub title: Option<String>,
    pub status: LiveStatus,
    pub ended_at: i64,
    /// What it cost and delivered, for the workstream's and items' totals.
    pub active_ms: i64,
    pub cost_usd: Option<f64>,
    pub cost_is_estimate: bool,
    pub tokens: i64,
    pub commits: i64,
    pub prs: Vec<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsAgent {
    pub session_id: String,
    /// "lead" or the sub-agent's id.
    pub id: String,
    pub name: Option<String>,
    /// "lead" or its agent type.
    pub role: String,
    pub active: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsUpdate {
    pub at: Option<i64>,
    pub session_id: String,
    pub agent: String,
    pub task_key: Option<String>,
    pub text: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsCounts {
    pub done: usize,
    pub in_progress: usize,
    /// `decisions` plus `waiting`.
    pub needs_you: usize,
    /// Human tasks not done that are yours: assigned to you, or to nobody.
    pub decisions: usize,
    /// Recent sessions waiting on you.
    pub waiting: usize,
    pub pending: usize,
    pub agent: usize,
    pub human: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workstream {
    pub key: String,
    pub title: String,
    /// Starred workstreams are listed first and whatever their age.
    pub starred: bool,
    /// You made it, rather than Tilly grouping it from a branch or session.
    pub custom: bool,
    /// The epic it was imported from, and when it was last fetched.
    pub source_url: Option<String>,
    pub synced_at: Option<i64>,
    /// Why the last fetch failed, until one succeeds.
    pub sync_error: Option<String>,
    /// The imported epic is closed in its tracker.
    pub closed: bool,
    /// The tracker's repository ("owner/repo") for an imported epic.
    pub tracker_repo: Option<String>,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub last_activity: i64,
    pub sessions: Vec<WsSession>,
    pub agents: Vec<WsAgent>,
    pub tasks: Vec<WsTask>,
    pub updates: Vec<WsUpdate>,
    pub counts: WsCounts,
}

/// Words in a task that make it work for a person, and why. Checked in order;
/// matched as whole lowercase words or phrases.
const HUMAN_RULES: &[(&[&str], &str)] = &[
    (
        &["approve", "approval", "sign off", "sign-off", "signoff"],
        "Needs your approval",
    ),
    (
        &[
            "decide on",
            "decide whether",
            "decide between",
            "decide which",
            "make a decision",
            "your decision",
            "choose between",
            "pick between",
        ],
        "Needs your decision",
    ),
    (
        &[
            "deploy to prod",
            "production deploy",
            "release to production",
            "publish the release",
            "flip the feature flag",
            "merge the pr",
        ],
        "A production or release step",
    ),
    (
        &[
            "ask the user",
            "confirm with",
            "check with the user",
            "user to provide",
            "manually",
            "by hand",
        ],
        "Needs your input",
    ),
];

/// Agent or human work for a task, and why. An override wins; then a task
/// the agent is blocked on while waiting for you; then the wording rules.
pub fn classify(
    title: &str,
    status: TaskStatus,
    session: LiveStatus,
    stored: Option<TaskKind>,
) -> (TaskKind, Option<String>) {
    if let Some(kind) = stored {
        return (kind, Some("Set by you".into()));
    }
    if status == TaskStatus::InProgress
        && matches!(session, LiveStatus::Asking | LiveStatus::NeedsPermission)
    {
        return (TaskKind::Human, Some("The agent is waiting on you".into()));
    }
    let t = title.to_lowercase();
    for (words, why) in HUMAN_RULES {
        if words.iter().any(|w| has_phrase(&t, w)) {
            return (TaskKind::Human, Some((*why).into()));
        }
    }
    if needs_credentials(&t) {
        return (TaskKind::Human, Some("Needs credentials".into()));
    }
    (TaskKind::Agent, None)
}

/// `phrase` in `text` as whole words: "secret" in "add the secret", not in
/// "secrets loader".
fn has_phrase(text: &str, phrase: &str) -> bool {
    text.match_indices(phrase).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Asking a person for a secret: a verb such as "create" or "provide" with a
/// credential within a few words ("Create staging API keys"), not code that
/// handles them ("Fix password reset").
fn needs_credentials(t: &str) -> bool {
    const VERBS: &[&str] = &[
        "provide", "add", "create", "set", "enter", "rotate", "generate", "share", "get",
    ];
    const THINGS: &[&str] = &[
        "credential",
        "credentials",
        "secret",
        "secrets",
        "api key",
        "api keys",
        "password",
        "token",
        "tokens",
        "keychain",
    ];
    let words: Vec<&str> = t
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().any(|(i, w)| {
        VERBS.contains(w) && {
            let near = words[i + 1..words.len().min(i + 5)].join(" ");
            THINGS.iter().any(|x| has_phrase(&near, x))
        }
    })
}

/// Stores (or with `None`, clears) the agent/human choice for a task.
pub fn set_kind(conn: &Connection, task_key: &str, kind: Option<TaskKind>) -> Result<()> {
    match kind {
        Some(k) => conn.execute(
            "INSERT INTO task_kinds (task_key, kind, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(task_key) DO UPDATE SET kind = excluded.kind, updated_at = excluded.updated_at",
            params![task_key, if k == TaskKind::Human { "human" } else { "agent" }, now_ms()],
        )?,
        None => conn.execute("DELETE FROM task_kinds WHERE task_key = ?1", [task_key])?,
    };
    Ok(())
}

/// "feature/apple-pay-at-checkout" → "Apple pay at checkout".
fn branch_title(branch: &str) -> String {
    let leaf = branch.rsplit('/').next().unwrap_or(branch);
    let words = leaf.replace(['-', '_'], " ");
    let mut c = words.trim().chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_else(|| branch.to_string())
}

/// The repository and feature branch, unless the branch holds everyone's work.
fn feature<'a>(repo: Option<&'a str>, branch: Option<&'a str>) -> Option<(&'a str, &'a str)> {
    repo.zip(branch.filter(|b| !DEFAULT_BRANCHES.contains(b)))
}

fn branch_key(repo: &str, branch: &str) -> String {
    format!("branch:{}:{branch}", crate::util::path_key(repo))
}

struct Row {
    id: String,
    source: String,
    entrypoint: Option<String>,
    value: Value,
    title: Option<String>,
    repo: Option<String>,
    branch: Option<String>,
    ended: i64,
    status: LiveStatus,
    team: Team,
    tasks: TaskList,
    intent: Option<String>,
    prompt: Option<String>,
}

/// A session's time, cost and output.
struct Value {
    active_ms: i64,
    cost_usd: Option<f64>,
    cost_is_estimate: bool,
    tokens: i64,
    commits: i64,
    prs: Vec<String>,
    outcome: Option<String>,
}

/// Workstreams with activity in the last `days` days, most recent first.
pub fn workstreams(conn: &Connection, days: i64) -> Result<Vec<Workstream>> {
    let now = now_ms();
    let since = if days <= 0 {
        0
    } else {
        now - days * crate::stats::DAY_MS
    };
    let mut stmt = conn.prepare(
        "SELECT s.id, s.source, s.title, COALESCE(s.repo_root, s.project_path), s.git_branch,
                s.ended_at, s.last_event, s.pending_tool, s.last_message, s.failed, s.team, s.tasks,
                (SELECT goal FROM intents i WHERE i.session_id = s.id ORDER BY i.created_at DESC LIMIT 1),
                substr(s.first_prompt, 1, 120), s.entrypoint,
                s.active_ms, s.cost_usd, s.cost_is_estimate,
                s.input_tokens + s.output_tokens + s.cache_read_tokens + s.cache_write_tokens,
                (SELECT COUNT(*) FROM session_commits c WHERE c.session_id = s.id), s.prs, s.outcome
         FROM sessions s
         WHERE s.ended_at >= ?1
            OR s.id IN (SELECT session_id FROM workstream_members)
            OR EXISTS (SELECT 1 FROM workstream_branches b
                       WHERE b.repo = COALESCE(s.repo_root, s.project_path) AND b.branch = s.git_branch)
         ORDER BY s.ended_at DESC",
    )?;
    let rows: Vec<Row> = stmt
        .query_map([since], |r| {
            let mut team: Team = serde_json::from_str(&r.get::<_, String>(10)?).unwrap_or_default();
            let ended: i64 = r.get(5)?;
            let last_event: Option<String> = r.get(6)?;
            let pending: Option<String> = r.get(7)?;
            let message: Option<String> = r.get(8)?;
            let status = live::status_of(
                &mut team,
                last_event.as_deref(),
                pending.as_deref(),
                message.as_deref(),
                r.get(9)?,
                ended,
                now,
            );
            Ok(Row {
                id: r.get(0)?,
                source: r.get(1)?,
                entrypoint: r.get(14)?,
                value: Value {
                    active_ms: r.get(15)?,
                    cost_usd: r.get(16)?,
                    cost_is_estimate: r.get(17)?,
                    tokens: r.get(18)?,
                    commits: r.get(19)?,
                    prs: serde_json::from_str(&r.get::<_, String>(20)?).unwrap_or_default(),
                    outcome: r.get(21)?,
                },
                title: r.get(2)?,
                repo: r.get(3)?,
                branch: r.get(4)?,
                ended,
                status,
                team,
                tasks: serde_json::from_str(&r.get::<_, String>(11)?).unwrap_or_default(),
                intent: r.get(12)?,
                prompt: r.get(13)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let stored: BTreeMap<String, TaskKind> = {
        let mut st = conn.prepare("SELECT task_key, kind FROM task_kinds")?;
        st.query_map([], |r| {
            let kind: String = r.get(1)?;
            Ok((
                r.get(0)?,
                if kind == "human" {
                    TaskKind::Human
                } else {
                    TaskKind::Agent
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?
    };

    let members: HashMap<String, String> =
        pairs(conn, "SELECT session_id, ws_key FROM workstream_members")?;
    let branches: HashMap<(String, String), String> = {
        let mut st = conn.prepare("SELECT repo, branch, ws_key FROM workstream_branches")?;
        st.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };

    // Group: where you put the session, or the workstream that claimed its
    // branch; else its feature branch in a repository, or the session alone.
    let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for r in rows {
        let feat = feature(r.repo.as_deref(), r.branch.as_deref());
        let claimed = r
            .repo
            .clone()
            .zip(r.branch.clone())
            .and_then(|k| branches.get(&k));
        let key = match (members.get(&r.id).or(claimed), feat) {
            (Some(k), _) => k.clone(),
            (None, Some((repo, b))) => branch_key(repo, b),
            (None, None) if !r.tasks.tasks.is_empty() || !r.team.agents.is_empty() => {
                format!("session:{}", r.id)
            }
            (None, None) => continue,
        };
        groups.entry(key).or_default().push(r);
    }

    let mut prefs = load_prefs(conn)?;
    let mut out: Vec<Workstream> = groups
        .into_iter()
        .map(|(key, rows)| {
            let mut ws = build(key, rows, &stored, now);
            if let Some(p) = prefs.remove(&ws.key) {
                p.apply(&mut ws);
            }
            ws
        })
        .collect();
    // Ones you made that have no sessions yet.
    for (key, p) in prefs.into_iter().filter(|(_, p)| p.custom) {
        let mut ws = empty(key, p.created_at);
        p.apply(&mut ws);
        out.push(ws);
    }
    let mut items = load_items(conn)?;
    let links = load_links(conn)?;
    for ws in &mut out {
        let active: HashSet<&str> = ws
            .sessions
            .iter()
            .filter(|s| s.status.group() == Group::Active)
            .map(|s| s.id.as_str())
            .collect();
        let mut item_tasks = vec![];
        for it in items.remove(&ws.key).unwrap_or_default() {
            let key = format!("item:{}:{}", ws.key, it.key);
            let (kind, reason) = item_kind(&it, stored.get(&key).copied());
            let sessions = links
                .get(&(ws.key.clone(), it.key.clone()))
                .cloned()
                .unwrap_or_default();
            // An agent is on it right now.
            let status = if it.status == TaskStatus::Pending
                && sessions.iter().any(|s| active.contains(s.as_str()))
            {
                TaskStatus::InProgress
            } else {
                it.status
            };
            item_tasks.push(WsTask {
                key,
                session_id: String::new(),
                title: it.title,
                status,
                kind,
                reason,
                agent: it.assignee,
                blocked_by: vec![],
                updated_at: None,
                url: it.url,
                mine: it.mine,
                parent: it.parent.map(|p| format!("item:{}:{p}", ws.key)),
                depth: it.depth,
                sessions,
            });
        }
        ws.tasks.splice(0..0, item_tasks);
        recount(ws, now);
    }
    out.sort_by_key(|w| {
        (
            std::cmp::Reverse(w.starred),
            w.closed,
            std::cmp::Reverse(w.last_activity),
        )
    });
    Ok(out)
}

fn pairs(conn: &Connection, sql: &str) -> Result<HashMap<String, String>> {
    let mut st = conn.prepare(sql)?;
    let out = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(out)
}

struct Prefs {
    title: Option<String>,
    starred: bool,
    custom: bool,
    created_at: i64,
    source_url: Option<String>,
    synced_at: Option<i64>,
    sync_error: Option<String>,
    closed: bool,
    tracker_repo: Option<String>,
}

impl Prefs {
    fn apply(self, ws: &mut Workstream) {
        if let Some(t) = self.title {
            ws.title = t;
        }
        ws.starred = self.starred;
        ws.custom = self.custom;
        ws.source_url = self.source_url;
        ws.synced_at = self.synced_at;
        ws.sync_error = self.sync_error;
        ws.closed = self.closed;
        if self.tracker_repo.is_some() {
            ws.tracker_repo = self.tracker_repo;
        }
    }
}

fn load_prefs(conn: &Connection) -> Result<HashMap<String, Prefs>> {
    let mut st = conn.prepare(
        "SELECT ws_key, title, starred, custom, created_at, source_url, synced_at, sync_error,
                source_closed, source_repo
         FROM workstream_prefs",
    )?;
    let out = st
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                Prefs {
                    title: r.get(1)?,
                    starred: r.get(2)?,
                    custom: r.get(3)?,
                    created_at: r.get(4)?,
                    source_url: r.get(5)?,
                    synced_at: r.get(6)?,
                    sync_error: r.get(7)?,
                    closed: r.get(8)?,
                    tracker_repo: r.get(9)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(out)
}

fn empty(key: String, created_at: i64) -> Workstream {
    Workstream {
        key,
        title: String::new(),
        starred: false,
        custom: false,
        source_url: None,
        synced_at: None,
        sync_error: None,
        closed: false,
        tracker_repo: None,
        repo: None,
        branch: None,
        last_activity: created_at,
        sessions: vec![],
        agents: vec![],
        tasks: vec![],
        updates: vec![],
        counts: WsCounts::default(),
    }
}

fn upsert_prefs(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO workstream_prefs (ws_key, created_at) VALUES (?1, ?2)",
        params![key, now_ms()],
    )?;
    Ok(())
}

/// Makes a workstream of your own, starred, holding `sessions`. Returns its key.
pub fn create(conn: &Connection, title: &str, sessions: &[String]) -> Result<String> {
    let title = title.trim();
    anyhow::ensure!(!title.is_empty(), "A workstream needs a name");
    let now = now_ms();
    // Keyed by creation time, counting up past any made in the same moment.
    let mut n = now;
    let key = loop {
        let key = format!("custom:{n}");
        let added = conn.execute(
            "INSERT OR IGNORE INTO workstream_prefs (ws_key, title, starred, custom, created_at) VALUES (?1, ?2, 1, 1, ?3)",
            params![key, title, now],
        )?;
        if added == 1 {
            break key;
        }
        n += 1;
    };
    for id in sessions {
        add_session(conn, &key, id)?;
    }
    Ok(key)
}

/// Gives a workstream your own name; an empty one goes back to the
/// automatic name (except for ones you made, which keep theirs).
pub fn rename(conn: &Connection, key: &str, title: &str) -> Result<()> {
    let title = title.trim();
    upsert_prefs(conn, key)?;
    conn.execute(
        "UPDATE workstream_prefs SET title = ?2 WHERE ws_key = ?1 AND (custom = 0 OR ?2 IS NOT NULL)",
        params![key, (!title.is_empty()).then_some(title)],
    )?;
    Ok(())
}

/// Stars or unstars a workstream. A starred one Tilly grouped keeps its
/// sessions or branch pinned to it, so it stays listed whatever its age.
pub fn set_starred(
    conn: &Connection,
    key: &str,
    starred: bool,
    repo: Option<&str>,
    branch: Option<&str>,
) -> Result<()> {
    upsert_prefs(conn, key)?;
    conn.execute(
        "UPDATE workstream_prefs SET starred = ?2 WHERE ws_key = ?1",
        params![key, starred],
    )?;
    let pin = feature(repo, branch).filter(|(r, b)| branch_key(r, b) == key);
    if let Some((repo, b)) = pin {
        if starred {
            conn.execute(
                "INSERT OR IGNORE INTO workstream_branches (repo, branch, ws_key) VALUES (?1, ?2, ?3)",
                params![repo, b, key],
            )?;
        } else {
            conn.execute(
                "DELETE FROM workstream_branches WHERE repo = ?1 AND branch = ?2 AND ws_key = ?3",
                params![repo, b, key],
            )?;
        }
    } else if let Some(id) = key.strip_prefix("session:") {
        if starred {
            conn.execute(
                "INSERT OR IGNORE INTO workstream_members (session_id, ws_key) VALUES (?1, ?2)",
                params![id, key],
            )?;
        } else {
            conn.execute(
                "DELETE FROM workstream_members WHERE session_id = ?1 AND ws_key = ?2",
                params![id, key],
            )?;
        }
    }
    Ok(())
}

/// Puts a session in a workstream. Its feature branch comes too, so the
/// branch's other and later sessions join it.
pub fn add_session(conn: &Connection, key: &str, session_id: &str) -> Result<()> {
    let (repo, branch): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT COALESCE(repo_root, project_path), git_branch FROM sessions WHERE id = ?1",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(NotFound("Session"))?;
    conn.execute(
        "INSERT INTO workstream_members (session_id, ws_key) VALUES (?1, ?2)
         ON CONFLICT(session_id) DO UPDATE SET ws_key = excluded.ws_key",
        params![session_id, key],
    )?;
    // A branch another workstream already holds stays with it; only this
    // session moves.
    if let Some((repo, b)) = feature(repo.as_deref(), branch.as_deref()) {
        conn.execute(
            "INSERT OR IGNORE INTO workstream_branches (repo, branch, ws_key) VALUES (?1, ?2, ?3)",
            params![repo, b, key],
        )?;
    }
    Ok(())
}

/// Takes a session, and the feature branch it brought, out of a workstream.
/// They go back to where Tilly would group them.
pub fn remove_session(conn: &Connection, key: &str, session_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM workstream_branches WHERE ws_key = ?1 AND (repo, branch) IN
           (SELECT COALESCE(repo_root, project_path), git_branch FROM sessions WHERE id = ?2)",
        params![key, session_id],
    )?;
    conn.execute(
        "DELETE FROM workstream_members WHERE ws_key = ?1 AND (session_id = ?2 OR session_id IN
           (SELECT o.id FROM sessions o, sessions s WHERE s.id = ?2
              AND COALESCE(o.repo_root, o.project_path) = COALESCE(s.repo_root, s.project_path)
              AND o.git_branch = s.git_branch))",
        params![key, session_id],
    )?;
    Ok(())
}

/// Deletes a workstream you made. Its sessions go back to where Tilly would
/// group them.
pub fn delete(conn: &Connection, key: &str) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM workstream_prefs WHERE ws_key = ?1 AND custom = 1",
        [key],
    )?;
    if n == 0 {
        return Err(NotFound("Workstream").into());
    }
    conn.execute("DELETE FROM workstream_members WHERE ws_key = ?1", [key])?;
    conn.execute("DELETE FROM workstream_branches WHERE ws_key = ?1", [key])?;
    conn.execute("DELETE FROM workstream_items WHERE ws_key = ?1", [key])?;
    conn.execute(
        "DELETE FROM workstream_item_sessions WHERE ws_key = ?1",
        [key],
    )?;
    Ok(())
}

/// Counts a workstream's tasks. With an epic from a tracker, progress is
/// the epic's leaf items alone (not the parents that group them, nor agents'
/// own todos), so 8 issues read as 8. Only human work that's yours, assigned
/// to you or to nobody, needs you.
fn recount(ws: &mut Workstream, now: i64) {
    let has_items = ws.tasks.iter().any(|t| t.session_id.is_empty());
    let parents: HashSet<&str> = ws
        .tasks
        .iter()
        .filter_map(|t| t.parent.as_deref())
        .collect();
    let mut c = WsCounts {
        waiting: ws
            .sessions
            .iter()
            .filter(|s| {
                s.status.group() == Group::Awaiting && now - s.ended_at < live::LIVE_WINDOW_MS
            })
            .count(),
        ..Default::default()
    };
    for t in ws
        .tasks
        .iter()
        .filter(|t| (!has_items || t.session_id.is_empty()) && !parents.contains(t.key.as_str()))
    {
        match t.status {
            TaskStatus::Done => c.done += 1,
            TaskStatus::InProgress => c.in_progress += 1,
            _ => c.pending += 1,
        }
        match t.kind {
            TaskKind::Agent => c.agent += 1,
            TaskKind::Human => {
                c.human += 1;
                let yours = !t.session_id.is_empty() || t.mine || t.agent.is_none();
                if t.status != TaskStatus::Done && yours {
                    c.decisions += 1;
                }
            }
        }
    }
    c.needs_you = c.decisions + c.waiting;
    ws.counts = c;
}

/// Sessions linked to each tracker item, by (workstream, item).
fn load_links(conn: &Connection) -> Result<HashMap<(String, String), Vec<String>>> {
    let mut st =
        conn.prepare("SELECT ws_key, item_key, session_id FROM workstream_item_sessions")?;
    let mut out: HashMap<(String, String), Vec<String>> = HashMap::new();
    for row in st.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))? {
        let (k, s) = row?;
        out.entry(k).or_default().push(s);
    }
    Ok(out)
}

/// Who does a tracker item: your choice, then an "agent" or "human" label,
/// then the wording rules.
fn item_kind(it: &crate::trackers::Item, stored: Option<TaskKind>) -> (TaskKind, Option<String>) {
    if stored.is_none() {
        for kind in TaskKind::ALL {
            let label = if kind == TaskKind::Human {
                "human"
            } else {
                "agent"
            };
            if it.labels.iter().any(|l| l == label) {
                return (kind, Some(format!("Labelled {label}")));
            }
        }
    }
    classify(&it.title, it.status, LiveStatus::Ended, stored)
}

fn load_items(conn: &Connection) -> Result<HashMap<String, Vec<crate::trackers::Item>>> {
    let mut st = conn.prepare(
        "SELECT ws_key, item_key, title, status, url, labels, assignee, mine, parent, depth
         FROM workstream_items WHERE status != 'cancelled' ORDER BY ws_key, position",
    )?;
    let mut out: HashMap<String, Vec<crate::trackers::Item>> = HashMap::new();
    let rows = st.query_map([], |r| {
        let status: String = r.get(3)?;
        Ok((
            r.get::<_, String>(0)?,
            crate::trackers::Item {
                key: r.get(1)?,
                title: r.get(2)?,
                status: TaskStatus::parse(&status).unwrap_or(TaskStatus::Pending),
                url: r.get(4)?,
                labels: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                assignee: r.get(6)?,
                mine: r.get(7)?,
                parent: r.get(8)?,
                depth: r.get(9)?,
            },
        ))
    })?;
    for row in rows {
        let (key, item) = row?;
        out.entry(key).or_default().push(item);
    }
    Ok(out)
}

/// Makes a starred workstream from an epic: its items become tasks, and
/// recent sessions that mention it or its items join. Returns its key.
pub fn import(conn: &Connection, epic: &Epic, link: &str) -> Result<String> {
    // Already imported, by the link given or where the issue lives now.
    let existing: Option<String> = conn
        .query_row(
            "SELECT ws_key FROM workstream_prefs WHERE source_url IN (?1, ?2)",
            params![epic.url, link.trim()],
            |r| r.get(0),
        )
        .optional()?;
    let key = match existing {
        Some(k) => k,
        None => {
            let k = create(conn, &epic.title, &[])?;
            conn.execute(
                "UPDATE workstream_prefs SET source_url = ?2 WHERE ws_key = ?1",
                params![k, epic.url],
            )?;
            k
        }
    };
    sync(conn, &key, epic)?;
    Ok(key)
}

/// The epic a workstream was imported from.
pub fn source_url(conn: &Connection, key: &str) -> Result<String> {
    conn.query_row(
        "SELECT source_url FROM workstream_prefs WHERE ws_key = ?1",
        [key],
        |r| r.get::<_, Option<String>>(0),
    )
    .optional()?
    .flatten()
    .ok_or_else(|| NotFound("Imported workstream").into())
}

/// Replaces a workstream's tracker items with the epic's latest, and adds
/// recent sessions that mention it.
pub fn sync(conn: &Connection, key: &str, epic: &Epic) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM workstream_items WHERE ws_key = ?1", [key])?;
    {
        let mut ins = tx.prepare(
            "INSERT OR IGNORE INTO workstream_items
               (ws_key, item_key, position, title, status, url, labels, assignee, mine, parent, depth)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        for (i, it) in epic.items.iter().enumerate() {
            ins.execute(params![
                key,
                it.key,
                i as i64,
                it.title,
                it.status.as_str(),
                it.url,
                serde_json::to_string(&it.labels)?,
                it.assignee,
                it.mine,
                it.parent,
                it.depth
            ])?;
        }
    }
    // Follow the epic: its title (unless you renamed it), state, where it lives.
    tx.execute(
        "UPDATE workstream_prefs SET
           title = CASE WHEN title IS source_title OR source_title IS NULL THEN ?3 ELSE title END,
           source_title = ?3, source_closed = ?4, source_repo = ?5, source_url = ?6,
           synced_at = ?2, sync_tried_at = ?2, sync_error = NULL
         WHERE ws_key = ?1",
        params![
            key,
            now_ms(),
            epic.title,
            epic.closed(),
            epic.repo,
            epic.url
        ],
    )?;
    tx.execute(
        "DELETE FROM workstream_item_sessions WHERE ws_key = ?1",
        [key],
    )?;
    let members: HashSet<String> = tx
        .prepare("SELECT session_id FROM workstream_members")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, item) in mentions(&tx, epic)? {
        if let Some(item) = item {
            tx.execute(
                "INSERT OR IGNORE INTO workstream_item_sessions (ws_key, item_key, session_id) VALUES (?1, ?2, ?3)",
                params![key, item, id],
            )?;
        }
        // A session already in a workstream stays there.
        if !members.contains(&id) {
            add_session(&tx, key, &id)?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Fetches an imported workstream's epic and syncs it. A failure is kept on
/// the workstream, so its card can say why it's out of date.
pub fn sync_from_tracker(conn: &Connection, key: &str) -> Result<()> {
    let url = source_url(conn, key)?;
    let res = crate::trackers::parse(&url)
        .and_then(|r| crate::trackers::fetch(&r))
        .and_then(|epic| sync(conn, key, &epic));
    if let Err(e) = &res {
        conn.execute(
            "UPDATE workstream_prefs SET sync_error = ?2, sync_tried_at = ?3 WHERE ws_key = ?1",
            params![key, e.to_string(), now_ms()],
        )?;
    }
    res
}

/// Imported workstreams not fetched, or tried, in the last `every_ms`;
/// the stalest first.
pub fn due_for_sync(conn: &Connection, every_ms: i64) -> Result<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT ws_key FROM workstream_prefs
         WHERE source_url IS NOT NULL
           AND MAX(COALESCE(synced_at, 0), COALESCE(sync_tried_at, 0)) < ?1
         ORDER BY COALESCE(synced_at, 0)",
    )?;
    let keys = st
        .query_map([now_ms() - every_ms], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(keys)
}

/// Sessions from the last 90 days that mention the epic or its items, in
/// their title, opening prompt, latest reply or branch, and the item each
/// names (none for the epic itself). "#12" counts only in a clone of the
/// epic's repository, known by its git remote.
fn mentions(conn: &Connection, epic: &Epic) -> Result<Vec<(String, Option<String>)>> {
    let since = now_ms() - 90 * crate::stats::DAY_MS;
    let mut st = conn.prepare(
        "SELECT id, COALESCE(repo_root, project_path), git_branch,
                lower(COALESCE(title, '') || ' ' || substr(COALESCE(first_prompt, ''), 1, 8000)
                      || ' ' || COALESCE(last_message, ''))
         FROM sessions WHERE ended_at >= ?1",
    )?;
    let repo = epic.repo.as_deref().map(str::to_lowercase);
    let mut remotes: HashMap<String, Option<String>> = HashMap::new();
    let rows = st.query_map([since], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = vec![];
    for row in rows {
        let (id, root, branch, text) = row?;
        let branch = branch.unwrap_or_default().to_lowercase();
        let mut hits: Vec<Option<String>> = epic
            .needles
            .iter()
            .filter(|n| {
                let t = n.text.to_lowercase();
                has_ref(&text, &t) || has_ref(&branch, &t)
            })
            .map(|n| n.item.clone())
            .collect();
        let same_repo = repo.is_some()
            && root.as_deref().is_some_and(|p| {
                remotes
                    .entry(p.to_string())
                    .or_insert_with(|| crate::git::remote_slug(Path::new(p)))
                    .as_deref()
                    .map(str::to_lowercase)
                    == repo
            });
        if same_repo {
            hits.extend(
                epic.numbers
                    .iter()
                    .filter(|(n, _)| mentions_number(&text, *n) || branch_has_number(&branch, *n))
                    .map(|(_, item)| item.clone()),
            );
        }
        hits.sort();
        hits.dedup();
        out.extend(hits.into_iter().map(|item| (id.clone(), item)));
    }
    Ok(out)
}

/// `needle` (a link or "owner/repo#12") in `text`, not as the start of a
/// longer one: "o/r#2" isn't in "o/r#27".
fn has_ref(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(i, _)| {
        !text[i + needle.len()..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
    })
}

/// "#12" not followed by another digit.
fn mentions_number(text: &str, n: u64) -> bool {
    let tag = format!("#{n}");
    text.match_indices(&tag).any(|(i, _)| {
        !text[i + tag.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

/// "feat/12-checkout" or "issue-12".
fn branch_has_number(branch: &str, n: u64) -> bool {
    let n = n.to_string();
    branch
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t == n)
}

fn build(key: String, rows: Vec<Row>, stored: &BTreeMap<String, TaskKind>, now: i64) -> Workstream {
    // Rows arrive newest first; the tree reads oldest first.
    let rows: Vec<Row> = rows.into_iter().rev().collect();
    let first = &rows[0];
    let branch =
        feature(first.repo.as_deref(), first.branch.as_deref()).map(|(_, b)| b.to_string());
    let title = rows
        .iter()
        .rev()
        .find_map(|r| r.intent.clone())
        .or_else(|| branch.as_deref().map(branch_title))
        .or_else(|| first.title.clone())
        .or_else(|| {
            first
                .prompt
                .as_deref()
                .map(|p| crate::util::truncate(p.trim(), 80))
        })
        .or_else(|| first.tasks.tasks.first().map(|t| t.title.clone()))
        .unwrap_or_else(|| "Untitled work".into());
    let mut ws = Workstream {
        key,
        title,
        starred: false,
        custom: false,
        source_url: None,
        synced_at: None,
        sync_error: None,
        closed: false,
        tracker_repo: None,
        repo: first.repo.clone(),
        branch,
        last_activity: rows.iter().map(|r| r.ended).max().unwrap_or(0),
        sessions: vec![],
        agents: vec![],
        tasks: vec![],
        updates: vec![],
        counts: WsCounts::default(),
    };
    for r in &rows {
        let busy = r.status.group() == Group::Active;
        ws.sessions.push(WsSession {
            id: r.id.clone(),
            source: r.source.clone(),
            entrypoint: r.entrypoint.clone(),
            title: r.title.clone(),
            status: r.status,
            ended_at: r.ended,
            active_ms: r.value.active_ms,
            cost_usd: r.value.cost_usd,
            cost_is_estimate: r.value.cost_is_estimate,
            tokens: r.value.tokens,
            commits: r.value.commits,
            prs: r.value.prs.clone(),
            outcome: r.value.outcome.clone(),
        });
        ws.agents.push(WsAgent {
            session_id: r.id.clone(),
            id: "lead".into(),
            name: None,
            role: "lead".into(),
            active: busy,
        });
        for a in &r.team.agents {
            ws.agents.push(WsAgent {
                session_id: r.id.clone(),
                id: a.id.clone(),
                name: a.name.clone(),
                role: a.role.clone(),
                active: a.status == crate::team::Status::Running,
            });
            if let (Some(text), at) = (a.result.as_deref(), a.ended_at) {
                ws.updates.push(WsUpdate {
                    at,
                    session_id: r.id.clone(),
                    agent: a.name.clone().unwrap_or_else(|| a.role.clone()),
                    task_key: None,
                    text: crate::util::truncate(text.trim(), 300),
                });
            }
        }
        let key_of = |id: &str| format!("{}:{id}", r.id);
        for t in &r.tasks.tasks {
            if t.status == TaskStatus::Cancelled {
                continue;
            }
            let key = key_of(&t.id);
            let (kind, reason) = classify(&t.title, t.status, r.status, stored.get(&key).copied());
            let open: Vec<String> = t
                .blocked_by
                .iter()
                .filter(|b| {
                    r.tasks
                        .tasks
                        .iter()
                        .any(|x| x.id == **b && x.status != TaskStatus::Done)
                })
                .map(|b| key_of(b))
                .collect();
            // The same todo carried into a later session: the latest one stands.
            let same = |x: &WsTask| x.title.trim().eq_ignore_ascii_case(t.title.trim());
            ws.tasks.retain(|x| !same(x));
            ws.tasks.push(WsTask {
                key,
                session_id: r.id.clone(),
                title: t.title.clone(),
                status: t.status,
                kind,
                reason,
                agent: t.agent.clone(),
                blocked_by: open,
                updated_at: t.updated_at,
                url: None,
                mine: false,
                parent: None,
                depth: 0,
                sessions: vec![],
            });
        }
        for u in &r.tasks.updates {
            ws.updates.push(WsUpdate {
                at: u.at,
                session_id: r.id.clone(),
                agent: u.agent.clone(),
                task_key: u.task.as_deref().map(key_of),
                text: u.text.clone(),
            });
        }
    }
    ws.updates.sort_by_key(|u| std::cmp::Reverse(u.at));
    ws.updates.truncate(MAX_UPDATES);
    recount(&mut ws, now);
    ws
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_tasks() {
        let c = |t: &str| classify(t, TaskStatus::Pending, LiveStatus::Working, None).0;
        assert_eq!(c("Migrate the webhook handler"), TaskKind::Agent);
        assert_eq!(c("Approve the rollout plan"), TaskKind::Human);
        assert_eq!(c("Create staging API keys"), TaskKind::Human);
        assert_eq!(c("Decide on the cache size"), TaskKind::Human);
        assert_eq!(c("Write decision log and open asks"), TaskKind::Agent);
        // The task in progress while the agent asks you something is yours.
        assert_eq!(
            classify(
                "Write tests",
                TaskStatus::InProgress,
                LiveStatus::Asking,
                None
            )
            .0,
            TaskKind::Human
        );
        // Your choice wins over the rules.
        assert_eq!(
            classify(
                "Approve the plan",
                TaskStatus::Pending,
                LiveStatus::Working,
                Some(TaskKind::Agent)
            ),
            (TaskKind::Agent, Some("Set by you".into()))
        );
    }

    #[test]
    fn titles_from_branches() {
        assert_eq!(
            branch_title("feature/apple-pay-at-checkout"),
            "Apple pay at checkout"
        );
        assert_eq!(branch_title("fix_flaky_tests"), "Fix flaky tests");
    }

    #[test]
    fn groups_sessions_into_workstreams() {
        let conn = crate::db::open_in_memory().unwrap();
        let now = now_ms();
        let tasks = serde_json::json!({
            "tasks": [
                {"id": "1", "title": "Swap the SDK", "status": "done"},
                {"id": "2", "title": "Approve the rollout", "status": "pending", "blockedBy": ["3"]},
                {"id": "3", "title": "Run staging tests", "status": "in_progress"}
            ],
            "updates": [{"at": now - 1000, "agent": "lead", "task": "3", "text": "Started: Run staging tests"}]
        })
        .to_string();
        for (id, branch, t, ended) in [
            ("a", Some("feature/payments"), tasks.as_str(), now - 5000),
            ("b", Some("feature/payments"), "{}", now - 2000),
            ("c", Some("main"), "{}", now - 1000),
            (
                "d",
                None,
                r#"{"tasks":[{"id":"x","title":"Fix bug","status":"pending"}]}"#,
                now - 3000,
            ),
        ] {
            conn.execute(
                "INSERT INTO sessions (id, source, external_id, file_path, repo_root, git_branch, started_at, ended_at, active_ms, user_turns, tasks, last_event, ingested_at)
                 VALUES (?1, 'claude_code', ?1, '/f', '/work/pay', ?2, ?3, ?3, 0, 1, ?4, 'turn_ended', ?3)",
                params![id, branch, ended, t],
            )
            .unwrap();
        }
        let ws = workstreams(&conn, 7).unwrap();
        // b's branch, then d alone; c (main, no tasks) is left out.
        assert_eq!(
            ws.iter().map(|w| w.title.as_str()).collect::<Vec<_>>(),
            ["Payments", "Fix bug"]
        );
        let pay = &ws[0];
        assert_eq!(pay.sessions.len(), 2);
        assert_eq!(
            (pay.counts.done, pay.counts.in_progress, pay.counts.pending),
            (1, 1, 1)
        );
        assert_eq!(
            (pay.counts.agent, pay.counts.human, pay.counts.needs_you),
            (2, 1, 1)
        );
        assert_eq!(pay.tasks[1].blocked_by, ["a:3"]);
        assert_eq!(pay.updates[0].task_key.as_deref(), Some("a:3"));

        set_kind(&conn, "a:2", Some(TaskKind::Agent)).unwrap();
        let ws = workstreams(&conn, 7).unwrap();
        assert_eq!(ws[0].tasks[1].kind, TaskKind::Agent);
        assert_eq!(ws[0].counts.human, 0);
    }

    fn insert(conn: &Connection, id: &str, branch: Option<&str>, tasks: &str, ended: i64) {
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, repo_root, git_branch, started_at, ended_at, active_ms, user_turns, tasks, last_event, ingested_at)
             VALUES (?1, 'claude_code', ?1, '/f', '/work/pay', ?2, ?3, ?3, 0, 1, ?4, 'turn_ended', ?3)",
            params![id, branch, ended, tasks],
        )
        .unwrap();
    }

    #[test]
    fn stars_and_your_own_workstreams() {
        let conn = crate::db::open_in_memory().unwrap();
        let now = now_ms();
        let old = now - 60 * crate::stats::DAY_MS;
        let list = r#"{"tasks":[{"id":"x","title":"Fix bug","status":"pending"}]}"#;
        insert(&conn, "a", Some("feature/payments"), "{}", now - 5000);
        insert(&conn, "b", Some("feature/payments"), "{}", now - 4000);
        insert(&conn, "d", None, list, old);
        insert(&conn, "e", None, "{}", now - 3000);
        let keys = |days| {
            workstreams(&conn, days)
                .unwrap()
                .into_iter()
                .map(|w| (w.title, w.starred, w.sessions.len()))
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(7), [("Payments".into(), false, 2)]);

        // Starring an old one keeps it listed.
        set_starred(&conn, "session:d", true, Some("/work/pay"), None).unwrap();
        assert_eq!(
            keys(7),
            [("Fix bug".into(), true, 1), ("Payments".into(), false, 2)]
        );

        // Your own: the session brings its branch, and later sessions join.
        let key = create(&conn, "Checkout revamp", &["a".into(), "e".into()]).unwrap();
        insert(&conn, "f", Some("feature/payments"), "{}", now - 1000);
        assert_eq!(
            keys(7),
            [
                ("Checkout revamp".into(), true, 4),
                ("Fix bug".into(), true, 1)
            ]
        );
        rename(&conn, &key, "Checkout").unwrap();
        rename(&conn, &key, "").unwrap();
        assert_eq!(keys(7)[0].0, "Checkout");

        // Taking a session out takes its branch too.
        remove_session(&conn, &key, "b").unwrap();
        assert_eq!(
            keys(7),
            [
                ("Checkout".into(), true, 1),
                ("Fix bug".into(), true, 1),
                ("Payments".into(), false, 3)
            ]
        );
        // Deleting it leaves an empty list of your own; e goes back.
        delete(&conn, &key).unwrap();
        set_starred(&conn, "session:d", false, Some("/work/pay"), None).unwrap();
        assert_eq!(keys(7), [("Payments".into(), false, 3)]);
        assert!(delete(&conn, "branch:/work/pay:feature/payments").is_err());

        // A made workstream with no sessions still shows.
        create(&conn, "Planning", &[]).unwrap();
        assert_eq!(keys(7)[0], ("Planning".into(), true, 0));
        assert!(create(&conn, "  ", &[]).is_err());
    }

    #[test]
    fn imports_epics() {
        use crate::trackers::{Item, Needle};
        let conn = crate::db::open_in_memory().unwrap();
        let now = now_ms();
        // A clone whose folder name isn't the repository's: its remote says.
        let clone = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(clone.path().join(".git")).unwrap();
        std::fs::write(
            clone.path().join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:o/pay.git\n",
        )
        .unwrap();
        let root = clone.path().to_string_lossy().into_owned();
        insert(&conn, "a", Some("main"), "{}", now - 3000);
        insert(&conn, "b", Some("feat/34-review"), "{}", now - 2000);
        insert(&conn, "c", None, "{}", now - 1000);
        insert(&conn, "d", None, "{}", now - 500);
        insert(&conn, "e", None, "{}", now - 400);
        for (id, prompt) in [
            ("a", "Fix #28 please"),
            ("c", "see o/pay#99 and https://github.com/o/pay/issues/280"),
            ("d", "unrelated #2800, and o/pay#9"),
            ("e", "Pick up o/pay#34"),
        ] {
            conn.execute(
                "UPDATE sessions SET first_prompt = ?2, repo_root = ?3 WHERE id = ?1",
                params![id, prompt, root],
            )
            .unwrap();
        }
        conn.execute("UPDATE sessions SET repo_root = ?1 WHERE id = 'b'", [&root])
            .unwrap();
        // e is already in a workstream of yours: it's linked, not moved.
        let other = create(&conn, "Other", &["e".into()]).unwrap();
        let item = |key: &str, title: &str, status, label: &str| Item {
            key: key.into(),
            title: title.into(),
            status,
            url: Some(format!("https://github.com/o/pay/issues/{key}")),
            labels: vec![label.into()],
            ..Default::default()
        };
        let needles = |keys: &[&str]| {
            keys.iter()
                .map(|k| Needle {
                    text: format!("o/pay#{k}"),
                    item: Some(k.to_string()),
                })
                .collect::<Vec<_>>()
        };
        let mut epic = Epic {
            title: "Checkout".into(),
            url: "https://github.com/o/pay/issues/27".into(),
            status: TaskStatus::Pending,
            items: vec![
                item("28", "Swap the SDK", TaskStatus::Done, "agent"),
                item("34", "Review the design", TaskStatus::Pending, "human"),
                Item {
                    assignee: Some("sam".into()),
                    ..item("35", "Approve the rollout", TaskStatus::Pending, "")
                },
                item("99", "Write docs", TaskStatus::Pending, ""),
            ],
            needles: needles(&["34", "99"]),
            numbers: [27, 28, 34, 35, 99]
                .iter()
                .map(|n| (*n, (*n != 27).then(|| n.to_string())))
                .collect(),
            repo: Some("o/pay".into()),
        };
        let key = import(&conn, &epic, "o/pay#27").unwrap();
        let ws = workstreams(&conn, 7).unwrap();
        let w = ws.iter().find(|w| w.key == key).unwrap();
        assert_eq!(
            (w.title.as_str(), w.starred, w.custom),
            ("Checkout", true, true)
        );
        assert_eq!(w.tracker_repo.as_deref(), Some("o/pay"));
        // #28 in the prompt, 34 in the branch, o/pay#99 by name; not #2800 or o/pay#9.
        let mut ids: Vec<_> = w.sessions.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["a", "b", "c"]);
        let task = |k: &str| {
            w.tasks
                .iter()
                .find(|t| t.key == format!("item:{key}:{k}"))
                .unwrap()
        };
        assert_eq!(task("28").sessions, ["a"]);
        let mut on34 = task("34").sessions.clone();
        on34.sort();
        assert_eq!(on34, ["b", "e"]);
        assert_eq!(task("34").reason.as_deref(), Some("Labelled human"));
        assert_eq!(task("35").kind, TaskKind::Human);
        // Progress is the 4 items, not agents' todos; Sam's approval isn't yours.
        assert_eq!((w.counts.done, w.counts.pending), (1, 3));
        assert_eq!((w.counts.decisions, w.counts.needs_you), (1, 1));
        assert!(
            workstreams(&conn, 7)
                .unwrap()
                .iter()
                .any(|w| w.key == other && w.sessions.len() == 1)
        );

        // Your choice and your name survive a sync; the epic's state follows.
        set_kind(&conn, &format!("item:{key}:34"), Some(TaskKind::Agent)).unwrap();
        rename(&conn, &key, "Checkout v2").unwrap();
        epic.items.pop();
        epic.items[1].status = TaskStatus::Done;
        epic.status = TaskStatus::Done;
        epic.title = "Checkout (renamed upstream)".into();
        // The issue moved: the old link still finds it.
        epic.url = "https://github.com/o/shop/issues/27".into();
        assert_eq!(
            import(&conn, &epic, "https://github.com/o/pay/issues/27").unwrap(),
            key
        );
        let w = workstreams(&conn, 7)
            .unwrap()
            .into_iter()
            .find(|w| w.key == key)
            .unwrap();
        assert_eq!((w.title.as_str(), w.closed), ("Checkout v2", true));
        assert_eq!(
            w.tasks.iter().filter(|t| t.session_id.is_empty()).count(),
            3
        );
        let t34 = w.tasks.iter().find(|t| t.key.ends_with(":34")).unwrap();
        assert_eq!((t34.kind, t34.status), (TaskKind::Agent, TaskStatus::Done));
        assert_eq!(source_url(&conn, &key).unwrap(), epic.url);
        // Not renamed: the title follows the tracker.
        rename(&conn, &key, "").unwrap();
        delete(&conn, &key).unwrap();
        assert!(
            workstreams(&conn, 7)
                .unwrap()
                .iter()
                .all(|w| w.tasks.is_empty())
        );
    }

    #[test]
    fn nested_items_count_leaves() {
        use crate::trackers::Item;
        let conn = crate::db::open_in_memory().unwrap();
        let it = |key: &str, parent: Option<&str>, status| Item {
            key: key.into(),
            title: format!("Item {key}"),
            status,
            parent: parent.map(String::from),
            depth: parent.is_some() as u8,
            ..Default::default()
        };
        let epic = Epic {
            title: "Big".into(),
            url: "https://github.com/o/r/issues/1".into(),
            items: vec![
                it("2", None, TaskStatus::Pending),
                it("3", Some("2"), TaskStatus::Done),
                it("4", Some("2"), TaskStatus::Pending),
                it("5", None, TaskStatus::Done),
            ],
            ..Default::default()
        };
        let key = import(&conn, &epic, &epic.url).unwrap();
        let w = workstreams(&conn, 7)
            .unwrap()
            .into_iter()
            .find(|w| w.key == key)
            .unwrap();
        // Feature 2 groups 3 and 4: the leaves are 3, 4 and 5.
        assert_eq!((w.counts.done, w.counts.pending), (2, 1));
        let t4 = w.tasks.iter().find(|t| t.key.ends_with(":4")).unwrap();
        assert_eq!(
            (t4.parent.clone(), t4.depth),
            (Some(format!("item:{key}:2")), 1)
        );
    }

    #[test]
    fn matches_references_and_wording_exactly() {
        assert!(has_ref("see o/r#2.", "o/r#2"));
        assert!(!has_ref("see o/r#27", "o/r#2"));
        assert!(!has_ref("ab#123", "ab#1"));
        let c = |t: &str| classify(t, TaskStatus::Pending, LiveStatus::Working, None).0;
        assert_eq!(c("Fix password reset"), TaskKind::Agent);
        assert_eq!(c("Refactor the secrets loader"), TaskKind::Agent);
        assert_eq!(c("Provide the staging password"), TaskKind::Human);
        assert_eq!(c("Disapprove nothing; approve the plan"), TaskKind::Human);
    }

    #[test]
    fn matches_issue_numbers() {
        assert!(mentions_number("fix #12.", 12));
        assert!(!mentions_number("fix #123", 12));
        assert!(branch_has_number("feat/12-checkout", 12));
        assert!(!branch_has_number("feat/120-checkout", 12));
    }

    #[test]
    fn schedules_syncs_and_keeps_errors() {
        let conn = crate::db::open_in_memory().unwrap();
        let epic = Epic {
            title: "Checkout".into(),
            url: "https://github.com/o/pay/issues/27".into(),
            status: TaskStatus::Pending,
            ..Default::default()
        };
        let key = import(&conn, &epic, &epic.url).unwrap();
        let mine = create(&conn, "Mine", &[]).unwrap();
        // Just synced: nothing due; due once older than the interval.
        assert!(due_for_sync(&conn, 60_000).unwrap().is_empty());
        assert_eq!(due_for_sync(&conn, -1).unwrap(), std::slice::from_ref(&key));

        // A failed fetch is kept, and counts as a try so it isn't retried at once.
        conn.execute(
            "UPDATE workstream_prefs SET source_url = 'https://example.com/nope' WHERE ws_key = ?1",
            [&key],
        )
        .unwrap();
        assert!(sync_from_tracker(&conn, &key).is_err());
        assert!(sync_from_tracker(&conn, &mine).is_err());
        let ws = workstreams(&conn, 7).unwrap();
        let w = ws.iter().find(|w| w.key == key).unwrap();
        assert!(w.sync_error.as_deref().unwrap().contains("GitHub issue"));
        assert!(due_for_sync(&conn, 60_000).unwrap().is_empty());

        // A successful sync clears it.
        sync(&conn, &key, &epic).unwrap();
        let ws = workstreams(&conn, 7).unwrap();
        assert!(ws.iter().all(|w| w.sync_error.is_none()));
    }
}
