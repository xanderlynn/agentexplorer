//! Claude Code transcripts: `~/.claude/projects/<project>/<session>.jsonl`, with
//! sub-agent transcripts in `<session>/subagents/*.jsonl`. The same files are
//! written by the CLI, the VS Code extension and the desktop app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::model::{LastEvent, ParsedSession, Prompt, TokenUsage};
use crate::team::{self, Finish, Handoff, Kind, LEAD, Status, Team};
use crate::util::{parse_ts, truncate};

use super::{MAX_PROMPT_CHARS, read_from};

const INTERRUPT_MARKER: &str = "[Request interrupted by user";

/// What has been read from one transcript file.
#[derive(Clone, Default)]
struct FileState {
    offset: u64,
    /// The sub-agent whose transcript this is; `None` for the main one.
    agent: Option<String>,
    /// Its `.meta.json` says its run is over; it is not read again after.
    finished: bool,
    first: Option<i64>,
    last: Option<i64>,
    /// Each content block is its own line but repeats the message's usage,
    /// so usage is keyed by message id and counted once.
    usage: HashMap<String, (String, TokenUsage)>,
}

/// A session parsed so far, before token totals and sub-agent spans are
/// added up, plus where each of its files was read up to.
#[derive(Clone, Default)]
pub struct State {
    s: ParsedSession,
    files: HashMap<PathBuf, FileState>,
}

impl State {
    pub fn rewritten(&self, paths: &[PathBuf]) -> bool {
        paths.iter().any(|p| {
            self.files
                .get(p)
                .is_some_and(|f| super::shrank(p, f.offset))
        })
    }
}

/// `paths[0]` is the main transcript; any further paths are sub-agent transcripts.
#[cfg(test)]
pub fn parse(paths: &[PathBuf]) -> Result<Option<ParsedSession>> {
    update(paths, &mut State::default())
}

/// Reads what was appended since `st` was last updated and returns the
/// session as it stands now.
pub fn update(paths: &[PathBuf], st: &mut State) -> Result<Option<ParsedSession>> {
    let Some(main) = paths.first() else {
        return Ok(None);
    };
    if st.files.is_empty() {
        st.s.external_id = main
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    let mut f = st.files.remove(main).unwrap_or_default();
    let read = read_file(main, &mut f, &mut st.s);
    let main_last = f.last;
    st.files.insert(main.clone(), f);
    read?;
    for sub in &paths[1..] {
        let mut f = match st.files.remove(sub) {
            Some(f) => f,
            // A sub-agent's file can appear before the hand-off that starts
            // it reaches the main transcript; it waits until that is read.
            None => match link_subagent(&mut st.s.team, sub, main_last) {
                Some(key) => FileState {
                    agent: Some(key),
                    ..Default::default()
                },
                None => continue,
            },
        };
        // A broken sub-agent file should not drop the whole session.
        let _ = read_file(sub, &mut f, &mut st.s);
        f.finished = f.finished || meta_finished(sub);
        st.files.insert(sub.clone(), f);
    }
    Ok(finish(st, paths))
}

/// Adds up tokens and sub-agent spans on a copy, so the state can keep
/// growing without counting anything twice.
fn finish(st: &State, paths: &[PathBuf]) -> Option<ParsedSession> {
    let mut s = st.s.clone();
    s.subagents = paths.len() as u64 - 1;
    for path in paths {
        let Some(f) = st.files.get(path) else {
            continue;
        };
        let mut ids: Vec<_> = f.usage.iter().collect();
        ids.sort_by(|a, b| a.0.cmp(b.0));
        if let Some(key) = &f.agent {
            s.team.span(key, f.first, f.last);
            if let Some(a) = s.team.agent(key) {
                let fresh: u64 = ids
                    .iter()
                    .map(|(_, (_, u))| u.input + u.output + u.cache_write)
                    .sum();
                a.tokens = a.tokens.max(fresh);
            }
            if f.finished
                && s.team
                    .agent(key)
                    .is_some_and(|a| a.status == Status::Running)
            {
                s.team.finish(key, None, Finish::default());
            }
        }
        for (_, (model, usage)) in ids {
            if f.agent.is_none() {
                s.assistant_turns += 1;
                *s.model_turns.entry(model.clone()).or_default() += 1;
            }
            s.add_tokens(model, usage);
        }
    }
    s.team.sort();
    if s.title.is_none() {
        s.title = s.prompts.first().map(|p| truncate(&p.text, 120));
    }
    (!s.is_empty()).then_some(s)
}

fn read_meta(path: &Path) -> Value {
    std::fs::read_to_string(path.with_extension("meta.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// The timestamp of a transcript's first line.
fn first_ts(path: &Path) -> Option<i64> {
    let mut first = String::new();
    std::io::BufRead::read_line(&mut super::open_lines(path).ok()?, &mut first).ok()?;
    let v: Value = serde_json::from_str(&first).ok()?;
    v.get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_ts)
}

/// The sub-agent's run is over, per the `.meta.json` beside its transcript.
fn meta_finished(path: &Path) -> bool {
    read_meta(path).get("finished").and_then(Value::as_bool) == Some(true)
}

/// Matches a sub-agent transcript to the hand-off that started it, using
/// the `.meta.json` written beside it, and returns its key. `None` when the
/// hand-off is named but the main transcript has not reached it yet. Once
/// the main transcript runs past the sub-agent's start (`main_last`), a
/// missing hand-off makes it a team member of its own.
fn link_subagent(team: &mut Team, path: &Path, main_last: Option<i64>) -> Option<String> {
    let stem = path
        .file_stem()
        .map(|x| x.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = stem.strip_prefix("agent-").unwrap_or(&stem).to_string();
    let meta = read_meta(path);
    let m = |k: &str| meta.get(k).and_then(Value::as_str);
    match m("toolUseId") {
        Some(call) if team.agent(call).is_some() => team.alias(call, &key),
        Some(_) if !main_last.is_some_and(|t| first_ts(path).is_some_and(|f| t > f)) => {
            return None;
        }
        _ => team.hand_off(
            &key,
            LEAD,
            None,
            Handoff {
                role: m("agentType"),
                task: m("description"),
                ..Default::default()
            },
        ),
    }
    Some(key)
}

/// Reads the lines appended to one file since `f.offset`.
fn read_file(path: &Path, f: &mut FileState, s: &mut ParsedSession) -> Result<()> {
    let agent = f.agent.clone();
    f.offset = read_from(path, f.offset, |line| {
        read_line(line, agent.as_deref(), f, s)
    })?;
    Ok(())
}

fn read_line(line: &str, agent: Option<&str>, f: &mut FileState, s: &mut ParsedSession) {
    let is_subagent = agent.is_some();
    let me = agent.unwrap_or(LEAD);
    {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let ts = v
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_ts);
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if ts.is_some() {
            f.first = f.first.or(ts);
            f.last = ts;
        }
        // Older versions wrote agent-to-agent lines into the main transcript
        // without saying which agent; they are left out of the team.
        let unattributed =
            !is_subagent && v.get("isSidechain").and_then(Value::as_bool) == Some(true);

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
                    let usage = read_usage(u);
                    if !is_subagent {
                        // Everything sent in this request: the context size.
                        s.note_context(usage.input + usage.cache_read + usage.cache_write);
                    }
                    f.usage.insert(id.to_string(), (model.to_string(), usage));
                    if is_subagent && let Some(a) = s.team.agent(me) {
                        a.model.get_or_insert_with(|| model.to_string());
                    }
                }
                if !unattributed {
                    for item in content_items(msg) {
                        if item.get("type").and_then(Value::as_str) == Some("tool_use") {
                            note_team_tool(&mut s.team, me, item, ts, is_subagent);
                            note_toolkit(s, item);
                        }
                    }
                }
                if is_subagent || v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                    return;
                }
                s.note_time(ts);
                let mut tool_uses = 0;
                for item in content_items(msg) {
                    match item.get("type").and_then(Value::as_str) {
                        Some("tool_use") => {
                            tool_uses += 1;
                            let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
                            if matches!(name, "ExitPlanMode" | "EnterPlanMode") {
                                s.planned = true;
                            }
                            if let Some(tid) = item.get("id").and_then(Value::as_str) {
                                s.tool_names.insert(tid.to_string(), name.to_string());
                                s.note_pending(tid, name);
                            }
                            let input = &item["input"];
                            let path = ["file_path", "path", "notebook_path"]
                                .iter()
                                .find_map(|k| input.get(*k).and_then(Value::as_str));
                            s.note_tool(name, path, ts);
                        }
                        Some("text") => {
                            if let Some(t) = item.get("text").and_then(Value::as_str) {
                                s.note_message(t);
                            }
                        }
                        _ => {}
                    }
                }
                let stop = msg.get("stop_reason").and_then(Value::as_str);
                let ended = tool_uses == 0
                    && matches!(stop, Some("end_turn" | "stop_sequence" | "max_tokens"));
                // Each lead reply decides it afresh, so a later good turn clears it.
                let error = v.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true);
                if error || ended {
                    s.failed = error || stop == Some("max_tokens");
                }
                if ended {
                    s.note_turn_end(ts);
                }
                s.last_event = Some(if ended {
                    LastEvent::TurnEnded
                } else {
                    LastEvent::Working
                });
            }
            "user" => {
                if !unattributed {
                    note_team_reply(&mut s.team, &v, ts);
                }
                if !is_subagent {
                    handle_user(s, &v, ts);
                }
            }
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
            "system"
                if !is_subagent
                    && v.get("subtype").and_then(Value::as_str) == Some("compact_boundary") =>
            {
                s.compactions += 1;
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
}

/// Skills and MCP servers a tool call used, by the lead or a sub-agent.
fn note_toolkit(s: &mut ParsedSession, item: &Value) {
    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
    if name == "Skill" {
        if let Some(skill) = item.pointer("/input/skill").and_then(Value::as_str) {
            s.note_skill(skill);
        }
    } else if let Some(rest) = name.strip_prefix("mcp__") {
        s.note_mcp(rest.split("__").next().unwrap_or(rest));
    }
}

/// Slash commands that are part of Claude Code itself, not skills.
const BUILTIN_COMMANDS: &[&str] = &[
    "add-dir",
    "agents",
    "bug",
    "clear",
    "compact",
    "config",
    "context",
    "cost",
    "doctor",
    "effort",
    "exit",
    "export",
    "fast",
    "help",
    "hooks",
    "ide",
    "init",
    "login",
    "logout",
    "mcp",
    "memory",
    "model",
    "output-style",
    "permissions",
    "plan",
    "plugin",
    "plugins",
    "pr-comments",
    "quit",
    "release-notes",
    "resume",
    "rewind",
    "status",
    "statusline",
    "terminal-setup",
    "theme",
    "upgrade",
    "usage",
    "vim",
];

/// Team traffic in a tool call made by `me`: hand-offs, messages, reports
/// and scheduled work.
fn note_team_tool(team: &mut Team, me: &str, item: &Value, ts: Option<i64>, is_subagent: bool) {
    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
    let input = &item["input"];
    let str_of = |k: &str| input.get(k).and_then(Value::as_str);
    if is_subagent {
        team.tool(me, name);
    }
    match name {
        "Agent" | "Task" => {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                return;
            };
            team.hand_off(
                id,
                me,
                ts,
                Handoff {
                    role: str_of("subagent_type"),
                    task: str_of("description"),
                    brief: str_of("prompt"),
                    name: str_of("name"),
                    model: str_of("model"),
                    background: input.get("run_in_background").and_then(Value::as_bool)
                        == Some(true),
                },
            );
        }
        "SendMessage" => {
            if let Some(to) = str_of("to") {
                let text = str_of("message").or(str_of("summary")).unwrap_or("");
                team.say(ts, me, to, Kind::Message, text);
            }
        }
        "SubagentHandback" => team.report(me, ts, str_of("message").unwrap_or("")),
        _ => team.schedule_from(name, input, me, ts),
    }
}

/// Results coming back to whoever started an agent: the hand-off's tool
/// result and, for background agents, the completion notice.
fn note_team_reply(team: &mut Team, v: &Value, ts: Option<i64>) {
    let msg = &v["message"];
    for item in content_items(msg) {
        if item.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let Some(call) = item.get("tool_use_id").and_then(Value::as_str) else {
            continue;
        };
        if team.agent(call).is_none() {
            continue;
        }
        let r = &v["toolUseResult"];
        if let Some(id) = r.get("agentId").and_then(Value::as_str) {
            team.alias(call, id);
        }
        let n = |k: &str| r.get(k).and_then(Value::as_u64);
        let failed = item.get("is_error").and_then(Value::as_bool) == Some(true);
        if failed || r.get("status").and_then(Value::as_str) == Some("completed") {
            let text = tool_result_text(item);
            team.finish(
                call,
                ts,
                Finish {
                    status: Some(if failed { Status::Failed } else { Status::Done }),
                    result: Some(&text),
                    tokens: n("totalTokens"),
                    tool_calls: n("totalToolUseCount"),
                    duration_ms: n("totalDurationMs").map(|d| d as i64),
                    ..Default::default()
                },
            );
        }
    }

    let text = user_text(msg);
    let Some(note) = team::tag(&text, "task-notification") else {
        return;
    };
    let (Some(call), Some(id)) = (team::tag(note, "tool-use-id"), team::tag(note, "task-id"))
    else {
        return;
    };
    if team.agent(call).is_none() {
        return;
    }
    team.alias(call, id);
    let num = |k: &str| team::tag(note, k).and_then(|x| x.parse::<u64>().ok());
    // When the agent handed its report back itself, the notice only says so.
    let result = team::tag(note, "result");
    let handed_back = result.is_some_and(|r| r.starts_with("This agent's report was delivered"));
    team.finish(
        call,
        ts,
        Finish {
            status: Some(match team::tag(note, "status") {
                Some("failed") => Status::Failed,
                Some("killed" | "stopped" | "cancelled") => Status::Stopped,
                _ => Status::Done,
            }),
            result: if handed_back {
                None
            } else {
                result.or(team::tag(note, "summary"))
            },
            handed_back,
            tokens: num("subagent_tokens"),
            tool_calls: num("tool_uses"),
            duration_ms: num("duration_ms").map(|d| d as i64),
            ..Default::default()
        },
    );
}

fn handle_user(s: &mut ParsedSession, v: &Value, ts: Option<i64>) {
    s.note_time(ts);
    if v.get("permissionMode").and_then(Value::as_str) == Some("plan") {
        s.planned = true;
    }
    // Sidechain lines are agent-to-agent traffic; older versions wrote them
    // into the main transcript without an `origin` tag.
    if v.get("isMeta").and_then(Value::as_bool) == Some(true)
        || v.get("isSidechain").and_then(Value::as_bool) == Some(true)
    {
        return;
    }
    let msg = &v["message"];

    let mut has_tool_result = false;
    for item in content_items(msg) {
        if item.get("type").and_then(Value::as_str) == Some("tool_result") {
            has_tool_result = true;
            if let Some(id) = item.get("tool_use_id").and_then(Value::as_str) {
                s.note_resolved(id);
            }
            // A tool call the user rejected or interrupted is also marked
            // is_error, but it says nothing about the agent's setup.
            if item.get("is_error").and_then(Value::as_bool) == Some(true) {
                let name = item
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .and_then(|id| s.tool_names.get(id))
                    .cloned()
                    .unwrap_or_else(|| "tool".into());
                if is_user_rejection(&tool_result_text(item)) {
                    s.note_veto(&name);
                } else {
                    s.tool_errors += 1;
                    s.note_tool_error(&name);
                }
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
    if let Some(cmd) = text
        .strip_prefix('/')
        .and_then(|c| c.split_whitespace().next())
        && !BUILTIN_COMMANDS.contains(&cmd)
    {
        s.note_skill(cmd);
    }
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

fn tool_result_text(item: &Value) -> String {
    match item.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn is_user_rejection(text: &str) -> bool {
    text.contains("doesn't want to proceed with this tool use") || text.contains(INTERRUPT_MARKER)
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
                r#"{"type":"user","timestamp":"2026-01-01T10:00:07Z","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected."}]}}"#,
                r#"{"type":"user","isSidechain":true,"timestamp":"2026-01-01T10:00:07Z","message":{"role":"user","content":"agent-to-agent message"}}"#,
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
        // A slash command that is not built in is a skill you invoked.
        assert_eq!(s.skills.get("review"), Some(&1));
        assert_eq!(s.assistant_turns, 2);
        assert_eq!(s.tool_calls, 2);
        assert_eq!(s.tool_counts.get("Read"), Some(&1));
        assert_eq!((s.reads, s.edits), (1, 1));
        assert_eq!(s.subagents, 1);
        assert_eq!(s.last_message.as_deref(), Some("done"));
        // Read + Edit in the first turn: coding. Context = input + cache.
        assert!(s.mode_ms.get("coding").is_some_and(|ms| *ms > 0));
        assert_eq!(s.peak_context, 1110);
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
    fn builds_the_team() {
        let dir = tempfile::tempdir().unwrap();
        // Lines are wrapped here for reading; a transcript has one per line.
        let one = |ls: &[&str]| ls.iter().map(|l| l.replace('\n', "")).collect::<Vec<_>>();
        let main_lines = one(&[
            r#"{"type":"user","origin":{"kind":"human"},"timestamp":"2026-01-01T10:00:00Z","message":{"role":"user","content":"Review the app"}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5-5","stop_reason":"tool_use","content":[
                    {"type":"tool_use","id":"tu1","name":"Agent","input":{"description":"Review the backend","prompt":"Read src-tauri and list bugs","subagent_type":"Explore","run_in_background":true}},
                    {"type":"tool_use","id":"tu2","name":"Agent","input":{"description":"Check the UI","prompt":"Look at src","name":"ui-check"}},
                    {"type":"tool_use","id":"tu3","name":"CronCreate","input":{"cron":"50 23 * * *","prompt":"Run the nightly review"}},
                    {"type":"tool_use","id":"tu4","name":"Skill","input":{"skill":"anthropic-skills:pdf"}},
                    {"type":"tool_use","id":"tu5","name":"mcp__github__create_pull_request","input":{}}
                ],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:00:06Z","toolUseResult":{"agentId":"aaa","status":"async_launched"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu1","content":"launched"}]}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:01:00Z","toolUseResult":{"agentId":"bbb","status":"completed","totalTokens":900,"totalToolUseCount":4},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu2","content":[{"type":"text","text":"UI looks fine"}]}]}}"#,
            r#"{"type":"user","timestamp":"2026-01-01T10:03:00Z","message":{"role":"user","content":"<task-notification>\n<task-id>aaa</task-id>\n<tool-use-id>tu1</tool-use-id>\n<status>completed</status>\n<summary>Agent finished</summary>\n<result>This agent's report was delivered to you as a message.</result>\n<usage><subagent_tokens>5000</subagent_tokens><tool_uses>7</tool_uses><duration_ms>170000</duration_ms></usage>\n</task-notification>"}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T10:04:00Z","message":{"id":"m2","model":"claude-opus-5-5","stop_reason":"end_turn","content":[{"type":"text","text":"done"}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
        ]);
        let main = write(
            dir.path(),
            "t.jsonl",
            &main_lines.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        let subs = dir.path().join("t/subagents");
        std::fs::create_dir_all(&subs).unwrap();
        std::fs::write(subs.join("agent-aaa.meta.json"), r#"{"agentType":"Explore","description":"Review the backend","toolUseId":"tu1","finished":true}"#).unwrap();
        let sub_lines = one(&[
            r#"{"type":"user","isSidechain":true,"agentId":"aaa","timestamp":"2026-01-01T10:00:10Z","message":{"role":"user","content":"Read src-tauri and list bugs"}}"#,
            r#"{"type":"assistant","isSidechain":true,"agentId":"aaa","timestamp":"2026-01-01T10:00:20Z","message":{"id":"s1","model":"claude-haiku-5-5","content":[
                    {"type":"tool_use","id":"x1","name":"Read","input":{"file_path":"a.rs"}},
                    {"type":"tool_use","id":"x2","name":"SendMessage","input":{"to":"ui-check","message":"Does the UI call get_session?"}}
                ],"usage":{"input_tokens":7,"output_tokens":3}}}"#,
            r#"{"type":"assistant","isSidechain":true,"agentId":"aaa","timestamp":"2026-01-01T10:02:50Z","message":{"id":"s2","model":"claude-haiku-5-5","content":[
                    {"type":"tool_use","id":"x3","name":"SubagentHandback","input":{"message":"Found two bugs in ingest.rs"}}
                ],"usage":{"input_tokens":7,"output_tokens":3}}}"#,
        ]);
        let sub = write(
            &subs,
            "agent-aaa.jsonl",
            &sub_lines.iter().map(String::as_str).collect::<Vec<_>>(),
        );

        let s = parse(&[main, sub]).unwrap().unwrap();

        // Read as it grows, a line at a time, it comes out the same.
        let grow = tempfile::tempdir().unwrap();
        let gsubs = grow.path().join("t/subagents");
        std::fs::create_dir_all(&gsubs).unwrap();
        std::fs::copy(
            subs.join("agent-aaa.meta.json"),
            gsubs.join("agent-aaa.meta.json"),
        )
        .unwrap();
        let (gmain, gsub) = (grow.path().join("t.jsonl"), gsubs.join("agent-aaa.jsonl"));
        let mut st = State::default();
        let mut last = None;
        for i in 0..main_lines.len().max(sub_lines.len()) {
            for (path, lines) in [(&gmain, &main_lines), (&gsub, &sub_lines)] {
                if let Some(l) = lines.get(i) {
                    let mut f = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .unwrap();
                    writeln!(f, "{l}").unwrap();
                }
            }
            last = update(&[gmain.clone(), gsub.clone()], &mut st).unwrap();
        }
        assert_eq!(
            super::super::digest(&last.unwrap()),
            super::super::digest(&s)
        );
        let t = &s.team;
        assert_eq!(t.agents.len(), 2);
        let be = &t.agents[0];
        assert_eq!(
            (be.role.as_str(), be.task.as_str(), be.status),
            ("Explore", "Review the backend", Status::Done)
        );
        assert!(be.background);
        assert_eq!((be.tokens, be.tool_calls), (5000, 7));
        assert_eq!(be.model.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(be.result.as_deref(), Some("Found two bugs in ingest.rs"));
        let ui = &t.agents[1];
        assert_eq!(
            (ui.name.as_deref(), ui.status, ui.result.as_deref()),
            (Some("ui-check"), Status::Done, Some("UI looks fine"))
        );

        let flow: Vec<_> = t
            .messages
            .iter()
            .map(|m| (m.from.as_str(), m.to.as_str(), m.kind))
            .collect();
        assert_eq!(
            flow,
            [
                (LEAD, "tu1", Kind::Handoff),
                (LEAD, "tu2", Kind::Handoff),
                ("tu1", "tu2", Kind::Message),
                ("tu2", LEAD, Kind::Report),
                ("tu1", LEAD, Kind::Report),
            ]
        );
        assert_eq!(t.scheduled.len(), 1);
        assert_eq!(t.scheduled[0].cron.as_deref(), Some("50 23 * * *"));
        assert_eq!(s.skills.get("anthropic-skills:pdf"), Some(&1));
        assert_eq!(s.mcp.get("github"), Some(&1));
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
