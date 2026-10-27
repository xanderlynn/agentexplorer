//! Copilot Chat in VS Code: `User/workspaceStorage/<hash>/chatSessions/<id>.json`
//! (older, a full snapshot) or `<id>.jsonl` (newer, a snapshot followed by
//! patches). The workspace folder lives in `<hash>/workspace.json`.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::model::{LastEvent, ParsedSession, Prompt, TokenUsage};
use crate::util::truncate;

use super::{MAX_PROMPT_CHARS, open_lines};

/// `modelState.value` meaning a response finished normally.
const STATE_COMPLETE: i64 = 1;

pub fn parse(paths: &[PathBuf]) -> Result<Option<ParsedSession>> {
    let Some(path) = paths.first() else {
        return Ok(None);
    };
    let state = if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
        replay_patches(path)?
    } else {
        serde_json::from_str(&std::fs::read_to_string(path)?)?
    };
    let Some(state) = state else {
        return Ok(None);
    };

    let mut s = ParsedSession {
        external_id: state
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| path.file_stem().map(|x| x.to_string_lossy().into_owned()))
            .unwrap_or_default(),
        project_path: workspace_folder(path),
        title: state
            .get("customTitle")
            .and_then(Value::as_str)
            .map(|t| truncate(t, 200)),
        entrypoint: Some("vscode".into()),
        ..Default::default()
    };
    if let Some(created) = state.get("creationDate").and_then(Value::as_i64) {
        s.note_time(Some(created));
    }

    let requests = state
        .get("requests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let last_index = requests.len().saturating_sub(1);
    for (i, req) in requests.iter().enumerate() {
        let ts = req.get("timestamp").and_then(Value::as_i64);
        s.note_time(ts);

        let text = req
            .pointer("/message/text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !text.is_empty() {
            s.user_turns += 1;
            s.note_human_prompt(ts);
            s.prompts.push(Prompt {
                ts,
                text: truncate(text, MAX_PROMPT_CHARS),
            });
        }

        let model = req
            .get("modelId")
            .and_then(Value::as_str)
            .or_else(|| {
                req.pointer("/result/metadata/resolvedModel")
                    .and_then(Value::as_str)
            })
            .map(|m| m.trim_start_matches("copilot/").to_string())
            .unwrap_or_else(|| "copilot".into());

        let result = req.get("result");
        let state_value = req.pointer("/modelState/value").and_then(Value::as_i64);
        let completed_at = req
            .pointer("/modelState/completedAt")
            .and_then(Value::as_i64);
        let elapsed = result
            .and_then(|r| r.pointer("/timings/totalElapsed"))
            .and_then(Value::as_i64)
            .or_else(|| req.get("elapsedMs").and_then(Value::as_i64));
        let end = completed_at.or_else(|| Some(ts? + elapsed?));
        s.note_time(end);

        if result.is_some() || state_value.is_some() {
            s.assistant_turns += 1;
            *s.model_turns.entry(model.clone()).or_default() += 1;
        }

        if let Some(items) = req.get("response").and_then(Value::as_array) {
            s.tool_calls += items
                .iter()
                .filter(|it| {
                    matches!(
                        it.get("kind").and_then(Value::as_str),
                        Some("toolInvocation") | Some("toolInvocationSerialized")
                    )
                })
                .count() as u64;
        }
        if result.and_then(|r| r.get("errorDetails")).is_some() {
            s.tool_errors += 1;
        }
        let canceled = req.get("isCanceled").and_then(Value::as_bool) == Some(true);
        let finished = result.is_some() || completed_at.is_some();
        if canceled || (finished && state_value.is_some_and(|v| v != STATE_COMPLETE)) {
            s.interruptions += 1;
        }

        let meta = result.and_then(|r| r.get("metadata"));
        let n = |k: &str| {
            meta.and_then(|m| m.get(k))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let output = match n("outputTokens") {
            0 => req
                .get("completionTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            x => x,
        };
        s.add_tokens(
            &model,
            &TokenUsage {
                input: n("promptTokens"),
                output,
                ..Default::default()
            },
        );

        if finished || canceled {
            s.note_turn_end(end);
        }

        if i == last_index {
            s.last_event = Some(if finished || canceled {
                LastEvent::TurnEnded
            } else {
                LastEvent::Working
            });
        }
    }

    if s.title.is_none() {
        s.title = s.prompts.first().map(|p| truncate(&p.text, 120));
    }
    Ok((!s.is_empty()).then_some(s))
}

/// Rebuilds the session object from a `.jsonl` patch log:
/// kind 0 = full snapshot, kind 1 = set the value at path `k`,
/// kind 2 = append `v` to the array at `k` (truncating to index `i` first).
fn replay_patches(path: &Path) -> Result<Option<Value>> {
    let mut state: Option<Value> = None;
    for line in open_lines(path)?.lines() {
        let Ok(line) = line else { break };
        let Ok(op) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match op.get("kind").and_then(Value::as_i64) {
            Some(0) => state = op.get("v").cloned(),
            Some(kind @ (1 | 2)) => {
                let (Some(root), Some(keys)) =
                    (state.as_mut(), op.get("k").and_then(Value::as_array))
                else {
                    continue;
                };
                let Some(target) = navigate(root, keys) else {
                    continue;
                };
                let value = op.get("v").cloned().unwrap_or(Value::Null);
                if kind == 1 {
                    *target = value;
                } else {
                    if !target.is_array() {
                        *target = Value::Array(vec![]);
                    }
                    let arr = target.as_array_mut().expect("just ensured array");
                    if let Some(i) = op.get("i").and_then(Value::as_u64) {
                        arr.truncate(i as usize);
                    }
                    match value {
                        Value::Array(items) => arr.extend(items),
                        other => arr.push(other),
                    }
                }
            }
            _ => {}
        }
    }
    Ok(state)
}

/// Walks to the value at `keys`, creating missing object keys on the way.
fn navigate<'a>(root: &'a mut Value, keys: &[Value]) -> Option<&'a mut Value> {
    let mut cur = root;
    for key in keys {
        cur = match key {
            Value::String(k) => {
                if !cur.is_object() {
                    *cur = Value::Object(Default::default());
                }
                cur.as_object_mut()?.entry(k.clone()).or_insert(Value::Null)
            }
            Value::Number(n) => cur.as_array_mut()?.get_mut(n.as_u64()? as usize)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn workspace_folder(session_file: &Path) -> Option<String> {
    let ws_dir = session_file.parent()?.parent()?;
    let text = std::fs::read_to_string(ws_dir.join("workspace.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let uri = v
        .get("folder")
        .or_else(|| v.get("workspace"))
        .and_then(Value::as_str)?;
    file_uri_to_path(uri)
}

/// `file:///Users/me/My%20App` → `/Users/me/My App`; `file:///c%3A/x` → `c:/x`.
fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode(rest);
    let bytes = decoded.as_bytes();
    let is_windows_drive = bytes.len() >= 3 && bytes[0] == b'/' && bytes[2] == b':';
    Some(if is_windows_drive {
        decoded[1..].to_string()
    } else {
        decoded
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_uris() {
        assert_eq!(
            file_uri_to_path("file:///Users/me/My%20App").as_deref(),
            Some("/Users/me/My App")
        );
        assert_eq!(
            file_uri_to_path("file:///c%3A/code/x").as_deref(),
            Some("c:/code/x")
        );
        assert_eq!(file_uri_to_path("vscode-remote://x"), None);
    }

    #[test]
    fn replays_jsonl_patches() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("hash1");
        std::fs::create_dir_all(ws.join("chatSessions")).unwrap();
        std::fs::write(
            ws.join("workspace.json"),
            r#"{"folder":"file:///work/site"}"#,
        )
        .unwrap();
        let file = ws.join("chatSessions/s1.jsonl");
        let lines = [
            r#"{"kind":0,"v":{"sessionId":"s1","creationDate":1000,"requests":[]}}"#,
            r#"{"kind":2,"k":["requests"],"v":[{"requestId":"r1","timestamp":2000,"modelId":"copilot/claude-sonnet-4.5","message":{"text":"Fix the mermaid diagram"},"response":[]}]}"#,
            r#"{"kind":2,"k":["requests",0,"response"],"v":[{"kind":"toolInvocationSerialized"},{"kind":"markdownContent"}]}"#,
            r#"{"kind":1,"k":["requests",0,"result"],"v":{"timings":{"totalElapsed":5000},"metadata":{"promptTokens":900,"outputTokens":120}}}"#,
            r#"{"kind":1,"k":["requests",0,"modelState"],"v":{"value":1,"completedAt":7000}}"#,
            r#"{"kind":2,"k":["requests"],"v":[{"requestId":"r2","timestamp":9000,"message":{"text":"Now the colors"},"response":[]}]}"#,
            r#"{"kind":1,"k":["customTitle"],"v":"Mermaid fixes"}"#,
        ];
        std::fs::write(&file, lines.join("\n")).unwrap();

        let s = parse(&[file]).unwrap().unwrap();
        assert_eq!(s.external_id, "s1");
        assert_eq!(s.project_path.as_deref(), Some("/work/site"));
        assert_eq!(s.title.as_deref(), Some("Mermaid fixes"));
        assert_eq!(s.user_turns, 2);
        assert_eq!(s.assistant_turns, 1);
        assert_eq!(s.tool_calls, 1);
        assert_eq!(s.interruptions, 0);
        // The second request has no result yet, so the agent is still working.
        assert_eq!(s.last_event, Some(LastEvent::Working));
        let d = s.derive();
        assert_eq!(d.model.as_deref(), Some("claude-sonnet-4.5"));
        assert_eq!(d.tokens.input, 900);
        assert_eq!(d.started_at, Some(1000));
        assert_eq!(d.ended_at, Some(9000));
    }

    #[test]
    fn splice_with_index_truncates_first() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.jsonl");
        std::fs::write(
            &file,
            [
                r#"{"kind":0,"v":{"requests":[{"message":{"text":"a"}},{"message":{"text":"b"}}]}}"#,
                r#"{"kind":2,"k":["requests"],"i":1,"v":[{"message":{"text":"c"}}]}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let state = replay_patches(&file).unwrap().unwrap();
        let texts: Vec<_> = state["requests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["message"]["text"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(texts, ["a", "c"]);
    }
}
