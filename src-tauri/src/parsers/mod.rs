pub mod claude;
pub mod copilot_cli;
pub mod copilot_vscode;

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{ParsedSession, Source};

/// Prompts are stored for analysis and the weekly retro; longer ones are cut.
pub const MAX_PROMPT_CHARS: usize = 4000;

/// What a parser has read so far, so the next pass over a growing
/// transcript reads only the lines appended since. Copilot Chat files are
/// rewritten in place, so they are always read whole.
#[derive(Clone)]
pub enum Resume {
    Claude(claude::State),
    CopilotCli(copilot_cli::State),
}

/// Parses a session, continuing from `resume` when given. Returns the
/// session and the state to continue from next time.
pub fn parse_from(
    source: Source,
    paths: &[PathBuf],
    resume: Option<Resume>,
) -> Result<(Option<ParsedSession>, Option<Resume>)> {
    Ok(match (source, resume) {
        (Source::ClaudeCode, r) => {
            let mut st = match r {
                Some(Resume::Claude(st)) if !st.rewritten(paths) => st,
                _ => claude::State::default(),
            };
            (claude::update(paths, &mut st)?, Some(Resume::Claude(st)))
        }
        (Source::CopilotCli, r) => {
            let mut st = match r {
                Some(Resume::CopilotCli(st)) if !st.rewritten(paths) => st,
                _ => copilot_cli::State::default(),
            };
            (
                copilot_cli::update(paths, &mut st)?,
                Some(Resume::CopilotCli(st)),
            )
        }
        (Source::CopilotVscode, _) => (copilot_vscode::parse(paths)?, None),
    })
}

pub(super) fn open_lines(path: &Path) -> Result<BufReader<File>> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    Ok(BufReader::with_capacity(256 * 1024, f))
}

/// Calls `f` with each line after byte `offset` and returns where the next
/// read should start. A last line still being written (no newline, not yet
/// valid JSON) is left for the next read.
fn read_from(path: &Path, offset: u64, mut f: impl FnMut(&str)) -> Result<u64> {
    let mut r = open_lines(path)?;
    r.seek(SeekFrom::Start(offset))?;
    let mut pos = offset;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = r.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let complete = buf.ends_with(b"\n");
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\n', '\r']);
        if !complete && serde_json::from_str::<serde_json::Value>(line).is_err() {
            break;
        }
        f(line);
        pos += n as u64;
    }
    Ok(pos)
}

/// True when a file is shorter than what was already read from it: it was
/// truncated or replaced, so reading must start over.
fn shrank(path: &Path, offset: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() < offset)
}

/// The parts of a session an incremental read must get right.
#[cfg(test)]
pub(crate) fn digest(s: &ParsedSession) -> String {
    let mut tokens: Vec<_> = s
        .tokens_by_model
        .iter()
        .map(|(m, u)| (m.clone(), format!("{u:?}")))
        .collect();
    tokens.sort();
    let agents: Vec<_> = s
        .team
        .agents
        .iter()
        .map(|a| (&a.id, a.status, a.tokens, a.tool_calls, &a.result))
        .collect();
    let msgs: Vec<_> = s
        .team
        .messages
        .iter()
        .map(|m| (&m.from, &m.to, m.kind, &m.text))
        .collect();
    format!(
        "{:?}",
        (
            (
                s.user_turns,
                s.assistant_turns,
                s.tool_calls,
                s.prompts.len(),
                tokens
            ),
            (agents, msgs, &s.timeline, &s.skills, &s.mcp),
            (s.failed, s.last_event, s.subagents, &s.title),
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_only_complete_new_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        std::fs::write(&p, "{\"a\":1}\n{\"a\":2}\n{\"a\":").unwrap();
        let mut seen = vec![];
        let off = read_from(&p, 0, |l| seen.push(l.to_string())).unwrap();
        assert_eq!(seen, ["{\"a\":1}", "{\"a\":2}"]);
        // The rest of the half-written line arrives; only it is read.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"3}\n")
            .unwrap();
        seen.clear();
        read_from(&p, off, |l| seen.push(l.to_string())).unwrap();
        assert_eq!(seen, ["{\"a\":3}"]);
        assert!(shrank(&p, 1_000));
    }
}
