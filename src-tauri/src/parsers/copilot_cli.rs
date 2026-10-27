//! GitHub Copilot CLI sessions: `~/.copilot/session-state/<id>/events.jsonl`
//! plus a `workspace.yaml` with the folder, branch and a session name.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::model::{LastEvent, ParsedSession, Prompt, TokenUsage};
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
            "assistant.turn_start" => s.last_event = Some(LastEvent::Working),
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
                let name = d.get("toolName").and_then(Value::as_str).unwrap_or("tool");
                let path = d.pointer("/arguments/path").and_then(Value::as_str);
                s.note_tool(name, path);
                s.last_event = Some(LastEvent::Working);
            }
            "tool.execution_complete" => {
                if d.get("success").and_then(Value::as_bool) == Some(false) {
                    s.tool_errors += 1;
                }
            }
            "session.error" => s.tool_errors += 1,
            "session.compaction_complete" => s.compactions += 1,
            "subagent.started" => s.subagents += 1,
            "abort" => {
                s.interruptions += 1;
                s.note_turn_end(ts);
                s.last_event = Some(LastEvent::TurnEnded);
            }
            "permission.requested" => {
                s.permission_requests += 1;
                s.last_event = Some(LastEvent::NeedsPermission);
            }
            "permission.completed" => s.last_event = Some(LastEvent::Working),
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
