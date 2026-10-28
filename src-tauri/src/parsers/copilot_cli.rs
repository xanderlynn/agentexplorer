//! GitHub Copilot CLI sessions: `~/.copilot/session-state/<id>/events.jsonl`
//! plus a `workspace.yaml` with the folder, branch and a session name.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::model::{LastEvent, ParsedSession, Prompt, TokenUsage};
use crate::team::{Finish, Handoff, Kind, LEAD, Status, Team};
use crate::util::{parse_ts, truncate};

use super::{MAX_PROMPT_CHARS, open_lines};

pub fn parse(paths: &[PathBuf]) -> Result<Option<ParsedSession>> {
    let Some(events) = paths.first() else {
        return Ok(None);
    };
    let dir = events.parent().unwrap_or(Path::new("."));
    let mut s = ParsedSession {
        external_id: dir
            .file_name()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default(),
        entrypoint: Some("cli".into()),
        ..Default::default()
    };
    read_workspace(&dir.join("workspace.yaml"), &mut s);

    let mut current_model = String::new();
    let mut fallback_output_tokens: u64 = 0;
    let mut shutdown_seen = false;
    let mut premium = 0.0;
    // read_agent call id → the agent it reads, to attribute its report.
    let mut reads: HashMap<String, String> = HashMap::new();

    for line in open_lines(events)?.lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let ts = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        let d = &v["data"];
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if !kind.starts_with("hook.") && kind != "system.message" {
            s.note_time(ts);
        }

        match kind {
            "session.start" | "session.resume" | "session.context_changed" => {
                let ctx = if kind == "session.context_changed" {
                    d
                } else {
                    &d["context"]
                };
                if let Some(cwd) = ctx.get("cwd").and_then(Value::as_str) {
                    s.project_path.get_or_insert_with(|| cwd.to_string());
                }
                if let Some(b) = ctx.get("branch").and_then(Value::as_str) {
                    s.git_branch.get_or_insert_with(|| b.to_string());
                }
                if let Some(m) = d.get("selectedModel").and_then(Value::as_str) {
                    current_model = m.to_string();
                }
                if kind != "session.context_changed" {
                    s.last_event = Some(LastEvent::TurnEnded);
                }
            }
            "session.model_change" => {
                if let Some(m) = d.get("newModel").and_then(Value::as_str) {
                    current_model = m.to_string();
                }
            }
            "user.message" => {
                // `source` marks text the CLI injected (e.g. a skill's
                // instructions), not something the human typed.
                if d.get("source")
                    .and_then(Value::as_str)
                    .is_some_and(|x| !x.is_empty())
                {
                    continue;
                }
                let text = d
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if text.is_empty() {
                    continue;
                }
                // A "steering" message corrects the agent mid-turn.
                if d.get("delivery").and_then(Value::as_str) == Some("steering") {
                    s.interruptions += 1;
                }
                s.user_turns += 1;
                s.note_human_prompt(ts);
                s.prompts.push(Prompt {
                    ts,
                    text: truncate(text, MAX_PROMPT_CHARS),
                });
                s.last_event = Some(LastEvent::Working);
            }
            "assistant.turn_start" => {
                s.last_event = Some(LastEvent::Working);
                s.failed = false;
            }
            "assistant.message" => {
                s.assistant_turns += 1;
                if !current_model.is_empty() {
                    *s.model_turns.entry(current_model.clone()).or_default() += 1;
                }
                fallback_output_tokens +=
                    d.get("outputTokens").and_then(Value::as_u64).unwrap_or(0);
                if let Some(text) = d.get("content").and_then(Value::as_str) {
                    s.note_message(text);
                }
            }
            "assistant.turn_end" => {
                s.note_turn_end(ts);
                s.last_event = Some(LastEvent::TurnEnded);
            }
            "tool.execution_start" => {
                let call = d.get("toolCallId").and_then(Value::as_str).unwrap_or("");
                let tool = d.get("toolName").and_then(Value::as_str).unwrap_or("tool");
                let args = &d["arguments"];
                let parent = ["parentToolCallId", "parentAgentTaskId"]
                    .iter()
                    .find_map(|k| d.get(*k).and_then(Value::as_str))
                    .filter(|x| !x.is_empty());
                note_team_tool(
                    &mut s.team,
                    &mut reads,
                    parent.unwrap_or(LEAD),
                    call,
                    tool,
                    args,
                    ts,
                );
                if let Some(server) = d.get("mcpServerName").and_then(Value::as_str) {
                    s.note_mcp(server);
                }
                if let Some(p) = parent {
                    s.team.tool(p, tool);
                    s.team.span(p, None, ts);
                    // Sub-agent work is reported by subagent.* events; counting
                    // its tool calls here would not match Claude Code's numbers.
                    continue;
                }
                let mut name = d.get("toolName").and_then(Value::as_str).unwrap_or("tool");
                // str_replace_editor multiplexes view/create/str_replace.
                if let Some(cmd) = d.pointer("/arguments/command").and_then(Value::as_str) {
                    if cmd == "view" {
                        name = "view";
                    } else if cmd == "create" {
                        name = "create";
                    }
                }
                let path = d.pointer("/arguments/path").and_then(Value::as_str);
                if let Some(id) = d.get("toolCallId").and_then(Value::as_str) {
                    s.tool_names.insert(id.to_string(), name.to_string());
                    s.note_pending(id, name);
                }
                s.note_tool(name, path, ts);
                s.last_event = Some(LastEvent::Working);
            }
            "tool.execution_complete" => {
                note_team_result(&mut s.team, &reads, d, ts);
                if let Some(id) = d.get("toolCallId").and_then(Value::as_str) {
                    s.note_resolved(id);
                }
                if d.get("success").and_then(Value::as_bool) == Some(false) {
                    s.tool_errors += 1;
                    let name = d
                        .get("toolCallId")
                        .and_then(Value::as_str)
                        .and_then(|id| s.tool_names.get(id))
                        .cloned()
                        .unwrap_or_else(|| "tool".into());
                    s.note_tool_error(&name);
                }
            }
            "session.error" => {
                s.tool_errors += 1;
                s.failed = true;
            }
            "session.compaction_complete" => s.compactions += 1,
            "session.compaction_start" => {
                let n = |k: &str| d.get(k).and_then(Value::as_u64).unwrap_or(0);
                s.note_context(
                    n("systemTokens") + n("conversationTokens") + n("toolDefinitionsTokens"),
                );
            }
            "session.plan_changed" => s.planned = true,
            "skill.invoked" => {
                if let Some(name) = d.get("name").and_then(Value::as_str) {
                    s.note_skill(name);
                }
            }
            "subagent.started" => {
                s.subagents += 1;
                if let Some(call) = d.get("toolCallId").and_then(Value::as_str)
                    && let Some(a) = s.team.agent(call)
                {
                    if let Some(r) = d.get("agentDisplayName").and_then(Value::as_str) {
                        a.role = r.to_string();
                    }
                    a.started_at = a.started_at.or(ts);
                }
            }
            "subagent.completed" | "subagent.failed" => {
                if let Some(call) = d.get("toolCallId").and_then(Value::as_str) {
                    let n = |k: &str| d.get(k).and_then(Value::as_u64);
                    let error = d.get("error").and_then(Value::as_str);
                    s.team.finish(
                        call,
                        ts,
                        Finish {
                            status: Some(if kind == "subagent.failed" {
                                Status::Failed
                            } else {
                                Status::Done
                            }),
                            result: error,
                            model: d.get("model").and_then(Value::as_str),
                            tokens: n("totalTokens"),
                            tool_calls: n("totalToolCalls"),
                            duration_ms: n("durationMs").map(|x| x as i64),
                            ..Default::default()
                        },
                    );
                }
            }
            "abort" => {
                s.interruptions += 1;
                s.note_turn_end(ts);
                s.last_event = Some(LastEvent::TurnEnded);
            }
            "permission.requested" => {
                s.permission_requests += 1;
                if let (Some(id), Some(kind)) = (
                    d.pointer("/permissionRequest/toolCallId")
                        .and_then(Value::as_str),
                    d.pointer("/permissionRequest/kind").and_then(Value::as_str),
                ) {
                    s.tool_names
                        .entry(id.to_string())
                        .or_insert_with(|| kind.to_string());
                }
                s.last_event = Some(LastEvent::NeedsPermission);
            }
            "permission.completed" => {
                s.last_event = Some(LastEvent::Working);
                if d.pointer("/result/kind")
                    .and_then(Value::as_str)
                    .is_some_and(|k| k.starts_with("denied"))
                {
                    let name = d
                        .get("toolCallId")
                        .and_then(Value::as_str)
                        .and_then(|id| s.tool_names.get(id))
                        .cloned()
                        .unwrap_or_else(|| "permission".into());
                    s.note_veto(&name);
                }
            }
            "session.shutdown" => {
                // Each process run reports its own totals, so a resumed session
                // has several shutdowns to add up.
                shutdown_seen = true;
                premium += d
                    .get("totalPremiumRequests")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                if let Some(metrics) = d.get("modelMetrics").and_then(Value::as_object) {
                    for (model, m) in metrics {
                        let u = &m["usage"];
                        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                        s.add_tokens(
                            model,
                            &TokenUsage {
                                input: n("inputTokens"),
                                output: n("outputTokens"),
                                cache_read: n("cacheReadTokens"),
                                cache_write: n("cacheWriteTokens"),
                            },
                        );
                    }
                }
                if let Some(cc) = d.get("codeChanges") {
                    let add = cc.get("linesAdded").and_then(Value::as_i64).unwrap_or(0);
                    let rem = cc.get("linesRemoved").and_then(Value::as_i64).unwrap_or(0);
                    s.lines_added = Some(s.lines_added.unwrap_or(0) + add);
                    s.lines_removed = Some(s.lines_removed.unwrap_or(0) + rem);
                }
                if let Some(t) = d.get("currentTokens").and_then(Value::as_u64) {
                    s.note_context(t);
                }
                s.last_event = Some(LastEvent::Closed);
            }
            _ => {}
        }
    }

    if !shutdown_seen && fallback_output_tokens > 0 {
        let model = if current_model.is_empty() {
            "unknown".to_string()
        } else {
            current_model
        };
        s.add_tokens(
            &model,
            &TokenUsage {
                output: fallback_output_tokens,
                ..Default::default()
            },
        );
    }
    if premium > 0.0 {
        s.premium_requests = Some(premium);
    }
    if s.title.is_none() {
        s.title = s.prompts.first().map(|p| truncate(&p.text, 120));
    }
    Ok((!s.is_empty()).then_some(s))
}

/// `workspace.yaml` is flat `key: value` pairs; a full YAML parser is overkill.
/// Team traffic in a tool call: `task` hands work to a new agent,
/// `write_agent` messages one, `read_agent` collects its report.
fn note_team_tool(
    team: &mut Team,
    reads: &mut HashMap<String, String>,
    me: &str,
    call: &str,
    tool: &str,
    args: &Value,
    ts: Option<i64>,
) {
    let arg = |k: &str| args.get(k).and_then(Value::as_str);
    match tool {
        "task" => team.hand_off(
            call,
            me,
            ts,
            Handoff {
                role: arg("agent_type"),
                task: arg("description"),
                brief: arg("prompt"),
                name: arg("name"),
                model: arg("model"),
                background: arg("mode") == Some("background"),
            },
        ),
        "write_agent" => {
            if let Some(to) = arg("agent_id") {
                team.say(ts, me, to, Kind::Message, arg("message").unwrap_or(""));
            }
        }
        "read_agent" => {
            if let Some(id) = arg("agent_id") {
                reads.insert(call.to_string(), id.to_string());
            }
        }
        _ => team.schedule_from(tool, args, me, ts),
    }
}

/// A `task` call that ran in the foreground returns the agent's report;
/// a `read_agent` call returns a background agent's.
fn note_team_result(team: &mut Team, reads: &HashMap<String, String>, d: &Value, ts: Option<i64>) {
    let Some(call) = d.get("toolCallId").and_then(Value::as_str) else {
        return;
    };
    let text = match &d["result"] {
        Value::String(t) => t.as_str(),
        r => r.get("content").and_then(Value::as_str).unwrap_or(""),
    };
    if let Some(agent) = reads.get(call) {
        if !text.is_empty() {
            team.report(agent, ts, text);
        }
    } else if team.agent(call).is_some()
        && d.pointer("/toolTelemetry/properties/execution_mode")
            .and_then(Value::as_str)
            != Some("background")
        && !text.starts_with("Agent started in background")
    {
        team.report(call, ts, text);
    }
}

fn read_workspace(path: &Path, s: &mut ParsedSession) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "cwd" => s.project_path = Some(value.to_string()),
            "branch" => s.git_branch = Some(value.to_string()),
            "name" | "summary" if s.title.is_none() => s.title = Some(truncate(value, 200)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn builds_the_team() {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("s").join("events.jsonl");
        std::fs::create_dir_all(events.parent().unwrap()).unwrap();
        let lines = [
            r#"{"type":"user.message","timestamp":"2026-02-01T09:00:00Z","data":{"content":"Scaffold the API"}}"#,
            r#"{"type":"tool.execution_start","timestamp":"2026-02-01T09:00:01Z","data":{"toolCallId":"c1","toolName":"task","arguments":{"name":"scaffold","agent_type":"task","description":"Scaffold the project","prompt":"dotnet new webapi","mode":"background"}}}"#,
            r#"{"type":"tool.execution_complete","timestamp":"2026-02-01T09:00:02Z","data":{"toolCallId":"c1","success":true,"result":{"content":"Agent started in background with agent_id: scaffold."},"toolTelemetry":{"properties":{"execution_mode":"background"}}}}"#,
            r#"{"type":"subagent.started","timestamp":"2026-02-01T09:00:02Z","data":{"toolCallId":"c1","agentName":"task","agentDisplayName":"Task Agent"}}"#,
            r#"{"type":"tool.execution_start","timestamp":"2026-02-01T09:00:03Z","data":{"toolCallId":"k1","toolName":"bash","parentToolCallId":"c1","arguments":{}}}"#,
            r#"{"type":"tool.execution_start","timestamp":"2026-02-01T09:00:04Z","data":{"toolCallId":"w1","toolName":"write_agent","arguments":{"agent_id":"scaffold","message":"use minimal APIs"}}}"#,
            r#"{"type":"skill.invoked","timestamp":"2026-02-01T09:00:04Z","data":{"name":"grill-me","trigger":"agent-invoked"}}"#,
            r#"{"type":"tool.execution_start","timestamp":"2026-02-01T09:00:04Z","data":{"toolCallId":"m1","toolName":"github-mcp-server-list_issues","mcpServerName":"github-mcp-server","arguments":{}}}"#,
            r#"{"type":"subagent.completed","timestamp":"2026-02-01T09:00:20Z","data":{"toolCallId":"c1","model":"claude-haiku-4.5","totalToolCalls":2,"totalTokens":6950,"durationMs":18000}}"#,
            r#"{"type":"tool.execution_start","timestamp":"2026-02-01T09:00:21Z","data":{"toolCallId":"r1","toolName":"read_agent","arguments":{"agent_id":"scaffold"}}}"#,
            r#"{"type":"tool.execution_complete","timestamp":"2026-02-01T09:00:22Z","data":{"toolCallId":"r1","success":true,"result":{"content":"Created Api.csproj and Program.cs"}}}"#,
        ];
        std::fs::write(&events, lines.join("\n")).unwrap();

        let s = parse(&[events]).unwrap().unwrap();
        let a = &s.team.agents[0];
        assert_eq!(
            (a.role.as_str(), a.name.as_deref(), a.status),
            ("Task Agent", Some("scaffold"), Status::Done)
        );
        assert_eq!(
            (a.tokens, a.tool_calls, a.model.as_deref()),
            (6950, 2, Some("claude-haiku-4.5"))
        );
        assert_eq!(
            a.result.as_deref(),
            Some("Created Api.csproj and Program.cs")
        );
        assert_eq!(s.skills.get("grill-me"), Some(&1));
        assert_eq!(s.mcp.get("github-mcp-server"), Some(&1));
        // The sub-agent's own tool call is not the lead's.
        assert_eq!(s.tool_counts.get("bash"), None);
        let flow: Vec<_> = s
            .team
            .messages
            .iter()
            .map(|m| (m.from.as_str(), m.kind))
            .collect();
        assert_eq!(
            flow,
            [
                (LEAD, Kind::Handoff),
                (LEAD, Kind::Message),
                ("c1", Kind::Report)
            ]
        );
    }

    #[test]
    fn parses_events_and_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let sess = dir.path().join("sess-1");
        std::fs::create_dir_all(&sess).unwrap();
        std::fs::write(
            sess.join("workspace.yaml"),
            "id: sess-1\ncwd: /work/api\nbranch: feature/x\nname: Fix the flaky test\n",
        )
        .unwrap();
        let events = sess.join("events.jsonl");
        let mut f = std::fs::File::create(&events).unwrap();
        for l in [
            r#"{"type":"session.start","timestamp":"2026-02-01T09:00:00Z","data":{"context":{"cwd":"/ignored"}}}"#,
            r#"{"type":"session.model_change","timestamp":"2026-02-01T09:00:01Z","data":{"newModel":"claude-sonnet-4.6"}}"#,
            r#"{"type":"user.message","timestamp":"2026-02-01T09:00:02Z","data":{"content":"Fix the flaky test"}}"#,
            r#"{"type":"assistant.turn_start","timestamp":"2026-02-01T09:00:03Z","data":{}}"#,
            r#"{"type":"assistant.message","timestamp":"2026-02-01T09:00:04Z","data":{"outputTokens":40}}"#,
            r#"{"type":"tool.execution_start","timestamp":"2026-02-01T09:00:05Z","data":{"toolName":"edit","arguments":{"path":"/work/api/a.rs"}}}"#,
            r#"{"type":"tool.execution_complete","timestamp":"2026-02-01T09:00:06Z","data":{"success":false}}"#,
            r#"{"type":"user.message","timestamp":"2026-02-01T09:00:07Z","data":{"content":"<skill-context>","source":"skill-x","parentAgentTaskId":"t1"}}"#,
            r#"{"type":"user.message","timestamp":"2026-02-01T09:00:07Z","data":{"content":"use tables instead","delivery":"steering","parentAgentTaskId":"t1"}}"#,
            r#"{"type":"permission.requested","timestamp":"2026-02-01T09:00:08Z","data":{}}"#,
            r#"{"type":"abort","timestamp":"2026-02-01T09:00:09Z","data":{"reason":"user"}}"#,
            r#"{"type":"session.shutdown","timestamp":"2026-02-01T09:05:00Z","data":{"totalPremiumRequests":3,"codeChanges":{"linesAdded":12,"linesRemoved":4},"modelMetrics":{"claude-sonnet-4.6":{"usage":{"inputTokens":1000,"outputTokens":200,"cacheReadTokens":500,"cacheWriteTokens":0}}}}}"#,
        ] {
            writeln!(f, "{l}").unwrap();
        }

        let s = parse(&[events]).unwrap().unwrap();
        assert_eq!(s.external_id, "sess-1");
        assert_eq!(s.project_path.as_deref(), Some("/work/api"));
        assert_eq!(s.git_branch.as_deref(), Some("feature/x"));
        assert_eq!(s.title.as_deref(), Some("Fix the flaky test"));
        assert_eq!(s.user_turns, 2);
        assert_eq!(s.assistant_turns, 1);
        assert_eq!(s.tool_calls, 1);
        assert_eq!((s.edits, s.edits_without_read), (1, 1));
        assert_eq!(s.tool_errors, 1);
        assert_eq!(s.permission_requests, 1);
        // One steering message plus one abort.
        assert_eq!(s.interruptions, 2);
        assert_eq!(s.premium_requests, Some(3.0));
        assert_eq!(s.lines_added, Some(12));
        assert_eq!(s.last_event, Some(LastEvent::Closed));
        let d = s.derive();
        assert_eq!(d.tokens.input, 1000);
        assert_eq!(d.model.as_deref(), Some("claude-sonnet-4.6"));
        assert_eq!(d.cost_usd.map(|c| c > 0.0), Some(true));
    }
}
