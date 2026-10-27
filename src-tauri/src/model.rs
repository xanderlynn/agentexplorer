//! Normalized representation of one AI session, independent of which tool wrote it.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::pricing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    ClaudeCode,
    CopilotCli,
    CopilotVscode,
}

impl Source {
    pub const ALL: [Source; 3] = [
        Source::ClaudeCode,
        Source::CopilotCli,
        Source::CopilotVscode,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Source::ClaudeCode => "claude_code",
            Source::CopilotCli => "copilot_cli",
            Source::CopilotVscode => "copilot_vscode",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Source::ClaudeCode => "Claude Code",
            Source::CopilotCli => "Copilot CLI",
            Source::CopilotVscode => "Copilot Chat (VS Code)",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenUsage {
    pub fn add(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub ts: Option<i64>,
    pub text: String,
}

/// What the parser saw last; drives the live monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastEvent {
    /// The agent finished its turn and is waiting for the human.
    TurnEnded,
    /// The agent is mid-turn (thinking, calling tools, streaming).
    Working,
    /// The agent asked for a permission that has not been answered.
    NeedsPermission,
    /// The session process exited.
    Closed,
}

impl LastEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            LastEvent::TurnEnded => "turn_ended",
            LastEvent::Working => "working",
            LastEvent::NeedsPermission => "needs_permission",
            LastEvent::Closed => "closed",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ParsedSession {
    pub external_id: String,
    pub project_path: Option<String>,
    pub git_branch: Option<String>,
    pub title: Option<String>,
    pub entrypoint: Option<String>,
    pub event_times: Vec<i64>,
    /// Assistant turns per model, used to pick the primary model.
    pub model_turns: HashMap<String, u64>,
    pub tokens_by_model: HashMap<String, TokenUsage>,
    pub user_turns: u64,
    pub assistant_turns: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub interruptions: u64,
    pub permission_requests: u64,
    pub prompts: Vec<Prompt>,
    /// Cost reported by the tool itself, when it records one.
    pub reported_cost_usd: Option<f64>,
    pub premium_requests: Option<f64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub last_event: Option<LastEvent>,
    /// How long the agent waited on the human between finishing a turn and
    /// the next prompt, one sample per turn.
    pub wait_samples: Vec<i64>,
    /// Set by `note_turn_end`, consumed by `note_human_prompt`.
    pub last_turn_end: Option<i64>,
    /// Calls per tool name.
    pub tool_counts: BTreeMap<String, u64>,
    pub reads: u64,
    pub edits: u64,
    pub shell_calls: u64,
    /// Edits to an existing file the agent had not read in this session.
    pub edits_without_read: u64,
    /// Edits to a file the agent had already edited in this session; a high
    /// share suggests a fix-it-again loop.
    pub re_edits: u64,
    pub compactions: u64,
    pub subagents: u64,
    /// The agent's most recent text reply, for "what is it asking me?".
    pub last_message: Option<String>,
    pub read_paths: HashSet<String>,
    /// Files changed with Edit-type tools in this session.
    pub edited_paths: HashSet<String>,
    /// Files created or overwritten whole with Write-type tools.
    pub written_paths: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    /// Changes an existing file.
    Edit,
    /// Creates or overwrites a whole file; no prior read is expected.
    Write,
    Shell,
    Other,
}

/// Groups tool names across Claude Code, Copilot CLI and VS Code.
pub fn tool_kind(name: &str) -> ToolKind {
    let n = name.to_ascii_lowercase();
    let n = n.rsplit(['.', '/', ':']).next().unwrap_or(&n);
    match n {
        "read" | "view" | "grep" | "glob" | "ls" | "read_file" | "list_dir" | "file_search"
        | "grep_search" | "semantic_search" | "codebase_search" | "notebookread" | "search" => {
            ToolKind::Read
        }
        "edit"
        | "multiedit"
        | "str_replace"
        | "str_replace_editor"
        | "edit_file"
        | "apply_patch"
        | "notebookedit"
        | "replace_string_in_file"
        | "insert_edit_into_file"
        | "multi_replace_string_in_file" => ToolKind::Edit,
        "write" | "create" | "create_file" | "write_file" => ToolKind::Write,
        "bash" | "shell" | "powershell" | "run_in_terminal" | "run_command" | "execute" => {
            ToolKind::Shell
        }
        _ => ToolKind::Other,
    }
}

/// Idle gaps longer than this do not count toward active time.
pub const ACTIVE_GAP_CAP_MS: i64 = 5 * 60 * 1000;

/// A gap longer than this means the human stepped away, not that they were
/// slow to respond, so it is left out of response-latency figures.
pub const WAIT_SAMPLE_CAP_MS: i64 = 30 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct Derived {
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub active_ms: i64,
    pub model: Option<String>,
    pub tokens: TokenUsage,
    pub cost_usd: Option<f64>,
    pub cost_is_estimate: bool,
}

impl ParsedSession {
    pub fn is_empty(&self) -> bool {
        self.user_turns == 0 && self.assistant_turns == 0
    }

    pub fn note_time(&mut self, ts: Option<i64>) {
        if let Some(t) = ts {
            self.event_times.push(t);
        }
    }

    pub fn note_turn_end(&mut self, ts: Option<i64>) {
        if ts.is_some() {
            self.last_turn_end = ts;
        }
    }

    pub fn note_human_prompt(&mut self, ts: Option<i64>) {
        if let (Some(end), Some(t)) = (self.last_turn_end.take(), ts) {
            let gap = t - end;
            if (0..=WAIT_SAMPLE_CAP_MS).contains(&gap) {
                self.wait_samples.push(gap);
            }
        }
    }

    /// Records one tool call. `path` is the file it touched, when known.
    pub fn note_tool(&mut self, name: &str, path: Option<&str>) {
        self.tool_calls += 1;
        *self.tool_counts.entry(name.to_string()).or_default() += 1;
        let path = path.map(crate::util::path_key);
        match tool_kind(name) {
            ToolKind::Read => {
                self.reads += 1;
                if let Some(p) = path {
                    self.read_paths.insert(p);
                }
            }
            kind @ (ToolKind::Edit | ToolKind::Write) => {
                self.edits += 1;
                if let Some(p) = path {
                    let edited_before = self.edited_paths.contains(&p);
                    let written_before = self.written_paths.contains(&p);
                    if kind == ToolKind::Edit {
                        if !self.read_paths.contains(&p) && !edited_before && !written_before {
                            self.edits_without_read += 1;
                        }
                        // The first edit after creating a file is not rework.
                        if edited_before {
                            self.re_edits += 1;
                        }
                        self.edited_paths.insert(p.clone());
                    } else {
                        if edited_before || written_before {
                            self.re_edits += 1;
                        }
                        self.written_paths.insert(p.clone());
                    }
                    // The agent has seen what it wrote.
                    self.read_paths.insert(p);
                }
            }
            ToolKind::Shell => self.shell_calls += 1,
            ToolKind::Other => {}
        }
    }

    pub fn note_message(&mut self, text: &str) {
        let t = text.trim();
        if !t.is_empty() {
            self.last_message = Some(crate::util::truncate(t, 600));
        }
    }

    pub fn add_tokens(&mut self, model: &str, usage: &TokenUsage) {
        self.tokens_by_model
            .entry(model.to_string())
            .or_default()
            .add(usage);
    }

    pub fn derive(&self) -> Derived {
        let mut times = self.event_times.clone();
        times.sort_unstable();
        let active_ms = times
            .windows(2)
            .map(|w| (w[1] - w[0]).clamp(0, ACTIVE_GAP_CAP_MS))
            .sum();

        let model = self
            .model_turns
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(m, _)| m.clone());

        let mut tokens = TokenUsage::default();
        for usage in self.tokens_by_model.values() {
            tokens.add(usage);
        }

        let (cost_usd, cost_is_estimate) = match self.reported_cost_usd {
            Some(c) => (Some(c), false),
            None => {
                let mut total = 0.0;
                let mut any = false;
                for (m, usage) in &self.tokens_by_model {
                    if let Some(c) = pricing::estimate_cost(m, usage) {
                        total += c;
                        any = true;
                    }
                }
                (any.then_some(total), true)
            }
        };

        Derived {
            started_at: times.first().copied(),
            ended_at: times.last().copied(),
            active_ms,
            model,
            tokens,
            cost_usd,
            cost_is_estimate,
        }
    }
}
