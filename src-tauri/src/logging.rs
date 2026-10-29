//! The app's log: warnings and errors from the background loop and failed
//! commands, written to `logs/tilly.log` in the data folder (and to stderr
//! when run from a terminal). The file is owner-only, like the rest of the
//! data folder, and starts over once it passes 5 MB, keeping one old copy.

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::fmt::writer::MakeWriterExt;

const MAX_BYTES: u64 = 5 * 1024 * 1024;

pub fn init(data_dir: &Path) {
    let dir = data_dir.join("logs");
    let path = dir.join("tilly.log");
    let file = (|| {
        tilly_core::util::create_private_dir(&dir)?;
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
            std::fs::rename(&path, dir.join("tilly.log.1"))?;
        }
        tilly_core::util::prepare_private_file(&path)?;
        OpenOptions::new().append(true).open(&path)
    })();
    let builder = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_target(false)
        .with_max_level(tracing::Level::INFO);
    let _ = match file {
        Ok(f) => builder
            .with_writer(std::io::stderr.and(Mutex::new(f)))
            .try_init(),
        Err(e) => {
            eprintln!("log file {}: {e}", path.display());
            builder.with_writer(std::io::stderr).try_init()
        }
    };
}
