//! Where your attention goes: how long agents wait on you, split into short
//! waits (replying) and away spells (meetings, the night), when in the day
//! they happen, and which projects wait most. From each session's timeline,
//! whose kind-0 spans are waits on you.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Local, TimeZone, Timelike};
use rusqlite::Connection;
use serde::Serialize;

use crate::live::AWAY_AFTER_MS;
use crate::store::Range;

#[derive(Debug, Default, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hour {
    pub hour: u32,
    pub short_ms: i64,
    pub away_ms: i64,
    pub waits: usize,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWait {
    pub project: String,
    pub wait_ms: i64,
    pub waits: usize,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    /// Agent time waiting on you in waits up to `AWAY_AFTER_MS`.
    pub short_ms: i64,
    /// Agent time waiting on you in longer waits: away spells.
    pub away_ms: i64,
    pub waits: usize,
    pub away_spells: usize,
    /// Your typical reply, over short waits only.
    pub median_reply_ms: Option<i64>,
    /// By the local hour the wait began.
    pub by_hour: Vec<Hour>,
    /// Projects whose agents waited on you most.
    pub projects: Vec<ProjectWait>,
    /// The hour when agents wait on you longest, by total time.
    pub worst_hour: Option<u32>,
}

pub fn attention(conn: &Connection, days: i64) -> Result<Attention> {
    let range = Range::last_days(days);
    let mut st = conn.prepare(
        "SELECT COALESCE(repo_root, project_path, ''), timeline FROM sessions
         WHERE ended_at >= ?1 AND started_at < ?2",
    )?;
    let mut a = Attention {
        by_hour: (0..24)
            .map(|hour| Hour {
                hour,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let mut replies = vec![];
    let mut projects: HashMap<String, (i64, usize)> = HashMap::new();
    let rows = st.query_map([range.since, range.until], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (project, timeline) = row?;
        let spans: Vec<(i64, i64, u8)> = serde_json::from_str(&timeline).unwrap_or_default();
        for (start, end, kind) in spans {
            if kind != 0 || end <= start || start < range.since {
                continue;
            }
            let ms = end - start;
            let hour = Local
                .timestamp_millis_opt(start)
                .single()
                .map(|t| t.hour())
                .unwrap_or(0) as usize;
            let h = &mut a.by_hour[hour];
            h.waits += 1;
            a.waits += 1;
            if ms > AWAY_AFTER_MS {
                a.away_ms += ms;
                a.away_spells += 1;
                h.away_ms += ms;
            } else {
                a.short_ms += ms;
                h.short_ms += ms;
                replies.push(ms);
            }
            let p = projects.entry(project.clone()).or_default();
            p.0 += ms;
            p.1 += 1;
        }
    }
    replies.sort_unstable();
    a.median_reply_ms = replies
        .get(replies.len() / 2)
        .copied()
        .filter(|_| !replies.is_empty());
    a.worst_hour = a
        .by_hour
        .iter()
        .filter(|h| h.waits > 0)
        .max_by_key(|h| h.short_ms + h.away_ms)
        .map(|h| h.hour);
    let mut ps: Vec<ProjectWait> = projects
        .into_iter()
        .filter(|(p, _)| !p.is_empty())
        .map(|(p, (ms, n))| ProjectWait {
            project: crate::analytics::project_name(&p),
            wait_ms: ms,
            waits: n,
        })
        .collect();
    ps.sort_by_key(|p| std::cmp::Reverse(p.wait_ms));
    ps.truncate(5);
    a.projects = ps;
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_replies_from_away_spells() {
        let conn = crate::db::open_in_memory().unwrap();
        let now = crate::util::now_ms();
        let t0 = now - 3 * 3_600_000;
        // A 2-minute reply, then a 2-hour meeting.
        let timeline = serde_json::to_string(&[
            (t0, t0 + 120_000, 0u8),
            (t0 + 120_000, t0 + 600_000, 1),
            (t0 + 600_000, t0 + 600_000 + 7_200_000, 0),
        ])
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, source, external_id, file_path, repo_root, started_at, ended_at, active_ms,
               user_turns, timeline, ingested_at)
             VALUES ('a', 'claude_code', 'a', '/f', '/code/pay', ?1, ?2, 0, 2, ?3, ?2)",
            rusqlite::params![t0, now, timeline],
        )
        .unwrap();
        let a = attention(&conn, 7).unwrap();
        assert_eq!(
            (a.short_ms, a.away_ms, a.waits, a.away_spells),
            (120_000, 7_200_000, 2, 1)
        );
        assert_eq!(a.median_reply_ms, Some(120_000));
        assert_eq!(a.projects[0].project, "pay");
        assert!(a.worst_hour.is_some());
    }
}
