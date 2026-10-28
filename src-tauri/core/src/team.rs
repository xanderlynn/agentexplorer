//! The team behind a session: the lead agent you talk to, the sub-agents it
//! hands work to, what they say to each other, and work scheduled to run
//! later. Parsers fill it in; it is stored as JSON on the session row.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::util::truncate;

/// Member ids for the two members every session has.
pub const YOU: &str = "you";
pub const LEAD: &str = "lead";

const BRIEF_CHARS: usize = 1500;
const NOTE_CHARS: usize = 600;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Team {
    pub agents: Vec<Agent>,
    pub messages: Vec<Message>,
    pub scheduled: Vec<Scheduled>,
    /// The session is still going, so running agents and scheduled work
    /// are live. Set when read.
    pub live: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Running,
    Done,
    Failed,
    /// Never reported back, and its session is over.
    Stopped,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Agent {
    pub id: String,
    /// A short handle the lead gave it, when it gave one.
    pub name: Option<String>,
    /// Its agent type: general-purpose, Explore, task, a custom agent…
    pub role: String,
    /// The one-line task it was handed.
    pub task: String,
    /// The full brief it was handed, cut short.
    pub brief: Option<String>,
    pub model: Option<String>,
    /// Who handed it the work: the lead or another agent's id.
    pub parent: String,
    pub background: bool,
    pub status: Status,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub tool_calls: u64,
    pub tokens: u64,
    pub tools: BTreeMap<String, u64>,
    /// Its latest logged activity; a running agent silent for long is
    /// probably stuck or gone.
    pub last_seen: Option<i64>,
    /// The start of its report.
    pub result: Option<String>,
    /// Other ids the logs use for it (tool call id, agent id, name).
    #[serde(skip)]
    aliases: Vec<String>,
    /// Its report is in since it was last asked for something.
    #[serde(skip)]
    reported: bool,
    /// A bare "finished" report its real report should replace.
    #[serde(skip)]
    placeholder: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Work handed down to an agent.
    Handoff,
    /// An agent's result coming back.
    Report,
    /// Anything else one member says to another.
    Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub ts: Option<i64>,
    pub from: String,
    pub to: String,
    pub kind: Kind,
    pub text: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scheduled {
    pub id: Option<String>,
    /// Who set it up: the lead or an agent id.
    pub by: String,
    pub created_at: Option<i64>,
    /// A cron expression for recurring work.
    pub cron: Option<String>,
    /// When a one-off run fires.
    pub at: Option<i64>,
    pub recurring: bool,
    pub task: String,
    pub cancelled: bool,
}

/// What a hand-off says about the agent.
#[derive(Default)]
pub struct Handoff<'a> {
    pub role: Option<&'a str>,
    pub task: Option<&'a str>,
    pub brief: Option<&'a str>,
    pub name: Option<&'a str>,
    pub model: Option<&'a str>,
    pub background: bool,
}

/// How an agent finished, as far as the logs say.
#[derive(Default)]
pub struct Finish<'a> {
    pub status: Option<Status>,
    pub result: Option<&'a str>,
    pub model: Option<&'a str>,
    pub tokens: Option<u64>,
    pub tool_calls: Option<u64>,
    pub duration_ms: Option<i64>,
    /// The report arrives separately (Claude Code's SubagentHandback), so
    /// finishing adds no message of its own.
    pub handed_back: bool,
}

impl Agent {
    /// Running agents silent for longer than `quiet_after_ms` are quiet.
    pub fn live_status(&self, now: i64, quiet_after_ms: i64) -> AgentLiveStatus {
        match self.status {
            Status::Running
                if self
                    .last_seen
                    .or(self.started_at)
                    .is_some_and(|t| now - t > quiet_after_ms) =>
            {
                AgentLiveStatus::Quiet
            }
            Status::Running => AgentLiveStatus::Running,
            Status::Done => AgentLiveStatus::Done,
            Status::Failed => AgentLiveStatus::Failed,
            Status::Stopped => AgentLiveStatus::Stopped,
        }
    }
}

/// A sub-agent's status as the live views show it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLiveStatus {
    #[default]
    Running,
    Quiet,
    Done,
    Failed,
    Stopped,
}

impl AgentLiveStatus {
    pub const ALL: [AgentLiveStatus; 5] = [
        AgentLiveStatus::Running,
        AgentLiveStatus::Quiet,
        AgentLiveStatus::Done,
        AgentLiveStatus::Failed,
        AgentLiveStatus::Stopped,
    ];
}

impl Team {
    fn index(&self, key: &str) -> Option<usize> {
        self.agents
            .iter()
            .position(|a| a.id == key || a.aliases.iter().any(|x| x == key))
    }

    pub fn agent(&mut self, key: &str) -> Option<&mut Agent> {
        self.index(key).map(|i| &mut self.agents[i])
    }

    /// The member id for a key the logs use; unknown keys stay as they are.
    pub fn resolve(&self, key: &str) -> String {
        match key {
            YOU | LEAD => key.to_string(),
            _ => self
                .index(key)
                .map(|i| self.agents[i].id.clone())
                .unwrap_or_else(|| key.to_string()),
        }
    }

    pub fn alias(&mut self, key: &str, alias: &str) {
        if let Some(a) = self.agent(key)
            && a.id != alias
            && !a.aliases.iter().any(|x| x == alias)
        {
            a.aliases.push(alias.to_string());
        }
    }

    /// `from` hands work to a new agent, identified by `call_id`.
    pub fn hand_off(&mut self, call_id: &str, from: &str, ts: Option<i64>, h: Handoff) {
        if self.index(call_id).is_some() {
            return;
        }
        let parent = self.resolve(from);
        let task = h.task.unwrap_or("").trim();
        let brief = h.brief.map(str::trim).filter(|b| !b.is_empty());
        self.agents.push(Agent {
            id: call_id.to_string(),
            name: h.name.filter(|n| !n.is_empty()).map(str::to_string),
            role: h
                .role
                .filter(|r| !r.is_empty())
                .unwrap_or("general-purpose")
                .to_string(),
            task: truncate(task, 200),
            brief: brief.map(|b| truncate(b, BRIEF_CHARS)),
            model: h.model.map(str::to_string),
            parent: parent.clone(),
            background: h.background,
            started_at: ts,
            ..Default::default()
        });
        if let Some(n) = h.name {
            self.alias(call_id, n);
        }
        let text = if task.is_empty() {
            brief.unwrap_or("")
        } else {
            task
        };
        self.say(ts, &parent, call_id, Kind::Handoff, text);
    }

    pub fn say(&mut self, ts: Option<i64>, from: &str, to: &str, kind: Kind, text: &str) {
        let (from, to) = (self.resolve(from), self.resolve(to));
        // Asking a finished agent for more resumes it. Sub-agent
        // transcripts are read last, so only a later message counts.
        if kind != Kind::Report
            && let Some(a) = self.agent(&to)
            && a.ended_at.is_none_or(|e| ts.is_some_and(|t| t > e))
        {
            a.reported = false;
            a.placeholder = None;
            if a.ended_at.is_some() {
                a.status = Status::Running;
                a.ended_at = None;
            }
        }
        self.messages.push(Message {
            ts,
            from,
            to,
            kind,
            text: truncate(text.trim(), NOTE_CHARS),
        });
    }

    /// An agent's report reaches whoever handed it the work.
    pub fn report(&mut self, key: &str, ts: Option<i64>, text: &str) {
        let Some(i) = self.index(key) else { return };
        let a = &mut self.agents[i];
        if a.result.is_none() && !text.trim().is_empty() {
            a.result = Some(truncate(text.trim(), BRIEF_CHARS));
        }
        a.reported = true;
        if let Some(i) = a.placeholder.take() {
            let m = &mut self.messages[i];
            m.ts = ts.or(m.ts);
            m.text = truncate(text.trim(), NOTE_CHARS);
            return;
        }
        let (id, parent) = (a.id.clone(), a.parent.clone());
        self.say(ts, &id, &parent, Kind::Report, text);
    }

    pub fn finish(&mut self, key: &str, ts: Option<i64>, f: Finish) {
        let next = self.messages.len();
        let Some(a) = self.agent(key) else { return };
        a.status = f.status.unwrap_or(Status::Done);
        if let Some(d) = f.duration_ms
            && let Some(start) = a.started_at
        {
            a.ended_at = Some(start + d);
        } else if ts.is_some() {
            a.ended_at = ts;
        }
        if let Some(m) = f.model {
            a.model = Some(m.to_string());
        }
        if let Some(t) = f.tokens {
            a.tokens = a.tokens.max(t);
        }
        if let Some(n) = f.tool_calls {
            a.tool_calls = a.tool_calls.max(n);
        }
        if a.reported || f.handed_back {
            return;
        }
        match f.result {
            Some(text) => self.report(key, ts, text),
            // Say that it finished without making that its result.
            None => {
                a.reported = true;
                a.placeholder = Some(next);
                let (id, parent) = (a.id.clone(), a.parent.clone());
                let text = if a.status == Status::Failed {
                    "failed"
                } else {
                    "finished"
                };
                self.say(ts, &id, &parent, Kind::Report, text);
            }
        }
    }

    pub fn tool(&mut self, key: &str, name: &str) {
        if let Some(a) = self.agent(key) {
            *a.tools.entry(name.to_string()).or_default() += 1;
            // A completion notice may already have reported the total.
            a.tool_calls = a.tool_calls.max(a.tools.values().sum());
        }
    }

    /// Widens an agent's working span to cover `first..=last`.
    pub fn span(&mut self, key: &str, first: Option<i64>, last: Option<i64>) {
        let Some(a) = self.agent(key) else { return };
        if let Some(f) = first {
            a.started_at = Some(a.started_at.map_or(f, |s| s.min(f)));
        }
        if let Some(l) = last {
            a.last_seen = Some(a.last_seen.map_or(l, |e| e.max(l)));
        }
        if let Some(l) = last
            && a.status != Status::Running
        {
            a.ended_at = Some(a.ended_at.map_or(l, |e| e.max(l)));
        }
    }

    /// Work set up to run later, from a scheduling tool call, if `name` is one.
    pub fn schedule_from(&mut self, name: &str, input: &Value, by: &str, ts: Option<i64>) {
        let s = |k: &str| input.get(k).and_then(Value::as_str);
        let task = [
            "prompt",
            "description",
            "task",
            "reason",
            "taskName",
            "name",
        ]
        .iter()
        .find_map(|k| s(k).filter(|t| !t.trim().is_empty()))
        .unwrap_or("")
        .trim();
        let cron = ["cron", "cronExpression", "schedule"]
            .iter()
            .find_map(|k| s(k))
            .map(str::to_string);
        let at = input
            .get("delaySeconds")
            .and_then(Value::as_f64)
            .and_then(|d| Some(ts? + (d * 1000.0) as i64))
            .or_else(|| {
                ["fireAt", "runAt", "at", "startTime"]
                    .iter()
                    .find_map(|k| s(k))
                    .and_then(crate::util::parse_ts)
            });
        let lname = name.to_ascii_lowercase();
        let creates = matches!(name, "CronCreate" | "ScheduleWakeup")
            || (lname.contains("schedul") && lname.contains("create"));
        if creates && (cron.is_some() || at.is_some()) {
            self.scheduled.push(Scheduled {
                id: s("id").or(s("taskId")).map(str::to_string),
                by: self.resolve(by),
                created_at: ts,
                recurring: input
                    .get("recurring")
                    .and_then(Value::as_bool)
                    .unwrap_or(cron.is_some() && at.is_none()),
                cron,
                at,
                task: truncate(task, NOTE_CHARS),
                cancelled: false,
            });
        } else if name == "CronDelete" || (lname.contains("schedul") && lname.contains("delete")) {
            let id = s("id").or(s("taskId"));
            if let Some(job) =
                self.scheduled.iter_mut().rev().find(|j| {
                    !j.cancelled && (id.is_none() || j.id.is_none() || j.id.as_deref() == id)
                })
            {
                job.cancelled = true;
            }
        }
    }

    /// Puts messages in time order; sub-agent transcripts are read after
    /// the main one.
    pub fn sort(&mut self) {
        self.messages.sort_by_key(|m| m.ts.unwrap_or(i64::MAX));
    }

    /// Agents still marked running in a session that is over never reported.
    pub fn settle(&mut self, live: bool) {
        self.live = live;
        if !live {
            for a in &mut self.agents {
                if a.status == Status::Running {
                    a.status = Status::Stopped;
                }
            }
        }
    }
}

/// Pulls `<tag>…</tag>` out of a message.
pub fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let i = text.find(&open)? + open.len();
    let j = text[i..].find(&close)? + i;
    Some(text[i..j].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_off_report_and_aliases() {
        let mut t = Team::default();
        t.hand_off(
            "call1",
            LEAD,
            Some(10),
            Handoff {
                role: Some("Explore"),
                task: Some("Find the parser"),
                name: Some("scout"),
                ..Default::default()
            },
        );
        t.alias("call1", "agent-x");
        t.tool("agent-x", "Read");
        t.say(Some(20), LEAD, "scout", Kind::Message, "also check tests");
        t.finish(
            "agent-x",
            Some(30),
            Finish {
                result: Some("It is in parsers/claude.rs"),
                tokens: Some(500),
                ..Default::default()
            },
        );
        // A second notice for the same run adds no second report.
        t.finish("call1", Some(31), Finish::default());

        let a = &t.agents[0];
        assert_eq!(
            (a.role.as_str(), a.status, a.tool_calls, a.tokens),
            ("Explore", Status::Done, 1, 500)
        );
        let kinds: Vec<_> = t
            .messages
            .iter()
            .map(|m| (m.kind, m.from.as_str(), m.to.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (Kind::Handoff, LEAD, "call1"),
                (Kind::Message, LEAD, "call1"),
                (Kind::Report, "call1", LEAD),
            ]
        );
    }

    #[test]
    fn schedules_and_cancels() {
        let mut t = Team::default();
        t.schedule_from(
            "CronCreate",
            &serde_json::json!({"cron": "50 23 * * *", "prompt": "run the retro"}),
            LEAD,
            Some(0),
        );
        t.schedule_from(
            "ScheduleWakeup",
            &serde_json::json!({"delaySeconds": 60, "prompt": "check CI"}),
            LEAD,
            Some(1_000),
        );
        t.schedule_from(
            "Read",
            &serde_json::json!({"file_path": "x"}),
            LEAD,
            Some(0),
        );
        assert_eq!(t.scheduled.len(), 2);
        assert!(t.scheduled[0].recurring);
        assert_eq!(t.scheduled[1].at, Some(61_000));
        t.schedule_from("CronDelete", &serde_json::json!({}), LEAD, Some(2));
        assert!(t.scheduled[1].cancelled && !t.scheduled[0].cancelled);
    }

    #[test]
    fn unreported_agents_stop_when_session_is_over() {
        let mut t = Team::default();
        t.hand_off("c", LEAD, None, Handoff::default());
        t.settle(true);
        assert_eq!(t.agents[0].status, Status::Running);
        t.settle(false);
        assert_eq!(t.agents[0].status, Status::Stopped);
    }
}
