//! Progress over months, and what pays off. A year of sessions answers "did
//! I get better?" (success, cost per shipped, reply time and waiting, month
//! by month, with one headline) and "what works?" (results by sub-agent team
//! size, by app and by kind of work). From session summaries only, so it
//! works on compacted history.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Local, TimeZone};
use rusqlite::Connection;
use serde::Serialize;

use crate::stats::{MIN_SAMPLE, significant};
use crate::store::{Range, SessionRow, load_sessions};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Month {
    /// "2026-03".
    pub month: String,
    pub sessions: usize,
    pub rated: usize,
    /// Shipped 1, partial ½, else 0, over rated sessions.
    pub success: Option<f64>,
    pub cost_usd: f64,
    pub cost_per_shipped: Option<f64>,
    /// Typical (median) per-session average wait for you to reply.
    pub reply_ms: Option<i64>,
    /// Agent time spent waiting on you.
    pub waited_ms: i64,
    pub active_ms: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub label: String,
    pub sessions: usize,
    pub rated: usize,
    pub success: Option<f64>,
    pub avg_cost_usd: Option<f64>,
    pub cost_per_shipped: Option<f64>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub months: Vec<Month>,
    /// One sentence on the year: better, worse or flat, and in what.
    pub headline: Option<String>,
    pub by_team: Vec<Group>,
    pub by_app: Vec<Group>,
    pub by_kind: Vec<Group>,
}

fn score(o: &str) -> f64 {
    crate::journal::outcome_score(o).unwrap_or(0.0)
}

fn group(label: String, rows: &[&SessionRow]) -> Group {
    let rated: Vec<_> = rows.iter().filter(|s| s.outcome.is_some()).collect();
    let shipped = rated
        .iter()
        .filter(|s| s.outcome.as_deref() == Some("shipped"))
        .count();
    let cost: f64 = rows.iter().filter_map(|s| s.cost_usd).sum();
    let with_cost = rows.iter().filter(|s| s.cost_usd.is_some()).count();
    Group {
        label,
        sessions: rows.len(),
        rated: rated.len(),
        success: (!rated.is_empty()).then(|| {
            rated
                .iter()
                .map(|s| score(s.outcome.as_deref().unwrap_or("")))
                .sum::<f64>()
                / rated.len() as f64
        }),
        avg_cost_usd: (with_cost > 0).then(|| cost / with_cost as f64),
        cost_per_shipped: (shipped > 0).then(|| {
            let rated_cost: f64 = rated.iter().filter_map(|s| s.cost_usd).sum();
            rated_cost / shipped as f64
        }),
    }
}

fn median(mut xs: Vec<i64>) -> Option<i64> {
    xs.sort_unstable();
    xs.get(xs.len() / 2).copied().filter(|_| !xs.is_empty())
}

fn app_label(s: &SessionRow) -> String {
    match (s.source.as_str(), s.entrypoint.as_deref()) {
        (_, Some("claude-desktop")) => "Claude app".into(),
        (_, Some("copilot-app")) => "Copilot app".into(),
        (src, _) => crate::analytics::source_label(src).into(),
    }
}

pub fn progress(conn: &Connection, days: i64) -> Result<Progress> {
    let rows = load_sessions(conn, Range::last_days(days))?;
    let month_of = |s: &SessionRow| {
        s.started_at
            .and_then(|t| Local.timestamp_millis_opt(t).single())
            .map(|t| t.format("%Y-%m").to_string())
    };
    let mut by_month: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
    for s in &rows {
        if let Some(m) = month_of(s) {
            by_month.entry(m).or_default().push(s);
        }
    }
    let months: Vec<Month> = by_month
        .into_iter()
        .map(|(month, ss)| {
            let g = group(month.clone(), &ss);
            Month {
                month,
                sessions: ss.len(),
                rated: g.rated,
                success: g.success,
                cost_usd: ss.iter().filter_map(|s| s.cost_usd).sum(),
                cost_per_shipped: g.cost_per_shipped,
                reply_ms: median(
                    ss.iter()
                        .filter(|s| s.wait_count > 0)
                        .map(|s| s.wait_ms_total / s.wait_count)
                        .collect(),
                ),
                waited_ms: ss.iter().map(|s| s.wait_ms_total).sum(),
                active_ms: ss.iter().map(|s| s.active_ms).sum(),
            }
        })
        .collect();
    let headline = headline(&months);

    let team = |n: i64| match n {
        0 => "No sub-agents",
        1..=2 => "1–2 sub-agents",
        3..=4 => "3–4 sub-agents",
        _ => "5+ sub-agents",
    };
    let groups = |key: &dyn Fn(&SessionRow) -> String| {
        let mut m: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
        for s in &rows {
            m.entry(key(s)).or_default().push(s);
        }
        let mut out: Vec<Group> = m.into_iter().map(|(k, v)| group(k, &v)).collect();
        out.sort_by_key(|g| std::cmp::Reverse(g.sessions));
        out
    };
    let mut by_team = groups(&|s| team(s.subagents).to_string());
    by_team.sort_by_key(|g| {
        [
            "No sub-agents",
            "1–2 sub-agents",
            "3–4 sub-agents",
            "5+ sub-agents",
        ]
        .iter()
        .position(|l| *l == g.label)
    });
    Ok(Progress {
        months,
        headline,
        by_team,
        by_app: groups(&app_label),
        by_kind: groups(&|s| s.category.clone().unwrap_or_else(|| "other".into())),
    })
}

/// Compares the first three months with data against the last three.
fn headline(months: &[Month]) -> Option<String> {
    if months.len() < 4 {
        return None;
    }
    let span = |ms: &[Month]| {
        let rated: usize = ms.iter().map(|m| m.rated).sum();
        let success = (rated > 0).then(|| {
            ms.iter()
                .filter_map(|m| m.success.map(|s| s * m.rated as f64))
                .sum::<f64>()
                / rated as f64
        });
        let reply = median(ms.iter().filter_map(|m| m.reply_ms).collect());
        let cps: Vec<f64> = ms.iter().filter_map(|m| m.cost_per_shipped).collect();
        let cost = (!cps.is_empty()).then(|| cps.iter().sum::<f64>() / cps.len() as f64);
        (rated, success, reply, cost)
    };
    let (first, last) = (&months[..3], &months[months.len() - 3..]);
    let (r0, s0, w0, c0) = span(first);
    let (r1, s1, w1, c1) = span(last);
    let mut changes = vec![];
    if let (Some(a), Some(b)) = (s0, s1)
        && r0 >= MIN_SAMPLE
        && r1 >= MIN_SAMPLE
        && significant(a, r0, b, r1)
    {
        changes.push(format!(
            "success {} {:.0}% → {:.0}%",
            if b > a { "up" } else { "down" },
            a * 100.0,
            b * 100.0
        ));
    }
    if let (Some(a), Some(b)) = (w0, w1)
        && a > 0
        && ((b - a) as f64 / a as f64).abs() >= 0.2
    {
        changes.push(format!(
            "replies {} {} → {}",
            if b < a { "faster" } else { "slower" },
            crate::util::human_duration(a),
            crate::util::human_duration(b)
        ));
    }
    if let (Some(a), Some(b)) = (c0, c1)
        && a > 0.0
        && ((b - a) / a).abs() >= 0.2
    {
        changes.push(format!(
            "cost per shipped {} ${a:.2} → ${b:.2}",
            if b < a { "down" } else { "up" }
        ));
    }
    let n = months.len();
    Some(if changes.is_empty() {
        format!(
            "Flat for {n} months: success, reply time and cost per shipped haven't really changed. To move them, try an experiment (Improve › Experiments) or one of the patterns below."
        )
    } else {
        format!("Over {n} months: {}.", changes.join("; "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(month: &str, rated: usize, success: f64, reply: i64, cps: f64) -> Month {
        Month {
            month: month.into(),
            sessions: rated * 2,
            rated,
            success: Some(success),
            reply_ms: Some(reply),
            cost_per_shipped: Some(cps),
            ..Default::default()
        }
    }

    #[test]
    fn says_when_a_year_was_flat_and_when_it_moved() {
        let flat: Vec<Month> = (1..=12)
            .map(|i| m(&format!("2026-{i:02}"), 100, 0.63, 90_000, 11.0))
            .collect();
        assert!(headline(&flat).unwrap().starts_with("Flat for 12 months"));
        let better: Vec<Month> = (1..=12)
            .map(|i| {
                if i > 9 {
                    m(&format!("2026-{i:02}"), 100, 0.8, 45_000, 7.0)
                } else {
                    m(&format!("2026-{i:02}"), 100, 0.6, 90_000, 11.0)
                }
            })
            .collect();
        let h = headline(&better).unwrap();
        assert!(h.contains("success up 60% → 80%"), "{h}");
        assert!(h.contains("replies faster"), "{h}");
        assert!(h.contains("cost per shipped down"), "{h}");
        assert!(headline(&flat[..3]).is_none());
    }
}
