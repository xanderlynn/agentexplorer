//! Claude Code doesn't log permission prompts, so Tilly can only guess at
//! them from a tool that's been pending a while. With your consent Tilly adds
//! a Notification hook to Claude Code's settings that runs `tilly --hook`,
//! which records the prompt (the session and the tool, nothing else) for the
//! live view. It covers Claude Code in a terminal and in the Claude app.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

use crate::util::{home, now_ms};

/// Prompts older than this are forgotten.
const KEEP_MS: i64 = 24 * 3_600_000;
/// The flag Tilly runs with as a hook; also how its entry is recognised.
pub const HOOK_FLAG: &str = "--hook";

/// A permission prompt Claude Code raised.
#[derive(Debug, PartialEq)]
pub struct Prompt {
    pub session: String,
    /// The tool it wants to use, when the message names it.
    pub tool: Option<String>,
}

/// Reads a Notification hook's input; a permission prompt, or none.
pub fn prompt(input: &Value) -> Option<Prompt> {
    if input["hook_event_name"] != "Notification" {
        return None;
    }
    let message = input["message"].as_str().unwrap_or("");
    let permission = input["notification_type"] == "permission_prompt"
        || message.to_lowercase().contains("permission");
    if !permission {
        return None;
    }
    // "Claude needs your permission to use Bash"
    let tool = message
        .split_once("to use ")
        .map(|(_, t)| t.trim().trim_end_matches('.').to_string())
        .filter(|t| !t.is_empty());
    Some(Prompt {
        session: input["session_id"].as_str()?.to_string(),
        tool,
    })
}

/// Runs as the hook: records a permission prompt from `stdin` in the
/// database at `db`. Never fails loudly, so Claude Code carries on.
pub fn record(db: &Path, stdin: &str) -> Result<()> {
    let Ok(input) = serde_json::from_str::<Value>(stdin) else {
        return Ok(());
    };
    let Some(p) = prompt(&input) else {
        return Ok(());
    };
    let conn = Connection::open(db)?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    let now = now_ms();
    conn.execute(
        "INSERT INTO hook_events (external_id, kind, detail, at) VALUES (?1, 'permission', ?2, ?3)
         ON CONFLICT(external_id, kind) DO UPDATE SET detail = excluded.detail, at = excluded.at",
        params![p.session, p.tool, now],
    )?;
    conn.execute("DELETE FROM hook_events WHERE at < ?1", [now - KEEP_MS])?;
    Ok(())
}

/// Claude Code's user settings file.
pub fn claude_settings() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
        .join("settings.json")
}

/// The hook command: this app, as a hook, writing to `db`.
fn command(exe: &Path, db: &Path) -> String {
    let q = |p: &Path| {
        let s = p.to_string_lossy();
        if cfg!(windows) {
            format!("\"{s}\"")
        } else {
            format!("'{}'", s.replace('\'', r"'\''"))
        }
    };
    format!("{} {HOOK_FLAG} {}", q(exe), q(db))
}

fn is_ours(entry: &Value) -> bool {
    entry["hooks"].as_array().is_some_and(|hs| {
        hs.iter().any(|h| {
            h["command"].as_str().is_some_and(|c| {
                c.contains(&format!(" {HOOK_FLAG} ")) && c.to_lowercase().contains("tilly")
            })
        })
    })
}

fn read(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => {
            serde_json::from_str(&s).with_context(|| format!("{} isn't valid JSON", path.display()))
        }
        Ok(_) => Ok(json!({})),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.into()),
    }
}

/// Whether Tilly's hook is in Claude Code's settings at `path`.
pub fn installed(path: &Path) -> Result<bool> {
    Ok(read(path)?["hooks"]["Notification"]
        .as_array()
        .is_some_and(|a| a.iter().any(is_ours)))
}

/// Adds (or with `on` false, removes) Tilly's hook in Claude Code's settings
/// at `path`, keeping everything else; the file before is kept beside it as
/// `settings.json.tilly-backup`.
pub fn set_installed(path: &Path, on: bool, exe: &Path, db: &Path) -> Result<()> {
    let mut v = read(path)?;
    if !v.is_object() {
        anyhow::bail!("{} isn't a JSON object", path.display());
    }
    if path.exists() {
        std::fs::copy(path, path.with_extension("json.tilly-backup"))?;
    }
    let hooks = v
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let list = hooks
        .as_object_mut()
        .context("\"hooks\" in Claude Code's settings isn't an object")?
        .entry("Notification")
        .or_insert_with(|| json!([]));
    let arr = list
        .as_array_mut()
        .context("\"hooks.Notification\" in Claude Code's settings isn't a list")?;
    arr.retain(|e| !is_ours(e));
    if on {
        arr.push(json!({
            "matcher": "",
            "hooks": [{ "type": "command", "command": command(exe, db) }]
        }));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&v)? + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_permission_prompts_only() {
        let p = prompt(
            &json!({"hook_event_name": "Notification", "session_id": "s1",
            "message": "Claude needs your permission to use Bash", "notification_type": "permission_prompt"}),
        );
        assert_eq!(
            p,
            Some(Prompt {
                session: "s1".into(),
                tool: Some("Bash".into())
            })
        );
        assert_eq!(
            prompt(
                &json!({"hook_event_name": "Notification", "session_id": "s1",
                "message": "Claude is waiting for your input"})
            ),
            None
        );
        assert_eq!(
            prompt(&json!({"hook_event_name": "Stop", "session_id": "s1"})),
            None
        );
    }

    #[test]
    fn records_prompts_for_the_live_view() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.sqlite3");
        crate::db::open(&db).unwrap();
        record(&db, r#"{"hook_event_name":"Notification","session_id":"s1","message":"Claude needs your permission to use Edit"}"#).unwrap();
        record(&db, "not json").unwrap();
        let conn = Connection::open(&db).unwrap();
        let (detail, n): (String, i64) = conn
            .query_row(
                "SELECT detail, (SELECT COUNT(*) FROM hook_events) FROM hook_events",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((detail.as_str(), n), ("Edit", 1));
    }

    #[test]
    fn installs_and_removes_its_hook_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"statusLine":{"type":"command"},"hooks":{"Notification":[{"matcher":"","hooks":[{"type":"command","command":"say hi"}]}]}}"#,
        )
        .unwrap();
        let exe = Path::new("/Applications/Tilly.app/Contents/MacOS/tilly");
        let db = Path::new("/data/it's here/ledger.sqlite3");
        assert!(!installed(&path).unwrap());
        set_installed(&path, true, exe, db).unwrap();
        set_installed(&path, true, exe, db).unwrap();
        assert!(installed(&path).unwrap());
        let v = read(&path).unwrap();
        let list = v["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(list.len(), 2, "theirs kept, ours once");
        assert_eq!(v["statusLine"]["type"], "command");
        let cmd = list[1]["hooks"][0]["command"].as_str().unwrap();
        // Quoted for the platform's shell, so the path with a quote survives.
        if cfg!(windows) {
            assert!(cmd.contains("\"/data/it's here/ledger.sqlite3\""), "{cmd}");
        } else {
            assert!(
                cmd.contains(r"'/data/it'\''s here/ledger.sqlite3'"),
                "{cmd}"
            );
        }
        assert!(path.with_extension("json.tilly-backup").exists());
        set_installed(&path, false, exe, db).unwrap();
        assert!(!installed(&path).unwrap());
        assert_eq!(
            read(&path).unwrap()["hooks"]["Notification"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
