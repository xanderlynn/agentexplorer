//! The Tauri command surface. Each command is a thin wrapper; the logic
//! lives in the domain modules so it can be tested without a window.
//!
//! Every command is `async` (runs on a worker thread) so that waiting on a
//! database the ingester is writing never freezes the window.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::analytics::{self, Insight, Overview, Review, Scorecard, SessionDetail};
use crate::experiments::{
    self, Experiment, GoalStatus, MetricDef, NewExperiment, NewGoal, PreflightAdvice,
};
use crate::ingest::{IngestReport, Ingestor, source_setting};
use crate::journal::{self, Calibration, CloseIntent, Intent, NewIntent};
use crate::live::{self, LiveSummary};
use crate::model::Source;
use crate::playbooks::{self, PlaybookStats};
use crate::retro::{self, RetroRow};
use crate::store::{SessionFilter, SessionRow};
use crate::{AppState, db, sources, store, trends};

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn with_db<T>(
    state: &State<AppState>,
    f: impl FnOnce(&Connection) -> anyhow::Result<T>,
) -> CmdResult<T> {
    let conn = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    f(&conn).map_err(err)
}

/// Long work gets its own connection on a blocking thread so the UI's
/// connection stays free for reads.
async fn blocking<T: Send + 'static>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&mut Connection, &std::path::Path) -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    let db_path = state.db_path.clone();
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = db::open(&db_path)?;
        f(&mut conn, &data_dir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)
}

#[tauri::command(async)]
pub fn get_overview(state: State<AppState>, days: i64) -> CmdResult<Overview> {
    with_db(&state, |c| analytics::overview(c, days))
}

#[tauri::command(async)]
pub fn get_insights(state: State<AppState>, days: i64) -> CmdResult<Vec<Insight>> {
    with_db(&state, |c| analytics::insights(c, days))
}

#[tauri::command(async)]
pub fn get_scorecard(state: State<AppState>, days: i64) -> CmdResult<Scorecard> {
    with_db(&state, |c| analytics::scorecard(c, days))
}

#[tauri::command(async)]
pub fn get_usage(state: State<AppState>, days: i64) -> CmdResult<trends::Usage> {
    with_db(&state, |c| trends::usage(c, days))
}

#[tauri::command(async)]
pub fn get_reality(state: State<AppState>, days: i64) -> CmdResult<trends::Reality> {
    with_db(&state, |c| trends::reality(c, days))
}

#[tauri::command(async)]
pub fn get_execution(state: State<AppState>, days: i64) -> CmdResult<trends::Execution> {
    with_db(&state, |c| trends::execution(c, days))
}

#[tauri::command(async)]
pub fn get_anomalies(state: State<AppState>, days: i64) -> CmdResult<Vec<trends::Anomaly>> {
    with_db(&state, |c| trends::anomalies(c, days))
}

#[tauri::command(async)]
pub fn get_failure_clusters(
    state: State<AppState>,
    days: i64,
) -> CmdResult<Vec<trends::FailureCluster>> {
    with_db(&state, |c| trends::failure_clusters(c, days))
}

#[tauri::command(async)]
pub fn get_timelines(state: State<AppState>, since: i64) -> CmdResult<Vec<trends::Lane>> {
    with_db(&state, |c| trends::timelines(c, since))
}

#[tauri::command(async)]
pub fn get_fit(state: State<AppState>, days: i64) -> CmdResult<Vec<trends::FitCell>> {
    with_db(&state, |c| trends::fit(c, days))
}

#[tauri::command(async)]
pub fn get_tool_health(state: State<AppState>, days: i64) -> CmdResult<Vec<trends::ToolHealth>> {
    with_db(&state, |c| trends::tool_health(c, days))
}

#[tauri::command(async)]
pub fn get_patterns(state: State<AppState>, days: i64) -> CmdResult<trends::Patterns> {
    with_db(&state, |c| trends::patterns(c, days))
}

/// Writes the filtered sessions as CSV to the Downloads folder and returns
/// the file's path.
#[tauri::command(async)]
pub fn export_sessions(state: State<AppState>, filter: SessionFilter) -> CmdResult<String> {
    with_db(&state, |c| {
        let rows = store::filter_sessions(c, &filter)?;
        let dir = tilly_core::util::download_dir().unwrap_or_else(|| state.data_dir.clone());
        let path = dir.join(format!(
            "tilly-sessions-{}.csv",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        tilly_core::util::write_private(&path, analytics::sessions_csv(&rows))?;
        Ok(path.to_string_lossy().into_owned())
    })
}

#[tauri::command(async)]
pub fn list_sessions(state: State<AppState>, filter: SessionFilter) -> CmdResult<Vec<SessionRow>> {
    with_db(&state, |c| store::list_sessions(c, &filter))
}

#[tauri::command(async)]
pub fn get_session(state: State<AppState>, id: String) -> CmdResult<SessionDetail> {
    with_db(&state, |c| analytics::session_detail(c, &id))
}

#[tauri::command(async)]
pub fn save_review(state: State<AppState>, review: Review) -> CmdResult<()> {
    with_db(&state, |c| analytics::save_review(c, &review))
}

#[tauri::command(async)]
pub fn get_live(state: State<AppState>) -> CmdResult<LiveSummary> {
    with_db(&state, live::summary)
}

#[tauri::command(async)]
pub fn get_toolkit(
    state: State<AppState>,
    days: i64,
    source: Option<String>,
) -> CmdResult<crate::toolkit::Toolkit> {
    with_db(&state, |c| {
        crate::toolkit::toolkit(c, days, source.as_deref())
    })
}

/// Takes the file's bytes as the raw request body.
#[tauri::command(async)]
pub fn import_alert_asset(
    state: State<AppState>,
    request: tauri::ipc::Request<'_>,
) -> CmdResult<crate::alerts::AlertAsset> {
    let bytes: Vec<u8> = match request.body() {
        tauri::ipc::InvokeBody::Raw(b) => b.clone(),
        // Without the custom IPC protocol the bytes arrive as a JSON array.
        tauri::ipc::InvokeBody::Json(v) => serde_json::from_value(v.clone())
            .map_err(|_| "Expected the file's bytes".to_string())?,
    };
    crate::alerts::import(&state.data_dir, &bytes).map_err(err)
}

/// Your status profile, or the defaults when `default` (for Reset).
#[tauri::command(async)]
pub fn get_alert_profile(
    state: State<AppState>,
    default: bool,
) -> CmdResult<crate::status_alerts::Profile> {
    if default {
        return Ok(Default::default());
    }
    with_db(&state, |c| {
        Ok(crate::status_alerts::Profile::parse(
            db::get_setting(c, crate::status_alerts::PROFILE_KEY)?.as_deref(),
        ))
    })
}

/// Plays a sound the way an alert would, for the Test buttons in Settings.
#[tauri::command(async)]
pub fn play_alert_sound(
    state: State<AppState>,
    sound: crate::status_alerts::Sound,
    volume: f32,
) -> CmdResult<()> {
    crate::sound::play(&sound, volume, &state.data_dir).map_err(err)
}

#[tauri::command(async)]
pub fn read_alert_asset(state: State<AppState>, id: String) -> CmdResult<tauri::ipc::Response> {
    crate::alerts::read(&state.data_dir, &id)
        .map(tauri::ipc::Response::new)
        .map_err(err)
}

#[tauri::command(async)]
pub fn delete_alert_asset(state: State<AppState>, id: String) -> CmdResult<()> {
    crate::alerts::delete(&state.data_dir, &id).map_err(err)
}

#[tauri::command(async)]
pub fn list_teams(state: State<AppState>) -> CmdResult<Vec<live::TeamSession>> {
    with_db(&state, live::teams)
}

#[tauri::command]
pub async fn rescan(state: State<'_, AppState>, force: bool) -> CmdResult<IngestReport> {
    blocking(&state, move |conn, data_dir| {
        Ingestor::new(data_dir).ingest_all(conn, force)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub source: Source,
    pub label: String,
    pub paths: Vec<String>,
    pub found: bool,
    pub enabled: bool,
    pub sessions: i64,
}

#[tauri::command(async)]
pub fn get_sources(state: State<AppState>) -> CmdResult<Vec<SourceStatus>> {
    with_db(&state, |c| {
        let roots = sources::roots();
        Source::ALL
            .iter()
            .map(|s| {
                let mine: Vec<_> = roots.iter().filter(|r| r.source == *s).collect();
                let sessions = store::count_by_source(c, s.as_str())?;
                Ok(SourceStatus {
                    source: *s,
                    label: s.label().into(),
                    paths: mine
                        .iter()
                        .filter(|r| r.path.exists())
                        .map(|r| r.path.to_string_lossy().into_owned())
                        .collect(),
                    found: mine.iter().any(|r| r.path.exists()),
                    enabled: db::setting_bool(c, &source_setting(*s), true),
                    sessions,
                })
            })
            .collect()
    })
}

#[tauri::command(async)]
pub fn set_source_enabled(state: State<AppState>, source: Source, enabled: bool) -> CmdResult<()> {
    with_db(&state, |c| {
        db::set_setting(
            c,
            &source_setting(source),
            if enabled { "true" } else { "false" },
        )
    })
}

#[tauri::command(async)]
pub fn list_projects(state: State<AppState>) -> CmdResult<Vec<String>> {
    with_db(&state, |c| store::recent_projects(c, 100))
}

// ----------------------------------------------------------------- journal

#[tauri::command(async)]
pub fn list_intents(state: State<AppState>) -> CmdResult<Vec<Intent>> {
    with_db(&state, journal::list)
}

#[tauri::command(async)]
pub fn create_intent(state: State<AppState>, input: NewIntent) -> CmdResult<i64> {
    with_db(&state, |c| journal::create(c, &input))
}

#[tauri::command(async)]
pub fn close_intent(state: State<AppState>, input: CloseIntent) -> CmdResult<()> {
    with_db(&state, |c| journal::close(c, &input))
}

#[tauri::command(async)]
pub fn reopen_intent(state: State<AppState>, id: i64) -> CmdResult<()> {
    with_db(&state, |c| journal::reopen(c, id))
}

#[tauri::command(async)]
pub fn delete_intent(state: State<AppState>, id: i64) -> CmdResult<()> {
    with_db(&state, |c| journal::delete(c, id))
}

#[tauri::command(async)]
pub fn link_intent(state: State<AppState>, id: i64, session_id: Option<String>) -> CmdResult<()> {
    with_db(&state, |c| journal::link(c, id, session_id.as_deref()))
}

#[tauri::command(async)]
pub fn get_calibration(state: State<AppState>, days: i64) -> CmdResult<Calibration> {
    with_db(&state, |c| {
        journal::calibration(c, store::Range::last_days(days).since)
    })
}

// --------------------------------------------------------------- playbooks

#[tauri::command(async)]
pub fn list_playbooks(state: State<AppState>) -> CmdResult<Vec<PlaybookStats>> {
    with_db(&state, playbooks::list)
}

#[tauri::command(async)]
pub fn create_prompt_template(
    state: State<AppState>,
    name: String,
    content: String,
) -> CmdResult<i64> {
    if name.trim().is_empty() || content.trim().is_empty() {
        return Err("Name and content are required.".into());
    }
    with_db(&state, |c| {
        playbooks::create_prompt_template(c, name.trim(), &content)
    })
}

#[tauri::command(async)]
pub fn update_prompt_template(
    state: State<AppState>,
    id: i64,
    name: String,
    content: String,
) -> CmdResult<()> {
    with_db(&state, |c| {
        playbooks::update_prompt_template(c, id, name.trim(), &content)
    })
}

#[tauri::command(async)]
pub fn delete_playbook(state: State<AppState>, id: i64) -> CmdResult<()> {
    with_db(&state, |c| playbooks::delete_prompt_template(c, id))
}

#[tauri::command(async)]
pub fn set_session_playbook(
    state: State<AppState>,
    session_id: String,
    version_id: i64,
    linked: bool,
) -> CmdResult<()> {
    with_db(&state, |c| {
        playbooks::set_session_link(c, &session_id, version_id, linked)
    })
}

// ------------------------------------------------------------------- retro

#[tauri::command(async)]
pub fn list_retros(state: State<AppState>) -> CmdResult<Vec<RetroRow>> {
    with_db(&state, retro::list)
}

#[tauri::command(async)]
pub fn preview_retro(
    state: State<AppState>,
    days: i64,
    reflection: Option<String>,
) -> CmdResult<String> {
    with_db(&state, |c| {
        Ok(retro::prepare(c, days, reflection.as_deref())?.prompt)
    })
}

#[tauri::command]
pub async fn generate_retro(
    state: State<'_, AppState>,
    engine: String,
    days: i64,
    reflection: Option<String>,
) -> CmdResult<RetroRow> {
    blocking(&state, move |conn, data_dir| {
        retro::generate(conn, data_dir, &engine, days, reflection.as_deref())
    })
    .await
}

#[tauri::command(async)]
pub fn delete_retro(state: State<AppState>, id: i64) -> CmdResult<()> {
    with_db(&state, |c| retro::delete(c, id))
}

// ---------------------------------------------------------------- settings

/// Settings the UI may write. Anything else is rejected.
const SETTING_KEYS: &[&str] = &[
    "archive.enabled",
    "notify.waiting",
    "retro.engine",
    "retro.days",
    "retro.cliModel",
    "retro.apiModel",
    "retro.claudePath",
    "budget.monthly",
    "context.smartZoneTokens",
    "alerts.profile",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub values: BTreeMap<String, String>,
    pub detected_claude_path: Option<String>,
    pub has_api_key: bool,
    pub keychain_error: Option<String>,
    pub data_dir: String,
    pub db_path: String,
    pub default_api_model: String,
}

#[tauri::command(async)]
pub fn get_settings(state: State<AppState>) -> CmdResult<Settings> {
    with_db(&state, |c| {
        let mut values = BTreeMap::new();
        for k in SETTING_KEYS {
            if let Some(v) = db::get_setting(c, k)? {
                values.insert(k.to_string(), v);
            }
        }
        let (has_api_key, keychain_error) = match retro::api_key() {
            Ok(k) => (k.is_some(), None),
            Err(e) => (false, Some(format!("{e:#}"))),
        };
        Ok(Settings {
            values,
            detected_claude_path: retro::find_claude_cli(c)
                .map(|p| p.to_string_lossy().into_owned()),
            has_api_key,
            keychain_error,
            data_dir: state.data_dir.to_string_lossy().into_owned(),
            db_path: state.db_path.to_string_lossy().into_owned(),
            default_api_model: retro::DEFAULT_API_MODEL.into(),
        })
    })
}

#[tauri::command(async)]
pub fn set_setting(state: State<AppState>, key: String, value: String) -> CmdResult<()> {
    if !SETTING_KEYS.contains(&key.as_str()) {
        return Err(format!("Unknown setting {key}"));
    }
    with_db(&state, |c| db::set_setting(c, &key, &value))
}

#[tauri::command(async)]
pub fn set_api_key(key: String) -> CmdResult<()> {
    retro::set_api_key(&key).map_err(err)
}

#[tauri::command(async)]
pub fn clear_api_key() -> CmdResult<()> {
    retro::clear_api_key().map_err(err)
}

// ------------------------------------------------------------ improvement

#[tauri::command(async)]
pub fn quick_review(
    state: State<AppState>,
    id: String,
    outcome: String,
    vs_manual: Option<String>,
) -> CmdResult<()> {
    with_db(&state, |c| {
        analytics::quick_review(c, &id, &outcome, vs_manual.as_deref())
    })
}

#[tauri::command(async)]
pub fn list_metrics() -> Vec<MetricDef> {
    experiments::METRICS.to_vec()
}

#[tauri::command(async)]
pub fn list_experiments(state: State<AppState>) -> CmdResult<Vec<Experiment>> {
    with_db(&state, experiments::list)
}

#[tauri::command(async)]
pub fn create_experiment(state: State<AppState>, input: NewExperiment) -> CmdResult<i64> {
    with_db(&state, |c| experiments::create(c, &input))
}

#[tauri::command(async)]
pub fn experiment_verdict(
    state: State<AppState>,
    id: i64,
    decision: String,
    note: Option<String>,
) -> CmdResult<()> {
    with_db(&state, |c| {
        experiments::verdict(c, id, &decision, note.as_deref())
    })
}

#[tauri::command(async)]
pub fn delete_experiment(state: State<AppState>, id: i64) -> CmdResult<()> {
    with_db(&state, |c| experiments::delete(c, id))
}

#[tauri::command(async)]
pub fn set_experiment_applied(
    state: State<AppState>,
    session_id: String,
    experiment_id: i64,
    applied: bool,
) -> CmdResult<()> {
    with_db(&state, |c| {
        experiments::set_applied(c, &session_id, experiment_id, applied)
    })
}

#[tauri::command(async)]
pub fn list_goals(state: State<AppState>) -> CmdResult<Vec<GoalStatus>> {
    with_db(&state, experiments::goals)
}

#[tauri::command(async)]
pub fn create_goal(state: State<AppState>, input: NewGoal) -> CmdResult<i64> {
    with_db(&state, |c| experiments::create_goal(c, &input))
}

#[tauri::command(async)]
pub fn retire_goal(state: State<AppState>, id: i64) -> CmdResult<()> {
    with_db(&state, |c| experiments::retire_goal(c, id))
}

#[tauri::command(async)]
pub fn preflight(
    state: State<AppState>,
    text: String,
    source: Option<String>,
) -> CmdResult<PreflightAdvice> {
    with_db(&state, |c| {
        experiments::preflight(c, &text, source.as_deref())
    })
}
