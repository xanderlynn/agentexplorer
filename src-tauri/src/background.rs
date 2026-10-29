//! Tilly as a background app: it lives in the tray, keeps collecting,
//! tracking statuses and alerting with no window open, and creates the
//! window only while you look at it. Closing the window frees the web view,
//! which is most of the app's memory.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Timelike;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_notification::NotificationExt;

use crate::ingest::IngestReport;
use crate::live::{Group, LiveSummary};
use crate::status_alerts::{Alert, PROFILE_KEY, Profile, plan};
use crate::{AppState, db, sound, watcher};

pub const MAIN: &str = "main";
const TRAY: &str = "tray";
/// Sounds closer together than this are dropped, so a burst stays one sound.
const MIN_SOUND_GAP: Duration = Duration::from_secs(2);
/// Notifications at once beyond this are folded into "and N more".
const MAX_NOTES: usize = 3;

/// The tray's status line, updated as the live state changes.
pub struct TrayStatus(MenuItem<Wry>);

/// Opens the window, or brings it forward if it is open.
pub fn open_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    match app.config().app.windows.first() {
        Some(cfg) => {
            if let Err(e) =
                tauri::WebviewWindowBuilder::from_config(app, cfg).and_then(|b| b.build())
            {
                tracing::warn!("open window: {e}");
            }
        }
        None => tracing::error!("open window: no window in tauri.conf.json"),
    }
}

/// The window is gone: on macOS, drop the Dock icon until it is opened again.
pub fn window_closed(_app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = _app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "Nothing running", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Tilly", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Tilly", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&status, &PredefinedMenuItem::separator(app)?, &open, &quit],
    )?;
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("Tilly")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_window(tray.app_handle());
            }
        });
    // macOS draws a one-colour template icon to match the menu bar, light
    // or dark; Windows keeps the colour icon, which reads on any taskbar.
    #[cfg(target_os = "macos")]
    {
        tray = tray
            .icon(tauri::image::Image::from_bytes(include_bytes!(
                "../icons/tray-template@2x.png"
            ))?)
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayStatus(status));
    Ok(())
}

/// Forwards the background loop to the window (when open), the tray,
/// notifications and sounds.
pub struct AppSink {
    app: AppHandle,
    data_dir: PathBuf,
    last_sound: Mutex<Option<Instant>>,
}

impl AppSink {
    pub fn new(app: AppHandle, data_dir: PathBuf) -> Self {
        AppSink {
            app,
            data_dir,
            last_sound: Mutex::new(None),
        }
    }

    fn setting(&self, key: &str) -> Option<String> {
        let state = self.app.state::<AppState>();
        let conn = state.db.lock().ok()?;
        db::get_setting(&conn, key).ok().flatten()
    }

    fn notify(&self, alerts: &[Alert]) {
        let label = crate::status_alerts::label;
        for a in alerts.iter().take(MAX_NOTES) {
            let title = match &a.agent {
                Some(who) => format!("{}: {who}", label(&a.key)),
                None => label(&a.key).to_string(),
            };
            let body = format!(
                "{} · {}",
                a.project_path
                    .as_deref()
                    .map(crate::analytics::project_name)
                    .unwrap_or_default(),
                a.title.as_deref().unwrap_or("Untitled session")
            );
            let _ = self
                .app
                .notification()
                .builder()
                .title(title)
                .body(body)
                .show();
        }
        if alerts.len() > MAX_NOTES {
            let _ = self
                .app
                .notification()
                .builder()
                .title(format!("and {} more", alerts.len() - MAX_NOTES))
                .body("Open Tilly to see them all.")
                .show();
        }
    }
}

impl watcher::Sink for AppSink {
    fn data_changed(&self, report: &IngestReport) {
        let _ = self.app.emit("data-changed", report);
    }

    fn live(&self, summary: &LiveSummary) {
        let _ = self.app.emit("live", summary);
        let awaiting = summary
            .sessions
            .iter()
            .filter(|s| s.status.group() == Group::Awaiting)
            .count();
        let text = match (awaiting, summary.working) {
            (0, 0) => "Nothing running".to_string(),
            (0, w) => format!("{w} working"),
            (a, 0) => format!("{a} awaiting you"),
            (a, w) => format!("{a} awaiting you · {w} working"),
        };
        if let Some(status) = self.app.try_state::<TrayStatus>() {
            let _ = status.0.set_text(&text);
        }
        if let Some(tray) = self.app.tray_by_id(TRAY) {
            let _ = tray.set_tooltip(Some(format!("Tilly · {text}")));
            // The count beside the icon in the macOS menu bar.
            let _ = tray.set_title((awaiting > 0).then(|| awaiting.to_string()));
        }
    }

    fn alerts(&self, alerts: Vec<Alert>) {
        let profile = Profile::parse(self.setting(PROFILE_KEY).as_deref());
        let focused = self
            .app
            .get_webview_window(MAIN)
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        let now = chrono::Local::now();
        let p = plan(&profile, &alerts, now.hour() * 60 + now.minute(), focused);
        if let Some((sound, volume)) = p.sound {
            let mut last = self.last_sound.lock().expect("sound lock");
            if last.is_none_or(|t| t.elapsed() >= MIN_SOUND_GAP) {
                *last = Some(Instant::now());
                sound::play_async(sound, volume, self.data_dir.clone());
            }
        }
        // Desktop notifications have their own switch in Settings.
        if !p.notify.is_empty() && self.setting("notify.waiting").as_deref() != Some("false") {
            self.notify(&p.notify);
        }
    }
}
