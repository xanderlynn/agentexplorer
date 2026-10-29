//! GitHub Copilot Chat in VS Code (and Insiders, VSCodium): one file per
//! chat in each workspace's `chatSessions/`, or in
//! `emptyWindowChatSessions/` for chats with no folder open.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{AgentSource, Resume, is_ext, read_dirs, read_files};
use crate::model::ParsedSession;
use crate::parsers::copilot_vscode as parser;

pub struct CopilotVscode;

impl AgentSource for CopilotVscode {
    fn id(&self) -> &'static str {
        "copilot_vscode"
    }

    fn label(&self) -> &'static str {
        "Copilot Chat (VS Code)"
    }

    fn short_label(&self) -> &'static str {
        "Copilot Chat"
    }

    fn roots(&self) -> Vec<PathBuf> {
        // dirs::config_dir(): ~/Library/Application Support, %APPDATA%, ~/.config.
        let Some(cfg) = dirs::config_dir() else {
            return vec![];
        };
        ["Code", "Code - Insiders", "VSCodium"]
            .iter()
            .flat_map(|flavor| {
                let user = cfg.join(flavor).join("User");
                [
                    user.join("workspaceStorage"),
                    user.join("globalStorage").join("emptyWindowChatSessions"),
                ]
            })
            .collect()
    }

    fn discover(&self, root: &Path) -> Vec<Vec<PathBuf>> {
        let dirs = if root.ends_with("emptyWindowChatSessions") {
            vec![root.to_path_buf()]
        } else {
            read_dirs(root)
                .iter()
                .map(|ws| ws.join("chatSessions"))
                .collect()
        };
        dirs.iter()
            .flat_map(|d| read_files(d))
            .filter(|f| is_chat_file(f))
            .map(|f| vec![f])
            .collect()
    }

    fn session_for(&self, _root: &Path, changed: &Path) -> Option<Vec<PathBuf>> {
        let in_chat_dir = changed
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "chatSessions" || n == "emptyWindowChatSessions");
        (in_chat_dir && is_chat_file(changed) && changed.is_file())
            .then(|| vec![changed.to_path_buf()])
    }

    /// Chat files are rewritten in place, so they are always read whole.
    fn parse(
        &self,
        paths: &[PathBuf],
        _resume: Option<Resume>,
    ) -> Result<(Option<ParsedSession>, Option<Resume>)> {
        Ok((parser::parse(paths)?, None))
    }
}

/// VS Code moved from `<id>.json` snapshots to `<id>.jsonl` patch logs; when
/// both exist for one session, the `.jsonl` is current.
fn is_chat_file(f: &Path) -> bool {
    is_ext(f, "jsonl") || (is_ext(f, "json") && !f.with_extension("jsonl").is_file())
}
