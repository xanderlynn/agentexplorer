//! The background loop: a full scan at startup and every few minutes,
//! filesystem events so changes land within seconds, and the live state:
//! after every pass, and on a timer for changes that come with time alone
//! (a quiet session becoming a long task), it recomputes the live summary,
//! pushes it to the `Sink` only when it changed, and reports status changes
//! worth an alert.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::db;
use crate::ingest::{IngestReport, Ingestor};
use crate::live::{self, Group, LiveSummary};
use crate::status_alerts::{Alert, Tracker};

/// Where the loop reports. The desktop app forwards to its window, tray,
/// notifications and sounds; a headless collector would send it upstream.
pub trait Sink: Send + 'static {
    /// Stored data changed: anything showing history should refresh.
    fn data_changed(&self, report: &IngestReport);
    /// The live summary changed.
    fn live(&self, summary: &LiveSummary);
    /// Status changes worth an alert, most urgent first.
    fn alerts(&self, alerts: Vec<Alert>);
}

/// How often the live state is re-checked while something is running or
/// waiting on you, and when nothing is.
const TICK_ACTIVE: Duration = Duration::from_secs(5);
const TICK_IDLE: Duration = Duration::from_secs(30);
const FULL_SCAN_EVERY: Duration = Duration::from_secs(5 * 60);
/// Wait for writes to settle before parsing; agents append many lines a second.
const DEBOUNCE: Duration = Duration::from_millis(1200);
const MAX_BATCH_DELAY: Duration = Duration::from_secs(5);

pub fn start(sink: impl Sink, db_path: PathBuf, data_dir: PathBuf) {
    std::thread::Builder::new()
        .name("ingest".into())
        .spawn(move || {
            if let Err(e) = run(&sink, &db_path, &data_dir) {
                eprintln!("ingest thread stopped: {e:#}");
            }
        })
        .expect("spawn ingest thread");
}

/// The last live summary pushed, and the status tracker.
struct LiveState {
    last: String,
    tracker: Tracker,
    active: bool,
}

impl LiveState {
    /// Recomputes the summary; pushes it if it changed and reports alerts.
    fn refresh(&mut self, conn: &rusqlite::Connection, sink: &impl Sink) {
        let summary = match live::summary(conn) {
            Ok(s) => s,
            Err(e) => return eprintln!("live summary: {e:#}"),
        };
        self.active = summary
            .sessions
            .iter()
            .any(|s| matches!(s.status.group(), Group::Active | Group::Awaiting));
        let all = summary.sessions.iter().chain(&summary.completed_today);
        let alerts = self.tracker.update(all);
        if !alerts.is_empty() {
            sink.alerts(alerts);
        }
        let json = serde_json::to_string(&summary).unwrap_or_default();
        if json != self.last {
            sink.live(&summary);
            self.last = json;
        }
    }
}

fn run(sink: &impl Sink, db_path: &Path, data_dir: &Path) -> anyhow::Result<()> {
    let mut conn = db::open(db_path)?;
    let ingestor = Ingestor::new(data_dir);
    let mut state = LiveState {
        last: String::new(),
        tracker: Tracker::default(),
        active: false,
    };
    let emit = |r: &IngestReport| sink.data_changed(r);

    // A failed first scan (say, the database is busy) must not stop live
    // updates; the periodic full scan retries it.
    match ingestor.ingest_all(&mut conn, false) {
        Ok(report) => emit(&report),
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
    state.refresh(&conn, sink);

    let mut last_full = Instant::now();
    loop {
        let mut pending: HashSet<PathBuf> = HashSet::new();
        // Look again soon while something runs, waits on you, or a change is
        // waiting to be confirmed.
        let tick = if state.active || state.tracker.has_pending() {
            TICK_ACTIVE
        } else {
            TICK_IDLE
        };
        match rx.recv_timeout(tick) {
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
                    Ok(r) if r.updated > 0 => emit(&r),
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
                Ok(r) if r.changed => emit(&r),
                Ok(_) => {}
                Err(e) => eprintln!("full scan: {e:#}"),
            }
        }
        state.refresh(&conn, sink);
    }
    Ok(())
}
