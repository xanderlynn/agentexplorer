//! The live control center: each session's status (working, awaiting you,
//! completed…), its sub-agents, and today's live statistics.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::team::{AgentLiveStatus, Status as AgentStatus, Team};
use crate::util::now_ms;

/// Sessions with activity inside this window appear in the control center.
pub const LIVE_WINDOW_MS: i64 = 3 * 3_600_000;
/// A working session silent this long is running something long.
const LONG_TASK_AFTER_MS: i64 = 2 * 60_000;
/// A non-shell tool still pending after this may be waiting on a permission
/// prompt; Claude Code does not log those.
const MAYBE_BLOCKED_AFTER_MS: i64 = 45_000;
/// Silent this long while working, it has stalled or the tool was closed.
const STALE_AFTER_MS: i64 = 30 * 60_000;
/// A question or permission prompt stays waiting on you this long, so one
/// left overnight is still there in the morning.
pub const AWAIT_HOLD_MS: i64 = 24 * 3_600_000;
/// A running sub-agent seen within this long keeps its session working.
const TEAM_BUSY_MS: i64 = 10 * 60_000;
/// A running sub-agent silent this long is shown as quiet.
const AGENT_QUIET_AFTER_MS: i64 = 3 * 60_000;
/// Width of the buckets in today's charts.
const BUCKET_MS: i64 = 15 * 60_000;

/// Tools that ask you something: the agent cannot go on until you answer.
const ASKING_TOOLS: &[&str] = &["AskUserQuestion", "ExitPlanMode", "ask_user"];
/// Tools that run commands, which can legitimately take minutes.
const SHELL_TOOLS: &[&str] = &["Bash", "bash", "shell", "powershell", "run_in_terminal"];
/// Tools that start sub-agents; they run as long as their agent does.
const AGENT_TOOLS: &[&str] = &["Agent", "Task", "task", "runSubagent"];

/// What the logs say about a session right now.
#[derive(Debug, Default, Clone, Copy)]
pub struct Signals<'a> {
    pub last_event: Option<&'a str>,
    /// Since the latest activity of the session or any running sub-agent.
    pub quiet_ms: i64,
    pub pending_tool: Option<&'a str>,
    pub last_message: Option<&'a str>,
    pub failed: bool,
    /// A sub-agent is running and was active recently.
    pub team_busy: bool,
    pub upcoming_jobs: bool,
}

/// One status per session. The UI's copy of this list is generated from it
/// (see `codegen`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStatus {
    Working,
    LongTask,
    MaybeBlocked,
    NeedsPermission,
    Asking,
    Done,
    Failed,
    Scheduled,
    #[default]
    Stale,
    Ended,
}

/// The part of the control center a status belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Awaiting,
    Active,
    Completed,
    Scheduled,
    Idle,
}

impl LiveStatus {
    pub const ALL: [LiveStatus; 10] = [
        LiveStatus::Working,
        LiveStatus::LongTask,
        LiveStatus::MaybeBlocked,
        LiveStatus::NeedsPermission,
        LiveStatus::Asking,
        LiveStatus::Done,
        LiveStatus::Failed,
        LiveStatus::Scheduled,
        LiveStatus::Stale,
        LiveStatus::Ended,
    ];

    /// The id used in alert profiles and the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            LiveStatus::Working => "working",
            LiveStatus::LongTask => "long_task",
            LiveStatus::MaybeBlocked => "maybe_blocked",
            LiveStatus::NeedsPermission => "needs_permission",
            LiveStatus::Asking => "asking",
            LiveStatus::Done => "done",
            LiveStatus::Failed => "failed",
            LiveStatus::Scheduled => "scheduled",
            LiveStatus::Stale => "stale",
            LiveStatus::Ended => "ended",
        }
    }

    pub fn group(self) -> Group {
        use LiveStatus::*;
        match self {
            Asking | NeedsPermission | MaybeBlocked | Failed => Group::Awaiting,
            Working | LongTask => Group::Active,
            Done | Ended => Group::Completed,
            Scheduled => Group::Scheduled,
            Stale => Group::Idle,
        }
    }

    fn is_live(self) -> bool {
        !matches!(self, LiveStatus::Stale | LiveStatus::Ended)
    }
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Group::Awaiting => "awaiting",
            Group::Active => "active",
            Group::Completed => "completed",
            Group::Scheduled => "scheduled",
            Group::Idle => "idle",
        }
    }
}

/// Awaiting you: `Asking`, `NeedsPermission`, `MaybeBlocked`, `Failed`.
/// Completed: `Done`, `Ended`.
pub fn status_for(s: Signals) -> LiveStatus {
    use LiveStatus::*;
    let pending = s.pending_tool.unwrap_or("");
    match s.last_event {
        Some("closed") => Ended,
        // A permission prompt nobody answered for a day was abandoned.
        Some("needs_permission") if s.quiet_ms >= AWAIT_HOLD_MS => Stale,
        Some("needs_permission") => NeedsPermission,
        Some("working") if ASKING_TOOLS.contains(&pending) => Asking,
        Some("working") if s.quiet_ms >= STALE_AFTER_MS && !s.team_busy => Stale,
        Some("working")
            if !pending.is_empty()
                && !SHELL_TOOLS.contains(&pending)
                && !AGENT_TOOLS.contains(&pending)
                && s.quiet_ms >= MAYBE_BLOCKED_AFTER_MS
                && !s.team_busy =>
        {
            MaybeBlocked
        }
        Some("working") if s.quiet_ms < LONG_TASK_AFTER_MS => Working,
        Some("working") => LongTask,
        // The lead finished its turn but its team is still at work.
        Some("turn_ended") if s.team_busy => Working,
        Some("turn_ended") if s.failed && s.quiet_ms < AWAIT_HOLD_MS => Failed,
        // An unanswered question still waits on you, even the next morning.
        Some("turn_ended")
            if s.quiet_ms < AWAIT_HOLD_MS && s.last_message.is_some_and(asks_question) =>
        {
            Asking
        }
        Some("turn_ended") if s.quiet_ms >= LIVE_WINDOW_MS => Done,
        Some("turn_ended") if s.upcoming_jobs => Scheduled,
        Some("turn_ended") => Done,
        _ => Stale,
    }
}

/// The reply ends by asking you something.
fn asks_question(msg: &str) -> bool {
    msg.trim_end()
        .trim_end_matches(['*', '_', '`', ')', '"'])
        .ends_with('?')
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAgent {
    pub id: String,
    pub name: Option<String>,
    pub role: String,
    pub task: String,
    pub status: AgentLiveStatus,
    pub started_at: Option<i64>,
    pub tokens: u64,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSession {
    pub id: String,
    pub source: String,
    /// Which app runs it: "cli", "claude-desktop", "copilot-app", "vscode"…
    pub entrypoint: Option<String>,
    pub title: Option<String>,
    pub project_path: Option<String>,
    pub git_branch: Option<String>,
    pub status: LiveStatus,
    /// When it entered its current status, as near as the logs tell.
    pub status_since: i64,
    pub ended_at: i64,
    pub user_turns: i64,
    pub active_ms: i64,
    /// The agent's latest reply: what it is asking when it waits on you.
    pub last_message: Option<String>,
    pub pending_tool: Option<String>,
    pub outcome: Option<String>,
    pub rating: Option<i64>,
    /// What the latest turn was doing: coding, research, running, discussion.
    pub mode: Option<String>,
    /// Context sent with the latest request, in tokens.
    pub context: i64,
    pub tokens: i64,
    pub agents: Vec<LiveAgent>,
    /// Scheduled jobs still to come, and the next one-off run.
    pub scheduled: usize,
    pub next_run: Option<i64>,
}

/// One bucket of today's charts.
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub start: i64,
    /// Sessions with agent work in the bucket.
    pub working: usize,
    /// Sessions waiting on you in the bucket.
    pub awaiting: usize,
    /// Agent turns that finished in the bucket.
    pub done: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSummary {
    /// Sessions active in the last few hours, plus any still working.
    pub sessions: Vec<LiveSession>,
    /// Sessions that finished today, newest first.
    pub completed_today: Vec<LiveSession>,
    pub working: usize,
    pub awaiting: usize,
    pub agents_running: usize,
    /// How long you took to answer an agent today: median, 75th percentile, n.
    pub reply_median_ms: Option<i64>,
    pub reply_p75_ms: Option<i64>,
    pub reply_n: usize,
    pub agent_ms_today: i64,
    pub wait_ms_today: i64,
    pub turns_today: usize,
    pub agents_done_today: usize,
    pub agents_failed_today: usize,
    /// Most sessions working at the same time today.
    pub peak_parallel_today: usize,
    pub buckets: Vec<Bucket>,
    /// Your replies today: under 1 min, 1–5, 5–15, 15–60, over 60.
    pub reply_histogram: [usize; 5],
    pub midnight: i64,
}

/// The mode of the most recent working span in a stored timeline.
fn last_mode(spans: &[(i64, i64, u8)]) -> Option<String> {
    let kind = spans.iter().rev().find(|s| s.2 != 0)?.2;
    crate::model::Mode::ALL
        .iter()
        .find(|m| **m as u8 == kind)
        .map(|m| m.key().to_string())
}

/// Latest activity of any running sub-agent.
fn team_seen(team: &Team) -> Option<i64> {
    team.agents
        .iter()
        .filter(|a| a.status == AgentStatus::Running)
        .filter_map(|a| a.last_seen.or(a.started_at))
        .max()
}

/// The session's status from its stored signals, with its team settled to
/// match: unreported agents are marked stopped once it goes quiet. The
/// control center, the Teams page and the session drawer all use this, so
/// they agree. Returns the status and how long it has been quiet.
fn derive(team: &mut Team, sig: Signals, ended: i64, now: i64) -> (LiveStatus, i64) {
    let seen = team_seen(team);
    let quiet_ms = (now - seen.map_or(ended, |s| s.max(ended))).max(0);
    let status = status_for(Signals {
        quiet_ms,
        team_busy: seen.is_some_and(|s| now - s < TEAM_BUSY_MS),
        upcoming_jobs: upcoming(team, now).0 > 0,
        ..sig
    });
    team.settle(status.is_live() && quiet_ms < STALE_AFTER_MS);
    (status, quiet_ms)
}

/// A stored session's status now, for views beyond the live window. Settles
/// its team the same way the live views do.
pub fn status_of(
    team: &mut Team,
    last_event: Option<&str>,
    pending_tool: Option<&str>,
    last_message: Option<&str>,
    failed: bool,
    ended: i64,
    now: i64,
) -> LiveStatus {
    derive(
        team,
        Signals {
            last_event,
            pending_tool,
            last_message,
            failed,
            ..Default::default()
        },
        ended,
        now,
    )
    .0
}

/// A stored team without its message log, which the live views never show.
fn team_lite(json: &str) -> Team {
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct Lite {
        agents: Vec<crate::team::Agent>,
        scheduled: Vec<crate::team::Scheduled>,
    }
    let l: Lite = serde_json::from_str(json).unwrap_or_default();
    Team {
        agents: l.agents,
        scheduled: l.scheduled,
        ..Default::default()
    }
}

/// A stored team, settled for the session's current status.
pub fn team_of(json: &str, last_event: Option<&str>, ended_at: i64) -> Team {
    let mut t: Team = serde_json::from_str(json).unwrap_or_default();
    derive(
        &mut t,
        Signals {
            last_event,
            ..Default::default()
        },
        ended_at,
        now_ms(),
    );
    t
}

fn upcoming(team: &Team, now: i64) -> (usize, Option<i64>) {
    let jobs: Vec<_> = team
        .scheduled
        .iter()
        .filter(|j| !j.cancelled && (j.recurring || j.at.is_some_and(|t| t > now)))
        .collect();
    (
        jobs.len(),
        jobs.iter().filter_map(|j| j.at).filter(|t| *t > now).min(),
    )
}

/// A live session with its timeline spans and team, for today's stats.
type Row = (LiveSession, Vec<(i64, i64, u8)>, Team);

fn percentile(sorted: &[i64], p: f64) -> Option<i64> {
    (!sorted.is_empty()).then(|| sorted[((sorted.len() - 1) as f64 * p).round() as usize])
}

/// Sessions active since `since`, newest first, with their live status.
fn rows(conn: &Connection, since: i64, now: i64) -> Result<Vec<Row>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, title, project_path, git_branch, last_event, ended_at, user_turns,
                active_ms, last_message, outcome, timeline, last_context,
                team, pending_tool, turn_started_at, failed, rating,
                input_tokens + output_tokens + cache_read_tokens + cache_write_tokens, entrypoint
         FROM sessions WHERE ended_at >= ?1 ORDER BY ended_at DESC LIMIT 300",
    )?;
    let rows: Vec<Row> = stmt
        .query_map([since], |r| {
            let last_event: Option<String> = r.get(5)?;
            let ended: i64 = r.get(6)?;
            let last_message: Option<String> = r.get(9)?;
            let spans: Vec<(i64, i64, u8)> =
                serde_json::from_str(&r.get::<_, String>(11)?).unwrap_or_default();
            let mut team = team_lite(&r.get::<_, String>(13)?);
            let pending_tool: Option<String> = r.get(14)?;
            let turn_started: Option<i64> = r.get(15)?;
            let (scheduled, next_run) = upcoming(&team, now);
            let (status, _) = derive(
                &mut team,
                Signals {
                    last_event: last_event.as_deref(),
                    pending_tool: pending_tool.as_deref(),
                    last_message: last_message.as_deref(),
                    failed: r.get(16)?,
                    ..Default::default()
                },
                ended,
                now,
            );
            let status_since = match status {
                LiveStatus::Working | LiveStatus::LongTask => turn_started.unwrap_or(ended),
                _ => ended,
            };
            let agents = team
                .agents
                .iter()
                .map(|a| LiveAgent {
                    id: a.id.clone(),
                    name: a.name.clone(),
                    role: a.role.clone(),
                    task: a.task.clone(),
                    status: a.live_status(now, AGENT_QUIET_AFTER_MS),
                    started_at: a.started_at,
                    tokens: a.tokens,
                })
                .collect();
            let session = LiveSession {
                id: r.get(0)?,
                source: r.get(1)?,
                entrypoint: r.get(19)?,
                title: r.get(2)?,
                project_path: r.get(3)?,
                git_branch: r.get(4)?,
                status,
                status_since,
                ended_at: ended,
                user_turns: r.get(7)?,
                active_ms: r.get(8)?,
                last_message,
                pending_tool,
                outcome: r.get(10)?,
                rating: r.get(17)?,
                mode: last_mode(&spans),
                context: r.get(12)?,
                tokens: r.get(18)?,
                agents,
                scheduled,
                next_run,
            };
            Ok((session, spans, team))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// What changed while you were away: sessions active since `since` that
/// finished, failed or are waiting on you, newest first.
pub fn since(conn: &Connection, since: i64) -> Result<Vec<LiveSession>> {
    Ok(rows(conn, since, now_ms())?
        .into_iter()
        .map(|(s, _, _)| s)
        .filter(|s| matches!(s.status.group(), Group::Awaiting | Group::Completed))
        .collect())
}

pub fn summary(conn: &Connection) -> Result<LiveSummary> {
    let now = now_ms();
    let midnight = crate::stats::local_midnight_ms(chrono::Local::now().date_naive());
    // Back far enough for anything still waiting on you.
    let since = midnight.min(now - AWAIT_HOLD_MS);
    let mut all = rows(conn, since, now)?;

    // Today's statistics, from every session active today.
    let clip = |a: i64, b: i64| (b.min(now) - a.max(midnight)).max(0);
    let mut replies: Vec<i64> = Vec::new();
    let (mut agent_ms_today, mut wait_ms_today, mut turns_today) = (0, 0, 0);
    let n_buckets = ((now - midnight) / BUCKET_MS + 1).max(1) as usize;
    let mut buckets: Vec<Bucket> = (0..n_buckets)
        .map(|i| Bucket {
            start: midnight + i as i64 * BUCKET_MS,
            ..Default::default()
        })
        .collect();
    let bucket_of = |t: i64| ((t - midnight) / BUCKET_MS).clamp(0, n_buckets as i64 - 1) as usize;
    let (mut agents_done_today, mut agents_failed_today) = (0, 0);
    for (s, spans, team) in &all {
        let mut work = vec![false; n_buckets];
        let mut wait = vec![false; n_buckets];
        for &(a, b, kind) in spans.iter().filter(|x| x.1 > midnight) {
            let (i, j) = (bucket_of(a), bucket_of(b));
            if kind == 0 {
                // A wait starts when a turn finished: one per answered turn.
                wait_ms_today += clip(a, b);
                replies.push(b - a);
                wait[i..=j].iter_mut().for_each(|x| *x = true);
                if a >= midnight {
                    turns_today += 1;
                    buckets[i].done += 1;
                }
            } else {
                agent_ms_today += clip(a, b);
                work[i..=j].iter_mut().for_each(|x| *x = true);
            }
        }
        // Its latest turn finished and is not answered yet.
        if matches!(
            s.status,
            LiveStatus::Done | LiveStatus::Asking | LiveStatus::Failed | LiveStatus::Scheduled
        ) && s.ended_at >= midnight
        {
            turns_today += 1;
            buckets[bucket_of(s.ended_at)].done += 1;
        }
        // Waiting on you right now: that wait counts up to now.
        if s.status.group() == Group::Awaiting {
            wait_ms_today += clip(s.status_since, now);
        }
        // A session working or awaiting you right now counts up to now.
        let now_in = match s.status.group() {
            Group::Active => Some(&mut work),
            Group::Awaiting => Some(&mut wait),
            _ => None,
        };
        if let Some(lane) = now_in {
            lane[bucket_of(s.status_since)..]
                .iter_mut()
                .for_each(|x| *x = true);
        }
        for (b, (w, a)) in buckets.iter_mut().zip(work.iter().zip(&wait)) {
            b.working += *w as usize;
            b.awaiting += *a as usize;
        }
        for a in &team.agents {
            if a.ended_at.is_some_and(|t| t >= midnight) {
                agents_done_today += (a.status == AgentStatus::Done) as usize;
                agents_failed_today += (a.status == AgentStatus::Failed) as usize;
            }
        }
    }
    replies.sort_unstable();
    let mut reply_histogram = [0usize; 5];
    for r in &replies {
        let i = match r / 60_000 {
            0 => 0,
            1..5 => 1,
            5..15 => 2,
            15..60 => 3,
            _ => 4,
        };
        reply_histogram[i] += 1;
    }

    let awaiting = |s: &LiveSession| s.status.group() == Group::Awaiting;
    let (mut live, mut completed) = (Vec::new(), Vec::new());
    for (s, _, _) in all.drain(..) {
        let done = s.status.group() == Group::Completed;
        if done && s.ended_at >= midnight {
            completed.push(s);
        } else if now - s.ended_at < LIVE_WINDOW_MS
            || awaiting(&s)
            || (s.status.is_live() && s.status != LiveStatus::Done)
        {
            live.push(s);
        }
    }

    Ok(LiveSummary {
        working: live
            .iter()
            .filter(|s| s.status.group() == Group::Active)
            .count(),
        awaiting: live.iter().filter(|s| awaiting(s)).count(),
        agents_running: live
            .iter()
            .flat_map(|s| &s.agents)
            .filter(|a| matches!(a.status, AgentLiveStatus::Running | AgentLiveStatus::Quiet))
            .count(),
        reply_median_ms: percentile(&replies, 0.5),
        reply_p75_ms: percentile(&replies, 0.75),
        reply_n: replies.len(),
        agent_ms_today,
        wait_ms_today,
        turns_today,
        agents_done_today,
        agents_failed_today,
        peak_parallel_today: buckets.iter().map(|b| b.working).max().unwrap_or(0),
        buckets,
        reply_histogram,
        midnight,
        sessions: live,
        completed_today: completed,
    })
}

/// A session that had a team: sub-agents or scheduled work.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSession {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub project_path: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: i64,
    pub tokens: i64,
    pub team: Team,
}

/// The most recent sessions that had a team, newest first.
pub fn teams(conn: &Connection) -> Result<Vec<TeamSession>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, title, project_path, started_at, ended_at, last_event, team,
                input_tokens + output_tokens + cache_read_tokens + cache_write_tokens
         FROM sessions
         WHERE json_array_length(team, '$.agents') > 0
            OR json_array_length(team, '$.scheduled') > 0
         ORDER BY ended_at DESC LIMIT 100",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let ended: i64 = r.get(5)?;
            let last_event: Option<String> = r.get(6)?;
            Ok(TeamSession {
                id: r.get(0)?,
                source: r.get(1)?,
                title: r.get(2)?,
                project_path: r.get(3)?,
                started_at: r.get(4)?,
                ended_at: ended,
                team: team_of(&r.get::<_, String>(7)?, last_event.as_deref(), ended),
                tokens: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(last_event: &str, quiet_ms: i64) -> Signals<'_> {
        Signals {
            last_event: Some(last_event),
            quiet_ms,
            ..Default::default()
        }
    }

    #[test]
    fn working_sessions_age_into_long_tasks_then_stale() {
        assert_eq!(status_for(st("working", 10_000)), LiveStatus::Working);
        assert_eq!(status_for(st("working", 5 * 60_000)), LiveStatus::LongTask);
        assert_eq!(status_for(st("working", 2 * 3_600_000)), LiveStatus::Stale);
    }

    #[test]
    fn pending_tools_tell_waiting_apart() {
        let mut s = st("working", 60_000);
        s.pending_tool = Some("Edit");
        assert_eq!(status_for(s), LiveStatus::MaybeBlocked);
        s.pending_tool = Some("Bash");
        assert_eq!(status_for(s), LiveStatus::Working);
        s.quiet_ms = 10 * 60_000;
        assert_eq!(status_for(s), LiveStatus::LongTask);
        s.pending_tool = Some("AskUserQuestion");
        s.quiet_ms = 1_000;
        assert_eq!(status_for(s), LiveStatus::Asking);
        // A pending tool with a busy team is the team working, not a prompt.
        s.pending_tool = Some("Agent");
        s.quiet_ms = 10 * 60_000;
        assert_eq!(status_for(s), LiveStatus::LongTask);
    }

    #[test]
    fn finished_turns_are_asking_done_failed_or_scheduled() {
        let mut s = st("turn_ended", 60_000);
        assert_eq!(status_for(s), LiveStatus::Done);
        s.last_message = Some("Should I also update the tests?");
        assert_eq!(status_for(s), LiveStatus::Asking);
        s.last_message = Some("All done.");
        s.upcoming_jobs = true;
        assert_eq!(status_for(s), LiveStatus::Scheduled);
        s.failed = true;
        assert_eq!(status_for(s), LiveStatus::Failed);
        s.team_busy = true;
        assert_eq!(status_for(s), LiveStatus::Working);
        assert_eq!(
            status_for(st("needs_permission", 60_000)),
            LiveStatus::NeedsPermission
        );
        // A prompt left through a meeting, or overnight, still waits on you.
        assert_eq!(
            status_for(st("needs_permission", 40 * 60_000)),
            LiveStatus::NeedsPermission
        );
        assert_eq!(
            status_for(st("needs_permission", AWAIT_HOLD_MS)),
            LiveStatus::Stale
        );
        let mut night = st("turn_ended", 10 * 3_600_000);
        night.last_message = Some("Want me to open the PR?");
        assert_eq!(status_for(night), LiveStatus::Asking);
        night.last_message = Some("All done.");
        assert_eq!(status_for(night), LiveStatus::Done);
        assert_eq!(status_for(st("closed", 0)), LiveStatus::Ended);
        assert_eq!(status_for(Signals::default()), LiveStatus::Stale);
    }

    #[test]
    fn percentiles() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[1, 2, 3, 4, 5], 0.5), Some(3));
        assert_eq!(percentile(&[1, 2, 3, 4], 0.75), Some(3));
    }
}
