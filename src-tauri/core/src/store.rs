//! The sessions table as the read models see it: one `SessionRow` per
//! session with its commit totals, loaded by date range, by filter or by id.
//! Read models build on these rows instead of querying the table themselves.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Datelike, Local, TimeZone, Timelike};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::journal::outcome_score;
use crate::signals::{self, Friction};
use crate::stats::DAY_MS;
use crate::util::now_ms;

#[derive(Debug, Clone, Copy)]
pub struct Range {
    pub since: i64,
    pub until: i64,
}

impl Range {
    /// `days == 0` means all time.
    pub fn last_days(days: i64) -> Self {
        let until = now_ms() + 1;
        let since = if days <= 0 { 0 } else { until - days * DAY_MS };
        Range { since, until }
    }

    pub fn previous(&self) -> Option<Range> {
        (self.since > 0).then(|| Range {
            since: self.since - (self.until - self.since),
            until: self.since,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: String,
    pub source: String,
    /// Which app ran it: "cli", "claude-desktop", "copilot-app", "vscode"…
    pub entrypoint: Option<String>,
    /// Pull requests it opened or worked on.
    pub prs: Vec<String>,
    /// For an unrated session, the outcome its git work points to, and why:
    /// for you to confirm, never counted until you do.
    pub suggested_outcome: Option<String>,
    pub evidence: Option<String>,
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub project_path: Option<String>,
    pub repo_root: Option<String>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub active_ms: i64,
    pub user_turns: i64,
    pub assistant_turns: i64,
    pub tool_calls: i64,
    pub tool_errors: i64,
    pub interruptions: i64,
    pub permission_requests: i64,
    pub cost_usd: Option<f64>,
    pub cost_is_estimate: bool,
    pub premium_requests: Option<f64>,
    pub outcome: Option<String>,
    pub rating: Option<i64>,
    pub commits: i64,
    pub insertions: i64,
    pub deletions: i64,
    pub ai_commits: i64,
    pub wait_ms_total: i64,
    pub wait_count: i64,
    pub first_prompt_len: i64,
    pub tokens: i64,
    pub last_event: Option<String>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub reads: i64,
    pub edits: i64,
    pub shell_calls: i64,
    pub edits_without_read: i64,
    pub re_edits: i64,
    pub compactions: i64,
    pub subagents: i64,
    pub corrections: i64,
    pub friction: Friction,
    /// False for sessions parsed before tool, correction and category
    /// signals existed; they are left out of statistics built on them.
    pub has_signals: bool,
    /// "Faster than doing it by hand?": faster, slower, same or unsure.
    pub vs_manual: Option<String>,
    /// A commit from this session was reverted or fixed within 7 days.
    pub reworked: bool,
    /// Time the agent spent working on your prompts.
    pub agent_ms: i64,
    /// Plan mode was used, or the session came from a pre-flight plan.
    pub planned: bool,
    /// Largest context sent to the model in this session, in tokens.
    pub peak_context: i64,
    /// Agent working time per mode: coding, research, running, discussion.
    pub mode_ms: BTreeMap<String, i64>,
    pub style: &'static str,
}

/// one_shot (a single prompt), planned (plan mode or a pre-flight plan),
/// or iterative (several prompts without a plan).
fn style_of(planned: bool, user_turns: i64) -> &'static str {
    if planned {
        "planned"
    } else if user_turns <= 1 {
        "one_shot"
    } else {
        "iterative"
    }
}

impl SessionRow {
    pub fn score(&self) -> Option<f64> {
        self.outcome.as_deref().and_then(outcome_score)
    }
}

const SESSION_SELECT: &str = "
SELECT s.id, s.source, s.title, substr(s.first_prompt, 1, 300), s.project_path, s.repo_root,
       s.git_branch, s.model, s.started_at, s.ended_at, s.active_ms, s.user_turns,
       s.assistant_turns, s.tool_calls, s.tool_errors, s.interruptions, s.permission_requests,
       s.cost_usd, s.cost_is_estimate, s.premium_requests, s.outcome, s.rating,
       COALESCE(c.n, 0), COALESCE(c.ins, 0), COALESCE(c.del, 0), COALESCE(c.ai, 0),
       s.wait_ms_total, s.wait_count, length(COALESCE(s.first_prompt, '')),
       s.input_tokens + s.output_tokens + s.cache_read_tokens + s.cache_write_tokens,
       s.last_event, s.tags, s.category, s.reads, s.edits, s.shell_calls,
       s.edits_without_read, s.re_edits, s.compactions, s.subagents, s.corrections,
       s.signals_version >= 1, s.ask_score, s.vs_manual, COALESCE(c.rw, 0) > 0, s.agent_ms,
       s.planned OR EXISTS(SELECT 1 FROM intents i WHERE i.session_id = s.id),
       s.peak_context, s.mode_ms, s.entrypoint, s.prs,
       (SELECT json_group_object(p.url, p.state) FROM json_each(s.prs) j JOIN pr_states p ON p.url = j.value)
FROM sessions s
LEFT JOIN (SELECT session_id, COUNT(*) AS n, SUM(insertions) AS ins, SUM(deletions) AS del,
                  SUM(ai_trailer) AS ai, SUM(reworked) AS rw
           FROM session_commits GROUP BY session_id) c ON c.session_id = s.id";

fn map_row(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
    let tags: String = r.get(31)?;
    let first_prompt: Option<String> = r.get(3)?;
    let corrections: i64 = r.get(40)?;
    let interruptions: i64 = r.get(15)?;
    let user_turns: i64 = r.get(11)?;
    let active_ms: i64 = r.get(10)?;
    let planned: bool = r.get(46)?;
    let prs: Vec<String> = serde_json::from_str(&r.get::<_, String>(50)?).unwrap_or_default();
    let outcome: Option<String> = r.get(20)?;
    let states: BTreeMap<String, String> = r
        .get::<_, Option<String>>(51)?
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let (suggested_outcome, evidence) =
        suggest(outcome.as_deref(), r.get(22)?, r.get(44)?, &prs, &states);
    Ok(SessionRow {
        prs,
        suggested_outcome,
        evidence,
        friction: signals::friction(
            match r.get::<_, i64>(42)? {
                // Parsed before ask scores existed: score the stored opening.
                0 => signals::check_ask(first_prompt.as_deref().unwrap_or("")).score,
                n => n as u8,
            },
            corrections,
            interruptions,
            user_turns,
            active_ms / 60_000,
        ),
        id: r.get(0)?,
        source: r.get(1)?,
        entrypoint: r.get(49)?,
        title: r.get(2)?,
        first_prompt: r.get(3)?,
        project_path: r.get(4)?,
        repo_root: r.get(5)?,
        git_branch: r.get(6)?,
        model: r.get(7)?,
        started_at: r.get(8)?,
        ended_at: r.get(9)?,
        active_ms: r.get(10)?,
        user_turns: r.get(11)?,
        assistant_turns: r.get(12)?,
        tool_calls: r.get(13)?,
        tool_errors: r.get(14)?,
        interruptions: r.get(15)?,
        permission_requests: r.get(16)?,
        cost_usd: r.get(17)?,
        cost_is_estimate: r.get(18)?,
        premium_requests: r.get(19)?,
        outcome,
        rating: r.get(21)?,
        commits: r.get(22)?,
        insertions: r.get(23)?,
        deletions: r.get(24)?,
        ai_commits: r.get(25)?,
        wait_ms_total: r.get(26)?,
        wait_count: r.get(27)?,
        first_prompt_len: r.get(28)?,
        tokens: r.get(29)?,
        last_event: r.get(30)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        category: r.get(32)?,
        reads: r.get(33)?,
        edits: r.get(34)?,
        shell_calls: r.get(35)?,
        edits_without_read: r.get(36)?,
        re_edits: r.get(37)?,
        compactions: r.get(38)?,
        subagents: r.get(39)?,
        corrections,
        has_signals: r.get(41)?,
        vs_manual: r.get(43)?,
        reworked: r.get(44)?,
        agent_ms: r.get(45)?,
        planned,
        peak_context: r.get(47)?,
        mode_ms: serde_json::from_str(&r.get::<_, String>(48)?).unwrap_or_default(),
        style: style_of(planned, user_turns),
    })
}

/// What an unrated session's git work says about it, for you to confirm: a
/// merged pull request, or commits that stood, look shipped; a pull request
/// closed without merging looks abandoned. An open one, or reworked commits,
/// suggest nothing yet.
fn suggest(
    outcome: Option<&str>,
    commits: i64,
    reworked: bool,
    prs: &[String],
    states: &BTreeMap<String, String>,
) -> (Option<String>, Option<String>) {
    if outcome.is_some() || reworked {
        return (None, None);
    }
    let num = |pr: &str| {
        pr.rsplit_once("/pull/")
            .map(|(_, n)| format!("PR #{n}"))
            .unwrap_or_else(|| "a PR".into())
    };
    let state = |pr: &String| states.get(pr).map(String::as_str);
    let merged: Vec<String> = prs
        .iter()
        .filter(|p| state(p) == Some("MERGED"))
        .map(|p| format!("{} merged", num(p)))
        .collect();
    let mut why = merged.clone();
    if commits > 0 {
        why.push(format!(
            "{commits} commit{}",
            if commits == 1 { "" } else { "s" }
        ));
    }
    if !why.is_empty() {
        return (Some("shipped".into()), Some(why.join(", ")));
    }
    if !prs.is_empty() && prs.iter().all(|p| state(p) == Some("CLOSED")) {
        return (
            Some("abandoned".into()),
            Some(format!("{} closed without merging", num(&prs[0]))),
        );
    }
    (None, None)
}

/// Pull requests from the last `days` days not yet checked, or still open
/// and last checked over an hour ago: what to ask gh about next.
pub fn prs_to_check(conn: &Connection, days: i64, limit: usize) -> Result<Vec<String>> {
    let now = now_ms();
    let mut st = conn.prepare(
        "SELECT DISTINCT j.value FROM sessions s, json_each(s.prs) j
         LEFT JOIN pr_states p ON p.url = j.value
         WHERE s.ended_at >= ?1
           AND (p.url IS NULL OR (p.state = 'OPEN' AND p.checked_at < ?2))
         LIMIT ?3",
    )?;
    let urls = st
        .query_map(
            params![now - days * DAY_MS, now - 3_600_000, limit as i64],
            |r| r.get(0),
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(urls)
}

/// Records a pull request's state: OPEN, MERGED or CLOSED.
pub fn record_pr_state(conn: &Connection, url: &str, state: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO pr_states (url, state, checked_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(url) DO UPDATE SET state = excluded.state, checked_at = excluded.checked_at",
        params![url, state, now_ms()],
    )?;
    Ok(())
}

/// Confirms an outcome for many sessions at once.
pub fn bulk_review(conn: &Connection, ids: &[String], outcome: &str) -> Result<usize> {
    for id in ids {
        crate::analytics::quick_review(conn, id, outcome, None)?;
    }
    Ok(ids.len())
}

pub fn load_sessions(conn: &Connection, range: Range) -> Result<Vec<SessionRow>> {
    let sql = format!(
        "{SESSION_SELECT} WHERE s.started_at >= ?1 AND s.started_at < ?2 ORDER BY s.started_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![range.since, range.until], map_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFilter {
    pub days: i64,
    pub source: Option<String>,
    /// An outcome, or "unrated".
    pub outcome: Option<String>,
    pub search: Option<String>,
    pub project: Option<String>,
    pub category: Option<String>,
    pub model: Option<String>,
    /// Local calendar date, YYYY-MM-DD.
    pub date: Option<String>,
    /// 0 = Monday.
    pub weekday: Option<u32>,
    pub hour: Option<u32>,
    /// Exactly these sessions (from an insight).
    pub ids: Option<Vec<String>>,
}

/// Rows shown in the Sessions table; exports are not capped.
const LIST_LIMIT: usize = 2000;

pub fn list_sessions(conn: &Connection, f: &SessionFilter) -> Result<Vec<SessionRow>> {
    let mut rows = filter_sessions(conn, f)?;
    rows.truncate(LIST_LIMIT);
    Ok(rows)
}

pub fn filter_sessions(conn: &Connection, f: &SessionFilter) -> Result<Vec<SessionRow>> {
    let needle = f
        .search
        .as_deref()
        .map(str::to_lowercase)
        .filter(|s| !s.is_empty());
    // Search every prompt, not just the opening one.
    // Lowercasing happens in Rust: SQLite's lower() only handles ASCII.
    let range = Range::last_days(f.days);
    let mut prompt_hits: std::collections::HashSet<String> = Default::default();
    if let Some(n) = &needle {
        let mut stmt = conn.prepare(
            "SELECT p.session_id, p.text FROM prompts p JOIN sessions s ON s.id = p.session_id
             WHERE s.started_at >= ?1 AND s.started_at < ?2",
        )?;
        for row in stmt.query_map(params![range.since, range.until], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, text) = row?;
            if !prompt_hits.contains(&id) && text.to_lowercase().contains(n.as_str()) {
                prompt_hits.insert(id);
            }
        }
    }
    let rows = load_sessions(conn, range)?
        .into_iter()
        .filter(|s| {
            f.category
                .as_deref()
                .is_none_or(|c| c.is_empty() || s.category.as_deref() == Some(c))
        })
        .filter(|s| match f.model.as_deref() {
            None => true,
            // Usage groups sessions without a recorded model as "unknown".
            Some("unknown") => s.model.is_none(),
            Some(m) => s.model.as_deref() == Some(m),
        })
        .filter(|s| f.ids.as_ref().is_none_or(|ids| ids.contains(&s.id)))
        .filter(|s| {
            if f.date.is_none() && f.weekday.is_none() && f.hour.is_none() {
                return true;
            }
            let Some(t) = s
                .started_at
                .and_then(|t| Local.timestamp_millis_opt(t).single())
            else {
                return false;
            };
            f.date
                .as_deref()
                .is_none_or(|d| t.format("%Y-%m-%d").to_string() == d)
                && f.weekday
                    .is_none_or(|w| t.weekday().num_days_from_monday() == w)
                && f.hour.is_none_or(|h| t.hour() == h)
        })
        .filter(|s| f.source.as_deref().is_none_or(|src| src == s.source))
        .filter(|s| match f.outcome.as_deref() {
            None | Some("") => true,
            Some("unrated") => s.outcome.is_none(),
            Some(o) => s.outcome.as_deref() == Some(o),
        })
        .filter(|s| {
            f.project
                .as_deref()
                .is_none_or(|p| s.repo_root.as_deref().or(s.project_path.as_deref()) == Some(p))
        })
        .filter(|s| {
            needle.as_deref().is_none_or(|n| {
                prompt_hits.contains(&s.id)
                    || [&s.title, &s.first_prompt, &s.project_path, &s.git_branch]
                        .iter()
                        .any(|field| {
                            field
                                .as_deref()
                                .is_some_and(|v| v.to_lowercase().contains(n))
                        })
            })
        })
        .collect();
    Ok(rows)
}

/// One session by id.
pub fn session(conn: &Connection, id: &str) -> Result<Option<SessionRow>> {
    let sql = format!("{SESSION_SELECT} WHERE s.id = ?1");
    Ok(conn.query_row(&sql, [id], map_row).optional()?)
}

/// How many sessions a tool has recorded.
pub fn count_by_source(conn: &Connection, source: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM sessions WHERE source = ?1",
        [source],
        |r| r.get(0),
    )?)
}

/// Repositories and folders with sessions, most recently used first.
pub fn recent_projects(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(repo_root, project_path) AS p FROM sessions
         WHERE p IS NOT NULL GROUP BY p ORDER BY MAX(started_at) DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit as i64], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

#[cfg(test)]
mod suggest_tests {
    use super::*;

    #[test]
    fn suggests_from_merged_prs_and_commits_only() {
        let pr = |n: u32| format!("https://github.com/o/r/pull/{n}");
        let states: BTreeMap<String, String> = [
            (pr(1), "MERGED".to_string()),
            (pr(2), "CLOSED".into()),
            (pr(3), "OPEN".into()),
        ]
        .into();
        let s = |c: i64, rw: bool, prs: &[String]| suggest(None, c, rw, prs, &states);
        assert_eq!(
            s(0, false, &[pr(1)]),
            (Some("shipped".into()), Some("PR #1 merged".into()))
        );
        assert_eq!(
            s(2, false, &[]),
            (Some("shipped".into()), Some("2 commits".into()))
        );
        assert_eq!(s(0, false, &[pr(2)]).0.as_deref(), Some("abandoned"));
        // Open, or not checked yet: nothing to suggest.
        assert_eq!(s(0, false, &[pr(3)]), (None, None));
        assert_eq!(s(0, false, &[pr(9)]), (None, None));
        assert_eq!(s(3, true, &[pr(1)]), (None, None));
        assert_eq!(
            suggest(Some("partial"), 3, false, &[pr(1)], &states),
            (None, None)
        );
    }

    #[test]
    fn queues_prs_to_check_once() {
        let conn = crate::db::open_in_memory().unwrap();
        let now = now_ms();
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, started_at, ended_at, active_ms, user_turns, prs, ingested_at)
             VALUES ('a', 'claude_code', 'a', '/f', ?1, ?1, 0, 1, '[\"https://github.com/o/r/pull/1\",\"https://github.com/o/r/pull/2\"]', ?1)",
            [now],
        )
        .unwrap();
        assert_eq!(prs_to_check(&conn, 30, 10).unwrap().len(), 2);
        record_pr_state(&conn, "https://github.com/o/r/pull/1", "MERGED").unwrap();
        record_pr_state(&conn, "https://github.com/o/r/pull/2", "OPEN").unwrap();
        // Merged is final; open is checked again after an hour.
        assert!(prs_to_check(&conn, 30, 10).unwrap().is_empty());
        let row = session(&conn, "a").unwrap().unwrap();
        assert_eq!(row.suggested_outcome.as_deref(), Some("shipped"));
        assert_eq!(row.evidence.as_deref(), Some("PR #1 merged"));
    }
}
