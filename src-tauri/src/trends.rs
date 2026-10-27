//! Read models for the Usage and Patterns pages: spend and tokens over time,
//! model comparison, activity heatmaps, task categories, tool usage, and
//! execution-quality trends.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Timelike};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::analytics::{Range, SessionRow, load_sessions};
use crate::journal::outcome_score;
use crate::pricing;
use crate::util::now_ms;

const DAY_MS: i64 = 86_400_000;

fn local_date(ms: i64) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.date_naive())
}

fn success(rows: &[&SessionRow]) -> (usize, Option<f64>) {
    let scores: Vec<f64> = rows
        .iter()
        .filter_map(|s| s.outcome.as_deref().and_then(outcome_score))
        .collect();
    let rate = (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64);
    (scores.len(), rate)
}

fn mean(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let v: Vec<f64> = xs.collect();
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn ratio(num: i64, den: i64) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64)
}

// ------------------------------------------------------------------- usage

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub date: String,
    /// Cost per tool (claude_code, copilot_cli, copilot_vscode).
    pub cost: BTreeMap<String, f64>,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub model: String,
    pub sessions: usize,
    pub cost_usd: f64,
    pub tokens: i64,
    pub rated: usize,
    pub success_rate: Option<f64>,
    pub avg_turns: f64,
    pub avg_active_minutes: f64,
    pub cost_per_session: f64,
    pub avg_friction: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub daily: Vec<UsageDay>,
    pub by_model: Vec<ModelRow>,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    /// Share of prompt tokens served from cache.
    pub cache_hit_rate: Option<f64>,
    /// What cached reads saved versus paying full input price (Claude models).
    pub cache_savings_usd: f64,
    pub month_to_date_usd: f64,
    pub projected_month_usd: f64,
    /// Cumulative spend for each day of the current month so far.
    pub month_cumulative: Vec<(String, f64)>,
    pub days_in_month: i64,
    /// Optional monthly budget from Settings.
    pub budget_usd: Option<f64>,
    pub cost_per_session: Option<f64>,
}

struct Tokens {
    source: String,
    model: Option<String>,
    started_at: i64,
    t: [i64; 4],
    cost: Option<f64>,
}

fn load_tokens(conn: &Connection, range: Range) -> Result<HashMap<String, Tokens>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, model, started_at, input_tokens, output_tokens, cache_read_tokens,
                cache_write_tokens, cost_usd
         FROM sessions WHERE started_at >= ?1 AND started_at < ?2",
    )?;
    let rows = stmt.query_map(params![range.since, range.until], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Tokens {
                source: r.get(1)?,
                model: r.get(2)?,
                started_at: r.get(3)?,
                t: [r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?],
                cost: r.get(8)?,
            },
        ))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn usage(conn: &Connection, days: i64) -> Result<Usage> {
    let range = Range::last_days(days);
    let rows = load_sessions(conn, range)?;
    let tokens = load_tokens(conn, range)?;

    let mut daily: BTreeMap<NaiveDate, UsageDay> = BTreeMap::new();
    let start = if range.since > 0 {
        local_date(range.since)
    } else {
        tokens
            .values()
            .map(|t| t.started_at)
            .min()
            .and_then(local_date)
    };
    if let (Some(mut d), Some(end)) = (start, local_date(now_ms())) {
        while d <= end {
            daily.insert(
                d,
                UsageDay {
                    date: d.format("%Y-%m-%d").to_string(),
                    cost: BTreeMap::new(),
                    input: 0,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                },
            );
            d += Duration::days(1);
        }
    }

    let (mut input, mut output, mut cache_read, mut cache_write) = (0, 0, 0, 0);
    let mut savings = 0.0;
    for t in tokens.values() {
        input += t.t[0];
        output += t.t[1];
        cache_read += t.t[2];
        cache_write += t.t[3];
        if let Some(p) = t.model.as_deref().and_then(pricing::price_for) {
            savings += t.t[2] as f64 * (p.input - p.cache_read) / 1_000_000.0;
        }
        if let Some(day) = local_date(t.started_at).and_then(|d| daily.get_mut(&d)) {
            *day.cost.entry(t.source.clone()).or_default() += t.cost.unwrap_or(0.0);
            day.input += t.t[0];
            day.output += t.t[1];
            day.cache_read += t.t[2];
            day.cache_write += t.t[3];
        }
    }

    let mut groups: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
    for s in &rows {
        groups
            .entry(s.model.clone().unwrap_or_else(|| "unknown".into()))
            .or_default()
            .push(s);
    }
    let mut by_model: Vec<ModelRow> = groups
        .into_iter()
        .map(|(model, g)| {
            let n = g.len() as f64;
            let (rated, success_rate) = success(&g);
            let costs: Vec<f64> = g.iter().filter_map(|s| s.cost_usd).collect();
            let cost: f64 = costs.iter().sum();
            ModelRow {
                model,
                sessions: g.len(),
                cost_usd: cost,
                tokens: g.iter().map(|s| s.tokens).sum(),
                rated,
                success_rate,
                avg_turns: g.iter().map(|s| s.user_turns as f64).sum::<f64>() / n,
                avg_active_minutes: g.iter().map(|s| s.active_ms as f64 / 60_000.0).sum::<f64>()
                    / n,
                cost_per_session: if costs.is_empty() {
                    0.0
                } else {
                    cost / costs.len() as f64
                },
                avg_friction: g.iter().map(|s| s.friction.overall).sum::<f64>() / n,
            }
        })
        .collect();
    by_model.sort_by_key(|m| std::cmp::Reverse(m.sessions));

    // Month to date and a straight-line projection, in local time.
    let today = Local::now().date_naive();
    let month_start = today.with_day(1).unwrap_or(today);
    let month_start_ms = month_start
        .and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(Local).earliest())
        .map(|d| d.timestamp_millis())
        .unwrap_or(0);
    let mtd: f64 = conn.query_row(
        "SELECT COALESCE(SUM(cost_usd), 0) FROM sessions WHERE started_at >= ?1",
        [month_start_ms],
        |r| r.get(0),
    )?;
    let days_in_month = {
        let next = if today.month() == 12 {
            NaiveDate::from_ymd_opt(today.year() + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1)
        };
        next.map(|n| (n - month_start).num_days()).unwrap_or(30) as f64
    };
    let elapsed = (today - month_start).num_days() as f64 + 1.0;
    let mut by_day: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT started_at, COALESCE(cost_usd, 0) FROM sessions WHERE started_at >= ?1",
        )?;
        for row in stmt.query_map([month_start_ms], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        })? {
            let (t, c) = row?;
            if let Some(d) = local_date(t) {
                *by_day.entry(d).or_default() += c;
            }
        }
    }
    let mut month_cumulative = vec![];
    let mut running = 0.0;
    let mut d = month_start;
    while d <= today {
        running += by_day.get(&d).copied().unwrap_or(0.0);
        month_cumulative.push((d.format("%Y-%m-%d").to_string(), running));
        d += Duration::days(1);
    }
    let budget_usd = crate::db::get_setting(conn, "budget.monthly")?
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|b| *b > 0.0);
    let costed: Vec<f64> = tokens.values().filter_map(|t| t.cost).collect();

    Ok(Usage {
        daily: daily.into_values().collect(),
        by_model,
        input,
        output,
        cache_read,
        cache_write,
        cache_hit_rate: ratio(cache_read, input + cache_read + cache_write),
        cache_savings_usd: savings,
        month_to_date_usd: mtd,
        projected_month_usd: mtd / elapsed * days_in_month,
        month_cumulative,
        days_in_month: days_in_month as i64,
        budget_usd,
        cost_per_session: (!costed.is_empty())
            .then(|| costed.iter().sum::<f64>() / costed.len() as f64),
    })
}

// ---------------------------------------------------------------- patterns

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDay {
    pub date: String,
    pub sessions: usize,
    pub active_minutes: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryRow {
    pub category: String,
    pub sessions: usize,
    pub rated: usize,
    pub success_rate: Option<f64>,
    pub avg_active_minutes: f64,
    pub avg_turns: f64,
    pub avg_friction: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityWeek {
    pub week_start: String,
    pub sessions: usize,
    pub success_rate: Option<f64>,
    /// Reads per edit; low values mean editing without looking.
    pub read_edit_ratio: Option<f64>,
    pub edits_without_read_rate: Option<f64>,
    /// Share of edits that touch a file already edited in the session.
    pub re_edit_rate: Option<f64>,
    pub corrections_per_session: f64,
    pub avg_friction: f64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Benchmark {
    pub label: String,
    pub sessions: usize,
    pub success_rate: Option<f64>,
    pub avg_turns: Option<f64>,
    pub corrections_per_session: Option<f64>,
    pub interruptions_per_session: Option<f64>,
    pub avg_active_minutes: Option<f64>,
    pub re_edit_rate: Option<f64>,
    pub avg_friction: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCount {
    pub tool: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LongSession {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub active_ms: i64,
    pub user_turns: i64,
    pub outcome: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrictionAvg {
    pub requirements: f64,
    pub steering: f64,
    pub scope: f64,
    pub overall: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Patterns {
    /// The last 365 days, regardless of the selected range.
    pub calendar: Vec<CalendarDay>,
    /// Sessions started per weekday (0 = Monday) and hour.
    pub heatmap: Vec<Vec<usize>>,
    pub categories: Vec<CategoryRow>,
    pub tools: Vec<ToolCount>,
    pub reads: i64,
    pub edits: i64,
    pub shell_calls: i64,
    pub other_tool_calls: i64,
    pub quality_weekly: Vec<QualityWeek>,
    pub friction: Option<FrictionAvg>,
    /// Your first 30 days of data against the most recent 30.
    pub then_vs_now: Option<(Benchmark, Benchmark)>,
    pub longest: Vec<LongSession>,
    pub with_compaction: Benchmark,
    pub without_compaction: Benchmark,
    pub with_subagents: Benchmark,
}

fn benchmark(label: &str, rows: &[&SessionRow]) -> Benchmark {
    let n = rows.len();
    let per = |f: &dyn Fn(&SessionRow) -> f64| mean(rows.iter().map(|s| f(s)));
    let edits: i64 = rows.iter().map(|s| s.edits).sum();
    Benchmark {
        label: label.into(),
        sessions: n,
        success_rate: success(rows).1,
        avg_turns: per(&|s| s.user_turns as f64),
        corrections_per_session: per(&|s| s.corrections as f64),
        interruptions_per_session: per(&|s| s.interruptions as f64),
        avg_active_minutes: per(&|s| s.active_ms as f64 / 60_000.0),
        re_edit_rate: ratio(rows.iter().map(|s| s.re_edits).sum(), edits),
        avg_friction: per(&|s| s.friction.overall),
    }
}

pub fn patterns(conn: &Connection, days: i64) -> Result<Patterns> {
    let range = Range::last_days(days);
    let rows = load_sessions(conn, range)?;
    // Statistics built on tool, correction and category signals use only the
    // sessions that have them (older ones may predate those signals).
    let measured: Vec<SessionRow> = rows.iter().filter(|s| s.has_signals).cloned().collect();
    let all: Vec<&SessionRow> = measured.iter().collect();

    // Calendar: always a full year so the heatmap reads like a contribution graph.
    let year = load_sessions(conn, Range::last_days(365))?;
    let mut calendar: BTreeMap<NaiveDate, CalendarDay> = BTreeMap::new();
    let today = Local::now().date_naive();
    let mut d = today - Duration::days(364);
    while d <= today {
        calendar.insert(
            d,
            CalendarDay {
                date: d.format("%Y-%m-%d").to_string(),
                sessions: 0,
                active_minutes: 0.0,
            },
        );
        d += Duration::days(1);
    }
    for s in &year {
        if let Some(day) = s
            .started_at
            .and_then(local_date)
            .and_then(|d| calendar.get_mut(&d))
        {
            day.sessions += 1;
            day.active_minutes += s.active_ms as f64 / 60_000.0;
        }
    }

    let mut heatmap = vec![vec![0usize; 24]; 7];
    for s in &rows {
        if let Some(t) = s
            .started_at
            .and_then(|t| Local.timestamp_millis_opt(t).single())
        {
            heatmap[t.weekday().num_days_from_monday() as usize][t.hour() as usize] += 1;
        }
    }

    let mut cats: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
    for s in &measured {
        cats.entry(s.category.clone().unwrap_or_else(|| "other".into()))
            .or_default()
            .push(s);
    }
    let mut categories: Vec<CategoryRow> = cats
        .into_iter()
        .map(|(category, g)| {
            let n = g.len() as f64;
            let (rated, success_rate) = success(&g);
            CategoryRow {
                category,
                sessions: g.len(),
                rated,
                success_rate,
                avg_active_minutes: g.iter().map(|s| s.active_ms as f64 / 60_000.0).sum::<f64>()
                    / n,
                avg_turns: g.iter().map(|s| s.user_turns as f64).sum::<f64>() / n,
                avg_friction: g.iter().map(|s| s.friction.overall).sum::<f64>() / n,
            }
        })
        .collect();
    categories.sort_by_key(|c| std::cmp::Reverse(c.sessions));

    // Tool names come from the per-session JSON counts.
    let mut tool_totals: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT tool_counts FROM sessions
             WHERE started_at >= ?1 AND started_at < ?2 AND signals_version >= 1",
        )?;
        for json in stmt.query_map(params![range.since, range.until], |r| r.get::<_, String>(0))? {
            let counts: BTreeMap<String, i64> = serde_json::from_str(&json?).unwrap_or_default();
            for (k, v) in counts {
                *tool_totals.entry(k).or_default() += v;
            }
        }
    }
    let mut tools: Vec<ToolCount> = tool_totals
        .into_iter()
        .map(|(tool, count)| ToolCount { tool, count })
        .collect();
    tools.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tool.cmp(&b.tool)));
    tools.truncate(15);

    let reads: i64 = measured.iter().map(|s| s.reads).sum();
    let edits: i64 = measured.iter().map(|s| s.edits).sum();
    let shell_calls: i64 = measured.iter().map(|s| s.shell_calls).sum();
    let tool_calls: i64 = measured.iter().map(|s| s.tool_calls).sum();

    let mut weeks: BTreeMap<NaiveDate, Vec<&SessionRow>> = BTreeMap::new();
    for s in &measured {
        if let Some(d) = s.started_at.and_then(local_date) {
            let monday = d - Duration::days(d.weekday().num_days_from_monday() as i64);
            weeks.entry(monday).or_default().push(s);
        }
    }
    let quality_weekly = weeks
        .into_iter()
        .map(|(monday, g)| {
            let n = g.len() as f64;
            let edits: i64 = g.iter().map(|s| s.edits).sum();
            QualityWeek {
                week_start: monday.format("%Y-%m-%d").to_string(),
                sessions: g.len(),
                success_rate: success(&g).1,
                read_edit_ratio: ratio(g.iter().map(|s| s.reads).sum(), edits),
                edits_without_read_rate: ratio(g.iter().map(|s| s.edits_without_read).sum(), edits),
                re_edit_rate: ratio(g.iter().map(|s| s.re_edits).sum(), edits),
                corrections_per_session: g.iter().map(|s| s.corrections as f64).sum::<f64>() / n,
                avg_friction: g.iter().map(|s| s.friction.overall).sum::<f64>() / n,
            }
        })
        .collect();

    let friction = (!measured.is_empty()).then(|| {
        let n = measured.len() as f64;
        let avg = |f: &dyn Fn(&SessionRow) -> f64| measured.iter().map(f).sum::<f64>() / n;
        FrictionAvg {
            requirements: avg(&|s| s.friction.requirements as f64),
            steering: avg(&|s| s.friction.steering as f64),
            scope: avg(&|s| s.friction.scope as f64),
            overall: avg(&|s| s.friction.overall),
        }
    });

    // Then vs now spans all history, independent of the selected range.
    let history: Vec<SessionRow> = load_sessions(conn, Range::last_days(0))?
        .into_iter()
        .filter(|s| s.has_signals)
        .collect();
    let then_vs_now = {
        let first = history.iter().filter_map(|s| s.started_at).min();
        let now = now_ms();
        match first {
            Some(f) if now - f >= 60 * DAY_MS => {
                let early: Vec<&SessionRow> = history
                    .iter()
                    .filter(|s| s.started_at.is_some_and(|t| t < f + 30 * DAY_MS))
                    .collect();
                let recent: Vec<&SessionRow> = history
                    .iter()
                    .filter(|s| s.started_at.is_some_and(|t| t >= now - 30 * DAY_MS))
                    .collect();
                Some((
                    benchmark("First 30 days", &early),
                    benchmark("Last 30 days", &recent),
                ))
            }
            _ => None,
        }
    };

    let mut longest: Vec<&SessionRow> = rows.iter().collect();
    longest.sort_by_key(|s| std::cmp::Reverse(s.active_ms));
    let longest = longest
        .into_iter()
        .take(8)
        .map(|s| LongSession {
            id: s.id.clone(),
            source: s.source.clone(),
            title: s.title.clone(),
            active_ms: s.active_ms,
            user_turns: s.user_turns,
            outcome: s.outcome.clone(),
        })
        .collect();

    let (compacted, clean): (Vec<&SessionRow>, Vec<&SessionRow>) =
        all.iter().partition(|s| s.compactions > 0);
    let with_subagents: Vec<&SessionRow> =
        all.iter().copied().filter(|s| s.subagents > 0).collect();

    Ok(Patterns {
        calendar: calendar.into_values().collect(),
        heatmap,
        categories,
        tools,
        reads,
        edits,
        shell_calls,
        other_tool_calls: (tool_calls - reads - edits - shell_calls).max(0),
        quality_weekly,
        friction,
        then_vs_now,
        longest,
        with_compaction: benchmark("Hit context compaction", &compacted),
        without_compaction: benchmark("No compaction", &clean),
        with_subagents: benchmark("Used sub-agents", &with_subagents),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn usage_and_patterns_on_small_dataset() {
        let conn = db::open_in_memory().unwrap();
        let now = now_ms();
        for i in 0..4 {
            conn.execute(
                "INSERT INTO sessions (id, source, external_id, file_path, first_prompt, model, started_at,
                                       ended_at, active_ms, user_turns, input_tokens, cache_read_tokens,
                                       cost_usd, reads, edits, re_edits, category, tool_counts, outcome,
                                       ingested_at, signals_version)
                 VALUES (?1, 'claude_code', ?1, '/f', 'Fix the failing test', 'claude-opus-5-5', ?2,
                         ?2 + 600000, 600000, 2, 1000, 3000, 1.5, 4, 2, 1, 'bugfix',
                         '{\"Read\":4,\"Edit\":2}', 'shipped', 0, 1)",
                params![format!("s{i}"), now - (i + 1) * 3_600_000],
            )
            .unwrap();
        }
        let u = usage(&conn, 7).unwrap();
        assert_eq!(u.by_model[0].sessions, 4);
        assert!((u.cache_hit_rate.unwrap() - 0.75).abs() < 1e-9);
        // 12k cached tokens at ($4 - $0.20)/MTok.
        assert!((u.cache_savings_usd - 12_000.0 * 3.8 / 1e6).abs() < 1e-9);

        let p = patterns(&conn, 7).unwrap();
        assert_eq!(p.calendar.len(), 365);
        assert_eq!(p.categories[0].category, "bugfix");
        assert_eq!(p.tools[0].tool, "Read");
        assert_eq!(p.tools[0].count, 16);
        assert_eq!(
            p.quality_weekly.iter().map(|w| w.sessions).sum::<usize>(),
            4
        );
        assert_eq!(p.heatmap.iter().flatten().sum::<usize>(), 4);
    }
}

// ----------------------------------------------------------- reality check

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerRow {
    /// faster, same, slower, unsure or unanswered.
    pub answer: String,
    pub sessions: usize,
    pub rated: usize,
    pub success_rate: Option<f64>,
    pub avg_active_minutes: Option<f64>,
    pub with_commits: usize,
    pub rework_rate: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reality {
    /// Sessions you rated shipped that produced commits.
    pub shipped_with_commits: usize,
    /// …and how many of those were reverted or fixed within 7 days.
    pub shipped_reworked: usize,
    pub rework_rate: Option<f64>,
    pub by_answer: Vec<AnswerRow>,
    /// Journal: actual ÷ expected time, and how many intents it rests on.
    pub time_ratio: Option<f64>,
    pub time_ratio_n: i64,
    /// Agent working minutes per minute the agent spent waiting on you.
    pub leverage: Option<f64>,
    pub agent_hours: f64,
    pub wait_hours: f64,
}

pub fn reality(conn: &Connection, days: i64) -> Result<Reality> {
    let range = Range::last_days(days);
    let rows = load_sessions(conn, range)?;
    let shipped: Vec<&SessionRow> = rows
        .iter()
        .filter(|s| s.outcome.as_deref() == Some("shipped") && s.commits > 0)
        .collect();
    let shipped_reworked = shipped.iter().filter(|s| s.reworked).count();

    let mut groups: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
    for s in rows.iter().filter(|s| s.outcome.is_some()) {
        groups
            .entry(s.vs_manual.clone().unwrap_or_else(|| "unanswered".into()))
            .or_default()
            .push(s);
    }
    let order = ["faster", "same", "slower", "unsure", "unanswered"];
    let mut by_answer: Vec<AnswerRow> = groups
        .into_iter()
        .map(|(answer, g)| {
            let (rated, success_rate) = success(&g);
            let with: Vec<&&SessionRow> = g.iter().filter(|s| s.commits > 0).collect();
            AnswerRow {
                answer,
                sessions: g.len(),
                rated,
                success_rate,
                avg_active_minutes: mean(g.iter().map(|s| s.active_ms as f64 / 60_000.0)),
                with_commits: with.len(),
                rework_rate: ratio(
                    with.iter().filter(|s| s.reworked).count() as i64,
                    with.len() as i64,
                ),
            }
        })
        .collect();
    by_answer.sort_by_key(|r| order.iter().position(|o| *o == r.answer).unwrap_or(9));

    let cal = crate::journal::calibration(conn, range.since)?;
    let agent_ms: i64 = rows
        .iter()
        .filter(|s| s.has_signals)
        .map(|s| s.agent_ms)
        .sum();
    let wait_ms: i64 = rows
        .iter()
        .filter(|s| s.has_signals)
        .map(|s| s.wait_ms_total)
        .sum();
    Ok(Reality {
        shipped_with_commits: shipped.len(),
        shipped_reworked,
        rework_rate: ratio(shipped_reworked as i64, shipped.len() as i64),
        by_answer,
        time_ratio: cal.overall.as_ref().and_then(|o| o.time_ratio),
        time_ratio_n: cal.overall.as_ref().map(|o| o.n).unwrap_or(0),
        leverage: ratio(agent_ms, wait_ms),
        agent_hours: agent_ms as f64 / 3_600_000.0,
        wait_hours: wait_ms as f64 / 3_600_000.0,
    })
}

// ------------------------------------------------------- delegation fit

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FitCell {
    pub category: String,
    pub source: String,
    pub sessions: usize,
    pub rated: usize,
    pub success_rate: Option<f64>,
    pub rework_rate: Option<f64>,
    /// delegate | structure | yourself | unknown
    pub verdict: String,
    pub ids: Vec<String>,
}

/// Task type × tool: where delegating has worked for you, where it needs
/// more structure, and where you may be better off doing it yourself.
pub fn fit(conn: &Connection, days: i64) -> Result<Vec<FitCell>> {
    let rows = load_sessions(conn, Range::last_days(days))?;
    let mut cells: BTreeMap<(String, String), Vec<&SessionRow>> = BTreeMap::new();
    for s in rows.iter().filter(|s| s.has_signals) {
        let cat = s.category.clone().unwrap_or_else(|| "other".into());
        cells.entry((cat, s.source.clone())).or_default().push(s);
    }
    Ok(cells
        .into_iter()
        .map(|((category, source), g)| {
            let (rated, success_rate) = success(&g);
            let with: Vec<&&SessionRow> = g.iter().filter(|s| s.commits > 0).collect();
            let rework_rate = ratio(
                with.iter().filter(|s| s.reworked).count() as i64,
                with.len() as i64,
            );
            let verdict = match (rated, success_rate) {
                (n, _) if n < crate::analytics::MIN_SAMPLE => "unknown",
                (_, Some(r)) if r >= 0.7 && rework_rate.is_none_or(|w| w <= 0.2) => "delegate",
                (_, Some(r)) if r >= 0.45 => "structure",
                _ => "yourself",
            };
            FitCell {
                category,
                source,
                sessions: g.len(),
                rated,
                success_rate,
                rework_rate,
                verdict: verdict.into(),
                ids: g.iter().map(|s| s.id.clone()).collect(),
            }
        })
        .collect())
}

// ---------------------------------------------------------- tool health

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolHealth {
    pub tool: String,
    pub calls: i64,
    pub errors: i64,
    pub error_rate: Option<f64>,
    /// Calls you rejected or denied.
    pub vetoes: i64,
}

/// Per-tool failures and vetoes. Failures point at environment problems
/// (missing commands, flaky tests); vetoes point at rules worth writing
/// into CLAUDE.md or AGENTS.md.
pub fn tool_health(conn: &Connection, days: i64) -> Result<Vec<ToolHealth>> {
    let range = Range::last_days(days);
    let mut stmt = conn.prepare(
        "SELECT tool_counts, tool_errors_by, tool_vetoes FROM sessions
         WHERE started_at >= ?1 AND started_at < ?2 AND signals_version >= 3",
    )?;
    let mut acc: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    for row in stmt.query_map(params![range.since, range.until], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (calls, errors, vetoes) = row?;
        let parse = |j: &str| serde_json::from_str::<BTreeMap<String, i64>>(j).unwrap_or_default();
        for (k, v) in parse(&calls) {
            acc.entry(k).or_default().0 += v;
        }
        for (k, v) in parse(&errors) {
            acc.entry(k).or_default().1 += v;
        }
        for (k, v) in parse(&vetoes) {
            acc.entry(k).or_default().2 += v;
        }
    }
    let mut out: Vec<ToolHealth> = acc
        .into_iter()
        .filter(|(_, (_, e, v))| *e > 0 || *v > 0)
        .map(|(tool, (calls, errors, vetoes))| ToolHealth {
            error_rate: ratio(errors, calls),
            tool,
            calls,
            errors,
            vetoes,
        })
        .collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.errors + t.vetoes));
    out.truncate(15);
    Ok(out)
}

// -------------------------------------------------------------- anomalies

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anomaly {
    pub id: String,
    pub title: Option<String>,
    pub source: String,
    pub started_at: Option<i64>,
    pub reasons: Vec<String>,
}

/// Sessions far outside your usual range on time, cost, steering or tool
/// failures. Uses the median and median absolute deviation, so a few
/// extreme sessions don't hide each other the way they would with a mean.
pub fn anomalies(conn: &Connection, days: i64) -> Result<Vec<Anomaly>> {
    let rows = load_sessions(conn, Range::last_days(days))?;
    if rows.len() < 10 {
        return Ok(vec![]);
    }
    type Probe = (
        &'static str,
        fn(&SessionRow) -> Option<f64>,
        f64,
        fn(f64) -> String,
    );
    let probes: [Probe; 4] = [
        (
            "active time",
            |s| Some(s.active_ms as f64 / 60_000.0),
            30.0,
            |v| format!("{v:.0} active minutes"),
        ),
        ("cost", |s| s.cost_usd, 2.0, |v| format!("${v:.2}")),
        (
            "steering",
            |s| Some((s.corrections + s.interruptions) as f64),
            3.0,
            |v| format!("{v:.0} corrections and interruptions"),
        ),
        (
            "tool failures",
            |s| Some(s.tool_errors as f64),
            5.0,
            |v| format!("{v:.0} failed tool calls"),
        ),
    ];
    let mut flagged: BTreeMap<String, Anomaly> = BTreeMap::new();
    for (_, get, floor, describe) in probes {
        let values: Vec<f64> = rows.iter().filter_map(get).collect();
        let Some(median) = median(&values) else {
            continue;
        };
        let deviations: Vec<f64> = values.iter().map(|v| (v - median).abs()).collect();
        let mad = median_of(deviations).unwrap_or(0.0) * 1.4826;
        let limit = (median + 3.0 * mad).max(floor);
        for s in &rows {
            if let Some(v) = get(s)
                && v > limit
            {
                flagged
                    .entry(s.id.clone())
                    .or_insert_with(|| Anomaly {
                        id: s.id.clone(),
                        title: s.title.clone(),
                        source: s.source.clone(),
                        started_at: s.started_at,
                        reasons: vec![],
                    })
                    .reasons
                    .push(format!(
                        "{} (usually about {})",
                        describe(v),
                        describe(median)
                    ));
            }
        }
    }
    let mut out: Vec<Anomaly> = flagged.into_values().collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.started_at));
    out.truncate(20);
    Ok(out)
}

fn median(values: &[f64]) -> Option<f64> {
    median_of(values.to_vec())
}

fn median_of(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let m = v.len() / 2;
    Some(if v.len().is_multiple_of(2) {
        (v[m - 1] + v[m]) / 2.0
    } else {
        v[m]
    })
}

// -------------------------------------------------------- failure causes

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureCluster {
    pub cause: String,
    pub advice: String,
    pub ids: Vec<String>,
}

/// Groups sessions that went wrong (abandoned, redone, or reworked after
/// shipping) by their most likely cause, from the session's own signals.
/// Each session goes to the first cause that fits.
pub fn failure_clusters(conn: &Connection, days: i64) -> Result<Vec<FailureCluster>> {
    let rows = load_sessions(conn, Range::last_days(days))?;
    type Rule = (&'static str, &'static str, fn(&SessionRow) -> bool);
    let rules: [Rule; 6] = [
        (
            "Ran out of context",
            "The session hit context compaction. Split the task so each part fits one context window.",
            |s| s.compactions > 0,
        ),
        (
            "Unclear ask",
            "The opening prompt lacked done-criteria or anchors. Use the pre-flight card.",
            |s| s.friction.requirements <= 2,
        ),
        (
            "Steering churn",
            "Two or more corrections or interruptions. Restart with a sharper prompt instead of steering.",
            |s| s.corrections + s.interruptions >= 2,
        ),
        (
            "Ran too long",
            "The session sprawled. Smaller, bounded tasks succeed more often.",
            |s| s.friction.scope <= 2,
        ),
        (
            "Tool trouble",
            "Many tool calls failed. Fix the environment (commands, paths, tests) or describe it in CLAUDE.md / AGENTS.md.",
            |s| s.tool_calls >= 10 && s.tool_errors as f64 / s.tool_calls as f64 > 0.15,
        ),
        (
            "Edited without reading",
            "The agent changed files it hadn't read. Point it at the right files first.",
            |s| s.edits >= 5 && s.edits_without_read as f64 / s.edits as f64 > 0.3,
        ),
    ];
    let failed = rows.iter().filter(|s| {
        s.has_signals && (matches!(s.outcome.as_deref(), Some("abandoned" | "redo")) || s.reworked)
    });
    let mut groups: Vec<FailureCluster> = vec![];
    let mut other = vec![];
    for s in failed {
        match rules.iter().find(|(_, _, test)| test(s)) {
            Some((cause, advice, _)) => match groups.iter_mut().find(|g| g.cause == *cause) {
                Some(g) => g.ids.push(s.id.clone()),
                None => groups.push(FailureCluster {
                    cause: (*cause).into(),
                    advice: (*advice).into(),
                    ids: vec![s.id.clone()],
                }),
            },
            None => other.push(s.id.clone()),
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.ids.len()));
    if !other.is_empty() {
        groups.push(FailureCluster {
            cause: "No clear signal".into(),
            advice: "Nothing in the transcript stands out. Your notes on these sessions are the best clue.".into(),
            ids: other,
        });
    }
    Ok(groups)
}

// --------------------------------------------------------------- timeline

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lane {
    pub id: String,
    pub source: String,
    pub title: Option<String>,
    pub project_path: Option<String>,
    /// (start, end, kind): 1 = agent working, 0 = waiting on you.
    pub spans: Vec<(i64, i64, u8)>,
}

/// One lane per session active since `since`, for the swimlane view.
pub fn timelines(conn: &Connection, since: i64) -> Result<Vec<Lane>> {
    let mut stmt = conn.prepare(
        "SELECT id, source, title, project_path, timeline FROM sessions
         WHERE ended_at >= ?1 AND timeline != '[]' ORDER BY started_at LIMIT 40",
    )?;
    let lanes = stmt
        .query_map([since], |r| {
            let spans: String = r.get(4)?;
            Ok(Lane {
                id: r.get(0)?,
                source: r.get(1)?,
                title: r.get(2)?,
                project_path: r.get(3)?,
                spans: serde_json::from_str::<Vec<(i64, i64, u8)>>(&spans)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(_, end, _)| *end >= since)
                    .collect(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(lanes.into_iter().filter(|l| !l.spans.is_empty()).collect())
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::db;

    #[test]
    fn flags_outliers_and_groups_failures() {
        let conn = db::open_in_memory().unwrap();
        let now = now_ms();
        for i in 0..12 {
            let (active, corrections, outcome, compactions) = match i {
                0 => (300 * 60_000, 0, "abandoned", 1),
                1 => (10 * 60_000, 4, "redo", 0),
                _ => (10 * 60_000 + i * 1000, 0, "shipped", 0),
            };
            conn.execute(
                "INSERT INTO sessions (id, source, external_id, file_path, started_at, ended_at, active_ms,
                                       user_turns, outcome, corrections, compactions, signals_version,
                                       ask_score, timeline, ingested_at)
                 VALUES (?1, 'claude_code', ?1, '/f', ?2, ?2, ?3, 3, ?4, ?5, ?6, 3, 4, '[[1,2,1]]', 0)",
                params![format!("s{i}"), now - (i + 1) * 60_000, active, outcome, corrections, compactions],
            )
            .unwrap();
        }
        let a = anomalies(&conn, 7).unwrap();
        assert!(
            a.iter()
                .any(|x| x.id == "s0" && x.reasons[0].contains("active minutes"))
        );
        assert!(a.iter().any(|x| x.id == "s1"));
        assert!(!a.iter().any(|x| x.id == "s5"));

        let f = failure_clusters(&conn, 7).unwrap();
        let cause = |id: &str| {
            f.iter()
                .find(|g| g.ids.contains(&id.to_string()))
                .map(|g| g.cause.clone())
        };
        assert_eq!(cause("s0").as_deref(), Some("Ran out of context"));
        assert_eq!(cause("s1").as_deref(), Some("Steering churn"));

        assert_eq!(timelines(&conn, 0).unwrap().len(), 12);
    }
}
