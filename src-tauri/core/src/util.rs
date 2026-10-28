use std::path::{Path, PathBuf};
use std::process::Command;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Parses an RFC 3339 timestamp into Unix milliseconds.
pub fn parse_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Truncates on a char boundary, appending an ellipsis when shortened.
pub fn truncate(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// The user's Downloads folder, where exports go.
pub fn download_dir() -> Option<PathBuf> {
    dirs::download_dir()
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Path comparison key: forward slashes, no trailing slash, and lowercase on
/// Windows where the filesystem is case-insensitive.
pub fn path_key(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    if cfg!(windows) {
        s = s.to_lowercase();
    }
    s
}

/// True when `child` is `parent` or inside it.
pub fn path_within(child: &str, parent: &str) -> bool {
    let c = path_key(child);
    let p = path_key(parent);
    c == p || c.starts_with(&format!("{p}/"))
}

/// A `Command` that never flashes a console window on Windows.
pub fn quiet_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn file_mtime_ms(p: &Path) -> Option<i64> {
    let meta = std::fs::metadata(p).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("héllo", 2), "hé…");
        assert_eq!(truncate("hi", 5), "hi");
    }

    #[test]
    fn path_within_requires_segment_boundary() {
        assert!(path_within("/a/b/c", "/a/b"));
        assert!(path_within("/a/b/", "/a/b"));
        assert!(!path_within("/a/bc", "/a/b"));
    }

    #[test]
    fn parses_rfc3339() {
        assert_eq!(parse_ts("1970-01-01T00:00:01.500Z"), Some(1500));
        assert_eq!(parse_ts("nope"), None);
    }
}
