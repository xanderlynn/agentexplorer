//! "Go to agent": brings forward where a session runs, so you can answer it.
//! The Claude and Copilot apps are brought to the front; VS Code opens on the
//! session's folder; a command-line agent is found among running processes
//! (its working folder is the session's) and its terminal is focused, down to
//! the tab in Terminal and iTerm2. macOS only; elsewhere it says how to resume.

use anyhow::{Result, anyhow, bail};

/// What's known about the session to find it.
pub struct Target {
    pub source: String,
    pub entrypoint: Option<String>,
    pub cwd: Option<String>,
    pub external_id: String,
    pub resume: Option<String>,
}

/// One running process.
#[derive(Debug, Clone, PartialEq)]
struct Proc {
    pid: u32,
    ppid: u32,
    tty: String,
    args: String,
}

/// `ps -Ao pid=,ppid=,tty=,args=` → processes.
fn parse_ps(out: &str) -> Vec<Proc> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            let tty = it.next()?.to_string();
            let args = it.collect::<Vec<_>>().join(" ");
            Some(Proc {
                pid,
                ppid,
                tty,
                args,
            })
        })
        .collect()
}

/// Is this process the agent's command-line tool?
fn is_agent(p: &Proc, source: &str) -> bool {
    let a = p.args.to_lowercase();
    match source {
        "claude_code" => {
            let exe = a.split_whitespace().next().unwrap_or("");
            exe.ends_with("/claude") || exe == "claude" || a.contains("@anthropic-ai/claude-code")
        }
        "copilot_cli" => {
            a.contains("@github/copilot")
                || a.split_whitespace()
                    .next()
                    .is_some_and(|e| e.ends_with("/copilot") || e == "copilot")
        }
        _ => false,
    }
}

/// The terminal app a process runs under, from its ancestors' commands.
fn terminal_of(procs: &[Proc], pid: u32) -> Option<&'static str> {
    const APPS: &[(&str, &str)] = &[
        ("/terminal.app/", "Terminal"),
        ("/iterm.app/", "iTerm"),
        ("/visual studio code.app/", "Visual Studio Code"),
        ("/cursor.app/", "Cursor"),
        ("/warp.app/", "Warp"),
        ("/ghostty.app/", "Ghostty"),
        ("/wezterm.app/", "WezTerm"),
        ("/kitty.app/", "kitty"),
        ("/alacritty.app/", "Alacritty"),
    ];
    let mut cur = pid;
    for _ in 0..32 {
        let p = procs.iter().find(|p| p.pid == cur)?;
        let a = p.args.to_lowercase();
        if let Some((_, app)) = APPS.iter().find(|(path, _)| a.contains(path)) {
            return Some(app);
        }
        if p.ppid == 0 || p.ppid == cur {
            return None;
        }
        cur = p.ppid;
    }
    None
}

/// Brings the session's app or terminal forward and says what it did.
pub fn go(t: &Target) -> Result<String> {
    if !cfg!(target_os = "macos") {
        return Err(not_found(t));
    }
    match (t.source.as_str(), t.entrypoint.as_deref()) {
        (_, Some("claude-desktop")) => {
            open_app("Claude", None).map(|_| "Opened the Claude app".into())
        }
        (_, Some("copilot-app")) => {
            open_app("GitHub Copilot", None).map(|_| "Opened the Copilot app".into())
        }
        ("copilot_vscode", _) | (_, Some("vscode")) => {
            open_app("Visual Studio Code", t.cwd.as_deref())
                .map(|_| "Opened VS Code on the session's folder".into())
        }
        _ => cli(t),
    }
}

fn not_found(t: &Target) -> anyhow::Error {
    match &t.resume {
        Some(cmd) => anyhow!("Couldn't find where it's running. To pick it up again, run: {cmd}"),
        None => anyhow!("Couldn't find where it's running"),
    }
}

fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = tilly_core::util::quiet_command(cmd).args(args).output()?;
    if !out.status.success() {
        bail!("{cmd}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn open_app(app: &str, path: Option<&str>) -> Result<()> {
    let mut args = vec!["-a", app];
    args.extend(path);
    run("open", &args).map(|_| ())
}

/// A command-line agent: its process, then its terminal and tab.
fn cli(t: &Target) -> Result<String> {
    let procs = parse_ps(&run("ps", &["-Ao", "pid=,ppid=,tty=,args="])?);
    let agents: Vec<&Proc> = procs.iter().filter(|p| is_agent(p, &t.source)).collect();
    if agents.is_empty() {
        return Err(not_found(t));
    }
    // Their working folders, in one lsof call.
    let pids: Vec<String> = agents.iter().map(|p| p.pid.to_string()).collect();
    let lsof = run("lsof", &["-a", "-d", "cwd", "-Fpn", "-p", &pids.join(",")]).unwrap_or_default();
    let mut cwd_of = std::collections::HashMap::new();
    let mut pid = 0u32;
    for line in lsof.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().unwrap_or(0);
        } else if let Some(n) = line.strip_prefix('n') {
            cwd_of.insert(pid, n.to_string());
        }
    }
    let want = t.cwd.as_deref().map(tilly_core::util::path_key);
    let mut matches: Vec<&&Proc> = agents
        .iter()
        .filter(|p| match (&want, cwd_of.get(&p.pid)) {
            (Some(w), Some(c)) => tilly_core::util::path_key(c) == *w,
            _ => false,
        })
        .collect();
    // The one resumed with this session's id, if any.
    matches.sort_by_key(|p| !p.args.contains(&t.external_id));
    let p = matches.first().ok_or_else(|| not_found(t))?;
    let app = terminal_of(&procs, p.pid).ok_or_else(|| not_found(t))?;
    let tty = format!("/dev/{}", p.tty);
    match app {
        "Terminal" => {
            osascript(&format!(
                r#"tell application "Terminal"
                    activate
                    repeat with w in windows
                        repeat with tb in tabs of w
                            if tty of tb is "{tty}" then
                                set selected of tb to true
                                set index of w to 1
                                return
                            end if
                        end repeat
                    end repeat
                end tell"#
            ))?;
            Ok("Opened its Terminal tab".into())
        }
        "iTerm" => {
            osascript(&format!(
                r#"tell application "iTerm"
                    activate
                    repeat with w in windows
                        repeat with tb in tabs of w
                            repeat with s in sessions of tb
                                if tty of s is "{tty}" then
                                    select w
                                    select tb
                                    select s
                                    return
                                end if
                            end repeat
                        end repeat
                    end repeat
                end tell"#
            ))?;
            Ok("Opened its iTerm tab".into())
        }
        "Visual Studio Code" | "Cursor" => {
            open_app(app, t.cwd.as_deref())?;
            Ok(format!("Opened {app}; it's in the integrated terminal"))
        }
        other => {
            open_app(other, None)?;
            Ok(format!("Opened {other}"))
        }
    }
}

fn osascript(script: &str) -> Result<()> {
    run("osascript", &["-e", script]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "    1     0 ??       /sbin/launchd
  500     1 ??       /System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal
  501   500 ttys003  login -pf me
  502   501 ttys003  -zsh
  503   502 ttys003  /Users/me/.local/bin/claude --resume abc
  600     1 ??       /Applications/iTerm.app/Contents/MacOS/iTerm2
  601   600 ttys005  /bin/zsh
  602   601 ttys005  node /opt/homebrew/lib/node_modules/@github/copilot/index.js
  700   502 ttys003  grep claude";

    #[test]
    fn finds_agents_and_their_terminals() {
        let procs = parse_ps(PS);
        let claude: Vec<u32> = procs
            .iter()
            .filter(|p| is_agent(p, "claude_code"))
            .map(|p| p.pid)
            .collect();
        assert_eq!(claude, [503]);
        let copilot: Vec<u32> = procs
            .iter()
            .filter(|p| is_agent(p, "copilot_cli"))
            .map(|p| p.pid)
            .collect();
        assert_eq!(copilot, [602]);
        assert_eq!(terminal_of(&procs, 503), Some("Terminal"));
        assert_eq!(terminal_of(&procs, 602), Some("iTerm"));
        assert_eq!(procs.iter().find(|p| p.pid == 503).unwrap().tty, "ttys003");
        assert_eq!(terminal_of(&procs, 1), None);
    }
}
