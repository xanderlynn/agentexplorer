//! Steps that run after sessions are written, linking them to data outside
//! the transcripts: git commits, instruction files and prompt templates, and
//! journal intents. `ENRICHERS` runs in order; to link something new, add a
//! step and list it there.
//!
//! A full pass (`changed` is `None`) relinks every session; a watcher pass
//! only the sessions that changed.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::{git, journal, playbooks};

pub trait Enricher: Sync {
    fn name(&self) -> &'static str;
    fn run(&self, conn: &Connection, changed: Option<&[String]>) -> Result<()>;
}

pub const ENRICHERS: &[&dyn Enricher] = &[&Commits, &Playbooks, &Intents];

/// Runs every step in order; the first failure stops the pass.
pub fn run_all(conn: &Connection, changed: Option<&[String]>) -> Result<()> {
    for step in ENRICHERS {
        step.run(conn, changed)
            .with_context(|| format!("linking {}", step.name()))?;
    }
    Ok(())
}

/// Commits made during each session. Full passes only: shelling out to git
/// on every file event is too slow, so commits appear with the next full scan.
struct Commits;

impl Enricher for Commits {
    fn name(&self) -> &'static str {
        "commits"
    }

    fn run(&self, conn: &Connection, changed: Option<&[String]>) -> Result<()> {
        if changed.is_some() {
            return Ok(());
        }
        let (repos, _) = all_roots(conn)?;
        for worktrees in group_worktrees(&repos) {
            // A repo that moved or lost its .git should not stop the others.
            let _ = git::link_commits(conn, &worktrees);
        }
        Ok(())
    }
}

/// Instruction files and prompt templates in use. A full pass first syncs
/// the files' versions from git.
struct Playbooks;

impl Enricher for Playbooks {
    fn name(&self) -> &'static str {
        "playbooks"
    }

    fn run(&self, conn: &Connection, changed: Option<&[String]>) -> Result<()> {
        if changed.is_none() {
            let (_, roots) = all_roots(conn)?;
            playbooks::sync(conn, &roots)?;
        }
        playbooks::relink(conn, changed)
    }
}

/// Journal intents matched to the sessions that carried them out.
struct Intents;

impl Enricher for Intents {
    fn name(&self) -> &'static str {
        "intents"
    }

    fn run(&self, conn: &Connection, _changed: Option<&[String]>) -> Result<()> {
        journal::auto_link(conn)
    }
}

/// Distinct git roots and project roots across all sessions.
fn all_roots(conn: &Connection) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mut repos = BTreeSet::new();
    let mut roots = BTreeSet::new();
    let mut stmt = conn.prepare("SELECT DISTINCT repo_root, project_path FROM sessions")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
        ))
    })? {
        let (repo, project) = row?;
        if let Some(r) = &repo {
            repos.insert(r.clone());
        }
        if let Some(root) = repo.or(project) {
            roots.insert(root);
        }
    }
    Ok((repos, roots))
}

/// Groups worktrees of the same repository so their commits are read once.
fn group_worktrees(repos: &BTreeSet<String>) -> Vec<Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for repo in repos {
        if let Some(common) = git::common_dir(repo) {
            groups.entry(common).or_default().push(repo.clone());
        }
    }
    groups.into_values().collect()
}
