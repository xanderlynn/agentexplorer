//! Tilly's engine, shared by the desktop app and (later) the headless
//! collector: it finds and parses AI-agent logs, stores sessions in SQLite,
//! tracks each session's live status, and computes the read models the UI
//! shows. Nothing here depends on a window.

/// The app's earlier identifiers, newest first (Governess, then Execution
/// Ledger). The newest data folder that exists is moved to the current one
/// on first launch.
pub const LEGACY_IDENTIFIERS: &[&str] =
    &["com.xanderlynn.governess", "com.xanderlynn.executionledger"];

pub mod alerts;
pub mod analytics;
pub mod codegen;
pub mod db;
pub mod enrich;
pub mod experiments;
pub mod git;
pub mod ingest;
pub mod journal;
pub mod live;
pub mod model;
pub mod parsers;
pub mod playbooks;
pub mod pricing;
pub mod signals;
pub mod sources;
pub mod stats;
pub mod status_alerts;
pub mod store;
pub mod team;
pub mod toolkit;
pub mod trends;
pub mod util;
pub mod watcher;
