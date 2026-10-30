//! Keeps workstreams imported from an issue tracker current: while Tilly
//! runs, each one is fetched again once it is older than the interval in
//! Settings (15 minutes unless changed; 0 turns it off), one at a time, and
//! on launch and when the window is focused. A failed fetch is kept on the
//! workstream for its card to show, and tried again a full interval later.
//! Each round also asks gh whether recent sessions' pull requests merged.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::{db, workstreams};

pub const SETTING: &str = "workstreams.syncMinutes";
const DEFAULT_MINUTES: i64 = 15;
/// How often to look for workstreams that are due.
const TICK: Duration = Duration::from_secs(60);
/// The first look after launch, leaving the start-up scan the disk.
const FIRST: Duration = Duration::from_secs(20);
/// Pull requests from sessions this recent are checked for merging.
const PR_DAYS: i64 = 30;
/// Pull requests checked per round, so gh isn't flooded.
const PRS_PER_ROUND: usize = 20;

/// Wakes the sync loop early.
struct Wake(Sender<()>);

pub fn start(app: AppHandle, db_path: PathBuf) {
    let (tx, rx) = mpsc::channel();
    app.manage(Wake(tx));
    let spawned = std::thread::Builder::new()
        .name("tracker-sync".into())
        .spawn(move || {
            let mut wait = FIRST;
            loop {
                if let Err(RecvTimeoutError::Disconnected) = rx.recv_timeout(wait) {
                    break;
                }
                wait = TICK;
                if let Err(e) = round(&app, &db_path) {
                    tracing::warn!("workstream sync: {e:#}");
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("couldn't start workstream sync: {e}");
    }
}

/// Looks for due workstreams now, as when the window is focused.
pub fn wake(app: &AppHandle) {
    if let Some(w) = app.try_state::<Wake>() {
        let _ = w.0.send(());
    }
}

fn round(app: &AppHandle, db_path: &Path) -> anyhow::Result<()> {
    let conn = db::open(db_path)?;
    // Once a day: compact old session detail, compress and prune the archive.
    if tilly_core::retention::due(&conn) {
        let data_dir = app.state::<crate::AppState>().data_dir.clone();
        match tilly_core::retention::maintain(&conn, &data_dir) {
            Ok(p) => tracing::info!(
                "retention: compacted {} sessions, compressed {} transcripts, pruned {} archived sessions",
                p.compacted,
                p.compressed,
                p.pruned
            ),
            Err(e) => tracing::warn!("retention: {e:#}"),
        }
    }
    let minutes = db::get_setting(&conn, SETTING)?
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_MINUTES);
    if minutes <= 0 {
        return Ok(());
    }
    // Pull requests sessions opened: merged or not decides the suggested outcome.
    if tilly_core::trackers::has_gh() {
        for url in tilly_core::store::prs_to_check(&conn, PR_DAYS, PRS_PER_ROUND)? {
            // Unreadable (deleted, no access): note it, so it isn't asked again soon.
            let state = tilly_core::trackers::pr_state(&url).unwrap_or_else(|_| "OPEN".into());
            tilly_core::store::record_pr_state(&conn, &url, &state)?;
        }
    }
    let due = workstreams::due_for_sync(&conn, minutes * 60_000)?;
    for key in &due {
        // Kept on the workstream; logged here too.
        if let Err(e) = workstreams::sync_from_tracker(&conn, key) {
            tracing::info!("couldn't sync {key}: {e:#}");
        }
    }
    if !due.is_empty() {
        let _ = app.emit("workstreams-changed", ());
    }
    Ok(())
}
