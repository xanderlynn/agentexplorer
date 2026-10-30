//! Normalized representation of one AI session, independent of which tool wrote it.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::pricing;

pub use crate::sources::Source;

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
    /// Sub-agents, their messages, and work scheduled for later.
    pub team: crate::team::Team,
    /// The task list its agents keep, and their progress notes.
    pub tasks: crate::tasks::TaskList,
    /// Pull requests the session opened or worked on.
    pub prs: Vec<String>,
    /// Tool calls the lead started that have no result yet: (call id, tool).
    pub pending: Vec<(String, String)>,
    /// The latest turn ended in an error (API error, output limit).
    pub failed: bool,
    /// Skills used, by name, whether the agent or you invoked them.
    pub skills: BTreeMap<String, u64>,
    /// MCP tool calls per server.
    pub mcp: BTreeMap<String, u64>,
    /// The agent's most recent text reply, for "what is it asking me?".
    pub last_message: Option<String>,
    /// Failed calls per tool name (excluding calls the user rejected).
    pub tool_errors_by: BTreeMap<String, u64>,
    /// Calls the user rejected or denied, per tool or permission kind.
    pub tool_vetoes: BTreeMap<String, u64>,
    /// Tool name per call id, to attribute results and errors.
    pub tool_names: HashMap<String, String>,
    /// Time the agent spent working on your prompts (prompt to turn end).
    pub agent_ms: i64,
    /// Working (1) and waiting-on-you (0) spans: (start, end, kind).
    pub timeline: Vec<(i64, i64, u8)>,
    pub turn_start: Option<i64>,
    /// Tool kinds used in the current turn; classifies the turn when tool
    /// calls carry no timestamps (VS Code).
    pub turn_tools: Vec<ToolKind>,
    /// The open stretch of the current turn: its start and mode. A new
    /// stretch starts whenever a timed tool call changes the mode.
    pub stretch: Option<(i64, Mode)>,
    /// Agent working time per mode.
    pub mode_ms: BTreeMap<String, i64>,
    /// Plan mode was used (Claude Code plan mode, a Copilot plan).
    pub planned: bool,
    /// Largest and most recent context size sent to the model, in tokens.
    pub peak_context: u64,
    pub last_context: u64,
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
    /// Web search and fetch.
    Web,
    Other,
}

/// What a turn of agent work was mostly doing, judged by its tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Changed files.
    Coding = 1,
    /// Only read, searched or browsed.
    Research = 2,
    /// Ran commands (tests, builds) without changing files.
    Running = 3,
    /// No tools: answering, planning, explaining.
    Discussion = 4,
}

impl Mode {
    pub const ALL: [Mode; 4] = [
        Mode::Coding,
        Mode::Research,
        Mode::Running,
        Mode::Discussion,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Mode::Coding => "coding",
            Mode::Research => "research",
            Mode::Running => "running",
            Mode::Discussion => "discussion",
        }
    }
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
        _ if n.contains("web") || n.starts_with("fetch") => ToolKind::Web,
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
        self.pending.clear();
        let Some(t) = ts else { return };
        self.last_turn_end = Some(t);
        if let Some(start) = self.turn_start.take()
            && t >= start
        {
            self.agent_ms += t - start;
            self.close_stretch(start, t, true);
        }
        self.turn_tools.clear();
        self.stretch = None;
    }

    pub fn note_pending(&mut self, id: &str, tool: &str) {
        self.pending.push((id.to_string(), tool.to_string()));
    }

    pub fn note_resolved(&mut self, id: &str) {
        self.pending.retain(|(x, _)| x != id);
    }

    /// The tool the lead is waiting on, while its turn is still open.
    pub fn pending_tool(&self) -> Option<&str> {
        (self.last_event == Some(LastEvent::Working))
            .then(|| self.pending.last().map(|(_, t)| t.as_str()))
            .flatten()
    }

    /// Ends the open stretch at `end`, recording it as a span (and, when
    /// `count`, as time in its mode). Without timed tool calls the whole
    /// turn gets the turn's overall mode.
    fn close_stretch(&mut self, turn_start: i64, end: i64, count: bool) {
        let (start, mode) = self.stretch.unwrap_or((turn_start, self.turn_mode()));
        if end > start {
            if count {
                *self.mode_ms.entry(mode.key().into()).or_default() += end - start;
            }
            self.push_span(start, end, mode as u8);
        }
    }

    pub fn note_human_prompt(&mut self, ts: Option<i64>) {
        self.failed = false;
        if let (Some(end), Some(t)) = (self.last_turn_end.take(), ts) {
            let gap = t - end;
            if (0..=WAIT_SAMPLE_CAP_MS).contains(&gap) {
                self.wait_samples.push(gap);
                self.push_span(end, t, 0);
            }
        }
        // A prompt queued while the agent is still working joins that turn.
        if self.turn_start.is_none() {
            self.turn_start = ts;
        }
    }

    /// Spans beyond this are dropped; the timeline is a sketch, not a log.
    const MAX_SPANS: usize = 400;

    fn push_span(&mut self, start: i64, end: i64, kind: u8) {
        if self.timeline.len() < Self::MAX_SPANS && end > start {
            self.timeline.push((start, end, kind));
        }
    }

    pub fn note_tool_error(&mut self, name: &str) {
        *self.tool_errors_by.entry(name.to_string()).or_default() += 1;
    }

    pub fn note_veto(&mut self, name: &str) {
        *self.tool_vetoes.entry(name.to_string()).or_default() += 1;
    }

    /// Records one tool call. `path` is the file it touched, when known.
    /// Records one tool call. `path` is the file it touched and `ts` when
    /// it was made, when known.
    pub fn note_tool(&mut self, name: &str, path: Option<&str>, ts: Option<i64>) {
        self.tool_calls += 1;
        *self.tool_counts.entry(name.to_string()).or_default() += 1;
        let path = path.map(crate::util::path_key);
        let kind = tool_kind(name);
        if !self.turn_tools.contains(&kind) {
            self.turn_tools.push(kind);
        }
        if let (Some(turn_start), Some(t)) = (self.turn_start, ts) {
            self.switch_stretch(turn_start, t, kind);
        }
        match kind {
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
            ToolKind::Web | ToolKind::Other => {}
        }
    }

    pub fn note_message(&mut self, text: &str) {
        let t = text.trim();
        if !t.is_empty() {
            self.last_message = Some(crate::util::truncate(t, 600));
        }
    }

    pub fn note_skill(&mut self, name: &str) {
        let name = name.trim().trim_start_matches('/');
        if !name.is_empty() {
            *self.skills.entry(name.to_string()).or_default() += 1;
        }
    }

    pub fn note_mcp(&mut self, server: &str) {
        if !server.is_empty() {
            *self.mcp.entry(server.to_string()).or_default() += 1;
        }
    }

    pub fn add_tokens(&mut self, model: &str, usage: &TokenUsage) {
        self.tokens_by_model
            .entry(model.to_string())
            .or_default()
            .add(usage);
    }

    /// Closes a turn still in progress at the end of the transcript, so
    /// the timeline shows current work. Call once, after parsing.
    pub fn close_open_turn(&mut self) {
        let last = self.event_times.iter().copied().max();
        if let (Some(start), Some(end)) = (self.turn_start, last)
            && end > start
        {
            self.close_stretch(start, end, false);
        }
    }

    /// Time up to a tool call belongs to the stretch it ends; the first
    /// tool's mode also claims the time before it (the agent deciding to
    /// call it). Tools with no clear mode continue the current stretch.
    fn switch_stretch(&mut self, turn_start: i64, t: i64, kind: ToolKind) {
        let mode = match kind {
            ToolKind::Edit | ToolKind::Write => Mode::Coding,
            ToolKind::Shell => Mode::Running,
            ToolKind::Read | ToolKind::Web => Mode::Research,
            ToolKind::Other => match self.stretch {
                Some((_, m)) => m,
                None => return,
            },
        };
        match self.stretch {
            None => self.stretch = Some((turn_start, mode)),
            Some((start, current)) if current != mode && t > start => {
                *self.mode_ms.entry(current.key().into()).or_default() += t - start;
                self.push_span(start, t, current as u8);
                self.stretch = Some((t, mode));
            }
            _ => {}
        }
    }

    /// Coding if the turn changed files, else running if it ran commands,
    /// else research if it read or browsed, else discussion.
    fn turn_mode(&self) -> Mode {
        let used = |k: ToolKind| self.turn_tools.contains(&k);
        if used(ToolKind::Edit) || used(ToolKind::Write) {
            Mode::Coding
        } else if used(ToolKind::Shell) {
            Mode::Running
        } else if used(ToolKind::Read) || used(ToolKind::Web) {
            Mode::Research
        } else {
            Mode::Discussion
        }
    }

    pub fn note_context(&mut self, tokens: u64) {
        if tokens > 0 {
            self.peak_context = self.peak_context.max(tokens);
            self.last_context = tokens;
        }
    }

    #[cfg(test)]
    pub fn mode_of(kinds: &[ToolKind]) -> Mode {
        ParsedSession {
            turn_tools: kinds.to_vec(),
            ..Default::default()
        }
        .turn_mode()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_turn_into_stretches_by_tool() {
        let mut s = ParsedSession::default();
        s.note_human_prompt(Some(0));
        s.note_tool("Read", Some("a.rs"), Some(1_000));
        s.note_tool("Grep", None, Some(5_000));
        s.note_tool("Edit", Some("a.rs"), Some(20_000));
        s.note_tool("Bash", None, Some(22_000));
        s.note_turn_end(Some(30_000));
        assert_eq!(s.mode_ms.get("research"), Some(&20_000));
        assert_eq!(s.mode_ms.get("coding"), Some(&2_000));
        assert_eq!(s.mode_ms.get("running"), Some(&8_000));
        assert_eq!(s.agent_ms, 30_000);
        assert_eq!(s.timeline.len(), 3);
    }

    #[test]
    fn classifies_turns_by_strongest_tool() {
        assert_eq!(
            ParsedSession::mode_of(&[ToolKind::Read, ToolKind::Edit]),
            Mode::Coding
        );
        assert_eq!(
            ParsedSession::mode_of(&[ToolKind::Read, ToolKind::Shell]),
            Mode::Running
        );
        assert_eq!(ParsedSession::mode_of(&[ToolKind::Web]), Mode::Research);
        assert_eq!(ParsedSession::mode_of(&[]), Mode::Discussion);
        assert_eq!(tool_kind("WebFetch"), ToolKind::Web);
        assert_eq!(tool_kind("fetch_copilot_cli_documentation"), ToolKind::Web);
    }
}
