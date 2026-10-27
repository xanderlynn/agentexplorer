//! Which agents are working, which are waiting on you, and for how long.

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

use crate::util::now_ms;

/// Sessions with activity inside this window appear on the Live page.
const LIVE_WINDOW_MS: i64 = 3 * 3_600_000;
/// A working agent silent this long may be stuck on an unlogged permission
/// prompt (Claude Code does not log those) or a long-running command.
const QUIET_AFTER_MS: i64 = 2 * 60_000;
const IDLE_AFTER_MS: i64 = 30 * 60_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSession {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub project_path: Option<String>,
    pub git_branch: Option<String>,
    /// working | quiet | needs_you | waiting | idle | ended
    pub state: String,
    pub last_event_at: i64,
    pub age_ms: i64,
    pub user_turns: i64,
    pub active_ms: i64,
    pub avg_wait_ms: Option<i64>,
    /// The agent's latest reply: what it is asking when it waits on you.
    pub last_message: Option<String>,
    pub outcome: Option<String>,
    /// What the latest turn was doing: coding, research, running, discussion.
    pub mode: Option<String>,
    /// Context sent with the latest request, in tokens.
    pub context: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSummary {
    pub sessions: Vec<LiveSession>,
    pub working: usize,
    pub waiting_on_you: usize,
    /// Time agents spent waiting on you across sessions started today.
    pub wait_ms_today: i64,
}

/// The mode of the most recent working span in a stored timeline.
fn last_mode(timeline: &str) -> Option<String> {
    let spans: Vec<(i64, i64, u8)> = serde_json::from_str(timeline).ok()?;
    let kind = spans.iter().rev().find(|s| s.2 != 0)?.2;
    crate::model::Mode::ALL
        .iter()
        .find(|m| **m as u8 == kind)
        .map(|m| m.key().to_string())
}

pub fn state_for(last_event: Option<&str>, age_ms: i64) -> &'static str {
    match last_event {
        Some("closed") => "ended",
        Some("needs_permission") => "needs_you",
        Some("turn_ended") if age_ms < LIVE_WINDOW_MS => "waiting",
        Some("working") if age_ms < QUIET_AFTER_MS => "working",
        Some("working") if age_ms < IDLE_AFTER_MS => "quiet",
        _ => "idle",
    }
}

pub fn summary(conn: &Connection) -> Result<LiveSummary> {
    let now = now_ms();
    let mut stmt = conn.prepare(
        "SELECT id, source, title, project_path, git_branch, last_event, ended_at, user_turns,
                active_ms, wait_ms_total, wait_count, last_message, outcome, timeline, last_context
         FROM sessions WHERE ended_at >= ?1 ORDER BY ended_at DESC LIMIT 50",
    )?;
    let sessions: Vec<LiveSession> = stmt
        .query_map([now - LIVE_WINDOW_MS], |r| {
            let last_event: Option<String> = r.get(5)?;
            let ended: i64 = r.get(6)?;
            let age = (now - ended).max(0);
            let wait_total: i64 = r.get(9)?;
            let wait_count: i64 = r.get(10)?;
            Ok(LiveSession {
                id: r.get(0)?,
                source: r.get(1)?,
                title: r.get(2)?,
                project_path: r.get(3)?,
                git_branch: r.get(4)?,
                state: state_for(last_event.as_deref(), age).to_string(),
                last_event_at: ended,
                age_ms: age,
                user_turns: r.get(7)?,
                active_ms: r.get(8)?,
                avg_wait_ms: (wait_count > 0).then(|| wait_total / wait_count),
                last_message: r.get(11)?,
                outcome: r.get(12)?,
                mode: last_mode(&r.get::<_, String>(13)?),
                context: r.get(14)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let midnight = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(chrono::Local).earliest())
        .map(|d| d.timestamp_millis())
        .unwrap_or(now - 86_400_000);
    let wait_ms_today: i64 = conn.query_row(
        "SELECT COALESCE(SUM(wait_ms_total), 0) FROM sessions WHERE started_at >= ?1",
        [midnight],
        |r| r.get(0),
    )?;

    Ok(LiveSummary {
        working: sessions
            .iter()
            .filter(|s| s.state == "working" || s.state == "quiet")
            .count(),
        waiting_on_you: sessions
            .iter()
            .filter(|s| s.state == "waiting" || s.state == "needs_you")
            .count(),
        sessions,
        wait_ms_today,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_age_out() {
        assert_eq!(state_for(Some("working"), 10_000), "working");
        assert_eq!(state_for(Some("working"), 5 * 60_000), "quiet");
        assert_eq!(state_for(Some("working"), 2 * 3_600_000), "idle");
        assert_eq!(state_for(Some("turn_ended"), 60_000), "waiting");
        assert_eq!(state_for(Some("needs_permission"), 60_000), "needs_you");
        assert_eq!(state_for(Some("closed"), 0), "ended");
        assert_eq!(state_for(None, 0), "idle");
    }
}
