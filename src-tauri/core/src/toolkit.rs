//! Your toolkit: which agent roles, tools, MCP servers and skills you use,
//! how that changes week to week, and how sessions that used each one went
//! compared with sessions that did not.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Result;
use chrono::{Duration, Local, NaiveDate};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::stats::{monday, success, week_of};
use crate::store::{Range, SessionRow, load_sessions};
use crate::team::Team;

/// Weeks shown in each item's trend.
const WEEKS: i64 = 12;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub key: String,
    /// Sessions in range that used it.
    pub sessions: usize,
    /// Calls, invocations or agents started.
    pub uses: u64,
    pub sources: Vec<String>,
    /// Sessions that used it, per week, oldest first.
    pub weekly: Vec<usize>,
    pub with: Outcomes,
    pub without: Outcomes,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcomes {
    /// Rated sessions and their mean outcome score.
    pub rated: usize,
    pub success: Option<f64>,
    /// Sessions with commits, and how many of those were reworked.
    pub shipped: usize,
    pub reworked: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toolkit {
    /// Monday of each trend week, oldest first.
    pub weeks: Vec<String>,
    pub sessions: usize,
    pub agents: Vec<Item>,
    pub tools: Vec<Item>,
    pub mcp: Vec<Item>,
    pub skills: Vec<Item>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Stored {
    skills: BTreeMap<String, u64>,
    mcp: BTreeMap<String, u64>,
}

/// One session's use of each kind, keyed by item.
#[derive(Default)]
struct Usage {
    agents: BTreeMap<String, u64>,
    tools: BTreeMap<String, u64>,
    mcp: BTreeMap<String, u64>,
    skills: BTreeMap<String, u64>,
}

/// "General Purpose Agent" (Copilot) and "general-purpose" (Claude Code)
/// are the same role.
pub fn role_key(role: &str) -> String {
    let r = role.trim().to_lowercase();
    let r = r.strip_suffix(" agent").unwrap_or(&r);
    r.split_whitespace().collect::<Vec<_>>().join("-")
}

/// True for an MCP tool: `mcp__server__tool` (Claude Code) or a name that
/// starts with a server the session used (Copilot, `github-mcp-server-…`).
fn is_mcp_tool(name: &str, servers: &BTreeMap<String, u64>) -> bool {
    name.starts_with("mcp__") || servers.keys().any(|s| name.starts_with(&format!("{s}-")))
}

fn usage(conn: &Connection, since: i64) -> Result<HashMap<String, Usage>> {
    let mut stmt =
        conn.prepare("SELECT id, tool_counts, team, toolkit FROM sessions WHERE started_at >= ?1")?;
    let rows = stmt.query_map([since], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, tools, team, kit) = row?;
        let stored: Stored = serde_json::from_str(&kit).unwrap_or_default();
        let team: Team = serde_json::from_str(&team).unwrap_or_default();
        let all_tools: BTreeMap<String, u64> = serde_json::from_str(&tools).unwrap_or_default();
        let mut u = Usage {
            tools: all_tools
                .into_iter()
                .filter(|(t, _)| !is_mcp_tool(t, &stored.mcp))
                .collect(),
            mcp: stored.mcp,
            skills: stored.skills,
            ..Default::default()
        };
        for a in &team.agents {
            *u.agents.entry(role_key(&a.role)).or_default() += 1;
        }
        out.insert(id, u);
    }
    Ok(out)
}

fn outcomes(rows: &[&SessionRow]) -> Outcomes {
    let (rated, success) = success(rows.iter().copied());
    let shipped: Vec<_> = rows.iter().filter(|s| s.commits > 0).collect();
    Outcomes {
        rated,
        success,
        shipped: shipped.len(),
        reworked: shipped.iter().filter(|s| s.reworked).count(),
    }
}

fn items(
    pick: impl Fn(&Usage) -> &BTreeMap<String, u64>,
    usage: &HashMap<String, Usage>,
    in_range: &[&SessionRow],
    trend: &[&SessionRow],
    weeks: &[NaiveDate],
) -> Vec<Item> {
    let empty = BTreeMap::new();
    let used = |s: &SessionRow| usage.get(&s.id).map(&pick).unwrap_or(&empty);
    let keys: BTreeSet<&String> = in_range.iter().flat_map(|s| used(s).keys()).collect();
    let mut out: Vec<Item> = keys
        .into_iter()
        .map(|key| {
            let (with, without): (Vec<&SessionRow>, Vec<&SessionRow>) =
                in_range.iter().partition(|s| used(s).contains_key(key));
            let mut weekly = vec![0; weeks.len()];
            for s in trend.iter().filter(|s| used(s).contains_key(key)) {
                if let Some(i) = s
                    .started_at
                    .and_then(week_of)
                    .and_then(|w| weeks.iter().position(|x| *x == w))
                {
                    weekly[i] += 1;
                }
            }
            Item {
                key: key.clone(),
                sessions: with.len(),
                uses: with.iter().map(|s| used(s)[key]).sum(),
                sources: with
                    .iter()
                    .map(|s| s.source.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                weekly,
                with: outcomes(&with),
                without: outcomes(&without),
            }
        })
        .collect();
    out.sort_by(|a, b| b.sessions.cmp(&a.sessions).then(b.uses.cmp(&a.uses)));
    out
}

/// `source` limits it to one tool (claude_code, copilot_cli, copilot_vscode).
pub fn toolkit(conn: &Connection, days: i64, source: Option<&str>) -> Result<Toolkit> {
    let range = Range::last_days(days);
    let this_week = monday(Local::now().date_naive());
    let weeks: Vec<NaiveDate> = (0..WEEKS)
        .rev()
        .map(|i| this_week - Duration::weeks(i))
        .collect();
    let trend_since = crate::stats::local_midnight_ms(weeks[0]);
    let since = range.since.min(trend_since);

    let rows: Vec<SessionRow> = load_sessions(
        conn,
        Range {
            since,
            until: range.until,
        },
    )?
    .into_iter()
    // Older rows predate skill and MCP tracking until they are re-read.
    .filter(|s| s.has_signals && source.is_none_or(|x| s.source == x))
    .collect();
    let usage = usage(conn, since)?;
    let in_range: Vec<&SessionRow> = rows
        .iter()
        .filter(|s| s.started_at.is_some_and(|t| t >= range.since))
        .collect();
    let trend: Vec<&SessionRow> = rows
        .iter()
        .filter(|s| s.started_at.is_some_and(|t| t >= trend_since))
        .collect();

    let build =
        |pick: fn(&Usage) -> &BTreeMap<String, u64>| items(pick, &usage, &in_range, &trend, &weeks);
    Ok(Toolkit {
        weeks: weeks
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect(),
        sessions: in_range.len(),
        agents: build(|u| &u.agents),
        tools: build(|u| &u.tools),
        mcp: build(|u| &u.mcp),
        skills: build(|u| &u.skills),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_match_across_tools() {
        assert_eq!(role_key("General Purpose Agent"), "general-purpose");
        assert_eq!(role_key("general-purpose"), "general-purpose");
        assert_eq!(role_key("Explore"), "explore");
    }

    #[test]
    fn mcp_tools_are_told_apart() {
        let servers = BTreeMap::from([("github-mcp-server".to_string(), 3)]);
        assert!(is_mcp_tool("mcp__github__create_pr", &BTreeMap::new()));
        assert!(is_mcp_tool("github-mcp-server-list_issues", &servers));
        assert!(!is_mcp_tool("bash", &servers));
    }
}
