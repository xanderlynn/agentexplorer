//! Your own sounds and images for agent statuses. Files are copied into the
//! app's data folder under a name derived from their content, checked by
//! their bytes (not their extension), and read back only by that name, so
//! the webview never gets access to arbitrary paths.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_SOUND_BYTES: usize = 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlertAsset {
    /// `<hash>.<ext>`: the file name in the alerts folder.
    pub id: String,
    /// sound | image
    pub kind: &'static str,
}

/// The file type, from its first bytes.
fn sniff(b: &[u8]) -> Option<(&'static str, &'static str)> {
    let riff = |tag: &[u8]| b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == tag;
    Some(if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        ("image", "png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ("image", "jpg")
    } else if b.starts_with(b"GIF8") {
        ("image", "gif")
    } else if riff(b"WEBP") {
        ("image", "webp")
    } else if riff(b"WAVE") {
        ("sound", "wav")
    } else if b.starts_with(b"OggS") {
        ("sound", "ogg")
    } else {
        return None;
    })
}

fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("alerts")
}

/// Only names this module wrote: hex hash, dot, known extension.
fn path_for(data_dir: &Path, id: &str) -> Result<PathBuf> {
    let ok = id.split_once('.').is_some_and(|(h, ext)| {
        h.len() == 16
            && h.chars().all(|c| c.is_ascii_hexdigit())
            && ["png", "jpg", "gif", "webp", "wav", "ogg"].contains(&ext)
    });
    if !ok {
        bail!("Unknown alert file");
    }
    Ok(dir(data_dir).join(id))
}

pub fn import(data_dir: &Path, bytes: &[u8]) -> Result<AlertAsset> {
    let Some((kind, ext)) = sniff(bytes) else {
        bail!("Use a PNG, JPEG, GIF or WebP image, or a WAV or Ogg sound.");
    };
    let limit = if kind == "sound" {
        MAX_SOUND_BYTES
    } else {
        MAX_IMAGE_BYTES
    };
    if bytes.len() > limit {
        bail!(
            "That {kind} is {} KB; the limit is {} KB.",
            bytes.len() / 1024,
            limit / 1024
        );
    }
    // Named by content, so the same file imported twice is stored once.
    let hash = Sha256::digest(bytes);
    let prefix: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let id = format!("{prefix}.{ext}");
    let path = path_for(data_dir, &id)?;
    crate::util::create_private_dir(&dir(data_dir))?;
    if path.is_file() {
        crate::util::restrict_private_if_exists(&path, false)?;
    } else {
        crate::util::write_private(&path, bytes)?;
    }
    Ok(AlertAsset { id, kind })
}

pub fn read(data_dir: &Path, id: &str) -> Result<Vec<u8>> {
    Ok(std::fs::read(path_for(data_dir, id)?)?)
}

pub fn delete(data_dir: &Path, id: &str) -> Result<()> {
    let p = path_for(data_dir, id)?;
    if p.is_file() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_by_content_and_rejects_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        let a = import(dir.path(), &png).unwrap();
        assert_eq!(a.kind, "image");
        assert_eq!(a.id, "7f47b756761a46e6.png");
        // Same bytes, same file.
        assert_eq!(import(dir.path(), &png).unwrap().id, a.id);
        assert_eq!(read(dir.path(), &a.id).unwrap(), png);

        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend([0; 8]);
        assert_eq!(import(dir.path(), &wav).unwrap().kind, "sound");

        assert!(import(dir.path(), b"<svg xmlns='http://www.w3.org/2000/svg'/>").is_err());
        assert!(import(dir.path(), &vec![0xFF; MAX_SOUND_BYTES + 10]).is_err());

        delete(dir.path(), &a.id).unwrap();
        assert!(read(dir.path(), &a.id).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn reimport_tightens_existing_asset_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
        let asset = import(dir.path(), &png).unwrap();
        let path = path_for(dir.path(), &asset.id).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        import(dir.path(), &png).unwrap();

        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn ids_cannot_reach_other_files() {
        let dir = tempfile::tempdir().unwrap();
        for id in [
            "../ledger.sqlite3",
            "0123456789abcdef.exe",
            "x.png",
            "/etc/passwd",
        ] {
            assert!(read(dir.path(), id).is_err(), "{id}");
        }
    }
}
