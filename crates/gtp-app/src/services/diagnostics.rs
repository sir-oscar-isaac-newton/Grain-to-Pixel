//! Diagnostics export (specification sections 12 and 14): one zip a friend can send by hand when
//! something goes wrong. It holds the logs and crash notes, the app and system versions, the
//! catalog schema version, the feature flags, the activation counters and the last job errors.
//! Never an image, and never the library's or the user's folder names: those are replaced by
//! `<library>` and `<home>` in everything copied. Nothing is uploaded.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use gtp_core::{AppError, ErrorCode};
use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::jobs::JobErrorEntry;
use crate::paths::display_path;
use crate::services::counters::{self, Counters};
use crate::services::library::Library;
use crate::services::settings::Flags;

/// The most of one log file that goes into the zip (its end, where the latest lines are).
pub const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
/// The most of all log files together.
pub const MAX_TOTAL_LOG_BYTES: u64 = 16 * 1024 * 1024;

/// What goes into the zip.
pub struct Source<'a> {
    pub app_version: &'a str,
    /// The folder with the log files and crash notes.
    pub log_dir: &'a Path,
    pub flags: &'a Flags,
    /// The open library, if any; without one there are no counters and no schema version.
    pub library: Option<&'a Library>,
    pub job_errors: &'a [JobErrorEntry],
}

#[derive(Serialize)]
struct Info<'a> {
    app_version: &'a str,
    os: &'static str,
    arch: &'static str,
    generated_at_unix_ms: u64,
    /// The newest catalog schema this app version understands.
    app_schema_version: u32,
    /// The schema version of the open library.
    library_schema_version: Option<u32>,
    library_read_only: Option<bool>,
    flags: &'a Flags,
    counters: Option<Counters>,
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

fn zip_error(err: zip::result::ZipError) -> AppError {
    match err {
        zip::result::ZipError::Io(err) => AppError::from_io(&err),
        other => AppError::internal(format!("could not write the zip: {other}")),
    }
}

/// The folder names to hide: each with the text that replaces it. Longest first, because the
/// library usually lies inside the home folder.
fn hidden_paths(library: Option<&Library>) -> Vec<(String, &'static str)> {
    let mut hidden = Vec::new();
    if let Some(library) = library {
        hidden.push((display_path(library.root()), "<library>"));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    if let Some(home) = home {
        hidden.push((display_path(Path::new(&home)), "<home>"));
    }
    hidden.retain(|(path, _)| path.len() > 3);
    hidden.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    hidden
}

/// `text` with every hidden folder replaced, whichever way the slashes are written (and with the
/// backslashes doubled, as they are inside JSON).
fn scrub(text: &str, hidden: &[(String, &'static str)]) -> String {
    let mut text = text.to_owned();
    for (path, token) in hidden {
        let forward = path.replace('\\', "/");
        let doubled = path.replace('\\', "\\\\");
        for form in [path.as_str(), forward.as_str(), doubled.as_str()] {
            text = text.replace(form, token);
        }
    }
    text
}

/// The last `MAX_LOG_BYTES` of a file, from the start of a whole line when it was cut.
fn read_tail(path: &Path, len: u64) -> Result<String, AppError> {
    let mut file = File::open(path).map_err(|err| AppError::from_io(&err))?;
    let cut = len > MAX_LOG_BYTES;
    if cut {
        file.seek(SeekFrom::Start(len - MAX_LOG_BYTES))
            .map_err(|err| AppError::from_io(&err))?;
    }
    let mut bytes = Vec::new();
    file.take(MAX_LOG_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|err| AppError::from_io(&err))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(match (cut, text.find('\n')) {
        (true, Some(at)) => text[at + 1..].to_owned(),
        _ => text.into_owned(),
    })
}

/// Regular files of the log folder, newest first.
fn log_files(dir: &Path) -> Vec<(String, std::path::PathBuf, u64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified = meta.modified().ok().unwrap_or(UNIX_EPOCH);
            Some((
                modified,
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
                meta.len(),
            ))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    files
        .into_iter()
        .map(|(_, name, path, len)| (name, path, len))
        .collect()
}

fn add_text(
    zip: &mut ZipWriter<File>,
    name: &str,
    text: &str,
    options: SimpleFileOptions,
) -> Result<(), AppError> {
    zip.start_file(name, options).map_err(zip_error)?;
    zip.write_all(text.as_bytes())
        .map_err(|err| AppError::from_io(&err))
}

fn json<T: Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_string_pretty(value)
        .map_err(|err| AppError::internal(format!("could not write the diagnostics: {err}")))
}

fn write_zip(source: &Source<'_>, file: File) -> Result<(), AppError> {
    let hidden = hidden_paths(source.library);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(file);

    let (schema, read_only, counters) = match source.library {
        Some(library) => (
            Some(library.read(|c| c.schema_version())?),
            Some(library.is_read_only()),
            Some(counters::read(library)?),
        ),
        None => (None, None, None),
    };
    let info = Info {
        app_version: source.app_version,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        generated_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        app_schema_version: gtp_catalog::SCHEMA_VERSION,
        library_schema_version: schema,
        library_read_only: read_only,
        flags: source.flags,
        counters,
    };
    add_text(
        &mut zip,
        "info.json",
        &scrub(&json(&info)?, &hidden),
        options,
    )?;
    add_text(
        &mut zip,
        "job-errors.json",
        &scrub(&json(&source.job_errors)?, &hidden),
        options,
    )?;

    let mut budget = MAX_TOTAL_LOG_BYTES;
    for (name, path, len) in log_files(source.log_dir) {
        if budget == 0 {
            break;
        }
        // A file that cannot be read (being rotated, say) is left out; the rest still helps.
        let Ok(text) = read_tail(&path, len) else {
            continue;
        };
        budget = budget.saturating_sub(text.len() as u64);
        add_text(
            &mut zip,
            &format!("logs/{name}"),
            &scrub(&text, &hidden),
            options,
        )?;
    }
    zip.finish()
        .map_err(zip_error)?
        .sync_all()
        .map_err(|err| AppError::from_io(&err))
}

/// Writes the diagnostics zip to `dest`, which must be a `.zip` file in an existing folder. The
/// zip is written beside it first and moved into place, so a failure leaves no half-written file.
pub fn export(source: &Source<'_>, dest: &Path) -> Result<(), AppError> {
    let is_zip = dest
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
    if !is_zip {
        return Err(invalid("Save the diagnostics as a .zip file."));
    }
    match dest.parent() {
        Some(parent) if parent.as_os_str().is_empty() || parent.is_dir() => {}
        _ => return Err(invalid("Choose an existing folder for the diagnostics.")),
    }
    if dest.is_dir() {
        return Err(invalid("Choose a file name, not a folder."));
    }
    let part = dest.with_extension("zip.part");
    let file = File::create(&part).map_err(|err| AppError::from_io(&err))?;
    let outcome = write_zip(source, file)
        .and_then(|()| fs::rename(&part, dest).map_err(|err| AppError::from_io(&err)));
    if outcome.is_err() {
        let _ = fs::remove_file(&part);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gtp_core::FrameId;
    use zip::ZipArchive;

    use super::*;

    struct Setup {
        dir: PathBuf,
        logs: PathBuf,
        library: Library,
    }

    impl Setup {
        fn new(label: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gtp-diag-{label}-{}-{}",
                std::process::id(),
                FrameId::new()
            ));
            let logs = dir.join("logs");
            fs::create_dir_all(&logs).unwrap();
            let library = Library::create(&dir.join("lib"), "Diag").unwrap();
            Self { dir, logs, library }
        }

        /// Closes the library first: Windows will not delete a database file that is open.
        fn finish(self) {
            let Self { dir, library, .. } = self;
            drop(library);
            fs::remove_dir_all(dir).unwrap();
        }

        fn export(&self, library: bool, errors: &[JobErrorEntry]) -> ZipArchive<File> {
            let dest = self.dir.join("diagnostics.zip");
            let source = Source {
                app_version: "9.9.9",
                log_dir: &self.logs,
                flags: &Flags::default(),
                library: library.then_some(&self.library),
                job_errors: errors,
            };
            export(&source, &dest).unwrap();
            ZipArchive::new(File::open(dest).unwrap()).unwrap()
        }
    }

    fn text(zip: &mut ZipArchive<File>, name: &str) -> String {
        let mut out = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
        out
    }

    fn names(zip: &mut ZipArchive<File>) -> Vec<String> {
        let mut names: Vec<String> = zip
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_zip_holds_versions_flags_counters_errors_and_logs_and_nothing_else() {
        let setup = Setup::new("contents");
        fs::write(setup.logs.join("app.log"), "INFO started\n").unwrap();
        fs::write(setup.logs.join("crash-1-2.txt"), "version: 9\npanic\n").unwrap();
        counters::count_import(&setup.library);
        let error = JobErrorEntry {
            at_unix_ms: 7,
            job: "Export".into(),
            code: "Io".into(),
            message: "disk full".into(),
            details: None,
        };
        let mut zip = setup.export(true, std::slice::from_ref(&error));

        assert_eq!(
            names(&mut zip),
            [
                "info.json",
                "job-errors.json",
                "logs/app.log",
                "logs/crash-1-2.txt"
            ]
        );
        let info: serde_json::Value = serde_json::from_str(&text(&mut zip, "info.json")).unwrap();
        assert_eq!(info["app_version"], "9.9.9");
        assert_eq!(info["app_schema_version"], gtp_catalog::SCHEMA_VERSION);
        assert_eq!(info["library_schema_version"], gtp_catalog::SCHEMA_VERSION);
        assert_eq!(info["library_read_only"], false);
        assert_eq!(info["counters"]["frames_imported"], 1);
        assert_eq!(info["flags"]["update_check"], true);
        assert!(info["os"].is_string() && info["arch"].is_string());
        let errors: serde_json::Value =
            serde_json::from_str(&text(&mut zip, "job-errors.json")).unwrap();
        assert_eq!(errors[0]["message"], "disk full");
        assert_eq!(text(&mut zip, "logs/app.log"), "INFO started\n");
        drop(zip);
        assert!(!setup.dir.join("diagnostics.zip.part").exists());
        setup.finish();
    }

    #[test]
    fn without_a_library_there_are_no_counters_or_schema_version() {
        let setup = Setup::new("nolib");
        let mut zip = setup.export(false, &[]);
        let info: serde_json::Value = serde_json::from_str(&text(&mut zip, "info.json")).unwrap();
        assert!(info["counters"].is_null());
        assert!(info["library_schema_version"].is_null());
        assert_eq!(names(&mut zip), ["info.json", "job-errors.json"]);
        drop(zip);
        setup.finish();
    }

    #[test]
    fn the_library_folder_never_appears_in_the_zip() {
        let setup = Setup::new("paths");
        let root = display_path(setup.library.root());
        let forward = root.replace('\\', "/");
        fs::write(
            setup.logs.join("app.log"),
            format!("INFO opened {root}\\originals\nWARN also {forward}/thumbs\n"),
        )
        .unwrap();
        let error = JobErrorEntry {
            at_unix_ms: 1,
            job: "Import".into(),
            code: "Io".into(),
            message: "cannot read".into(),
            details: Some(format!("{root}\\originals\\a.tif")),
        };
        let mut zip = setup.export(true, &[error]);
        let log = text(&mut zip, "logs/app.log");
        let errors = text(&mut zip, "job-errors.json");
        assert!(log.contains("<library>"), "{log}");
        assert!(errors.contains("<library>"), "{errors}");
        for name in names(&mut zip) {
            let body = text(&mut zip, &name);
            assert!(!body.contains(&forward), "{name}: {body}");
            assert!(!body.contains(&root), "{name}: {body}");
            assert!(
                !body.contains(&root.replace('\\', "\\\\")),
                "{name}: {body}"
            );
        }
        drop(zip);
        setup.finish();
    }

    #[test]
    fn the_originals_of_the_library_are_not_read_into_the_zip() {
        let setup = Setup::new("pixels");
        let originals = setup.library.root().join("originals");
        fs::create_dir_all(&originals).unwrap();
        fs::write(originals.join("scan.tif"), vec![7u8; 300_000]).unwrap();
        let mut zip = setup.export(true, &[]);
        assert_eq!(names(&mut zip), ["info.json", "job-errors.json"]);
        let total: u64 = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().size())
            .sum();
        assert!(total < 10_000, "{total} bytes");
        drop(zip);
        setup.finish();
    }

    #[test]
    fn a_big_log_is_cut_to_its_end_at_a_line_start() {
        let setup = Setup::new("big");
        let line = "INFO a line of the log that is long enough to matter\n";
        let count = (MAX_LOG_BYTES as usize / line.len()) * 2;
        let mut body = String::new();
        for i in 0..count {
            body.push_str(&format!("{i:08} {line}"));
        }
        fs::write(setup.logs.join("app.log"), &body).unwrap();
        let mut zip = setup.export(false, &[]);
        let kept = text(&mut zip, "logs/app.log");
        assert!(kept.len() as u64 <= MAX_LOG_BYTES);
        assert!(kept.len() as u64 > MAX_LOG_BYTES / 2);
        assert!(kept.ends_with(&format!("{:08} {line}", count - 1)));
        assert!(kept.starts_with(char::is_numeric) && kept.as_bytes()[8] == b' ');
        drop(zip);
        setup.finish();
    }

    #[test]
    fn a_missing_log_folder_still_gives_a_zip() {
        let setup = Setup::new("nologs");
        fs::remove_dir_all(&setup.logs).unwrap();
        let mut zip = setup.export(true, &[]);
        assert_eq!(names(&mut zip), ["info.json", "job-errors.json"]);
        drop(zip);
        setup.finish();
    }

    #[test]
    fn a_bad_destination_is_refused_before_anything_is_written() {
        let setup = Setup::new("dest");
        let source = Source {
            app_version: "9.9.9",
            log_dir: &setup.logs,
            flags: &Flags::default(),
            library: None,
            job_errors: &[],
        };
        let not_zip = export(&source, &setup.dir.join("out.txt")).unwrap_err();
        assert_eq!(not_zip.code, ErrorCode::InvalidInput);
        let no_folder = export(&source, &setup.dir.join("missing").join("out.zip")).unwrap_err();
        assert_eq!(no_folder.code, ErrorCode::InvalidInput);
        let folder = setup.dir.join("folder.zip");
        fs::create_dir(&folder).unwrap();
        assert_eq!(
            export(&source, &folder).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert!(!setup.dir.join("out.txt").exists());
        setup.finish();
    }
}
