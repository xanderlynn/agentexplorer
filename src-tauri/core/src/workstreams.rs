//! Workstreams: larger efforts made of many sessions. Sessions on the same
//! repository and feature branch form one workstream; sessions on a default
//! branch stand alone, and only count when their agents kept a task list or
//! a team. Each workstream merges its sessions' task lists into one tree
//! (session → tasks), marks every task as agent or human work, and gathers
//! the agents' updates.

use std::collections::BTreeMap;

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::live::{self, Group, LiveStatus};
use crate::tasks::{TaskList, TaskStatus};
use crate::team::Team;
use crate::util::now_ms;

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
    /// `<session id>:<task id>`, the key overrides are stored under.
    pub key: String,
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsSession {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub status: LiveStatus,
    pub ended_at: i64,
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
    /// Human tasks not yet done, plus sessions waiting on you.
    pub needs_you: usize,
    pub pending: usize,
    pub agent: usize,
    pub human: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workstream {
    pub key: String,
    pub title: String,
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
/// matched as lowercase substrings.
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
            "credential",
            "secret",
            "api key",
            "password",
            "keychain",
            "token for",
        ],
        "Needs credentials",
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
        if words.iter().any(|w| t.contains(w)) {
            return (TaskKind::Human, Some((*why).into()));
        }
    }
    (TaskKind::Agent, None)
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

struct Row {
    id: String,
    source: String,
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
                substr(s.first_prompt, 1, 120)
         FROM sessions s WHERE s.ended_at >= ?1 ORDER BY s.ended_at DESC",
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

    // Group: a feature branch in a repository, or a single session.
    let mut groups: BTreeMap<String, Vec<Row>> = BTreeMap::new();
    for r in rows {
        let feature = r
            .branch
            .as_deref()
            .filter(|b| !DEFAULT_BRANCHES.contains(b))
            .zip(r.repo.as_deref());
        let key = match feature {
            Some((b, repo)) => format!("branch:{}:{b}", crate::util::path_key(repo)),
            None if !r.tasks.tasks.is_empty() || !r.team.agents.is_empty() => {
                format!("session:{}", r.id)
            }
            None => continue,
        };
        groups.entry(key).or_default().push(r);
    }

    let mut out: Vec<Workstream> = groups
        .into_iter()
        .map(|(key, rows)| build(key, rows, &stored))
        .collect();
    out.sort_by_key(|w| std::cmp::Reverse(w.last_activity));
    Ok(out)
}

fn build(key: String, rows: Vec<Row>, stored: &BTreeMap<String, TaskKind>) -> Workstream {
    // Rows arrive newest first; the tree reads oldest first.
    let rows: Vec<Row> = rows.into_iter().rev().collect();
    let first = &rows[0];
    let branch = first
        .branch
        .clone()
        .filter(|b| !DEFAULT_BRANCHES.contains(&b.as_str()));
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
            title: r.title.clone(),
            status: r.status,
            ended_at: r.ended,
        });
        if r.status.group() == Group::Awaiting {
            ws.counts.needs_you += 1;
        }
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
            match t.status {
                TaskStatus::Done => ws.counts.done += 1,
                TaskStatus::InProgress => ws.counts.in_progress += 1,
                _ => ws.counts.pending += 1,
            }
            match kind {
                TaskKind::Agent => ws.counts.agent += 1,
                TaskKind::Human => {
                    ws.counts.human += 1;
                    if t.status != TaskStatus::Done {
                        ws.counts.needs_you += 1;
                    }
                }
            }
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
}
