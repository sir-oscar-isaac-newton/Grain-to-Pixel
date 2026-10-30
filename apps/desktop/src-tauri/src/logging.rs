//! Rolling JSON log file plus dev stderr output (docs/specification.md section 12).
//!
//! `tracing-appender` rotates daily and keeps [`KEEP_DAYS`] files but has no size cap, so
//! [`prune_to_cap`] deletes the oldest files once their total size exceeds the cap.

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{InitError, RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

const LOG_PREFIX: &str = "gtp";
const LOG_SUFFIX: &str = "log";
/// Daily files kept (spec section 12).
const KEEP_DAYS: usize = 7;
/// Total size of the log files kept (spec section 12).
pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;
/// The cap is also enforced while the app runs, so a long session over midnight stays bounded.
const PRUNE_INTERVAL: Duration = Duration::from_secs(60 * 60);

fn build_appender(dir: &Path) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix(LOG_SUFFIX)
        .max_log_files(KEEP_DAYS)
        .build(dir)
}

/// Deletes the oldest log files until their total size is at most `max_total_bytes`.
/// The newest file is never deleted, even if it alone is over the cap. Returns the number removed.
pub fn prune_to_cap(dir: &Path, max_total_bytes: u64) -> io::Result<usize> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(LOG_PREFIX) && name.ends_with(LOG_SUFFIX) {
            files.push((name, entry.path(), entry.metadata()?.len()));
        }
    }
    // File names carry the date (gtp.2026-10-30.log), so name order is age order.
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut total: u64 = files.iter().map(|f| f.2).sum();
    let mut removed = 0;
    while total > max_total_bytes && files.len() - removed > 1 {
        let (_, path, len) = &files[removed];
        fs::remove_file(path)?;
        total -= len;
        removed += 1;
    }
    Ok(removed)
}

/// Installs the global subscriber: JSON lines to the rolling file, plus stderr in debug builds.
/// Keep the returned guard alive for the whole run; dropping it flushes and stops the writer.
pub fn init(dir: &Path) -> Result<WorkerGuard, Box<dyn Error>> {
    fs::create_dir_all(dir)?;
    let pruned = prune_to_cap(dir, MAX_TOTAL_BYTES);
    let (writer, guard) = tracing_appender::non_blocking(build_appender(dir)?);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().json().with_ansi(false).with_writer(writer));
    #[cfg(debug_assertions)]
    let registry = registry.with(fmt::layer().with_writer(io::stderr));
    registry.try_init()?;

    match pruned {
        Ok(0) => {}
        Ok(n) => tracing::info!(removed = n, "pruned old log files over the size cap"),
        Err(err) => tracing::warn!(%err, "could not prune old log files"),
    }
    spawn_pruner(dir.to_path_buf());
    Ok(guard)
}

fn spawn_pruner(dir: std::path::PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("log-pruner".into())
        .spawn(move || loop {
            std::thread::sleep(PRUNE_INTERVAL);
            if let Err(err) = prune_to_cap(&dir, MAX_TOTAL_BYTES) {
                tracing::warn!(%err, "could not prune old log files");
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "log pruning thread did not start");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gtp-log-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, len: usize) {
        fs::write(dir.join(name), vec![b'x'; len]).unwrap();
    }

    #[test]
    fn prune_removes_the_oldest_files_until_under_the_cap() {
        let dir = temp_dir("prune");
        write(&dir, "gtp.2026-10-28.log", 100);
        write(&dir, "gtp.2026-10-29.log", 100);
        write(&dir, "gtp.2026-10-30.log", 100);
        write(&dir, "other.txt", 1000);

        assert_eq!(prune_to_cap(&dir, 250).unwrap(), 1);
        assert!(!dir.join("gtp.2026-10-28.log").exists());
        assert!(dir.join("gtp.2026-10-29.log").exists());
        assert!(dir.join("other.txt").exists(), "unrelated files are kept");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prune_keeps_the_newest_file_even_when_it_alone_exceeds_the_cap() {
        let dir = temp_dir("newest");
        write(&dir, "gtp.2026-10-29.log", 100);
        write(&dir, "gtp.2026-10-30.log", 500);

        assert_eq!(prune_to_cap(&dir, 10).unwrap(), 1);
        assert!(dir.join("gtp.2026-10-30.log").exists());
        assert_eq!(prune_to_cap(&dir, 10).unwrap(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn prune_does_nothing_under_the_cap() {
        let dir = temp_dir("under");
        write(&dir, "gtp.2026-10-30.log", 100);
        assert_eq!(prune_to_cap(&dir, 1000).unwrap(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn events_go_to_a_daily_json_file() {
        let dir = temp_dir("json");
        let appender = build_appender(&dir).unwrap();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_writer(appender)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(frame_id = 7, "library opened");
        });

        let file = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                name.starts_with("gtp.") && name.ends_with(".log")
            })
            .expect("a gtp.<date>.log file");
        let line = fs::read_to_string(file).unwrap();
        let value: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
        assert_eq!(value["level"], "INFO");
        assert_eq!(value["fields"]["message"], "library opened");
        assert_eq!(value["fields"]["frame_id"], 7);
        fs::remove_dir_all(&dir).unwrap();
    }
}
