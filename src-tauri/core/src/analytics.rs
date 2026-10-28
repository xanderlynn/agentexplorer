//! Read models for the UI: session detail and reviews, the overview, the
//! scorecard, and rule-based insights that point at specific habits to change.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use chrono::{Duration, Local, NaiveDate, TimeZone, Timelike};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::journal;
use crate::model::Source;
use crate::stats::{
    HOUR_MS, MIN_SAMPLE, local_date, local_midnight_ms, monday, pct, success, week_of,
};
use crate::store::{self, Range, SessionRow, load_sessions};
use crate::util::now_ms;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRow {
    pub idx: i64,
    pub ts: Option<i64>,
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRow {
    pub sha: String,
    pub repo_root: String,
    pub committed_at: i64,
    pub subject: String,
    pub insertions: i64,
    pub deletions: i64,
    pub files: i64,
    pub ai_trailer: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPlaybook {
    pub version_id: i64,
    pub playbook_id: i64,
    pub name: String,
    pub kind: String,
    pub first_seen: i64,
    pub link: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedIntent {
    pub id: i64,
    pub goal: String,
    pub expected_minutes: i64,
    pub confidence: i64,
    pub outcome: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: SessionRow,
    pub file_path: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub notes: Option<String>,
    pub tool_counts: BTreeMap<String, i64>,
    pub tool_errors_by: BTreeMap<String, i64>,
    pub tool_vetoes: BTreeMap<String, i64>,
    pub last_message: Option<String>,
    /// A command that reopens the session in its tool, when it has one.
    pub resume_command: Option<String>,
    pub prompts: Vec<PromptRow>,
    pub commits: Vec<CommitRow>,
    pub playbooks: Vec<SessionPlaybook>,
    pub intent: Option<LinkedIntent>,
    pub team: crate::team::Team,
    /// The lead's (start, end, kind) spans: 0 waiting on you, else a mode.
    pub timeline: Vec<(i64, i64, u8)>,
}

pub fn session_detail(conn: &Connection, id: &str) -> Result<SessionDetail> {
    let Some(session) = store::session(conn, id)? else {
        bail!("Session not found");
    };
    struct Extra {
        file_path: String,
        tokens: [i64; 4],
        lines: (Option<i64>, Option<i64>),
        notes: Option<String>,
        tool_counts: String,
        last_message: Option<String>,
        external_id: String,
        tool_errors_by: String,
        tool_vetoes: String,
        team: crate::team::Team,
        timeline: String,
    }
    let x = conn.query_row(
        "SELECT file_path, input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, lines_added, lines_removed, notes, tool_counts,
                last_message, external_id, tool_errors_by, tool_vetoes, team, last_event,
                ended_at, timeline
         FROM sessions WHERE id = ?1",
        [id],
        |r| {
            Ok(Extra {
                file_path: r.get(0)?,
                tokens: [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?],
                lines: (r.get(5)?, r.get(6)?),
                notes: r.get(7)?,
                tool_counts: r.get(8)?,
                last_message: r.get(9)?,
                external_id: r.get(10)?,
                tool_errors_by: r.get(11)?,
                tool_vetoes: r.get(12)?,
                team: crate::live::team_of(
                    &r.get::<_, String>(13)?,
                    r.get::<_, Option<String>>(14)?.as_deref(),
                    r.get(15)?,
                ),
                timeline: r.get(16)?,
            })
        },
    )?;
    let resume_command = resume_command(&session, &x.external_id);
    let prompts = conn
        .prepare("SELECT idx, ts, text FROM prompts WHERE session_id = ?1 ORDER BY idx")?
        .query_map([id], |r| {
            Ok(PromptRow {
                idx: r.get(0)?,
                ts: r.get(1)?,
                text: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let commits = conn
        .prepare(
            "SELECT sha, repo_root, committed_at, subject, insertions, deletions, files, ai_trailer
             FROM session_commits WHERE session_id = ?1 ORDER BY committed_at",
        )?
        .query_map([id], |r| {
            Ok(CommitRow {
                sha: r.get(0)?,
                repo_root: r.get(1)?,
                committed_at: r.get(2)?,
                subject: r.get(3)?,
                insertions: r.get(4)?,
                deletions: r.get(5)?,
                files: r.get(6)?,
                ai_trailer: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let playbooks = conn
        .prepare(
            "SELECT v.id, p.id, p.name, p.kind, v.first_seen, sp.link
             FROM session_playbooks sp
             JOIN playbook_versions v ON v.id = sp.version_id
             JOIN playbooks p ON p.id = v.playbook_id
             WHERE sp.session_id = ?1 ORDER BY p.kind, p.name",
        )?
        .query_map([id], |r| {
            Ok(SessionPlaybook {
                version_id: r.get(0)?,
                playbook_id: r.get(1)?,
                name: r.get(2)?,
                kind: r.get(3)?,
                first_seen: r.get(4)?,
                link: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let intent = conn
        .query_row(
            "SELECT id, goal, expected_minutes, confidence, outcome FROM intents WHERE session_id = ?1",
            [id],
            |r| {
                Ok(LinkedIntent {
                    id: r.get(0)?,
                    goal: r.get(1)?,
                    expected_minutes: r.get(2)?,
                    confidence: r.get(3)?,
                    outcome: r.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(SessionDetail {
        session,
        file_path: x.file_path,
        input_tokens: x.tokens[0],
        output_tokens: x.tokens[1],
        cache_read_tokens: x.tokens[2],
        cache_write_tokens: x.tokens[3],
        lines_added: x.lines.0,
        lines_removed: x.lines.1,
        notes: x.notes,
        tool_counts: serde_json::from_str(&x.tool_counts).unwrap_or_default(),
        tool_errors_by: serde_json::from_str(&x.tool_errors_by).unwrap_or_default(),
        tool_vetoes: serde_json::from_str(&x.tool_vetoes).unwrap_or_default(),
        last_message: x.last_message,
        resume_command,
        prompts,
        commits,
        playbooks,
        intent,
        team: x.team,
        timeline: serde_json::from_str(&x.timeline).unwrap_or_default(),
    })
}

/// The command that reopens the session in its tool, when it has one.
fn resume_command(s: &SessionRow, external_id: &str) -> Option<String> {
    Source::from_id(&s.source)?.resume_command(s.project_path.as_deref(), external_id)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: String,
    pub outcome: Option<String>,
    pub rating: Option<i64>,
    pub notes: Option<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub vs_manual: Option<String>,
}

pub const VS_MANUAL: [&str; 4] = ["faster", "slower", "same", "unsure"];

fn check_vs_manual(v: Option<&str>) -> Result<()> {
    if let Some(v) = v
        && !VS_MANUAL.contains(&v)
    {
        bail!("Unknown answer {v}");
    }
    Ok(())
}

/// The one-tap review: outcome and "faster than by hand?", keeping any
/// rating, notes and tags already written.
pub fn quick_review(
    conn: &Connection,
    id: &str,
    outcome: &str,
    vs_manual: Option<&str>,
) -> Result<()> {
    if !journal::OUTCOMES.contains(&outcome) {
        bail!("Unknown outcome {outcome}");
    }
    check_vs_manual(vs_manual)?;
    conn.execute(
        "UPDATE sessions SET outcome = ?1, vs_manual = COALESCE(?2, vs_manual), reviewed_at = ?3 WHERE id = ?4",
        params![outcome, vs_manual, now_ms(), id],
    )?;
    Ok(())
}

pub fn save_review(conn: &Connection, r: &Review) -> Result<()> {
    check_vs_manual(r.vs_manual.as_deref())?;
    if let Some(o) = r.outcome.as_deref()
        && !journal::OUTCOMES.contains(&o)
    {
        bail!("Unknown outcome {o}");
    }
    if r.rating.is_some_and(|x| !(1..=5).contains(&x)) {
        bail!("Rating must be 1–5");
    }
    // Experiment tags ("exp-N") are owned by the experiment toggles; keep
    // whatever is stored rather than trusting a possibly stale form.
    let stored: String = conn
        .query_row("SELECT tags FROM sessions WHERE id = ?1", [&r.id], |row| {
            row.get(0)
        })
        .optional()?
        .unwrap_or_else(|| "[]".into());
    let mut tags: Vec<String> = r
        .tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty() && !t.starts_with("exp-"))
        .collect();
    tags.extend(
        serde_json::from_str::<Vec<String>>(&stored)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.starts_with("exp-")),
    );
    conn.execute(
        "UPDATE sessions SET outcome = ?1, rating = ?2, notes = ?3, tags = ?4, reviewed_at = ?5,
                vs_manual = ?7 WHERE id = ?6",
        params![
            r.outcome,
            r.rating,
            r.notes.as_deref().filter(|n| !n.trim().is_empty()),
            serde_json::to_string(&tags)?,
            now_ms(),
            r.id,
            r.vs_manual
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- overview

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub sessions: usize,
    pub active_hours: f64,
    pub user_turns: i64,
    pub tool_calls: i64,
    pub tool_errors: i64,
    pub interruptions: i64,
    pub cost_usd: f64,
    pub cost_has_estimates: bool,
    pub premium_requests: f64,
    pub tokens: i64,
    pub commits: i64,
    pub ai_commits: i64,
    pub rated: usize,
    pub shipped: usize,
    pub success_rate: Option<f64>,
    pub avg_wait_seconds: Option<f64>,
    pub wait_hours: f64,
}

fn totals(rows: &[SessionRow]) -> Totals {
    let mut t = Totals {
        sessions: rows.len(),
        ..Default::default()
    };
    let mut wait_ms = 0i64;
    let mut wait_n = 0i64;
    for s in rows {
        t.active_hours += s.active_ms as f64 / HOUR_MS;
        t.user_turns += s.user_turns;
        t.tool_calls += s.tool_calls;
        t.tool_errors += s.tool_errors;
        t.interruptions += s.interruptions;
        if let Some(c) = s.cost_usd {
            t.cost_usd += c;
            t.cost_has_estimates |= s.cost_is_estimate;
        }
        t.premium_requests += s.premium_requests.unwrap_or(0.0);
        t.tokens += s.tokens;
        t.commits += s.commits;
        t.ai_commits += s.ai_commits;
        if s.outcome.is_some() {
            t.rated += 1;
        }
        if s.outcome.as_deref() == Some("shipped") {
            t.shipped += 1;
        }
        wait_ms += s.wait_ms_total;
        wait_n += s.wait_count;
    }
    t.success_rate = success_rate(rows.iter());
    t.avg_wait_seconds = (wait_n > 0).then(|| wait_ms as f64 / wait_n as f64 / 1000.0);
    t.wait_hours = wait_ms as f64 / HOUR_MS;
    t
}

fn success_rate<'a>(rows: impl IntoIterator<Item = &'a SessionRow>) -> Option<f64> {
    success(rows).1
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupStat {
    pub key: String,
    pub sessions: usize,
    pub active_hours: f64,
    pub cost_usd: f64,
    pub commits: i64,
    pub rated: usize,
    pub success_rate: Option<f64>,
    pub avg_turns: f64,
    pub avg_interruptions: f64,
}

fn group_by(rows: &[SessionRow], key: impl Fn(&SessionRow) -> Option<String>) -> Vec<GroupStat> {
    let mut groups: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
    for s in rows {
        if let Some(k) = key(s) {
            groups.entry(k).or_default().push(s);
        }
    }
    let mut out: Vec<GroupStat> = groups
        .into_iter()
        .map(|(key, g)| {
            let n = g.len() as f64;
            GroupStat {
                key,
                sessions: g.len(),
                active_hours: g.iter().map(|s| s.active_ms as f64 / HOUR_MS).sum(),
                cost_usd: g.iter().filter_map(|s| s.cost_usd).sum(),
                commits: g.iter().map(|s| s.commits).sum(),
                rated: g.iter().filter(|s| s.outcome.is_some()).count(),
                success_rate: success_rate(g.iter().copied()),
                avg_turns: g.iter().map(|s| s.user_turns as f64).sum::<f64>() / n,
                avg_interruptions: g.iter().map(|s| s.interruptions as f64).sum::<f64>() / n,
            }
        })
        .collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.sessions));
    out
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStat {
    pub date: String,
    pub sessions: usize,
    pub active_minutes: f64,
    pub cost_usd: f64,
    pub shipped: usize,
    pub commits: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub totals: Totals,
    pub previous: Option<Totals>,
    pub by_source: Vec<GroupStat>,
    pub by_project: Vec<GroupStat>,
    pub by_model: Vec<GroupStat>,
    pub daily: Vec<DayStat>,
    pub outcomes: BTreeMap<String, usize>,
    /// The last 12 weeks, independent of the selected range, so every KPI
    /// can show its trend, usual range and personal best.
    pub trend: Trend,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trend {
    /// Monday of each week, oldest first.
    pub weeks: Vec<String>,
    pub sessions: Vec<f64>,
    pub active_hours: Vec<f64>,
    pub success_rate: Vec<Option<f64>>,
    pub rated: Vec<f64>,
    pub commits: Vec<f64>,
    pub shipped: Vec<f64>,
    pub wait_hours: Vec<f64>,
    pub interruptions_per_session: Vec<Option<f64>>,
    pub tool_error_rate: Vec<Option<f64>>,
    pub cost_usd: Vec<f64>,
}

pub const TREND_WEEKS: i64 = 12;

fn trend(conn: &Connection) -> Result<Trend> {
    let today = Local::now().date_naive();
    let first = monday(today) - Duration::weeks(TREND_WEEKS - 1);
    let since = local_midnight_ms(first);
    let rows = load_sessions(
        conn,
        Range {
            since,
            until: now_ms() + 1,
        },
    )?;
    let mut weeks: Vec<Vec<&SessionRow>> = vec![vec![]; TREND_WEEKS as usize];
    for s in &rows {
        if let Some(d) = s.started_at.and_then(local_date) {
            let i = (d - first).num_days() / 7;
            if (0..TREND_WEEKS).contains(&i) {
                weeks[i as usize].push(s);
            }
        }
    }
    let mut t = Trend::default();
    for (i, w) in weeks.iter().enumerate() {
        let tt = totals(&w.iter().map(|s| (*s).clone()).collect::<Vec<_>>());
        t.weeks.push(
            (first + Duration::weeks(i as i64))
                .format("%Y-%m-%d")
                .to_string(),
        );
        t.sessions.push(tt.sessions as f64);
        t.active_hours.push(tt.active_hours);
        t.success_rate.push(tt.success_rate);
        t.rated.push(tt.rated as f64);
        t.commits.push(tt.commits as f64);
        t.shipped.push(tt.shipped as f64);
        t.wait_hours.push(tt.wait_hours);
        t.interruptions_per_session
            .push((tt.sessions > 0).then(|| tt.interruptions as f64 / tt.sessions as f64));
        t.tool_error_rate
            .push((tt.tool_calls > 0).then(|| tt.tool_errors as f64 / tt.tool_calls as f64));
        t.cost_usd.push(tt.cost_usd);
    }
    Ok(t)
}

pub fn project_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

pub fn overview(conn: &Connection, days: i64) -> Result<Overview> {
    let range = Range::last_days(days);
    let rows = load_sessions(conn, range)?;
    let previous = match range.previous() {
        Some(p) => Some(totals(&load_sessions(conn, p)?)),
        None => None,
    };

    let mut daily: BTreeMap<NaiveDate, DayStat> = BTreeMap::new();
    let first_day = rows
        .iter()
        .filter_map(|s| s.started_at)
        .min()
        .and_then(local_date);
    let start = if range.since > 0 {
        local_date(range.since)
    } else {
        first_day
    };
    if let (Some(mut d), Some(end)) = (start, local_date(now_ms())) {
        while d <= end {
            daily.insert(
                d,
                DayStat {
                    date: d.format("%Y-%m-%d").to_string(),
                    sessions: 0,
                    active_minutes: 0.0,
                    cost_usd: 0.0,
                    shipped: 0,
                    commits: 0,
                },
            );
            d += Duration::days(1);
        }
    }
    let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
    for s in &rows {
        *outcomes
            .entry(s.outcome.clone().unwrap_or_else(|| "unrated".into()))
            .or_default() += 1;
        if let Some(day) = s
            .started_at
            .and_then(local_date)
            .and_then(|d| daily.get_mut(&d))
        {
            day.sessions += 1;
            day.active_minutes += s.active_ms as f64 / 60_000.0;
            day.cost_usd += s.cost_usd.unwrap_or(0.0);
            day.commits += s.commits;
            if s.outcome.as_deref() == Some("shipped") {
                day.shipped += 1;
            }
        }
    }

    let mut by_project = group_by(&rows, |s| {
        s.repo_root.clone().or_else(|| s.project_path.clone())
    });
    by_project.truncate(12);
    Ok(Overview {
        totals: totals(&rows),
        previous,
        by_source: group_by(&rows, |s| Some(s.source.clone())),
        by_project,
        by_model: group_by(&rows, |s| s.model.clone()),
        daily: daily.into_values().collect(),
        outcomes,
        trend: trend(conn)?,
    })
}

// --------------------------------------------------------------- scorecard

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekStat {
    pub week_start: String,
    pub sessions: usize,
    pub sessions_with_commits: usize,
    pub commits: i64,
    pub shipped: usize,
    pub active_hours: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scorecard {
    pub sessions: usize,
    pub sessions_in_repos: usize,
    pub sessions_with_commits: usize,
    /// Share of sessions in a git repo that produced at least one commit.
    pub commit_rate: Option<f64>,
    pub commits: i64,
    pub ai_commits: i64,
    pub insertions: i64,
    pub deletions: i64,
    pub cost_per_commit: Option<f64>,
    pub active_minutes_per_commit: Option<f64>,
    pub cost_per_shipped: Option<f64>,
    pub shipped: usize,
    pub by_source: Vec<GroupStat>,
    pub by_project: Vec<GroupStat>,
    pub weekly: Vec<WeekStat>,
}

pub fn scorecard(conn: &Connection, days: i64) -> Result<Scorecard> {
    let rows = load_sessions(conn, Range::last_days(days))?;
    let in_repos: Vec<&SessionRow> = rows.iter().filter(|s| s.repo_root.is_some()).collect();
    let with_commits = rows.iter().filter(|s| s.commits > 0).count();
    let commits: i64 = rows.iter().map(|s| s.commits).sum();
    let cost: f64 = rows.iter().filter_map(|s| s.cost_usd).sum();
    let active_min: f64 = rows.iter().map(|s| s.active_ms as f64 / 60_000.0).sum();
    let shipped = rows
        .iter()
        .filter(|s| s.outcome.as_deref() == Some("shipped"))
        .count();
    let per = |x: f64, n: f64| (n > 0.0).then(|| x / n);

    let mut weekly: BTreeMap<NaiveDate, WeekStat> = BTreeMap::new();
    for s in &rows {
        let Some(monday) = s.started_at.and_then(week_of) else {
            continue;
        };
        let w = weekly.entry(monday).or_insert_with(|| WeekStat {
            week_start: monday.format("%Y-%m-%d").to_string(),
            sessions: 0,
            sessions_with_commits: 0,
            commits: 0,
            shipped: 0,
            active_hours: 0.0,
        });
        w.sessions += 1;
        w.commits += s.commits;
        w.active_hours += s.active_ms as f64 / HOUR_MS;
        if s.commits > 0 {
            w.sessions_with_commits += 1;
        }
        if s.outcome.as_deref() == Some("shipped") {
            w.shipped += 1;
        }
    }

    let mut by_project = group_by(&rows, |s| s.repo_root.clone());
    by_project.truncate(15);
    Ok(Scorecard {
        sessions: rows.len(),
        sessions_in_repos: in_repos.len(),
        sessions_with_commits: with_commits,
        commit_rate: per(with_commits as f64, in_repos.len() as f64),
        commits,
        ai_commits: rows.iter().map(|s| s.ai_commits).sum(),
        insertions: rows.iter().map(|s| s.insertions).sum(),
        deletions: rows.iter().map(|s| s.deletions).sum(),
        cost_per_commit: per(cost, commits as f64),
        active_minutes_per_commit: per(active_min, commits as f64),
        cost_per_shipped: per(cost, shipped as f64),
        shipped,
        by_source: group_by(&rows, |s| Some(s.source.clone())),
        by_project,
        weekly: weekly.into_values().collect(),
    })
}

// ---------------------------------------------------------------- insights

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub id: String,
    /// "positive", "warning" or "info".
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub evidence: String,
    /// The sessions the insight is about, so the UI can open them.
    pub session_ids: Vec<String>,
}

fn insight(id: &str, kind: &str, title: String, detail: String, evidence: String) -> Insight {
    Insight {
        id: id.into(),
        kind: kind.into(),
        title,
        detail,
        evidence,
        session_ids: vec![],
    }
}

/// Attaches the sessions behind the most recently added insight.
fn attach<'a>(out: &mut [Insight], rows: impl IntoIterator<Item = &'a SessionRow>) {
    if let Some(last) = out.last_mut() {
        last.session_ids = rows.into_iter().map(|s| s.id.clone()).collect();
    }
}

struct Bucket<'a> {
    label: &'static str,
    rows: Vec<&'a SessionRow>,
}

impl Bucket<'_> {
    fn rated(&self) -> usize {
        self.rows.iter().filter(|s| s.outcome.is_some()).count()
    }
    fn success(&self) -> Option<f64> {
        success_rate(self.rows.iter().copied())
    }
    fn avg(&self, f: impl Fn(&SessionRow) -> f64) -> f64 {
        self.rows.iter().map(|s| f(s)).sum::<f64>() / self.rows.len().max(1) as f64
    }
}

fn buckets<'a>(
    rows: &'a [SessionRow],
    labels: &[&'static str],
    pick: impl Fn(&SessionRow) -> Option<usize>,
) -> Vec<Bucket<'a>> {
    let mut out: Vec<Bucket> = labels
        .iter()
        .map(|l| Bucket {
            label: l,
            rows: vec![],
        })
        .collect();
    for s in rows {
        if let Some(i) = pick(s) {
            out[i].rows.push(s);
        }
    }
    out
}

/// The best and worst rated buckets, when both have enough samples and differ
/// by at least 20 points.
fn success_spread<'a, 'b>(
    bs: &'b [Bucket<'a>],
) -> Option<(&'b Bucket<'a>, f64, &'b Bucket<'a>, f64)> {
    let scored: Vec<_> = bs
        .iter()
        .filter(|b| b.rated() >= MIN_SAMPLE)
        .filter_map(|b| b.success().map(|r| (b, r)))
        .collect();
    let best = scored.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let worst = scored.iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    (best.1 - worst.1 >= 0.2).then_some((best.0, best.1, worst.0, worst.1))
}

pub fn insights(conn: &Connection, days: i64) -> Result<Vec<Insight>> {
    let range = Range::last_days(days);
    let rows = load_sessions(conn, range)?;
    let mut out = vec![];
    if rows.is_empty() {
        return Ok(out);
    }
    let rated = rows.iter().filter(|s| s.outcome.is_some()).count();
    if rated * 2 < rows.len() {
        out.push(insight(
            "rate-more",
            "info",
            format!("Rate {} more sessions to sharpen these insights", rows.len() - rated),
            "Most comparisons need outcomes. Mark sessions shipped, partial, abandoned or redo on the Sessions page; it takes a few seconds each.".into(),
            format!("{rated} of {} sessions rated", rows.len()),
        ));
        attach(&mut out, rows.iter().filter(|s| s.outcome.is_none()));
    }

    // Opening prompt length.
    let by_len = buckets(
        &rows,
        &["short (<80 chars)", "medium (80–300)", "detailed (300+)"],
        |s| {
            Some(match s.first_prompt_len {
                0..80 => 0,
                80..300 => 1,
                _ => 2,
            })
        },
    );
    if let Some((best, br, worst, wr)) = success_spread(&by_len) {
        out.push(insight(
            "prompt-length",
            if best.label.starts_with("detailed") { "positive" } else { "info" },
            format!("Sessions that open with {} prompts succeed more", best.label),
            format!(
                "Success rate is {} for {} opening prompts versus {} for {}. Spend a minute more on the first message: goal, constraints, and how you will check the result.",
                pct(br), best.label, pct(wr), worst.label
            ),
            format!("{} vs {} rated sessions", best.rated(), worst.rated()),
        ));
        attach(&mut out, worst.rows.iter().copied());
    }
    let (short, long) = (&by_len[0], &by_len[2]);
    if short.rows.len() >= MIN_SAMPLE && long.rows.len() >= MIN_SAMPLE {
        let (st, lt) = (
            short.avg(|s| s.user_turns as f64),
            long.avg(|s| s.user_turns as f64),
        );
        if st >= lt * 1.5 && st - lt >= 2.0 {
            out.push(insight(
                "short-prompt-followups",
                "warning",
                "Short opening prompts lead to more back-and-forth".into(),
                format!("Sessions with a short first prompt took {st:.1} prompts on average, against {lt:.1} for detailed ones. The follow-ups are often context you could have given up front."),
                format!("{} short, {} detailed sessions", short.rows.len(), long.rows.len()),
            ));
            attach(&mut out, short.rows.iter().copied());
        }
    }

    // Interruptions.
    let by_int = buckets(
        &rows,
        &["no interruptions", "one interruption", "2+ interruptions"],
        |s| Some(s.interruptions.min(2) as usize),
    );
    if let (Some(calm), Some(rough)) = (by_int[0].success(), by_int[2].success())
        && by_int[0].rated() >= MIN_SAMPLE
        && by_int[2].rated() >= MIN_SAMPLE
        && calm - rough >= 0.2
    {
        out.push(insight(
                "interruptions",
                "warning",
                "Sessions you interrupt twice or more rarely ship".into(),
                format!(
                    "Success drops from {} with no interruptions to {} with 2+. When you need to stop an agent twice, the plan was probably off; restart with a sharper prompt instead of steering mid-flight.",
                    pct(calm), pct(rough)
                ),
                format!("{} vs {} rated sessions", by_int[0].rated(), by_int[2].rated()),
            ));
        attach(&mut out, by_int[2].rows.iter().copied());
    }

    // Session length.
    let by_dur = buckets(
        &rows,
        &["under 20 min", "20–60 min", "over 60 min"],
        |s| {
            Some(match s.active_ms / 60_000 {
                0..20 => 0,
                20..60 => 1,
                _ => 2,
            })
        },
    );
    if let Some((best, br, worst, wr)) = success_spread(&by_dur) {
        out.push(insight(
            "duration",
            "info",
            format!("Your sweet spot is sessions of {} active time", best.label),
            format!(
                "{} succeed {} of the time; {} only {}. Consider splitting work so each session fits the range that works for you.",
                best.label, pct(br), worst.label, pct(wr)
            ),
            format!("{} vs {} rated sessions", best.rated(), worst.rated()),
        ));
        attach(&mut out, worst.rows.iter().copied());
    }

    // Time of day.
    let by_hour = buckets(
        &rows,
        &[
            "mornings (5–12)",
            "afternoons (12–17)",
            "evenings (17–22)",
            "late nights (22–5)",
        ],
        |s| {
            let h = Local.timestamp_millis_opt(s.started_at?).single()?.hour();
            Some(match h {
                5..12 => 0,
                12..17 => 1,
                17..22 => 2,
                _ => 3,
            })
        },
    );
    if let Some((best, br, worst, wr)) = success_spread(&by_hour) {
        out.push(insight(
            "time-of-day",
            "info",
            format!("You execute best in the {}", best.label),
            format!("Success is {} in the {} versus {} in the {}. Schedule hard delegation for your strong hours.", pct(br), best.label, pct(wr), worst.label),
            format!("{} vs {} rated sessions", best.rated(), worst.rated()),
        ));
        attach(&mut out, worst.rows.iter().copied());
    }

    // Tools compared.
    let by_source = group_by(&rows, |s| Some(s.source.clone()));
    let comparable: Vec<_> = by_source
        .iter()
        .filter(|g| g.rated >= MIN_SAMPLE && g.success_rate.is_some())
        .collect();
    if comparable.len() >= 2 {
        let best = comparable
            .iter()
            .max_by(|a, b| a.success_rate.unwrap().total_cmp(&b.success_rate.unwrap()))
            .unwrap();
        let worst = comparable
            .iter()
            .min_by(|a, b| a.success_rate.unwrap().total_cmp(&b.success_rate.unwrap()))
            .unwrap();
        if best.success_rate.unwrap() - worst.success_rate.unwrap() >= 0.15 {
            out.push(insight(
                "tool-compare",
                "info",
                format!("{} is landing more of your work than {}", source_label(&best.key), source_label(&worst.key)),
                format!(
                    "{} sessions succeed {} of the time against {} for {}. Check whether that is the tool or the kind of task you give each one.",
                    source_label(&best.key), pct(best.success_rate.unwrap()), pct(worst.success_rate.unwrap()), source_label(&worst.key)
                ),
                format!("{} vs {} rated sessions", best.rated, worst.rated),
            ));
        }
    }

    // Response latency: the human as the bottleneck.
    let t = totals(&rows);
    if let Some(avg) = t.avg_wait_seconds {
        let waits: i64 = rows.iter().map(|s| s.wait_count).sum();
        if waits >= 10 && avg >= 120.0 {
            out.push(insight(
                "latency",
                "warning",
                format!("Agents waited {:.1} hours on you", t.wait_hours),
                format!(
                    "After an agent finishes a turn, your next prompt comes {:.1} minutes later on average. Batch reviews, keep the Live page open, or give longer autonomous tasks so agents don't sit idle.",
                    avg / 60.0
                ),
                format!("{waits} hand-backs measured (gaps over 30 min excluded)"),
            ));
        } else if waits >= 10 {
            out.push(insight(
                "latency-good",
                "positive",
                format!("You respond to agents in {:.0} seconds on average", avg),
                "Agents rarely sit idle waiting on you. If you want more throughput, run sessions in parallel rather than responding faster.".into(),
                format!("{waits} hand-backs measured"),
            ));
        }
    }

    // Tool errors.
    if t.tool_calls >= 50 {
        let rate = t.tool_errors as f64 / t.tool_calls as f64;
        if rate >= 0.12 {
            out.push(insight(
                "tool-errors",
                "warning",
                format!("{} of agent tool calls fail", pct(rate)),
                "Frequent failures usually mean missing setup: commands that don't exist, wrong paths, absent env vars or tests that can't run. Put the right commands in CLAUDE.md / AGENTS.md so agents stop guessing.".into(),
                format!("{} errors in {} tool calls", t.tool_errors, t.tool_calls),
            ));
            attach(&mut out, rows.iter().filter(|s| s.tool_errors > 0));
        }
    }

    // The checks below rely on per-session signals that older sessions
    // (parsed before those signals existed) don't have.
    let measured: Vec<SessionRow> = rows.iter().filter(|s| s.has_signals).cloned().collect();
    // Corrections: the human had to say "no, that's not it".
    let by_corr = buckets(
        &measured,
        &["no corrections", "one correction", "2+ corrections"],
        |s| Some(s.corrections.min(2) as usize),
    );
    if let (Some(clean), Some(rough)) = (by_corr[0].success(), by_corr[2].success())
        && by_corr[0].rated() >= MIN_SAMPLE
        && by_corr[2].rated() >= MIN_SAMPLE
        && clean - rough >= 0.2
    {
        out.push(insight(
            "corrections",
            "warning",
            "Sessions you have to correct twice rarely recover".into(),
            format!(
                "Success is {} with no corrections and {} after two or more. After the second \"no, that's not it\", start a fresh session with what you learned instead of steering further.",
                pct(clean),
                pct(rough)
            ),
            format!("{} vs {} rated sessions", by_corr[0].rated(), by_corr[2].rated()),
        ));
        attach(&mut out, by_corr[2].rows.iter().copied());
    }

    // Context compaction: the session outgrew its context window.
    let by_compact = buckets(&measured, &["no compaction", "compacted"], |s| {
        Some(usize::from(s.compactions > 0))
    });
    if let (Some(clean), Some(compacted)) = (by_compact[0].success(), by_compact[1].success())
        && by_compact[0].rated() >= MIN_SAMPLE
        && by_compact[1].rated() >= MIN_SAMPLE
        && clean - compacted >= 0.15
    {
        out.push(insight(
            "compaction",
            "warning",
            "Sessions that hit context compaction succeed less".into(),
            format!(
                "{} succeed without compaction versus {} once the context fills up. Split big tasks so each fits one context window.",
                pct(clean),
                pct(compacted)
            ),
            format!("{} vs {} rated sessions", by_compact[0].rated(), by_compact[1].rated()),
        ));
        attach(&mut out, by_compact[1].rows.iter().copied());
    }

    // Agent edit hygiene.
    let edits: i64 = measured.iter().map(|s| s.edits).sum();
    if edits >= 40 {
        let blind =
            measured.iter().map(|s| s.edits_without_read).sum::<i64>() as f64 / edits as f64;
        let re = measured.iter().map(|s| s.re_edits).sum::<i64>() as f64 / edits as f64;
        if blind >= 0.25 {
            out.push(insight(
                "blind-edits",
                "warning",
                format!("{} of agent edits change files it hasn't read", pct(blind)),
                "Edits made without reading the file first are more likely to be wrong. Point the agent at the relevant files in your prompt, or add \"read before editing\" to your instructions file.".into(),
                format!("{edits} edits"),
            ));
            attach(
                &mut out,
                measured.iter().filter(|s| s.edits_without_read > 0),
            );
        }
        if re >= 0.35 {
            out.push(insight(
                "re-edits",
                "warning",
                format!("{} of edits rework a file already changed in the session", pct(re)),
                "Repeated edits to the same file usually mean a fix-try-fix loop. Ask for a plan before code, or give the agent a test to run so it can check itself.".into(),
                format!("{edits} edits"),
            ));
            attach(&mut out, measured.iter().filter(|s| s.re_edits > 0));
        }
    }

    // Task categories.
    let mut cats: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
    for s in &measured {
        if let Some(c) = &s.category {
            cats.entry(c.clone()).or_default().push(s);
        }
    }
    let cat_rates: Vec<(String, usize, f64)> = cats
        .iter()
        .filter_map(|(c, g)| {
            let rated = g.iter().filter(|s| s.outcome.is_some()).count();
            let rate = success_rate(g.iter().copied())?;
            (rated >= MIN_SAMPLE).then(|| (c.clone(), rated, rate))
        })
        .collect();
    if let (Some(best), Some(worst)) = (
        cat_rates.iter().max_by(|a, b| a.2.total_cmp(&b.2)),
        cat_rates.iter().min_by(|a, b| a.2.total_cmp(&b.2)),
    ) && best.2 - worst.2 >= 0.25
    {
        out.push(insight(
            "categories",
            "info",
            format!("You delegate {} work well and {} work poorly", best.0, worst.0),
            format!(
                "{} sessions succeed {} of the time; {} sessions {}. Give {} tasks more structure (a failing test, a spec, or smaller steps), or do more of them yourself.",
                best.0,
                pct(best.2),
                worst.0,
                pct(worst.2),
                worst.0
            ),
            format!("{} vs {} rated sessions", best.1, worst.1),
        ));
        attach(&mut out, cats[&worst.0].iter().copied());
    }

    // Journal calibration.
    let cal = journal::calibration(conn, range.since)?;
    if let Some(all) = &cal.overall {
        if all.n >= MIN_SAMPLE as i64
            && let Some(r) = all.time_ratio
        {
            if r >= 1.3 {
                out.push(insight(
                        "calibration-time",
                        "warning",
                        format!("Tasks take {r:.1}× as long as you expect"),
                        "Your estimates are consistently optimistic. Either budget more time, or scope tasks smaller before handing them off.".into(),
                        format!("{} closed intents", all.n),
                    ));
            } else if r <= 0.8 {
                out.push(insight(
                    "calibration-fast",
                    "positive",
                    format!("Agents finish in {:.0}% of your estimate", r * 100.0),
                    "You are delegating well-scoped work. You can likely hand off larger chunks."
                        .into(),
                    format!("{} closed intents", all.n),
                ));
            }
        }
        let high: Vec<_> = cal
            .by_confidence
            .iter()
            .filter(|c| c.key == "4" || c.key == "5")
            .collect();
        let n_high: i64 = high.iter().map(|c| c.n).sum();
        if n_high >= MIN_SAMPLE as i64 {
            let rate = high
                .iter()
                .map(|c| c.success_rate * c.n as f64)
                .sum::<f64>()
                / n_high as f64;
            if rate < 0.6 {
                out.push(insight(
                    "overconfident",
                    "warning",
                    "High-confidence tasks still fail often".into(),
                    format!("When you rate yourself 4–5 out of 5, only {} succeed. Look at the reflections on those intents for the common surprise.", pct(rate)),
                    format!("{n_high} high-confidence intents"),
                ));
            }
        }
    }
    Ok(out)
}

fn csv_field(v: &str) -> String {
    // A leading = + - @ makes spreadsheets evaluate the cell as a formula.
    let v = if v.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{v}")
    } else {
        v.to_string()
    };
    if v.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v
    }
}

pub fn sessions_csv(rows: &[SessionRow]) -> String {
    let mut out = String::from(
        "started,tool,project,branch,title,category,model,active_minutes,prompts,corrections,\
         interruptions,tool_calls,tool_errors,commits,cost_usd,cost_is_estimate,outcome,rating,\
         friction_requirements,friction_steering,friction_scope\n",
    );
    for s in rows {
        let started = s
            .started_at
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| d.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let fields = [
            started,
            source_label(&s.source).to_string(),
            s.repo_root
                .as_deref()
                .or(s.project_path.as_deref())
                .unwrap_or("")
                .to_string(),
            s.git_branch.clone().unwrap_or_default(),
            s.title.clone().unwrap_or_default(),
            s.category.clone().unwrap_or_default(),
            s.model.clone().unwrap_or_default(),
            (s.active_ms / 60_000).to_string(),
            s.user_turns.to_string(),
            s.corrections.to_string(),
            s.interruptions.to_string(),
            s.tool_calls.to_string(),
            s.tool_errors.to_string(),
            s.commits.to_string(),
            s.cost_usd.map(|c| format!("{c:.4}")).unwrap_or_default(),
            s.cost_is_estimate.to_string(),
            s.outcome.clone().unwrap_or_default(),
            s.rating.map(|r| r.to_string()).unwrap_or_default(),
            s.friction.requirements.to_string(),
            s.friction.steering.to_string(),
            s.friction.scope.to_string(),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

pub fn source_label(s: &str) -> &str {
    Source::from_id(s).map_or(s, |source| source.short_label())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn insert(conn: &Connection, id: &str, prompt_len: usize, outcome: Option<&str>, started: i64) {
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, first_prompt, started_at, ended_at,
                                   active_ms, user_turns, outcome, ingested_at)
             VALUES (?1, 'claude_code', ?1, '/f', ?2, ?3, ?3 + 600000, 600000, 3, ?4, 0)",
            params![id, "x".repeat(prompt_len), started, outcome],
        )
        .unwrap();
    }

    #[test]
    fn overview_and_insights_on_rated_sessions() {
        let conn = db::open_in_memory().unwrap();
        let now = now_ms();
        for i in 0..6 {
            insert(
                &conn,
                &format!("long{i}"),
                500,
                Some("shipped"),
                now - (i + 1) * 3_600_000,
            );
            insert(
                &conn,
                &format!("short{i}"),
                20,
                Some("abandoned"),
                now - (i + 1) * 3_600_000,
            );
        }
        let ov = overview(&conn, 7).unwrap();
        assert_eq!(ov.totals.sessions, 12);
        assert_eq!(ov.totals.shipped, 6);
        assert_eq!(ov.totals.success_rate, Some(0.5));
        assert_eq!(ov.daily.len(), 8);

        let ins = insights(&conn, 7).unwrap();
        let p = ins
            .iter()
            .find(|i| i.id == "prompt-length")
            .expect("prompt-length insight");
        assert!(p.title.contains("detailed"));
    }

    #[test]
    fn review_round_trip_and_validation() {
        let conn = db::open_in_memory().unwrap();
        insert(&conn, "a", 10, None, now_ms() - 1000);
        save_review(
            &conn,
            &Review {
                id: "a".into(),
                outcome: Some("partial".into()),
                rating: Some(3),
                notes: Some("tests missing".into()),
                tags: vec![" Refactor ".into(), "".into()],
                vs_manual: Some("faster".into()),
            },
        )
        .unwrap();
        let d = session_detail(&conn, "a").unwrap();
        assert_eq!(d.session.outcome.as_deref(), Some("partial"));
        assert_eq!(d.session.tags, ["refactor"]);
        assert!(
            save_review(
                &conn,
                &Review {
                    id: "a".into(),
                    outcome: Some("great".into()),
                    rating: None,
                    notes: None,
                    tags: vec![],
                    vs_manual: None,
                }
            )
            .is_err()
        );
    }
}
