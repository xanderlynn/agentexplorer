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

/// Sessions waiting on you listed in the tray menu, at most.
const TRAY_SESSIONS: usize = 5;

/// What the tray menu shows now: its status line and the waiting sessions,
/// rebuilt only when they change.
#[derive(Default, PartialEq)]
struct TrayState {
    text: String,
    waiting: Vec<(String, String)>,
}

pub struct TrayMenu(Mutex<TrayState>);

/// A session picked from the tray or a notification, for the window to open
/// once it's ready.
#[derive(Default)]
pub struct OpenRequest(pub Mutex<Option<String>>);

/// Clicking a notification only brings Tilly forward, so a focus this soon
/// after one opens the session it was about.
const NOTIFICATION_CLICK: Duration = Duration::from_secs(60);

/// The session the last notification was about, and when it was shown.
#[derive(Default)]
pub struct LastNotified(Mutex<Option<(String, Instant)>>);

/// The window was focused: if a notification was just shown, open its session.
pub fn focused(app: &AppHandle) {
    let Some(last) = app.try_state::<LastNotified>() else {
        return;
    };
    let hit = last
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .filter(|(_, at)| at.elapsed() < NOTIFICATION_CLICK);
    if let Some((id, _)) = hit {
        open_session(app, &id);
    }
}

/// Shows the window and asks it to open session `id`.
pub fn open_session(app: &AppHandle, id: &str) {
    if let Some(r) = app.try_state::<OpenRequest>() {
        *r.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
    }
    open_window(app);
    let _ = app.emit("open-session", id);
}

fn tray_menu(app: &AppHandle, state: &TrayState) -> tauri::Result<Menu<Wry>> {
    let status = MenuItem::with_id(app, "status", &state.text, false, None::<&str>)?;
    let sessions: Vec<MenuItem<Wry>> = state
        .waiting
        .iter()
        .map(|(id, label)| {
            MenuItem::with_id(app, format!("session:{id}"), label, true, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let open = MenuItem::with_id(app, "open", "Open Tilly", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Tilly", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = vec![&status];
    if !sessions.is_empty() {
        items.push(&sep2);
        items.extend(
            sessions
                .iter()
                .map(|m| m as &dyn tauri::menu::IsMenuItem<Wry>),
        );
    }
    items.extend([&sep as &dyn tauri::menu::IsMenuItem<Wry>, &open, &quit]);
    Menu::with_items(app, &items)
}

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
    let state = TrayState {
        text: "Nothing running".into(),
        waiting: vec![],
    };
    let menu = tray_menu(app, &state)?;
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("Tilly")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open_window(app),
            "quit" => app.exit(0),
            id => {
                if let Some(session) = id.strip_prefix("session:") {
                    open_session(app, session);
                }
            }
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
    app.manage(TrayMenu(Mutex::new(state)));
    app.manage(OpenRequest::default());
    app.manage(LastNotified::default());
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
        if let (Some(a), Some(last)) = (alerts.first(), self.app.try_state::<LastNotified>()) {
            *last.0.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((a.session_id.clone(), Instant::now()));
        }
        for a in alerts.iter().take(MAX_NOTES) {
            let title = match (&a.agent, a.reminder) {
                (Some(who), _) => format!("{}: {who}", label(&a.key)),
                (None, true) => format!("Still waiting: {}", label(&a.key)),
                (None, false) => label(&a.key).to_string(),
            };
            let place = format!(
                "{} · {}",
                a.project_path
                    .as_deref()
                    .map(crate::analytics::project_name)
                    .unwrap_or_default(),
                a.title.as_deref().unwrap_or("Untitled session")
            );
            // What it wants first, then where.
            let body = match &a.detail {
                Some(d) => format!("{d}\n{place}"),
                None => place,
            };
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
        // The waiting sessions, most urgent first, each a click away.
        let mut waiting: Vec<_> = summary
            .sessions
            .iter()
            .filter(|s| s.status.group() == Group::Awaiting)
            .collect();
        waiting.sort_by_key(|s| {
            (
                crate::status_alerts::urgency(s.status.as_str()),
                s.status_since,
            )
        });
        let next = TrayState {
            text: text.clone(),
            waiting: waiting
                .iter()
                .take(TRAY_SESSIONS)
                .map(|s| {
                    let title = s.title.as_deref().unwrap_or("Untitled session");
                    let label = crate::status_alerts::label(s.status.as_str());
                    (
                        s.id.clone(),
                        crate::util::truncate(&format!("{label}: {title}"), 60),
                    )
                })
                .collect(),
        };
        if let Some(menu) = self.app.try_state::<TrayMenu>() {
            let mut current = menu.0.lock().unwrap_or_else(|e| e.into_inner());
            if *current != next {
                if let (Some(tray), Ok(m)) =
                    (self.app.tray_by_id(TRAY), tray_menu(&self.app, &next))
                {
                    let _ = tray.set_menu(Some(m));
                }
                *current = next;
            }
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
