//! Panic hook and crash notes (docs/specification.md section 12).
//!
//! A panic writes `crash-<unix ms>-<pid>.txt` next to the logs. On the next start the notes are
//! collected once, logged, and renamed to `.reported`; the UI is told how many there were.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use specta::Type;

const NOTE_PREFIX: &str = "crash-";
const NOTE_SUFFIX: &str = ".txt";
const REPORTED_SUFFIX: &str = ".reported";

/// A crash from an earlier run, as the UI sees it. Details stay in the log and the note file.
#[derive(Debug, Clone, Serialize, Type, PartialEq, Eq)]
pub struct CrashNote {
    pub file: String,
}

/// Writes a crash note into `dir` and returns its path.
pub fn write_note(dir: &Path, panic_text: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let path = dir.join(format!(
        "{NOTE_PREFIX}{millis}-{}{NOTE_SUFFIX}",
        std::process::id()
    ));
    let body = format!(
        "version: {}\ntime_unix_ms: {millis}\n{panic_text}\n",
        env!("CARGO_PKG_VERSION")
    );
    fs::write(&path, body)?;
    Ok(path)
}

/// Logs the panic, writes a crash note, then runs the previous hook (default message on stderr).
pub fn install_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = info.to_string();
        tracing::error!(target: "panic", "{text}");
        if let Err(err) = write_note(&dir, &text) {
            tracing::error!(%err, "could not write the crash note");
        }
        previous(info);
    }));
}

/// Finds crash notes not yet reported, marks them reported and returns them (oldest first).
pub fn take_unreported(dir: &Path) -> io::Result<Vec<CrashNote>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(NOTE_PREFIX) && name.ends_with(NOTE_SUFFIX) {
            found.push((name, entry.path()));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    let mut notes = Vec::with_capacity(found.len());
    for (name, path) in found {
        let text = fs::read_to_string(&path).unwrap_or_default();
        tracing::warn!(file = %name, note = %text.trim(), "previous run ended in a panic");
        fs::rename(
            &path,
            path.with_file_name(format!("{name}{REPORTED_SUFFIX}")),
        )?;
        notes.push(CrashNote { file: name });
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHILD_DIR_ENV: &str = "GTP_CRASH_CHILD_DIR";
    const TEST_NAME: &str = "crash::tests::a_panic_leaves_a_note_that_the_next_start_reports_once";

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gtp-crash-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_panic_leaves_a_note_that_the_next_start_reports_once() {
        // Child mode: the parent re-runs this test in a subprocess with the env var set.
        if let Some(dir) = std::env::var_os(CHILD_DIR_ENV) {
            install_hook(PathBuf::from(dir));
            panic!("deliberate panic for the crash-note test");
        }

        let dir = temp_dir("subprocess");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--test-threads=1"])
            .env(CHILD_DIR_ENV, &dir)
            .status()
            .unwrap();
        assert!(!status.success(), "the child must have panicked");

        let notes = take_unreported(&dir).unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].file.starts_with("crash-"));
        let kept = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .find(|n| n.ends_with(REPORTED_SUFFIX))
            .expect("the note is renamed to .reported");
        let text = fs::read_to_string(dir.join(kept)).unwrap();
        assert!(text.contains("deliberate panic"));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));

        assert!(
            take_unreported(&dir).unwrap().is_empty(),
            "reported once only"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_directory_means_no_notes() {
        let dir = temp_dir("missing");
        assert!(take_unreported(&dir).unwrap().is_empty());
    }

    #[test]
    fn notes_are_returned_oldest_first_and_other_files_are_ignored() {
        let dir = temp_dir("order");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("crash-200-1.txt"), "b").unwrap();
        fs::write(dir.join("crash-100-1.txt"), "a").unwrap();
        fs::write(dir.join("gtp.2026-10-30.log"), "log").unwrap();

        let files: Vec<String> = take_unreported(&dir)
            .unwrap()
            .into_iter()
            .map(|n| n.file)
            .collect();
        assert_eq!(files, ["crash-100-1.txt", "crash-200-1.txt"]);
        assert!(dir.join("gtp.2026-10-30.log").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
