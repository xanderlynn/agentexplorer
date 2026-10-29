//! The agent tools Tilly reads. Each one is a plug-in behind `AgentSource`:
//! where the tool keeps its logs, how its files group into sessions, and how
//! to parse them. To support another tool, add a module that implements the
//! trait and list it in `Source::ALL`.

use std::any::Any;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::ParsedSession;
use crate::util::file_mtime_ms;

mod claude_code;
mod copilot_cli;
mod copilot_vscode;

pub const CLAUDE_CODE: Source = Source(&claude_code::ClaudeCode);
pub const COPILOT_CLI: Source = Source(&copilot_cli::CopilotCli);
pub const COPILOT_VSCODE: Source = Source(&copilot_vscode::CopilotVscode);

/// What a parser has read so far, so the next pass over a growing
/// transcript reads only the lines appended since. Each source stores its
/// own state type; one that always reads whole files returns none.
pub type Resume = Box<dyn Any + Send>;

/// One agent tool.
pub trait AgentSource: Sync {
    /// Stored in the database, settings and archive paths: never change it.
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// For tight spaces: tables, charts and CSV exports.
    fn short_label(&self) -> &'static str {
        self.label()
    }
    /// Folders the tool writes its logs to; ones that don't exist are skipped.
    fn roots(&self) -> Vec<PathBuf>;
    /// Every session under `root`, as its files with the primary one first.
    fn discover(&self, root: &Path) -> Vec<Vec<PathBuf>>;
    /// The files of the session that `changed`, a file under `root`, belongs to.
    fn session_for(&self, root: &Path, changed: &Path) -> Option<Vec<PathBuf>>;
    /// Parses a session, continuing from `resume` when given. Returns the
    /// session and the state to continue from next time.
    fn parse(
        &self,
        paths: &[PathBuf],
        resume: Option<Resume>,
    ) -> Result<(Option<ParsedSession>, Option<Resume>)>;
    /// The tool's own id for a session, from its primary file. Names the
    /// session's archive folder.
    fn session_id<'a>(&self, main: &'a Path) -> Option<&'a OsStr> {
        main.file_stem()
    }
    /// The shell command that reopens a session, when the tool has one.
    fn resume_command(&self, _project: Option<&str>, _external_id: &str) -> Option<String> {
        None
    }
    /// Whether the tool reads instruction files of this kind (see
    /// `playbooks::INSTRUCTION_FILES`).
    fn reads_instructions(&self, kind: &str) -> bool {
        kind != "claude_md"
    }
}

/// A supported tool: a handle to its plug-in, serialized as its id.
#[derive(Clone, Copy)]
pub struct Source(&'static dyn AgentSource);

impl Source {
    /// Every supported tool, in the order Settings lists them.
    pub const ALL: &[Source] = &[CLAUDE_CODE, COPILOT_CLI, COPILOT_VSCODE];

    pub fn from_id(id: &str) -> Option<Source> {
        Source::ALL.iter().copied().find(|s| s.id() == id)
    }

    pub fn as_str(&self) -> &'static str {
        self.id()
    }
}

impl std::ops::Deref for Source {
    type Target = dyn AgentSource;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for Source {}

impl Hash for Source {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl Serialize for Source {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let id = String::deserialize(d)?;
        Source::from_id(&id).ok_or_else(|| serde::de::Error::custom(format!("unknown source {id}")))
    }
}

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

    /// Changes whenever any file in the unit grows or is rewritten, or the
    /// parsers start recording new signals.
    pub fn fingerprint(&self) -> String {
        let mut size = 0u64;
        let mut mtime = 0i64;
        for p in &self.paths {
            if let Ok(m) = std::fs::metadata(p) {
                size += m.len();
            }
            mtime = mtime.max(file_mtime_ms(p).unwrap_or(0));
        }
        let version = crate::ingest::SIGNALS_VERSION;
        format!("v{version}:{size}:{mtime}:{}", self.paths.len())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Root {
    pub source: Source,
    pub path: PathBuf,
}

pub fn roots() -> Vec<Root> {
    Source::ALL
        .iter()
        .flat_map(|&source| {
            source
                .roots()
                .into_iter()
                .map(move |path| Root { source, path })
        })
        .collect()
}

pub fn discover(root: &Root) -> Vec<Unit> {
    root.source
        .discover(&root.path)
        .into_iter()
        .map(|paths| Unit {
            source: root.source,
            paths,
        })
        .collect()
}

/// Maps a changed file back to the unit it belongs to.
pub fn unit_for_path(root: &Root, changed: &Path) -> Option<Unit> {
    changed.strip_prefix(&root.path).ok()?;
    let paths = root.source.session_for(&root.path, changed)?;
    Some(Unit {
        source: root.source,
        paths,
    })
}

/// The saved parse state, unless it belongs to another parser or its files
/// were rewritten since; then a fresh one.
fn resume_state<S: Default + 'static>(
    resume: Option<Resume>,
    rewritten: impl FnOnce(&S) -> bool,
) -> S {
    resume
        .and_then(|r| r.downcast::<S>().ok())
        .map(|st| *st)
        .filter(|st| !rewritten(st))
        .unwrap_or_default()
}

/// The components of `changed` below `root`.
fn rel_parts(root: &Path, changed: &Path) -> Option<Vec<OsString>> {
    let rel = changed.strip_prefix(root).ok()?;
    Some(rel.components().map(|c| c.as_os_str().to_owned()).collect())
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
            source: CLAUDE_CODE,
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

    #[test]
    fn ids_are_unique_and_round_trip() {
        let mut ids: Vec<_> = Source::ALL.iter().map(|s| s.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Source::ALL.len());
        for &s in Source::ALL {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(json, format!("\"{}\"", s.id()));
            assert_eq!(serde_json::from_str::<Source>(&json).unwrap(), s);
        }
        assert!(serde_json::from_str::<Source>("\"nope\"").is_err());
    }
}
