//! Panic hook and crash notes (docs/specification.md section 12).
//!
//! A panic writes `crash-<unix ms>-<pid>.txt` next to the logs. On the next start the notes are
//! collected once, logged, and renamed to `.reported`; the UI is told how many there were.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use gtp_app::SharedLight;
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

/// The light's part of the panic hook: a best-effort switch-off that never waits for the light's
/// lock (`SharedLight::emergency_off` uses `try_lock`, recovers a poisoned lock and catches a
/// panic of the driver), so the hook can neither deadlock nor panic. A light that could not be
/// reached is logged; a killed process or a power loss cannot be covered at all.
fn switch_light_off(light: Option<&SharedLight>) {
    if let Some(light) = light {
        if !light.emergency_off() {
            tracing::error!(target: "panic", "the light could not be switched off from the panic hook");
        }
    }
}

/// Switches the light off, logs the panic, writes a crash note (when there is a folder for it),
/// then runs the previous hook (default message on stderr).
pub fn install_hook(dir: Option<PathBuf>, light: Option<SharedLight>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // First: nothing the hook does after this may leave the light on.
        switch_light_off(light.as_ref());
        let text = info.to_string();
        tracing::error!(target: "panic", "{text}");
        if let Some(dir) = &dir {
            if let Err(err) = write_note(dir, &text) {
                tracing::error!(%err, "could not write the crash note");
            }
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
    use gtp_app::fake_capture::{FakeLight, LightInfo, LightSource, LightStatus};
    use gtp_app::shared_light::fake_ports;
    use gtp_core::AppError;

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
            install_hook(Some(PathBuf::from(dir)), None);
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

    /// A light whose `set_levels` waits for a release, to hold its lock from another thread.
    struct SlowLight {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    }

    impl LightSource for SlowLight {
        fn connect(&mut self, _: Option<&str>) -> Result<LightInfo, AppError> {
            Err(AppError::internal("unused"))
        }
        fn set_levels(&mut self, _: [u8; 3]) -> Result<(), AppError> {
            let _ = self.entered.send(());
            let _ = self
                .release
                .recv_timeout(std::time::Duration::from_secs(30));
            Ok(())
        }
        fn shutter_pulse(&mut self, _: u8) -> Result<(), AppError> {
            Ok(())
        }
        fn status(&self) -> LightStatus {
            LightStatus::default()
        }
    }

    #[test]
    fn the_panic_hook_switches_the_light_off() {
        let fake = FakeLight::new();
        let handle = fake.handle();
        let mut light = SharedLight::new(Box::new(fake), fake_ports);
        light.set_levels([44, 0, 0]).unwrap();
        switch_light_off(Some(&light));
        assert_eq!(handle.levels(), [0; 3]);
        switch_light_off(None);
    }

    #[test]
    fn the_panic_hook_does_not_deadlock_when_the_light_is_locked() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let light = SharedLight::new(
            Box::new(SlowLight {
                entered: entered_tx,
                release: release_rx,
            }),
            fake_ports,
        );
        let holder = {
            let mut light = light.clone();
            std::thread::spawn(move || light.set_levels([0; 3]))
        };
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        // The lock is held by the other thread: the hook returns instead of waiting for it.
        switch_light_off(Some(&light));
        release_tx.send(()).unwrap();
        holder.join().unwrap().unwrap();
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
