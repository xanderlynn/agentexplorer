//! Where each tool keeps its logs, and how files group into sessions.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::Source;
use crate::util::{file_mtime_ms, home};

/// One session's worth of files. `paths[0]` is the primary file; its path is
/// the unit's key for change detection.
#[derive(Debug, Clone)]
pub struct Unit {
    pub source: Source,
    pub paths: Vec<PathBuf>,
}

impl Unit {
    pub fn key(&self) -> String {
        self.paths[0].to_string_lossy().into_owned()
    }

    /// Changes whenever any file in the unit grows or is rewritten.
    pub fn fingerprint(&self) -> String {
        let mut size = 0u64;
        let mut mtime = 0i64;
        for p in &self.paths {
            if let Ok(m) = std::fs::metadata(p) {
                size += m.len();
            }
            mtime = mtime.max(file_mtime_ms(p).unwrap_or(0));
        }
        format!("{size}:{mtime}:{}", self.paths.len())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Root {
    pub source: Source,
    pub path: PathBuf,
}

pub fn roots() -> Vec<Root> {
    let mut out = vec![];

    let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"));
    out.push(Root {
        source: Source::ClaudeCode,
        path: claude_home.join("projects"),
    });

    let copilot_home = std::env::var_os("COPILOT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".copilot"));
    out.push(Root {
        source: Source::CopilotCli,
        path: copilot_home.join("session-state"),
    });

    // dirs::config_dir(): ~/Library/Application Support, %APPDATA%, ~/.config.
    if let Some(cfg) = dirs::config_dir() {
        for flavor in ["Code", "Code - Insiders", "VSCodium"] {
            let user = cfg.join(flavor).join("User");
            out.push(Root {
                source: Source::CopilotVscode,
                path: user.join("workspaceStorage"),
            });
            out.push(Root {
                source: Source::CopilotVscode,
                path: user.join("globalStorage").join("emptyWindowChatSessions"),
            });
        }
    }
    out
}

pub fn discover(root: &Root) -> Vec<Unit> {
    let mut units = vec![];
    match root.source {
        Source::ClaudeCode => {
            for project in read_dirs(&root.path) {
                for f in read_files(&project) {
                    if is_ext(&f, "jsonl") {
                        units.push(claude_unit(f));
                    }
                }
            }
        }
        Source::CopilotCli => {
            for dir in read_dirs(&root.path) {
                let events = dir.join("events.jsonl");
                if events.is_file() {
                    units.push(Unit {
                        source: Source::CopilotCli,
                        paths: vec![events, dir.join("workspace.yaml")],
                    });
                }
            }
        }
        Source::CopilotVscode => {
            let mut push_chat_files = |dir: &Path| {
                for f in read_files(dir) {
                    if is_ext(&f, "json") || is_ext(&f, "jsonl") {
                        units.push(Unit {
                            source: Source::CopilotVscode,
                            paths: vec![f],
                        });
                    }
                }
            };
            if root.path.ends_with("emptyWindowChatSessions") {
                push_chat_files(&root.path);
            } else {
                for ws in read_dirs(&root.path) {
                    push_chat_files(&ws.join("chatSessions"));
                }
            }
        }
    }
    units
}

/// Maps a changed file back to the unit it belongs to.
pub fn unit_for_path(root: &Root, changed: &Path) -> Option<Unit> {
    let rel = changed.strip_prefix(&root.path).ok()?;
    let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_owned()).collect();
    match root.source {
        Source::ClaudeCode => {
            // <project>/<id>.jsonl  or  <project>/<id>/subagents/<agent>.jsonl
            let project = root.path.join(parts.first()?);
            let id = match parts.len() {
                2 if is_ext(changed, "jsonl") => Path::new(&parts[1]).file_stem()?.to_owned(),
                n if n >= 3 => parts[1].clone(),
                _ => return None,
            };
            let main = project.join(format!("{}.jsonl", id.to_string_lossy()));
            main.is_file().then(|| claude_unit(main))
        }
        Source::CopilotCli => {
            let dir = root.path.join(parts.first()?);
            let events = dir.join("events.jsonl");
            events.is_file().then(|| Unit {
                source: Source::CopilotCli,
                paths: vec![events, dir.join("workspace.yaml")],
            })
        }
        Source::CopilotVscode => {
            let in_chat_dir = changed
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "chatSessions" || n == "emptyWindowChatSessions");
            (in_chat_dir
                && (is_ext(changed, "json") || is_ext(changed, "jsonl"))
                && changed.is_file())
            .then(|| Unit {
                source: Source::CopilotVscode,
                paths: vec![changed.to_path_buf()],
            })
        }
    }
}

fn claude_unit(main: PathBuf) -> Unit {
    let mut paths = vec![main.clone()];
    if let (Some(dir), Some(stem)) = (main.parent(), main.file_stem()) {
        let mut subs: Vec<_> = read_files(&dir.join(stem).join("subagents"))
            .into_iter()
            .filter(|f| is_ext(f, "jsonl"))
            .collect();
        subs.sort();
        paths.extend(subs);
    }
    Unit {
        source: Source::ClaudeCode,
        paths,
    }
}

fn is_ext(p: &Path, ext: &str) -> bool {
    p.extension().and_then(|e| e.to_str()) == Some(ext)
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    entries(dir).into_iter().filter(|p| p.is_dir()).collect()
}

fn read_files(dir: &Path) -> Vec<PathBuf> {
    entries(dir).into_iter().filter(|p| p.is_file()).collect()
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_subagent_change_to_parent_session() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root {
            source: Source::ClaudeCode,
            path: dir.path().to_path_buf(),
        };
        let proj = dir.path().join("-work-app");
        std::fs::create_dir_all(proj.join("s1/subagents")).unwrap();
        std::fs::write(proj.join("s1.jsonl"), "{}").unwrap();
        let sub = proj.join("s1/subagents/agent-a.jsonl");
        std::fs::write(&sub, "{}").unwrap();

        let unit = unit_for_path(&root, &sub).unwrap();
        assert_eq!(unit.paths[0], proj.join("s1.jsonl"));
        assert_eq!(unit.paths.len(), 2);
        assert_eq!(discover(&root).len(), 1);
    }
}
