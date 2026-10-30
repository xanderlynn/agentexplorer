//! Claude Code: one JSONL transcript per session in
//! `~/.claude/projects/<project>/`, and each sub-agent's transcript in
//! `<session>/subagents/`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{AgentSource, Resume, is_ext, read_dirs, read_files, rel_parts, resume_state};
use crate::model::ParsedSession;
use crate::parsers::claude as parser;
use crate::util::home;

pub struct ClaudeCode;

impl AgentSource for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude_code"
    }

    fn label(&self) -> &'static str {
        "Claude Code"
    }

    fn roots(&self) -> Vec<PathBuf> {
        let config = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".claude"));
        vec![config.join("projects")]
    }

    fn logs_in(&self) -> Option<&'static str> {
        Some("projects")
    }

    fn discover(&self, root: &Path) -> Vec<Vec<PathBuf>> {
        read_dirs(root)
            .iter()
            .flat_map(|project| read_files(project))
            .filter(|f| is_ext(f, "jsonl"))
            .map(session_files)
            .collect()
    }

    fn session_for(&self, root: &Path, changed: &Path) -> Option<Vec<PathBuf>> {
        // <project>/<id>.jsonl  or  <project>/<id>/subagents/<agent>.jsonl
        let parts = rel_parts(root, changed)?;
        let project = root.join(parts.first()?);
        let id = match parts.len() {
            2 if is_ext(changed, "jsonl") => Path::new(&parts[1]).file_stem()?.to_owned(),
            n if n >= 3 => parts[1].clone(),
            _ => return None,
        };
        let main = project.join(format!("{}.jsonl", id.to_string_lossy()));
        main.is_file().then(|| session_files(main))
    }

    fn parse(
        &self,
        paths: &[PathBuf],
        resume: Option<Resume>,
    ) -> Result<(Option<ParsedSession>, Option<Resume>)> {
        let mut st = resume_state(resume, |st: &parser::State| st.rewritten(paths));
        Ok((parser::update(paths, &mut st)?, Some(Box::new(st))))
    }

    /// Claude Code resumes from the session's folder.
    fn resume_command(&self, project: Option<&str>, external_id: &str) -> Option<String> {
        Some(match project {
            Some(p) => format!(
                "cd \"{}\" && claude --resume {external_id}",
                p.replace('"', "\\\"")
            ),
            None => format!("claude --resume {external_id}"),
        })
    }

    fn reads_instructions(&self, kind: &str) -> bool {
        !matches!(kind, "agents_md" | "copilot_instructions")
    }
}

/// A transcript and its sub-agents' transcripts, sorted.
fn session_files(main: PathBuf) -> Vec<PathBuf> {
    let mut paths = vec![main.clone()];
    if let (Some(dir), Some(stem)) = (main.parent(), main.file_stem()) {
        let mut subs: Vec<_> = read_files(&dir.join(stem).join("subagents"))
            .into_iter()
            .filter(|f| is_ext(f, "jsonl"))
            .collect();
        subs.sort();
        paths.extend(subs);
    }
    paths
}
