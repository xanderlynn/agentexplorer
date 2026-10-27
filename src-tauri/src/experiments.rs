//! Experiments and goals: the "decide" and "measure" half of the loop.
//!
//! An experiment changes one habit for a fixed window and compares one
//! metric against a baseline locked when it starts. A goal is a target on
//! one metric, always paired with a counter-metric so it can't be met by
//! making something else worse. Volume counts (sessions, commits, prompts)
//! are deliberately not in the catalog: they reward doing more, not better.

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::analytics::{Range, SessionRow, load_sessions};
use crate::journal::outcome_score;
use crate::util::now_ms;

const DAY_MS: i64 = 86_400_000;
/// The baseline is the same metric over this many days before the start.
const BASELINE_DAYS: i64 = 28;
const MAX_ACTIVE_GOALS: i64 = 3;
/// Results with fewer sessions than this are "too early to tell".
pub const MIN_N: usize = 5;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricDef {
    pub key: &'static str,
    pub label: &'static str,
    pub higher_is_better: bool,
    /// "rate" (0..1), "score" (1..5) or "number".
    pub unit: &'static str,
}

pub const METRICS: &[MetricDef] = &[
    MetricDef {
        key: "success_rate",
        label: "Success rate (rated sessions)",
        higher_is_better: true,
        unit: "rate",
    },
    MetricDef {
        key: "corrections_per_session",
        label: "Corrections per session",
        higher_is_better: false,
        unit: "number",
    },
    MetricDef {
        key: "interruptions_per_session",
        label: "Interruptions per session",
        higher_is_better: false,
        unit: "number",
    },
    MetricDef {
        key: "re_edit_rate",
        label: "Re-edit rate",
        higher_is_better: false,
        unit: "rate",
    },
    MetricDef {
        key: "blind_edit_rate",
        label: "Blind-edit rate",
        higher_is_better: false,
        unit: "rate",
    },
    MetricDef {
        key: "clear_ask",
        label: "Clear-ask score",
        higher_is_better: true,
        unit: "score",
    },
    MetricDef {
        key: "flow_score",
        label: "Flow score",
        higher_is_better: true,
        unit: "score",
    },
    MetricDef {
        key: "active_minutes",
        label: "Active minutes per session",
        higher_is_better: false,
        unit: "number",
    },
    MetricDef {
        key: "commit_rate",
        label: "Sessions that reach a commit",
        higher_is_better: true,
        unit: "rate",
    },
    MetricDef {
        key: "cost_per_session",
        label: "Cost per session (USD)",
        higher_is_better: false,
        unit: "number",
    },
    MetricDef {
        key: "rework_rate",
        label: "Rework rate (fixes or reverts within 7 days)",
        higher_is_better: false,
        unit: "rate",
    },
];

pub fn metric(key: &str) -> Result<&'static MetricDef> {
    METRICS
        .iter()
        .find(|m| m.key == key)
        .ok_or_else(|| anyhow::anyhow!("Unknown metric {key}"))
}

/// The metric's value over `rows` and how many sessions it rests on.
pub fn measure(key: &str, rows: &[&SessionRow]) -> (Option<f64>, usize) {
    let mean = |vals: Vec<f64>| -> (Option<f64>, usize) {
        let n = vals.len();
        ((n > 0).then(|| vals.iter().sum::<f64>() / n as f64), n)
    };
    let ratio = |num: i64, den: i64, n: usize| ((den > 0).then(|| num as f64 / den as f64), n);
    match key {
        "success_rate" => mean(
            rows.iter()
                .filter_map(|s| s.outcome.as_deref().and_then(outcome_score))
                .collect(),
        ),
        "corrections_per_session" => mean(rows.iter().map(|s| s.corrections as f64).collect()),
        "interruptions_per_session" => mean(rows.iter().map(|s| s.interruptions as f64).collect()),
        "re_edit_rate" => ratio(
            rows.iter().map(|s| s.re_edits).sum(),
            rows.iter().map(|s| s.edits).sum(),
            rows.len(),
        ),
        "blind_edit_rate" => ratio(
            rows.iter().map(|s| s.edits_without_read).sum(),
            rows.iter().map(|s| s.edits).sum(),
            rows.len(),
        ),
        "clear_ask" => mean(
            rows.iter()
                .map(|s| s.friction.requirements as f64)
                .collect(),
        ),
        "flow_score" => mean(rows.iter().map(|s| s.friction.overall).collect()),
        "active_minutes" => mean(rows.iter().map(|s| s.active_ms as f64 / 60_000.0).collect()),
        "commit_rate" => {
            let in_repo: Vec<_> = rows.iter().filter(|s| s.repo_root.is_some()).collect();
            ratio(
                in_repo.iter().filter(|s| s.commits > 0).count() as i64,
                in_repo.len() as i64,
                in_repo.len(),
            )
        }
        "cost_per_session" => mean(rows.iter().filter_map(|s| s.cost_usd).collect()),
        "rework_rate" => {
            let shipped: Vec<_> = rows.iter().filter(|s| s.commits > 0).collect();
            ratio(
                shipped.iter().filter(|s| s.reworked).count() as i64,
                shipped.len() as i64,
                shipped.len(),
            )
        }
        _ => (None, 0),
    }
}

fn in_scope<'a>(
    rows: &'a [SessionRow],
    category: Option<&str>,
    source: Option<&str>,
) -> Vec<&'a SessionRow> {
    rows.iter()
        .filter(|s| s.has_signals)
        .filter(|s| category.is_none_or(|c| s.category.as_deref() == Some(c)))
        .filter(|s| source.is_none_or(|src| s.source == src))
        .collect()
}

// ------------------------------------------------------------- experiments

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewExperiment {
    pub title: String,
    pub hypothesis: Option<String>,
    pub metric: String,
    pub category: Option<String>,
    pub source: Option<String>,
    pub days: i64,
    pub retro_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Experiment {
    pub id: i64,
    pub created_at: i64,
    pub title: String,
    pub hypothesis: Option<String>,
    pub metric: MetricDef,
    pub category: Option<String>,
    pub source: Option<String>,
    pub start_at: i64,
    pub end_at: i64,
    pub baseline_value: Option<f64>,
    pub baseline_n: i64,
    pub current_value: Option<f64>,
    pub current_n: usize,
    /// Sessions in the window you tagged as applying the experiment.
    pub applied: usize,
    pub eligible: usize,
    /// "too_early", "better", "worse" or "no_change".
    pub reading: String,
    pub status: String,
    pub verdict_note: Option<String>,
}

/// The tag that marks a session as applying an experiment.
pub fn tag_for(id: i64) -> String {
    format!("exp-{id}")
}

pub fn create(conn: &Connection, e: &NewExperiment) -> Result<i64> {
    if e.title.trim().is_empty() {
        bail!("Give the experiment a name.");
    }
    metric(&e.metric)?;
    let start = now_ms();
    let days = e.days.clamp(3, 60);
    let base_rows = load_sessions(
        conn,
        Range {
            since: start - BASELINE_DAYS * DAY_MS,
            until: start,
        },
    )?;
    let category = e.category.as_deref().filter(|c| !c.is_empty());
    let source = e.source.as_deref().filter(|c| !c.is_empty());
    let (baseline, n) = measure(&e.metric, &in_scope(&base_rows, category, source));
    conn.execute(
        "INSERT INTO experiments (created_at, title, hypothesis, metric, category, source, start_at, end_at,
                                  baseline_value, baseline_n, retro_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?1, ?7, ?8, ?9, ?10)",
        params![
            start,
            e.title.trim(),
            e.hypothesis.as_deref().map(str::trim).filter(|h| !h.is_empty()),
            e.metric,
            category,
            source,
            start + days * DAY_MS,
            baseline,
            n as i64,
            e.retro_id
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn reading(def: &MetricDef, baseline: Option<f64>, current: Option<f64>, n: usize) -> &'static str {
    let (Some(b), Some(c)) = (baseline, current) else {
        return "too_early";
    };
    if n < MIN_N {
        return "too_early";
    }
    // Ignore changes smaller than 10% of the baseline (or 0.05 on a rate).
    let threshold = if def.unit == "rate" {
        0.05
    } else {
        (b.abs() * 0.1).max(0.1)
    };
    let d = c - b;
    if d.abs() < threshold {
        "no_change"
    } else if (d > 0.0) == def.higher_is_better {
        "better"
    } else {
        "worse"
    }
}

pub fn list(conn: &Connection) -> Result<Vec<Experiment>> {
    struct Row {
        id: i64,
        created_at: i64,
        title: String,
        hypothesis: Option<String>,
        metric: String,
        category: Option<String>,
        source: Option<String>,
        start_at: i64,
        end_at: i64,
        baseline_value: Option<f64>,
        baseline_n: i64,
        status: String,
        verdict_note: Option<String>,
    }
    let rows: Vec<Row> = conn
        .prepare(
            "SELECT id, created_at, title, hypothesis, metric, category, source, start_at, end_at,
                    baseline_value, baseline_n, status, verdict_note
             FROM experiments ORDER BY status = 'active' DESC, start_at DESC",
        )?
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                created_at: r.get(1)?,
                title: r.get(2)?,
                hypothesis: r.get(3)?,
                metric: r.get(4)?,
                category: r.get(5)?,
                source: r.get(6)?,
                start_at: r.get(7)?,
                end_at: r.get(8)?,
                baseline_value: r.get(9)?,
                baseline_n: r.get(10)?,
                status: r.get(11)?,
                verdict_note: r.get(12)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut out = vec![];
    for e in rows {
        let Ok(def) = metric(&e.metric) else { continue };
        let window = load_sessions(
            conn,
            Range {
                since: e.start_at,
                until: e.end_at.min(now_ms() + 1),
            },
        )?;
        let eligible = in_scope(&window, e.category.as_deref(), e.source.as_deref());
        let tag = tag_for(e.id);
        let applied: Vec<&SessionRow> = eligible
            .iter()
            .copied()
            .filter(|s| s.tags.contains(&tag))
            .collect();
        // Measure the sessions where the habit was applied when you've
        // tagged any; otherwise everything in scope.
        let measured = if applied.is_empty() {
            eligible.clone()
        } else {
            applied.clone()
        };
        let (current, n) = measure(&e.metric, &measured);
        out.push(Experiment {
            id: e.id,
            created_at: e.created_at,
            title: e.title,
            hypothesis: e.hypothesis,
            metric: *def,
            category: e.category,
            source: e.source,
            start_at: e.start_at,
            end_at: e.end_at,
            baseline_value: e.baseline_value,
            baseline_n: e.baseline_n,
            current_value: current,
            current_n: n,
            applied: applied.len(),
            eligible: eligible.len(),
            reading: reading(def, e.baseline_value, current, n).into(),
            status: e.status,
            verdict_note: e.verdict_note,
        });
    }
    Ok(out)
}

/// "keep" or "drop" closes the experiment; "extend" adds a week.
pub fn verdict(conn: &Connection, id: i64, decision: &str, note: Option<&str>) -> Result<()> {
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    match decision {
        "keep" | "drop" => {
            let status = if decision == "keep" {
                "kept"
            } else {
                "dropped"
            };
            conn.execute(
                "UPDATE experiments SET status = ?1, verdict_note = ?2, end_at = MIN(end_at, ?3) WHERE id = ?4",
                params![status, note, now_ms(), id],
            )?;
        }
        "extend" => {
            conn.execute(
                "UPDATE experiments SET end_at = MAX(end_at, ?1) + ?2, status = 'active' WHERE id = ?3",
                params![now_ms(), 7 * DAY_MS, id],
            )?;
        }
        other => bail!("Unknown decision {other}"),
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM experiments WHERE id = ?1", [id])?;
    Ok(())
}

/// Marks a session as applying (or not) an experiment.
pub fn set_applied(
    conn: &Connection,
    session_id: &str,
    experiment_id: i64,
    applied: bool,
) -> Result<()> {
    let tags: String = conn
        .query_row(
            "SELECT tags FROM sessions WHERE id = ?1",
            [session_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| "[]".into());
    let mut tags: Vec<String> = serde_json::from_str(&tags).unwrap_or_default();
    let tag = tag_for(experiment_id);
    tags.retain(|t| t != &tag);
    if applied {
        tags.push(tag);
    }
    conn.execute(
        "UPDATE sessions SET tags = ?1 WHERE id = ?2",
        params![serde_json::to_string(&tags)?, session_id],
    )?;
    Ok(())
}

// ------------------------------------------------------------------- goals

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewGoal {
    pub metric: String,
    pub target: f64,
    pub counter_metric: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalStatus {
    pub id: i64,
    pub metric: MetricDef,
    pub target: f64,
    pub current: Option<f64>,
    pub n: usize,
    /// True when the last 7 days meet the target.
    pub met: Option<bool>,
    pub counter: MetricDef,
    pub counter_current: Option<f64>,
    pub counter_baseline: Option<f64>,
    /// True when the counter-metric moved the wrong way by more than 10%.
    pub counter_slipping: bool,
}

pub fn create_goal(conn: &Connection, g: &NewGoal) -> Result<i64> {
    metric(&g.metric)?;
    metric(&g.counter_metric)?;
    if g.metric == g.counter_metric {
        bail!(
            "Pick a different counter-metric: it guards against meeting the goal by making something else worse."
        );
    }
    let active: i64 = conn.query_row("SELECT COUNT(*) FROM goals WHERE active = 1", [], |r| {
        r.get(0)
    })?;
    if active >= MAX_ACTIVE_GOALS {
        bail!(
            "You already have {MAX_ACTIVE_GOALS} goals. Retire one first; fewer goals work better."
        );
    }
    conn.execute(
        "INSERT INTO goals (created_at, metric, target, counter_metric) VALUES (?1, ?2, ?3, ?4)",
        params![now_ms(), g.metric, g.target, g.counter_metric],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn retire_goal(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE goals SET active = 0 WHERE id = ?1", [id])?;
    Ok(())
}

pub fn goals(conn: &Connection) -> Result<Vec<GoalStatus>> {
    let now = now_ms();
    let week = load_sessions(
        conn,
        Range {
            since: now - 7 * DAY_MS,
            until: now + 1,
        },
    )?;
    let before = load_sessions(
        conn,
        Range {
            since: now - (7 + BASELINE_DAYS) * DAY_MS,
            until: now - 7 * DAY_MS,
        },
    )?;
    let week_rows = in_scope(&week, None, None);
    let before_rows = in_scope(&before, None, None);
    let mut stmt = conn.prepare(
        "SELECT id, metric, target, counter_metric FROM goals WHERE active = 1 ORDER BY id",
    )?;
    let goals: Vec<(i64, String, f64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = vec![];
    for (id, m, target, c) in goals {
        let (Ok(def), Ok(counter)) = (metric(&m), metric(&c)) else {
            continue;
        };
        let (current, n) = measure(&m, &week_rows);
        let (counter_current, _) = measure(&c, &week_rows);
        let (counter_baseline, _) = measure(&c, &before_rows);
        let met = current.map(|v| {
            if def.higher_is_better {
                v >= target
            } else {
                v <= target
            }
        });
        let counter_slipping = match (counter_current, counter_baseline) {
            (Some(now), Some(base)) => {
                let margin =
                    (base.abs() * 0.1).max(if counter.unit == "rate" { 0.05 } else { 0.1 });
                if counter.higher_is_better {
                    now < base - margin
                } else {
                    now > base + margin
                }
            }
            _ => false,
        };
        out.push(GoalStatus {
            id,
            metric: *def,
            target,
            current,
            n,
            met,
            counter: *counter,
            counter_current,
            counter_baseline,
            counter_slipping,
        });
    }
    Ok(out)
}

// -------------------------------------------------------------- pre-flight

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightAdvice {
    pub category: String,
    pub category_success: Option<f64>,
    pub category_n: usize,
    /// Success when the opening prompt had done-criteria and anchors vs not.
    pub clear_success: Option<f64>,
    pub clear_n: usize,
    pub vague_success: Option<f64>,
    pub vague_n: usize,
    /// Actual ÷ expected time for this category in your journal.
    pub time_ratio: Option<f64>,
    pub ask: crate::signals::AskCheck,
}

pub fn preflight(conn: &Connection, text: &str, source: Option<&str>) -> Result<PreflightAdvice> {
    let category = crate::signals::categorize(text, "");
    let rows = load_sessions(conn, Range::last_days(180))?;
    let scoped = in_scope(&rows, Some(category), source.filter(|s| !s.is_empty()));
    let (category_success, category_n) = measure("success_rate", &scoped);
    let clear: Vec<&SessionRow> = scoped
        .iter()
        .copied()
        .filter(|s| s.friction.requirements >= 4)
        .collect();
    let vague: Vec<&SessionRow> = scoped
        .iter()
        .copied()
        .filter(|s| s.friction.requirements <= 2)
        .collect();
    let (clear_success, clear_n) = measure("success_rate", &clear);
    let (vague_success, vague_n) = measure("success_rate", &vague);
    let cal = crate::journal::calibration(conn, 0)?;
    let time_ratio = cal
        .by_task_type
        .iter()
        .find(|r| r.key == category && r.n >= 3)
        .and_then(|r| r.time_ratio);
    Ok(PreflightAdvice {
        category: category.into(),
        category_success,
        category_n,
        clear_success,
        clear_n,
        vague_success,
        vague_n,
        time_ratio,
        ask: crate::signals::check_ask(text),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn insert(
        conn: &Connection,
        id: &str,
        started: i64,
        outcome: &str,
        corrections: i64,
        tags: &str,
    ) {
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, started_at, ended_at, active_ms,
                                   user_turns, outcome, corrections, category, tags, signals_version,
                                   ask_score, ingested_at)
             VALUES (?1, 'claude_code', ?1, '/f', ?2, ?2, 600000, 3, ?3, ?4, 'bugfix', ?5, 2, 3, 0)",
            params![id, started, outcome, corrections, tags],
        )
        .unwrap();
    }

    #[test]
    fn experiment_locks_baseline_and_reads_tagged_sessions() {
        let conn = db::open_in_memory().unwrap();
        let now = now_ms();
        for i in 0..6 {
            insert(
                &conn,
                &format!("b{i}"),
                now - (i + 1) * DAY_MS,
                "abandoned",
                2,
                "[]",
            );
        }
        let id = create(
            &conn,
            &NewExperiment {
                title: "Failing test first".into(),
                hypothesis: None,
                metric: "success_rate".into(),
                category: Some("bugfix".into()),
                source: None,
                days: 14,
                retro_id: None,
            },
        )
        .unwrap();
        // Sessions after the start, timestamped "now" so the window includes them.
        let after = now_ms();
        for i in 0..5 {
            insert(
                &conn,
                &format!("a{i}"),
                after,
                "shipped",
                0,
                &format!("[\"exp-{id}\"]"),
            );
        }
        insert(&conn, "untagged", after, "abandoned", 0, "[]");
        let e = &list(&conn).unwrap()[0];
        assert_eq!(e.baseline_value, Some(0.0));
        assert_eq!(e.baseline_n, 6);
        assert_eq!((e.applied, e.eligible), (5, 6));
        assert_eq!(e.current_value, Some(1.0));
        assert_eq!(e.reading, "better");
    }

    #[test]
    fn goals_need_a_distinct_counter_metric_and_are_capped() {
        let conn = db::open_in_memory().unwrap();
        let g = |m: &str, c: &str| NewGoal {
            metric: m.into(),
            target: 0.6,
            counter_metric: c.into(),
        };
        assert!(create_goal(&conn, &g("success_rate", "success_rate")).is_err());
        for _ in 0..3 {
            create_goal(&conn, &g("success_rate", "rework_rate")).unwrap();
        }
        assert!(create_goal(&conn, &g("success_rate", "rework_rate")).is_err());
        assert!(create_goal(&conn, &g("sessions", "rework_rate")).is_err());
    }
}
