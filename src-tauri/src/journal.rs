//! Intent → outcome journal: say what you are about to hand an agent and how
//! sure you are, then record what happened. Over time this measures how well
//! you judge which work to delegate and how long it takes.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::util::{now_ms, path_within};

/// A session counts as the intent's session if it starts this close to it.
const LINK_BEFORE_MS: i64 = 5 * 60 * 1000;
const LINK_AFTER_MS: i64 = 60 * 60 * 1000;

pub const OUTCOMES: [&str; 4] = ["shipped", "partial", "abandoned", "redo"];

pub fn outcome_score(outcome: &str) -> Option<f64> {
    match outcome {
        "shipped" => Some(1.0),
        "partial" => Some(0.5),
        "abandoned" | "redo" => Some(0.0),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewIntent {
    pub goal: String,
    pub task_type: String,
    pub project_path: Option<String>,
    pub tool: Option<String>,
    pub expected_minutes: i64,
    pub confidence: i64,
    #[serde(default)]
    pub done_when: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseIntent {
    pub id: i64,
    pub outcome: String,
    pub actual_minutes: Option<i64>,
    pub reflection: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Intent {
    pub id: i64,
    pub created_at: i64,
    pub goal: String,
    pub task_type: String,
    pub project_path: Option<String>,
    pub tool: Option<String>,
    pub expected_minutes: i64,
    pub confidence: i64,
    pub session_id: Option<String>,
    pub session_title: Option<String>,
    pub session_active_minutes: Option<f64>,
    pub outcome: Option<String>,
    pub actual_minutes: Option<i64>,
    pub reflection: Option<String>,
    pub closed_at: Option<i64>,
}

pub fn create(conn: &Connection, input: &NewIntent) -> Result<i64> {
    if input.goal.trim().is_empty() {
        bail!("Describe the goal first.");
    }
    if !(1..=5).contains(&input.confidence) {
        bail!("Confidence must be 1–5.");
    }
    if input.expected_minutes <= 0 {
        bail!("Expected minutes must be positive.");
    }
    conn.execute(
        "INSERT INTO intents (created_at, goal, task_type, project_path, tool, expected_minutes, confidence,
                              done_when, context)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            now_ms(),
            input.goal.trim(),
            input.task_type,
            input.project_path.as_deref().filter(|p| !p.trim().is_empty()),
            input.tool.as_deref().filter(|t| !t.is_empty()),
            input.expected_minutes,
            input.confidence,
            input.done_when.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            input.context.as_deref().map(str::trim).filter(|s| !s.is_empty())
        ],
    )?;
    let id = conn.last_insert_rowid();
    auto_link(conn)?;
    Ok(id)
}

pub fn close(conn: &Connection, input: &CloseIntent) -> Result<()> {
    if !OUTCOMES.contains(&input.outcome.as_str()) {
        bail!("Unknown outcome {}", input.outcome);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE intents SET outcome = ?1, actual_minutes = ?2, reflection = ?3, closed_at = ?4 WHERE id = ?5",
        params![
            input.outcome,
            input.actual_minutes,
            input.reflection.as_deref().filter(|r| !r.trim().is_empty()),
            now_ms(),
            input.id
        ],
    )?;
    // Carry the outcome over to the linked session unless it was rated already.
    tx.execute(
        "UPDATE sessions SET outcome = ?1, reviewed_at = ?2
         WHERE outcome IS NULL AND id = (SELECT session_id FROM intents WHERE id = ?3)",
        params![input.outcome, now_ms(), input.id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn reopen(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE intents SET outcome = NULL, actual_minutes = NULL, closed_at = NULL WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM intents WHERE id = ?1", [id])?;
    Ok(())
}

pub fn link(conn: &Connection, id: i64, session_id: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE intents SET session_id = ?1 WHERE id = ?2",
        params![session_id, id],
    )?;
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Intent>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.created_at, i.goal, i.task_type, i.project_path, i.tool, i.expected_minutes,
                i.confidence, i.session_id, s.title, s.active_ms / 60000.0,
                i.outcome, i.actual_minutes, i.reflection, i.closed_at
         FROM intents i LEFT JOIN sessions s ON s.id = i.session_id
         ORDER BY i.closed_at IS NOT NULL, i.created_at DESC
         LIMIT 500",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Intent {
                id: r.get(0)?,
                created_at: r.get(1)?,
                goal: r.get(2)?,
                task_type: r.get(3)?,
                project_path: r.get(4)?,
                tool: r.get(5)?,
                expected_minutes: r.get(6)?,
                confidence: r.get(7)?,
                session_id: r.get(8)?,
                session_title: r.get(9)?,
                session_active_minutes: r.get(10)?,
                outcome: r.get(11)?,
                actual_minutes: r.get(12)?,
                reflection: r.get(13)?,
                closed_at: r.get(14)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Links open intents that have a project to the first session that started
/// in that project around the time the intent was written.
pub fn auto_link(conn: &Connection) -> Result<()> {
    let open: Vec<(i64, i64, String, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT id, created_at, project_path, tool FROM intents
             WHERE session_id IS NULL AND closed_at IS NULL AND project_path IS NOT NULL",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    if open.is_empty() {
        return Ok(());
    }
    let taken: HashSet<String> = {
        let mut stmt =
            conn.prepare("SELECT session_id FROM intents WHERE session_id IS NOT NULL")?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut cand = conn.prepare(
        "SELECT id, source, project_path FROM sessions
         WHERE started_at BETWEEN ?1 AND ?2 AND project_path IS NOT NULL
         ORDER BY started_at",
    )?;
    let mut taken = taken;
    for (iid, created, project, tool) in open {
        let candidates: Vec<(String, String, String)> = cand
            .query_map(
                params![created - LINK_BEFORE_MS, created + LINK_AFTER_MS],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?
            .collect::<rusqlite::Result<_>>()?;
        let hit = candidates.into_iter().find(|(sid, source, sp)| {
            !taken.contains(sid)
                && path_within(sp, &project)
                && tool.as_deref().is_none_or(|t| t == source)
        });
        if let Some((sid, _, _)) = hit {
            conn.execute(
                "UPDATE intents SET session_id = ?1 WHERE id = ?2",
                params![sid, iid],
            )?;
            taken.insert(sid);
        }
    }
    Ok(())
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationRow {
    pub key: String,
    pub n: i64,
    /// Actual ÷ expected; above 1 means you underestimate.
    pub time_ratio: Option<f64>,
    pub success_rate: f64,
    pub avg_confidence: f64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Calibration {
    pub closed: i64,
    pub open: i64,
    pub overall: Option<CalibrationRow>,
    pub by_task_type: Vec<CalibrationRow>,
    pub by_confidence: Vec<CalibrationRow>,
    pub by_tool: Vec<CalibrationRow>,
}

struct Closed {
    task_type: String,
    tool: String,
    confidence: i64,
    expected: i64,
    actual: Option<f64>,
    score: f64,
}

pub fn calibration(conn: &Connection, since_ms: i64) -> Result<Calibration> {
    let mut stmt = conn.prepare(
        "SELECT i.task_type, COALESCE(i.tool, s.source, 'unspecified'), i.confidence, i.expected_minutes,
                COALESCE(i.actual_minutes, s.active_ms / 60000.0), i.outcome
         FROM intents i LEFT JOIN sessions s ON s.id = i.session_id
         WHERE i.closed_at IS NOT NULL AND i.created_at >= ?1",
    )?;
    let closed: Vec<Closed> = stmt
        .query_map([since_ms], |r| {
            let outcome: String = r.get(5)?;
            Ok(Closed {
                task_type: r.get(0)?,
                tool: r.get(1)?,
                confidence: r.get(2)?,
                expected: r.get(3)?,
                actual: r.get(4)?,
                score: outcome_score(&outcome).unwrap_or(0.0),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let open: i64 = conn.query_row(
        "SELECT COUNT(*) FROM intents WHERE closed_at IS NULL",
        [],
        |r| r.get(0),
    )?;

    let summarize = |key: String, rows: &[&Closed]| -> CalibrationRow {
        let n = rows.len() as f64;
        let timed: Vec<_> = rows
            .iter()
            .filter_map(|c| c.actual.map(|a| (c.expected as f64, a)))
            .collect();
        let expected_timed: f64 = timed.iter().map(|t| t.0).sum();
        let time_ratio =
            (expected_timed > 0.0).then(|| timed.iter().map(|t| t.1).sum::<f64>() / expected_timed);
        CalibrationRow {
            key,
            n: rows.len() as i64,
            time_ratio,
            success_rate: rows.iter().map(|c| c.score).sum::<f64>() / n,
            avg_confidence: rows.iter().map(|c| c.confidence as f64).sum::<f64>() / n,
        }
    };
    let group = |f: &dyn Fn(&Closed) -> String| -> Vec<CalibrationRow> {
        let mut groups: BTreeMap<String, Vec<&Closed>> = BTreeMap::new();
        for c in &closed {
            groups.entry(f(c)).or_default().push(c);
        }
        groups
            .into_iter()
            .map(|(k, rows)| summarize(k, &rows))
            .collect()
    };

    let all: Vec<&Closed> = closed.iter().collect();
    Ok(Calibration {
        closed: closed.len() as i64,
        open,
        overall: (!all.is_empty()).then(|| summarize("all".into(), &all)),
        by_task_type: group(&|c| c.task_type.clone()),
        by_confidence: group(&|c| c.confidence.to_string()),
        by_tool: group(&|c| c.tool.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn links_intent_to_session_in_same_project() {
        let conn = db::open_in_memory().unwrap();
        let id = create(
            &conn,
            &NewIntent {
                goal: "Add CSV export".into(),
                task_type: "feature".into(),
                project_path: Some("/work/app".into()),
                tool: None,
                expected_minutes: 30,
                confidence: 4,
                done_when: None,
                context: None,
            },
        )
        .unwrap();
        let created: i64 = conn
            .query_row("SELECT created_at FROM intents", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, project_path, started_at, ended_at, active_ms, ingested_at)
             VALUES ('claude_code:x', 'claude_code', 'x', '/f', '/work/app/web', ?1, ?2, 2400000, 0)",
            params![created + 60_000, created + 3_000_000],
        )
        .unwrap();
        auto_link(&conn).unwrap();
        close(
            &conn,
            &CloseIntent {
                id,
                outcome: "shipped".into(),
                actual_minutes: None,
                reflection: Some("Smaller scope would have been faster".into()),
            },
        )
        .unwrap();

        let session_outcome: String = conn
            .query_row(
                "SELECT outcome FROM sessions WHERE id = 'claude_code:x'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(session_outcome, "shipped");

        let cal = calibration(&conn, 0).unwrap();
        let overall = cal.overall.unwrap();
        assert_eq!(overall.n, 1);
        // 40 active minutes against 30 expected.
        assert!((overall.time_ratio.unwrap() - 40.0 / 30.0).abs() < 1e-9);
        assert_eq!(overall.success_rate, 1.0);
    }
}
