mod background;
mod commands;
mod retro;
mod sound;

// The engine lives in governess-core; these keep `crate::analytics` and
// friends working in the app's own modules.
pub(crate) use governess_core::{
    alerts, analytics, db, experiments, ingest, journal, live, model, playbooks, sources,
    status_alerts, toolkit, trends, util, watcher,
};

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

use governess_core::LEGACY_IDENTIFIER;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let legacy = data_dir.with_file_name(LEGACY_IDENTIFIER);
            if !data_dir.exists() && legacy.is_dir() {
                // Same parent folder, so this is a cheap rename. On failure
                // the app starts fresh and the old folder is left untouched.
                if let Err(e) = std::fs::rename(&legacy, &data_dir) {
                    eprintln!("could not move {}: {e}", legacy.display());
                }
            }
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("ledger.sqlite3");
            let conn = db::open(&db_path)?;
            app.manage(AppState {
                db: Mutex::new(conn),
                db_path: db_path.clone(),
                data_dir: data_dir.clone(),
            });
            background::build_tray(app.handle())?;
            // `--background` starts in the tray with no window, as at login.
            if std::env::args().any(|a| a == "--background") {
                background::window_closed(app.handle());
            } else {
                background::open_window(app.handle());
            }
            watcher::start(
                background::AppSink::new(app.handle().clone(), data_dir.clone()),
                db_path,
                data_dir,
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == background::MAIN && matches!(event, tauri::WindowEvent::Destroyed)
            {
                background::window_closed(window.app_handle());
            }
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
            commands::get_execution,
            commands::get_anomalies,
            commands::get_failure_clusters,
            commands::get_timelines,
            commands::get_fit,
            commands::get_tool_health,
            commands::get_patterns,
            commands::export_sessions,
            commands::list_sessions,
            commands::get_session,
            commands::save_review,
            commands::get_live,
            commands::list_teams,
            commands::import_alert_asset,
            commands::read_alert_asset,
            commands::delete_alert_asset,
            commands::get_alert_profile,
            commands::play_alert_sound,
            commands::get_toolkit,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Closing the window keeps Governess running in the tray; only
            // Quit (which passes an exit code) ends it.
            tauri::RunEvent::ExitRequested {
                api, code: None, ..
            } => api.prevent_exit(),
            // macOS: clicking the Dock icon with no window open.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => background::open_window(app),
            _ => {}
        });
}
