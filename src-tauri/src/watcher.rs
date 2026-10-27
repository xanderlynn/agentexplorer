//! Background ingestion: a full scan at startup and every few minutes, plus
//! filesystem events so the Live page updates within seconds.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::db;
use crate::ingest::{IngestReport, Ingestor};

pub const DATA_CHANGED: &str = "data-changed";
const FULL_SCAN_EVERY: Duration = Duration::from_secs(5 * 60);
/// Wait for writes to settle before parsing; agents append many lines a second.
const DEBOUNCE: Duration = Duration::from_millis(1200);
const MAX_BATCH_DELAY: Duration = Duration::from_secs(5);

pub fn start(app: AppHandle, db_path: PathBuf, data_dir: PathBuf) {
    std::thread::Builder::new()
        .name("ingest".into())
        .spawn(move || {
            if let Err(e) = run(&app, &db_path, &data_dir) {
                eprintln!("ingest thread stopped: {e:#}");
            }
        })
        .expect("spawn ingest thread");
}

fn emit(app: &AppHandle, report: &IngestReport) {
    let _ = app.emit(DATA_CHANGED, report);
}

fn run(app: &AppHandle, db_path: &Path, data_dir: &Path) -> anyhow::Result<()> {
    let mut conn = db::open(db_path)?;
    let ingestor = Ingestor::new(data_dir);

    // A failed first scan (say, the database is busy) must not stop live
    // updates; the periodic full scan retries it.
    match ingestor.ingest_all(&mut conn, false) {
        Ok(report) => emit(app, &report),
        Err(e) => eprintln!("initial scan: {e:#}"),
    }

    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut refresh_watches = |watcher: &mut notify::RecommendedWatcher,
                               conn: &rusqlite::Connection| {
        for root in ingestor.enabled_roots(conn) {
            if root.path.is_dir()
                && !watched.contains(&root.path)
                && watcher.watch(&root.path, RecursiveMode::Recursive).is_ok()
            {
                watched.insert(root.path);
            }
        }
    };
    refresh_watches(&mut watcher, &conn);

    let mut last_full = Instant::now();
    loop {
        let mut pending: HashSet<PathBuf> = HashSet::new();
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(ev)) => pending.extend(ev.paths),
            Ok(Err(_)) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if !pending.is_empty() {
            let batch_start = Instant::now();
            while batch_start.elapsed() < MAX_BATCH_DELAY {
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(Ok(ev)) => pending.extend(ev.paths),
                    Ok(Err(_)) => {}
                    Err(_) => break,
                }
            }
            let paths: Vec<PathBuf> = pending
                .into_iter()
                .filter(|p| {
                    matches!(
                        p.extension().and_then(|e| e.to_str()),
                        Some("jsonl" | "json" | "yaml")
                    )
                })
                .collect();
            if !paths.is_empty() {
                match ingestor.ingest_paths(&mut conn, &paths) {
                    Ok(r) if r.updated > 0 => emit(app, &r),
                    Ok(_) => {}
                    Err(e) => eprintln!("ingest: {e:#}"),
                }
            }
        }
        if last_full.elapsed() >= FULL_SCAN_EVERY {
            last_full = Instant::now();
            // New tools may have been installed since startup.
            refresh_watches(&mut watcher, &conn);
            match ingestor.ingest_all(&mut conn, false) {
                Ok(r) if r.changed => emit(app, &r),
                Ok(_) => {}
                Err(e) => eprintln!("full scan: {e:#}"),
            }
        }
    }
    Ok(())
}
