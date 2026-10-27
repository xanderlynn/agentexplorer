mod analytics;
mod commands;
mod db;
mod experiments;
mod git;
mod ingest;
mod journal;
mod live;
mod model;
mod parsers;
mod playbooks;
mod pricing;
mod retro;
mod signals;
mod sources;
mod trends;
mod util;
mod watcher;

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

pub struct AppState {
    /// Connection for quick UI reads and writes. Long jobs open their own.
    pub db: Mutex<Connection>,
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("ledger.sqlite3");
            let conn = db::open(&db_path)?;
            app.manage(AppState {
                db: Mutex::new(conn),
                db_path: db_path.clone(),
                data_dir: data_dir.clone(),
            });
            watcher::start(app.handle().clone(), db_path, data_dir);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::quick_review,
            commands::list_metrics,
            commands::list_experiments,
            commands::create_experiment,
            commands::experiment_verdict,
            commands::delete_experiment,
            commands::set_experiment_applied,
            commands::list_goals,
            commands::create_goal,
            commands::retire_goal,
            commands::preflight,
            commands::get_insights,
            commands::get_scorecard,
            commands::get_usage,
            commands::get_reality,
            commands::get_fit,
            commands::get_tool_health,
            commands::get_patterns,
            commands::export_sessions,
            commands::list_sessions,
            commands::get_session,
            commands::save_review,
            commands::get_live,
            commands::rescan,
            commands::get_sources,
            commands::set_source_enabled,
            commands::list_projects,
            commands::list_intents,
            commands::create_intent,
            commands::close_intent,
            commands::reopen_intent,
            commands::delete_intent,
            commands::link_intent,
            commands::get_calibration,
            commands::list_playbooks,
            commands::create_prompt_template,
            commands::update_prompt_template,
            commands::delete_playbook,
            commands::set_session_playbook,
            commands::list_retros,
            commands::preview_retro,
            commands::generate_retro,
            commands::delete_retro,
            commands::get_settings,
            commands::set_setting,
            commands::set_api_key,
            commands::clear_api_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
