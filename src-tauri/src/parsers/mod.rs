pub mod claude;
pub mod copilot_cli;
pub mod copilot_vscode;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{ParsedSession, Source};

/// Prompts are stored for analysis and the weekly retro; longer ones are cut.
pub const MAX_PROMPT_CHARS: usize = 4000;

pub fn parse(source: Source, paths: &[PathBuf]) -> Result<Option<ParsedSession>> {
    match source {
        Source::ClaudeCode => claude::parse(paths),
        Source::CopilotCli => copilot_cli::parse(paths),
        Source::CopilotVscode => copilot_vscode::parse(paths),
    }
}

fn open_lines(path: &Path) -> Result<BufReader<File>> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    Ok(BufReader::with_capacity(256 * 1024, f))
}
