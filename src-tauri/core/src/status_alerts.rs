//! Which status changes deserve a sound or a notification, and how your
//! status profile (setting `alerts.profile`, edited in Settings) says to
//! announce them. Runs in the background loop, so alerts work with no
//! window open.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::live::{LiveSession, LiveStatus};
use crate::team::AgentLiveStatus;

pub const PROFILE_KEY: &str = "alerts.profile";

/// A change must hold this long before it is announced, so a session that
/// flips between turns in a few seconds stays quiet.
const CONFIRM_AFTER: Duration = Duration::from_secs(4);
/// A session missing from the live list this long is forgotten.
const FORGET_AFTER: Duration = Duration::from_secs(5 * 60);

/// Most urgent first. Mirrored by `URGENCY` in src/status.ts.
pub const URGENCY: &[&str] = &[
    "needs_permission",
    "asking",
    "failed",
    "maybe_blocked",
    "agent_failed",
    "working",
    "long_task",
    "scheduled",
    "done",
    "agent_done",
    "ended",
    "stale",
];

/// What a notification calls each status. Mirrors the labels in src/status.ts.
pub fn label(key: &str) -> &'static str {
    match key {
        "asking" => "Your turn",
        "needs_permission" => "Needs permission",
        "maybe_blocked" => "Might need approval",
        "failed" => "Failed",
        "working" => "Working",
        "long_task" => "Long task",
        "done" => "Completed",
        "ended" => "Closed",
        "scheduled" => "Scheduled",
        "agent_done" => "Sub-agent done",
        "agent_failed" => "Sub-agent failed",
        _ => "Idle",
    }
}

fn urgency(key: &str) -> usize {
    URGENCY
        .iter()
        .position(|k| *k == key)
        .unwrap_or(URGENCY.len())
}

/// One status change to announce.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    /// A session status, or `agent_done` / `agent_failed`.
    pub key: String,
    pub session_id: String,
    pub title: Option<String>,
    pub project_path: Option<String>,
    /// The sub-agent, for agent alerts.
    pub agent: Option<String>,
}

struct Seen {
    status: LiveStatus,
    /// A new status seen, and since when; announced once it has held.
    pending: Option<(LiveStatus, Instant)>,
    agents: HashMap<String, AgentLiveStatus>,
    last_seen: Instant,
}

/// Turns successive live lists into status changes. The first list is a
/// baseline: sessions already running when the app starts make no noise.
#[derive(Default)]
pub struct Tracker {
    seen: Option<HashMap<String, Seen>>,
}

impl Tracker {
    pub fn update<'a>(&mut self, sessions: impl Iterator<Item = &'a LiveSession>) -> Vec<Alert> {
        self.update_at(sessions, Instant::now())
    }

    fn update_at<'a>(
        &mut self,
        sessions: impl Iterator<Item = &'a LiveSession>,
        now: Instant,
    ) -> Vec<Alert> {
        let first = self.seen.is_none();
        let map = self.seen.get_or_insert_with(HashMap::new);
        let mut out = vec![];
        for s in sessions {
            let agents: HashMap<String, AgentLiveStatus> =
                s.agents.iter().map(|a| (a.id.clone(), a.status)).collect();
            let alert = |key: &str, agent: Option<String>| Alert {
                key: key.to_string(),
                session_id: s.id.clone(),
                title: s.title.clone(),
                project_path: s.project_path.clone(),
                agent,
            };
            let Some(prev) = map.get_mut(&s.id).filter(|_| !first) else {
                // Unseen after the baseline: it comes from idle, so one that
                // appears already waiting on you still alerts.
                let status = if first { s.status } else { LiveStatus::Stale };
                let pending = (!first && s.status != LiveStatus::Stale).then_some((s.status, now));
                map.insert(
                    s.id.clone(),
                    Seen {
                        status,
                        pending,
                        agents,
                        last_seen: now,
                    },
                );
                continue;
            };
            prev.last_seen = now;
            if s.status == prev.status {
                prev.pending = None;
            } else {
                match &prev.pending {
                    Some((p, since)) if *p == s.status => {
                        if now.duration_since(*since) >= CONFIRM_AFTER {
                            prev.status = s.status;
                            prev.pending = None;
                            out.push(alert(s.status.as_str(), None));
                        }
                    }
                    _ => prev.pending = Some((s.status, now)),
                }
            }
            for a in &s.agents {
                let was = prev.agents.get(&a.id).copied();
                if was == Some(a.status)
                    || matches!(
                        was,
                        Some(
                            AgentLiveStatus::Done
                                | AgentLiveStatus::Failed
                                | AgentLiveStatus::Stopped
                        )
                    )
                {
                    continue;
                }
                let who = a.name.clone().unwrap_or_else(|| a.role.clone());
                match a.status {
                    AgentLiveStatus::Done => out.push(alert("agent_done", Some(who))),
                    // Stopped means the session ended without a report, not a failure.
                    AgentLiveStatus::Failed => out.push(alert("agent_failed", Some(who))),
                    _ => {}
                }
            }
            prev.agents = agents;
        }
        map.retain(|_, x| now.duration_since(x.last_seen) < FORGET_AFTER);
        out.sort_by_key(|a| urgency(&a.key));
        out
    }

    /// Changes still waiting to be confirmed: the loop should look again soon.
    pub fn has_pending(&self) -> bool {
        self.seen
            .as_ref()
            .is_some_and(|m| m.values().any(|x| x.pending.is_some()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Sound {
    Tone { tone: String },
    File { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    pub sound: Option<Sound>,
    /// An imported image id, shown for this status.
    pub image: Option<String>,
    pub notify: bool,
    /// 0–1, multiplied by the master volume.
    pub volume: f32,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            sound: None,
            image: None,
            notify: false,
            volume: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QuietHours {
    pub enabled: bool,
    /// "HH:MM", local time; the span may wrap past midnight.
    pub start: String,
    pub end: String,
}

impl Default for QuietHours {
    fn default() -> Self {
        QuietHours {
            enabled: false,
            start: "22:00".into(),
            end: "08:00".into(),
        }
    }
}

/// Your sounds, images and notifications per status. The UI edits it as
/// JSON in setting `alerts.profile`; missing fields take these defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    pub version: u32,
    pub volume: f32,
    pub muted: bool,
    pub quiet_hours: QuietHours,
    /// Only play sounds while Tilly's window is not in front.
    pub only_unfocused: bool,
    pub rules: BTreeMap<String, Rule>,
}

impl Default for Profile {
    fn default() -> Self {
        let rule = |tone: Option<&str>, notify: bool| Rule {
            sound: tone.map(|t| Sound::Tone { tone: t.into() }),
            notify,
            ..Default::default()
        };
        Profile {
            version: 1,
            volume: 0.7,
            muted: false,
            quiet_hours: QuietHours::default(),
            only_unfocused: false,
            rules: BTreeMap::from([
                ("asking".into(), rule(Some("chime"), true)),
                ("needs_permission".into(), rule(Some("knock"), true)),
                ("maybe_blocked".into(), rule(Some("ping"), false)),
                ("done".into(), rule(Some("rise"), false)),
                ("failed".into(), rule(Some("fall"), true)),
                ("agent_failed".into(), rule(None, false)),
                ("agent_done".into(), rule(None, false)),
            ]),
        }
    }
}

impl Profile {
    /// The stored profile, or the defaults when there is none or it is unreadable.
    pub fn parse(json: Option<&str>) -> Profile {
        json.and_then(|j| serde_json::from_str::<Profile>(j).ok())
            .filter(|p| p.version == 1)
            .unwrap_or_default()
    }

    pub fn rule(&self, key: &str) -> Rule {
        self.rules.get(key).cloned().unwrap_or_default()
    }

    /// True inside the quiet hours. `minutes` is minutes since local midnight.
    pub fn quiet_at(&self, minutes: u32) -> bool {
        let q = &self.quiet_hours;
        if !q.enabled {
            return false;
        }
        let m = |s: &str| {
            let (h, mm) = s.split_once(':').unwrap_or((s, "0"));
            h.parse::<u32>().unwrap_or(0) * 60 + mm.parse::<u32>().unwrap_or(0)
        };
        let (a, b) = (m(&q.start), m(&q.end));
        if a <= b {
            minutes >= a && minutes < b
        } else {
            minutes >= a || minutes < b
        }
    }
}

/// What to do about a burst of alerts.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// One sound, the most urgent with a sound set, at its volume.
    pub sound: Option<(Sound, f32)>,
    /// Alerts whose rule asks for a notification.
    pub notify: Vec<Alert>,
}

/// Applies the profile: quiet hours hold sounds and notifications, mute
/// holds sounds only, and only-unfocused holds sounds while you are looking.
pub fn plan(p: &Profile, alerts: &[Alert], minutes: u32, focused: bool) -> Plan {
    if p.quiet_at(minutes) {
        return Plan::default();
    }
    let sound = (!p.muted && !(p.only_unfocused && focused))
        .then(|| {
            alerts.iter().find_map(|a| {
                let r = p.rule(&a.key);
                r.sound.map(|s| (s, (p.volume * r.volume).clamp(0.0, 1.0)))
            })
        })
        .flatten()
        .filter(|(_, v)| *v > 0.0);
    Plan {
        sound,
        notify: alerts
            .iter()
            .filter(|a| p.rule(&a.key).notify)
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{LiveAgent, LiveSession};

    fn s(id: &str, status: &str) -> LiveSession {
        LiveSession {
            id: id.into(),
            status: LiveStatus::ALL
                .into_iter()
                .find(|x| x.as_str() == status)
                .unwrap(),
            ..Default::default()
        }
    }

    #[test]
    fn announces_held_changes_once_after_a_baseline() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        assert!(t.update_at([s("a", "working")].iter(), t0).is_empty());
        // Seen once: not yet.
        assert!(
            t.update_at([s("a", "asking")].iter(), t0 + Duration::from_secs(1))
                .is_empty()
        );
        let out = t.update_at([s("a", "asking")].iter(), t0 + Duration::from_secs(6));
        assert_eq!(
            out.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(),
            ["asking"]
        );
        assert!(
            t.update_at([s("a", "asking")].iter(), t0 + Duration::from_secs(9))
                .is_empty()
        );
        // A flap back and forth is never announced.
        assert!(
            t.update_at([s("a", "working")].iter(), t0 + Duration::from_secs(10))
                .is_empty()
        );
        assert!(
            t.update_at([s("a", "asking")].iter(), t0 + Duration::from_secs(11))
                .is_empty()
        );
    }

    #[test]
    fn new_sessions_waiting_on_you_still_alert_and_agents_report() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        t.update_at(std::iter::empty(), t0);
        t.update_at([s("b", "needs_permission")].iter(), t0);
        let out = t.update_at(
            [s("b", "needs_permission")].iter(),
            t0 + Duration::from_secs(5),
        );
        assert_eq!(out[0].key, "needs_permission");

        let mut with = s("b", "needs_permission");
        with.agents = vec![LiveAgent {
            id: "x".into(),
            status: AgentLiveStatus::Running,
            ..Default::default()
        }];
        t.update_at([with.clone()].iter(), t0 + Duration::from_secs(6));
        with.agents[0].status = AgentLiveStatus::Failed;
        let out = t.update_at([with].iter(), t0 + Duration::from_secs(7));
        assert_eq!(out[0].key, "agent_failed");
    }

    #[test]
    fn plan_follows_the_profile() {
        let p = Profile::default();
        let alerts = vec![
            Alert {
                key: "done".into(),
                session_id: "a".into(),
                title: None,
                project_path: None,
                agent: None,
            },
            Alert {
                key: "asking".into(),
                session_id: "b".into(),
                title: None,
                project_path: None,
                agent: None,
            },
        ];
        let mut sorted = alerts.clone();
        sorted.sort_by_key(|a| urgency(&a.key));
        let plan = plan(&p, &sorted, 12 * 60, false);
        assert_eq!(
            plan.sound.as_ref().map(|s| &s.0),
            Some(&Sound::Tone {
                tone: "chime".into()
            })
        );
        assert_eq!(plan.notify.len(), 1);

        let mut quiet = Profile::default();
        quiet.quiet_hours.enabled = true;
        assert_eq!(
            super::plan(&quiet, &sorted, 23 * 60, false),
            Plan::default()
        );
        assert!(
            !super::plan(&quiet, &sorted, 12 * 60, false)
                .notify
                .is_empty()
        );
        let muted = Profile {
            muted: true,
            ..Default::default()
        };
        assert!(super::plan(&muted, &sorted, 12 * 60, false).sound.is_none());
        assert_eq!(Profile::parse(Some("not json")).volume, 0.7);
    }
}
