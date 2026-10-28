//! GitHub Copilot CLI: one folder per session in `~/.copilot/session-state/`,
//! holding `events.jsonl` and `workspace.yaml`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{AgentSource, Resume, read_dirs, rel_parts, resume_state};
use crate::model::ParsedSession;
use crate::parsers::copilot_cli as parser;
use crate::util::home;

pub struct CopilotCli;

impl AgentSource for CopilotCli {
    fn id(&self) -> &'static str {
        "copilot_cli"
    }

    fn label(&self) -> &'static str {
        "Copilot CLI"
    }

    fn roots(&self) -> Vec<PathBuf> {
        let config = std::env::var_os("COPILOT_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".copilot"));
        vec![config.join("session-state")]
    }

    fn discover(&self, root: &Path) -> Vec<Vec<PathBuf>> {
        read_dirs(root)
            .iter()
            .filter_map(|dir| session_files(dir))
            .collect()
    }

    fn session_for(&self, root: &Path, changed: &Path) -> Option<Vec<PathBuf>> {
        let parts = rel_parts(root, changed)?;
        session_files(&root.join(parts.first()?))
    }

    fn parse(
        &self,
        paths: &[PathBuf],
        resume: Option<Resume>,
    ) -> Result<(Option<ParsedSession>, Option<Resume>)> {
        let mut st = resume_state(resume, |st: &parser::State| st.rewritten(paths));
        Ok((parser::update(paths, &mut st)?, Some(Box::new(st))))
    }

    /// The session folder's name.
    fn session_id<'a>(&self, main: &'a Path) -> Option<&'a OsStr> {
        main.parent().and_then(|d| d.file_name())
    }

    fn resume_command(&self, _project: Option<&str>, external_id: &str) -> Option<String> {
        Some(format!("copilot --resume {external_id}"))
    }
}

fn session_files(dir: &Path) -> Option<Vec<PathBuf>> {
    let events = dir.join("events.jsonl");
    events
        .is_file()
        .then(|| vec![events, dir.join("workspace.yaml")])
}
