//! Claude Code transcripts: `~/.claude/projects/<project>/<session>.jsonl`, with
//! sub-agent transcripts in `<session>/subagents/*.jsonl`. The same files are
//! written by the CLI, the VS Code extension and the desktop app.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::model::{LastEvent, ParsedSession, Prompt, TokenUsage};
use crate::util::{parse_ts, truncate};

use super::{MAX_PROMPT_CHARS, open_lines};

const INTERRUPT_MARKER: &str = "[Request interrupted by user";

/// `paths[0]` is the main transcript; any further paths are sub-agent transcripts.
pub fn parse(paths: &[PathBuf]) -> Result<Option<ParsedSession>> {
    let Some(main) = paths.first() else {
        return Ok(None);
    };
    let mut s = ParsedSession {
        external_id: main
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default(),
        ..Default::default()
    };
    parse_file(main, &mut s, false)?;
    for sub in &paths[1..] {
        // A broken sub-agent file should not drop the whole session.
        let _ = parse_file(sub, &mut s, true);
    }
    if s.title.is_none() {
        s.title = s.prompts.first().map(|p| truncate(&p.text, 120));
    }
    Ok((!s.is_empty()).then_some(s))
}

fn parse_file(path: &Path, s: &mut ParsedSession, is_subagent: bool) -> Result<()> {
    // Each content block is its own line but repeats the message's usage, so
    // usage is keyed by message id and counted once.
    let mut usage_by_msg: HashMap<String, (String, TokenUsage)> = HashMap::new();

    for line in open_lines(path)?.lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let ts = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");

        if !is_subagent {
            fill_context(s, &v);
        }

        match kind {
            "assistant" => {
                let msg = &v["message"];
                let model = msg.get("model").and_then(Value::as_str).unwrap_or("");
                if let (Some(id), Some(u)) =
                    (msg.get("id").and_then(Value::as_str), msg.get("usage"))
                    && !model.is_empty()
                    && model != "<synthetic>"
                {
                    usage_by_msg.insert(id.to_string(), (model.to_string(), read_usage(u)));
                }
                if is_subagent {
                    continue;
                }
                s.note_time(ts);
                let tool_uses = content_items(msg)
                    .filter(|i| i.get("type").and_then(Value::as_str) == Some("tool_use"))
                    .count() as u64;
                s.tool_calls += tool_uses;
                let ended = tool_uses == 0
                    && matches!(
                        msg.get("stop_reason").and_then(Value::as_str),
                        Some("end_turn" | "stop_sequence" | "max_tokens")
                    );
                if ended {
                    s.note_turn_end(ts);
                }
                s.last_event = Some(if ended {
                    LastEvent::TurnEnded
                } else {
                    LastEvent::Working
                });
            }
            "user" if !is_subagent => handle_user(s, &v, ts),
            "custom-title" => {
                if let Some(t) = v.get("customTitle").and_then(Value::as_str) {
                    s.title = Some(truncate(t, 200));
                }
            }
            "summary" if s.title.is_none() => {
                if let Some(t) = v.get("summary").and_then(Value::as_str) {
                    s.title = Some(truncate(t, 200));
                }
            }
            "cost-state" if !is_subagent => {
                if let Some(c) = v.get("totalCostUSD").and_then(Value::as_f64) {
                    s.reported_cost_usd = Some(c);
                }
                s.lines_added = v
                    .get("totalLinesAdded")
                    .and_then(Value::as_i64)
                    .or(s.lines_added);
                s.lines_removed = v
                    .get("totalLinesRemoved")
                    .and_then(Value::as_i64)
                    .or(s.lines_removed);
            }
            _ => {}
        }
    }

    let mut ids: Vec<_> = usage_by_msg.into_iter().collect();
    ids.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, (model, usage)) in ids {
        if !is_subagent {
            s.assistant_turns += 1;
            *s.model_turns.entry(model.clone()).or_default() += 1;
        }
        s.add_tokens(&model, &usage);
    }
    Ok(())
}

fn handle_user(s: &mut ParsedSession, v: &Value, ts: Option<i64>) {
    s.note_time(ts);
    if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return;
    }
    let msg = &v["message"];

    let mut has_tool_result = false;
    for item in content_items(msg) {
        if item.get("type").and_then(Value::as_str) == Some("tool_result") {
            has_tool_result = true;
            if item.get("is_error").and_then(Value::as_bool) == Some(true) {
                s.tool_errors += 1;
            }
        }
    }

    let text = user_text(msg);
    if text.contains(INTERRUPT_MARKER) {
        s.interruptions += 1;
        s.note_turn_end(ts);
        s.last_event = Some(LastEvent::TurnEnded);
        return;
    }
    if has_tool_result {
        s.last_event = Some(LastEvent::Working);
        return;
    }

    // Newer versions tag who wrote each message; skip agent-to-agent traffic.
    if let Some(origin) = v.pointer("/origin/kind").and_then(Value::as_str)
        && origin != "human"
    {
        return;
    }
    let Some(text) = clean_prompt(&text) else {
        return;
    };
    s.user_turns += 1;
    s.note_human_prompt(ts);
    s.last_event = Some(LastEvent::Working);
    s.prompts.push(Prompt {
        ts,
        text: truncate(&text, MAX_PROMPT_CHARS),
    });
}

fn fill_context(s: &mut ParsedSession, v: &Value) {
    if s.project_path.is_none() {
        s.project_path = v.get("cwd").and_then(Value::as_str).map(str::to_string);
    }
    if s.git_branch.is_none() {
        s.git_branch = v
            .get("gitBranch")
            .and_then(Value::as_str)
            .filter(|b| !b.is_empty() && *b != "HEAD")
            .map(str::to_string);
    }
    if s.entrypoint.is_none() {
        s.entrypoint = v
            .get("entrypoint")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
}

fn content_items(msg: &Value) -> impl Iterator<Item = &Value> {
    msg.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn user_text(msg: &Value) -> String {
    match msg.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|i| i.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .filter(|t| !t.trim_start().starts_with("<system-reminder>"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Turns a raw user message into prompt text, or `None` for tool chatter
/// (command output, caveats, reminders). Slash commands keep their name.
fn clean_prompt(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if t.starts_with("<command-name>") || t.starts_with("<command-message>") {
        let name = between(t, "<command-name>", "</command-name>").unwrap_or("");
        let args = between(t, "<command-args>", "</command-args>").unwrap_or("");
        let joined = format!("{name} {args}").trim().to_string();
        return (!joined.is_empty()).then_some(joined);
    }
    if t.starts_with('<') {
        return None;
    }
    Some(t.to_string())
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(s[i..j].trim())
}

fn read_usage(u: &Value) -> TokenUsage {
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    TokenUsage {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn parses_turns_tokens_and_interruptions() {
        let dir = tempfile::tempdir().unwrap();
        let main = write(
            dir.path(),
            "abc.jsonl",
            &[
                r#"{"type":"user","cwd":"/work/app","gitBranch":"main","entrypoint":"cli","origin":{"kind":"human"},"timestamp":"2026-01-01T10:00:00Z","message":{"role":"user","content":"Add a login page"}}"#,
                // Two lines of the same assistant message: usage must count once.
                r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5-5","stop_reason":"tool_use","content":[{"type":"tool_use","name":"Read"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":1000,"cache_creation_input_tokens":10}}}"#,
                r#"{"type":"assistant","timestamp":"2026-01-01T10:00:06Z","message":{"id":"m1","model":"claude-opus-5-5","stop_reason":"tool_use","content":[{"type":"tool_use","name":"Edit"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":1000,"cache_creation_input_tokens":10}}}"#,
                r#"{"type":"user","timestamp":"2026-01-01T10:00:07Z","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"content":"boom"}]}}"#,
                r#"{"type":"user","timestamp":"2026-01-01T10:00:08Z","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#,
                r#"{"type":"user","isMeta":true,"timestamp":"2026-01-01T10:00:09Z","message":{"role":"user","content":"<local-command-caveat>x</local-command-caveat>"}}"#,
                r#"{"type":"user","origin":{"kind":"human"},"timestamp":"2026-01-01T10:01:00Z","message":{"role":"user","content":"<command-name>/review</command-name><command-args>auth</command-args>"}}"#,
                r#"{"type":"assistant","timestamp":"2026-01-01T10:02:00Z","message":{"id":"m2","model":"claude-opus-5-5","stop_reason":"end_turn","content":[{"type":"text","text":"done"}],"usage":{"input_tokens":10,"output_tokens":5}}}"#,
                r#"{"type":"custom-title","customTitle":"Login page"}"#,
                r#"{"type":"cost-state","totalCostUSD":1.25,"totalLinesAdded":40,"totalLinesRemoved":3}"#,
                "not json",
            ],
        );
        std::fs::create_dir_all(dir.path().join("abc/subagents")).unwrap();
        let sub = write(
            &dir.path().join("abc/subagents"),
            "agent-1.jsonl",
            &[
                r#"{"type":"assistant","isSidechain":true,"message":{"id":"s1","model":"claude-haiku-5-5","usage":{"input_tokens":7,"output_tokens":3}}}"#,
            ],
        );

        let s = parse(&[main, sub]).unwrap().unwrap();
        assert_eq!(s.external_id, "abc");
        assert_eq!(s.project_path.as_deref(), Some("/work/app"));
        assert_eq!(s.git_branch.as_deref(), Some("main"));
        assert_eq!(s.user_turns, 2);
        assert_eq!(s.prompts[1].text, "/review auth");
        assert_eq!(s.assistant_turns, 2);
        assert_eq!(s.tool_calls, 2);
        assert_eq!(s.tool_errors, 1);
        assert_eq!(s.interruptions, 1);
        assert_eq!(s.title.as_deref(), Some("Login page"));
        assert_eq!(s.last_event, Some(LastEvent::TurnEnded));
        assert_eq!(s.reported_cost_usd, Some(1.25));
        assert_eq!(s.lines_added, Some(40));

        let d = s.derive();
        assert_eq!(d.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(d.tokens.input, 117);
        assert_eq!(d.tokens.cache_read, 1000);
        assert_eq!(d.cost_usd, Some(1.25));
        assert!(!d.cost_is_estimate);
        assert_eq!(d.ended_at.unwrap() - d.started_at.unwrap(), 120_000);
    }

    #[test]
    fn empty_transcript_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        let main = write(
            dir.path(),
            "e.jsonl",
            &[r#"{"type":"summary","summary":"x"}"#],
        );
        assert!(parse(&[main]).unwrap().is_none());
    }
}
