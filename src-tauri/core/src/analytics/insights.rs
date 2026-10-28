//! Rule-based insights that point at specific habits to change. Each rule is
//! a small function over the period's sessions; `RULES` lists them in the
//! order they are shown. To add one, write a rule and list it there.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Local, TimeZone, Timelike};
use rusqlite::Connection;
use serde::Serialize;

use super::{Totals, group_by, source_label, success_rate, totals};
use crate::journal;
use crate::stats::{MIN_SAMPLE, pct};
use crate::store::{Range, SessionRow, load_sessions};

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

impl Insight {
    /// Attaches the sessions behind the insight.
    fn about<'a>(mut self, rows: impl IntoIterator<Item = &'a SessionRow>) -> Self {
        self.session_ids = rows.into_iter().map(|s| s.id.clone()).collect();
        self
    }
}

/// What every rule sees: the period's sessions, those with per-session
/// signals (older sessions were parsed before the signals existed), and
/// their totals.
struct Ctx<'a> {
    conn: &'a Connection,
    range: Range,
    rows: &'a [SessionRow],
    measured: Vec<SessionRow>,
    totals: Totals,
}

/// One insight rule; most produce zero or one insight.
type Rule = fn(&Ctx) -> Result<Vec<Insight>>;

/// Every rule, in the order their insights are shown.
const RULES: &[Rule] = &[
    rate_more,
    prompt_length,
    short_prompt_followups,
    interruptions,
    duration,
    time_of_day,
    tool_compare,
    latency,
    tool_errors,
    corrections,
    compaction,
    edit_hygiene,
    categories,
    calibration,
];

pub fn insights(conn: &Connection, days: i64) -> Result<Vec<Insight>> {
    let range = Range::last_days(days);
    let rows = load_sessions(conn, range)?;
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ctx = Ctx {
        conn,
        range,
        measured: rows.iter().filter(|s| s.has_signals).cloned().collect(),
        totals: totals(&rows),
        rows: &rows,
    };
    let mut out = vec![];
    for rule in RULES {
        out.extend(rule(&ctx)?);
    }
    Ok(out)
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

/// Success with none of something against two or more, when it drops by `gap`.
fn first_vs_last<'a, 'b>(bs: &'b [Bucket<'a>], gap: f64) -> Option<(f64, f64)> {
    let (first, last) = (bs.first()?, bs.last()?);
    let (a, b) = (first.success()?, last.success()?);
    (first.rated() >= MIN_SAMPLE && last.rated() >= MIN_SAMPLE && a - b >= gap).then_some((a, b))
}

fn by_prompt_length(rows: &[SessionRow]) -> Vec<Bucket<'_>> {
    buckets(
        rows,
        &["short (<80 chars)", "medium (80–300)", "detailed (300+)"],
        |s| {
            Some(match s.first_prompt_len {
                0..80 => 0,
                80..300 => 1,
                _ => 2,
            })
        },
    )
}

fn rate_more(c: &Ctx) -> Result<Vec<Insight>> {
    let rows = c.rows;
    let rated = rows.iter().filter(|s| s.outcome.is_some()).count();
    if rated * 2 >= rows.len() {
        return Ok(vec![]);
    }
    Ok(vec![insight(
        "rate-more",
        "info",
        format!("Rate {} more sessions to sharpen these insights", rows.len() - rated),
        "Most comparisons need outcomes. Mark sessions shipped, partial, abandoned or redo on the Sessions page; it takes a few seconds each.".into(),
        format!("{rated} of {} sessions rated", rows.len()),
    )
    .about(rows.iter().filter(|s| s.outcome.is_none()))])
}

fn prompt_length(c: &Ctx) -> Result<Vec<Insight>> {
    let by_len = by_prompt_length(c.rows);
    let Some((best, br, worst, wr)) = success_spread(&by_len) else {
        return Ok(vec![]);
    };
    Ok(vec![insight(
        "prompt-length",
        if best.label.starts_with("detailed") { "positive" } else { "info" },
        format!("Sessions that open with {} prompts succeed more", best.label),
        format!(
            "Success rate is {} for {} opening prompts versus {} for {}. Spend a minute more on the first message: goal, constraints, and how you will check the result.",
            pct(br), best.label, pct(wr), worst.label
        ),
        format!("{} vs {} rated sessions", best.rated(), worst.rated()),
    )
    .about(worst.rows.iter().copied())])
}

fn short_prompt_followups(c: &Ctx) -> Result<Vec<Insight>> {
    let by_len = by_prompt_length(c.rows);
    let (short, long) = (&by_len[0], &by_len[2]);
    if short.rows.len() < MIN_SAMPLE || long.rows.len() < MIN_SAMPLE {
        return Ok(vec![]);
    }
    let (st, lt) = (
        short.avg(|s| s.user_turns as f64),
        long.avg(|s| s.user_turns as f64),
    );
    if st < lt * 1.5 || st - lt < 2.0 {
        return Ok(vec![]);
    }
    Ok(vec![insight(
        "short-prompt-followups",
        "warning",
        "Short opening prompts lead to more back-and-forth".into(),
        format!("Sessions with a short first prompt took {st:.1} prompts on average, against {lt:.1} for detailed ones. The follow-ups are often context you could have given up front."),
        format!("{} short, {} detailed sessions", short.rows.len(), long.rows.len()),
    )
    .about(short.rows.iter().copied())])
}

fn interruptions(c: &Ctx) -> Result<Vec<Insight>> {
    let by_int = buckets(
        c.rows,
        &["no interruptions", "one interruption", "2+ interruptions"],
        |s| Some(s.interruptions.min(2) as usize),
    );
    let Some((calm, rough)) = first_vs_last(&by_int, 0.2) else {
        return Ok(vec![]);
    };
    Ok(vec![insight(
        "interruptions",
        "warning",
        "Sessions you interrupt twice or more rarely ship".into(),
        format!(
            "Success drops from {} with no interruptions to {} with 2+. When you need to stop an agent twice, the plan was probably off; restart with a sharper prompt instead of steering mid-flight.",
            pct(calm), pct(rough)
        ),
        format!("{} vs {} rated sessions", by_int[0].rated(), by_int[2].rated()),
    )
    .about(by_int[2].rows.iter().copied())])
}

fn duration(c: &Ctx) -> Result<Vec<Insight>> {
    let by_dur = buckets(
        c.rows,
        &["under 20 min", "20–60 min", "over 60 min"],
        |s| {
            Some(match s.active_ms / 60_000 {
                0..20 => 0,
                20..60 => 1,
                _ => 2,
            })
        },
    );
    let Some((best, br, worst, wr)) = success_spread(&by_dur) else {
        return Ok(vec![]);
    };
    Ok(vec![insight(
        "duration",
        "info",
        format!("Your sweet spot is sessions of {} active time", best.label),
        format!(
            "{} succeed {} of the time; {} only {}. Consider splitting work so each session fits the range that works for you.",
            best.label, pct(br), worst.label, pct(wr)
        ),
        format!("{} vs {} rated sessions", best.rated(), worst.rated()),
    )
    .about(worst.rows.iter().copied())])
}

fn time_of_day(c: &Ctx) -> Result<Vec<Insight>> {
    let by_hour = buckets(
        c.rows,
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
    let Some((best, br, worst, wr)) = success_spread(&by_hour) else {
        return Ok(vec![]);
    };
    Ok(vec![insight(
        "time-of-day",
        "info",
        format!("You execute best in the {}", best.label),
        format!("Success is {} in the {} versus {} in the {}. Schedule hard delegation for your strong hours.", pct(br), best.label, pct(wr), worst.label),
        format!("{} vs {} rated sessions", best.rated(), worst.rated()),
    )
    .about(worst.rows.iter().copied())])
}

fn tool_compare(c: &Ctx) -> Result<Vec<Insight>> {
    let by_source = group_by(c.rows, |s| Some(s.source.clone()));
    let comparable: Vec<(&str, usize, f64)> = by_source
        .iter()
        .filter(|g| g.rated >= MIN_SAMPLE)
        .filter_map(|g| Some((g.key.as_str(), g.rated, g.success_rate?)))
        .collect();
    if comparable.len() < 2 {
        return Ok(vec![]);
    }
    let best = comparable
        .iter()
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .unwrap();
    let worst = comparable
        .iter()
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .unwrap();
    if best.2 - worst.2 < 0.15 {
        return Ok(vec![]);
    }
    let (b, w) = (source_label(best.0), source_label(worst.0));
    Ok(vec![insight(
        "tool-compare",
        "info",
        format!("{b} is landing more of your work than {w}"),
        format!(
            "{b} sessions succeed {} of the time against {} for {w}. Check whether that is the tool or the kind of task you give each one.",
            pct(best.2),
            pct(worst.2)
        ),
        format!("{} vs {} rated sessions", best.1, worst.1),
    )])
}

/// Response latency: the human as the bottleneck.
fn latency(c: &Ctx) -> Result<Vec<Insight>> {
    let t = &c.totals;
    let Some(avg) = t.avg_wait_seconds else {
        return Ok(vec![]);
    };
    let waits: i64 = c.rows.iter().map(|s| s.wait_count).sum();
    if waits < 10 {
        return Ok(vec![]);
    }
    Ok(vec![if avg >= 120.0 {
        insight(
            "latency",
            "warning",
            format!("Agents waited {:.1} hours on you", t.wait_hours),
            format!(
                "After an agent finishes a turn, your next prompt comes {:.1} minutes later on average. Batch reviews, keep the Live page open, or give longer autonomous tasks so agents don't sit idle.",
                avg / 60.0
            ),
            format!("{waits} hand-backs measured (gaps over 30 min excluded)"),
        )
    } else {
        insight(
            "latency-good",
            "positive",
            format!("You respond to agents in {:.0} seconds on average", avg),
            "Agents rarely sit idle waiting on you. If you want more throughput, run sessions in parallel rather than responding faster.".into(),
            format!("{waits} hand-backs measured"),
        )
    }])
}

fn tool_errors(c: &Ctx) -> Result<Vec<Insight>> {
    let t = &c.totals;
    if t.tool_calls < 50 {
        return Ok(vec![]);
    }
    let rate = t.tool_errors as f64 / t.tool_calls as f64;
    if rate < 0.12 {
        return Ok(vec![]);
    }
    Ok(vec![insight(
        "tool-errors",
        "warning",
        format!("{} of agent tool calls fail", pct(rate)),
        "Frequent failures usually mean missing setup: commands that don't exist, wrong paths, absent env vars or tests that can't run. Put the right commands in CLAUDE.md / AGENTS.md so agents stop guessing.".into(),
        format!("{} errors in {} tool calls", t.tool_errors, t.tool_calls),
    )
    .about(c.rows.iter().filter(|s| s.tool_errors > 0))])
}

/// Corrections: the human had to say "no, that's not it".
fn corrections(c: &Ctx) -> Result<Vec<Insight>> {
    let by_corr = buckets(
        &c.measured,
        &["no corrections", "one correction", "2+ corrections"],
        |s| Some(s.corrections.min(2) as usize),
    );
    let Some((clean, rough)) = first_vs_last(&by_corr, 0.2) else {
        return Ok(vec![]);
    };
    Ok(vec![insight(
        "corrections",
        "warning",
        "Sessions you have to correct twice rarely recover".into(),
        format!(
            "Success is {} with no corrections and {} after two or more. After the second \"no, that's not it\", start a fresh session with what you learned instead of steering further.",
            pct(clean),
            pct(rough)
        ),
        format!("{} vs {} rated sessions", by_corr[0].rated(), by_corr[2].rated()),
    )
    .about(by_corr[2].rows.iter().copied())])
}

/// Context compaction: the session outgrew its context window.
fn compaction(c: &Ctx) -> Result<Vec<Insight>> {
    let by_compact = buckets(&c.measured, &["no compaction", "compacted"], |s| {
        Some(usize::from(s.compactions > 0))
    });
    let Some((clean, compacted)) = first_vs_last(&by_compact, 0.15) else {
        return Ok(vec![]);
    };
    Ok(vec![insight(
        "compaction",
        "warning",
        "Sessions that hit context compaction succeed less".into(),
        format!(
            "{} succeed without compaction versus {} once the context fills up. Split big tasks so each fits one context window.",
            pct(clean),
            pct(compacted)
        ),
        format!("{} vs {} rated sessions", by_compact[0].rated(), by_compact[1].rated()),
    )
    .about(by_compact[1].rows.iter().copied())])
}

fn edit_hygiene(c: &Ctx) -> Result<Vec<Insight>> {
    let measured = &c.measured;
    let edits: i64 = measured.iter().map(|s| s.edits).sum();
    if edits < 40 {
        return Ok(vec![]);
    }
    let blind = measured.iter().map(|s| s.edits_without_read).sum::<i64>() as f64 / edits as f64;
    let re = measured.iter().map(|s| s.re_edits).sum::<i64>() as f64 / edits as f64;
    let mut out = vec![];
    if blind >= 0.25 {
        out.push(
            insight(
                "blind-edits",
                "warning",
                format!("{} of agent edits change files it hasn't read", pct(blind)),
                "Edits made without reading the file first are more likely to be wrong. Point the agent at the relevant files in your prompt, or add \"read before editing\" to your instructions file.".into(),
                format!("{edits} edits"),
            )
            .about(measured.iter().filter(|s| s.edits_without_read > 0)),
        );
    }
    if re >= 0.35 {
        out.push(
            insight(
                "re-edits",
                "warning",
                format!("{} of edits rework a file already changed in the session", pct(re)),
                "Repeated edits to the same file usually mean a fix-try-fix loop. Ask for a plan before code, or give the agent a test to run so it can check itself.".into(),
                format!("{edits} edits"),
            )
            .about(measured.iter().filter(|s| s.re_edits > 0)),
        );
    }
    Ok(out)
}

/// Task categories.
fn categories(c: &Ctx) -> Result<Vec<Insight>> {
    let mut cats: BTreeMap<String, Vec<&SessionRow>> = BTreeMap::new();
    for s in &c.measured {
        if let Some(cat) = &s.category {
            cats.entry(cat.clone()).or_default().push(s);
        }
    }
    let cat_rates: Vec<(String, usize, f64)> = cats
        .iter()
        .filter_map(|(cat, g)| {
            let rated = g.iter().filter(|s| s.outcome.is_some()).count();
            let rate = success_rate(g.iter().copied())?;
            (rated >= MIN_SAMPLE).then(|| (cat.clone(), rated, rate))
        })
        .collect();
    let (Some(best), Some(worst)) = (
        cat_rates.iter().max_by(|a, b| a.2.total_cmp(&b.2)),
        cat_rates.iter().min_by(|a, b| a.2.total_cmp(&b.2)),
    ) else {
        return Ok(vec![]);
    };
    if best.2 - worst.2 < 0.25 {
        return Ok(vec![]);
    }
    Ok(vec![insight(
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
    )
    .about(cats[&worst.0].iter().copied())])
}

/// Journal calibration: estimates against what happened.
fn calibration(c: &Ctx) -> Result<Vec<Insight>> {
    let cal = journal::calibration(c.conn, c.range.since)?;
    let Some(all) = &cal.overall else {
        return Ok(vec![]);
    };
    let mut out = vec![];
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
    Ok(out)
}
