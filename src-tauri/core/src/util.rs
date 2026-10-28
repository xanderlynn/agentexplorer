use std::io::Write;
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

/// Creates an app-data directory and limits it to the current user.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700).create(path)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(path)?;
    restrict_private(path, true)
}

/// Creates (or opens) a sensitive file without truncating it and restricts it
/// to the current user. This is used before libraries such as SQLite open it.
pub fn prepare_private_file(path: &Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(path)?);
    restrict_private(path, false)
}

/// Writes a sensitive file with owner-only permissions from creation.
pub fn write_private(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    restrict_private(path, false)?;
    file.write_all(bytes.as_ref())
}

/// Copies a sensitive file without inheriting the source file's permissions.
pub fn copy_private(from: &Path, to: &Path) -> std::io::Result<u64> {
    let mut source = std::fs::File::open(from)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut target = options.open(to)?;
    restrict_private(to, false)?;
    let copied = std::io::copy(&mut source, &mut target)?;
    target.flush()?;
    Ok(copied)
}

/// Tightens an existing file or directory. Missing paths are ignored so this
/// can also be used for transient SQLite sidecars.
pub fn restrict_private_if_exists(path: &Path, directory: bool) -> std::io::Result<()> {
    match restrict_private(path, directory) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(unix)]
fn restrict_private(path: &Path, directory: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
    )
}

#[cfg(windows)]
fn restrict_private(path: &Path, directory: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SetFileSecurityW,
    };

    // OW is the object's owner (the user who created this app-data object).
    // Child files inherit the same owner-only ACL from directories.
    let sddl = if directory {
        "D:P(A;OICI;FA;;;OW)"
    } else {
        "D:P(A;;FA;;;OW)"
    };
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let wide_sddl: Vec<u16> = std::ffi::OsStr::new(sddl)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both strings are NUL-terminated, descriptor is initialized by
    // the conversion call, and LocalFree releases that allocation afterward.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide_sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let ok = SetFileSecurityW(
            wide_path.as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        let error = (ok == 0).then(std::io::Error::last_os_error);
        LocalFree(descriptor);
        if let Some(error) = error {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn restrict_private(_path: &Path, _directory: bool) -> std::io::Result<()> {
    Ok(())
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

    #[cfg(unix)]
    #[test]
    fn private_files_and_directories_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("private");
        create_private_dir(&dir).unwrap();
        let file = dir.join("data");
        write_private(&file, b"secret").unwrap();

        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        prepare_private_file(&file).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
