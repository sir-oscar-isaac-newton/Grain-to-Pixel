//! Export (docs/specification.md sections 6 and 11): edited frames written at full resolution as
//! 16-bit TIFF or JPEG files with the ICC profile of the chosen colour space embedded.
//!
//! One job exports the frames one after the other, so memory holds a single full-size image. Per
//! frame: decode the original (and its infrared plane, for dust removal), render it with the saved recipe (the automatic one when the frame
//! was never edited, as for thumbnails), convert and write through a temporary file and a rename,
//! and log the file in `export_log`. A frame that cannot be exported is reported in the result and
//! the others carry on; a cancel stops before the next frame or before a write, and the files
//! already written stay. An existing file is never overwritten: the name gets " (2)", " (3)", ...

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use gtp_core::recipe::OutputSpace;
use gtp_core::{
    AppError, ErrorCode, ExportResult, ExportedFile, FailedExport, FrameId, ImageF32, Recipe,
};
use gtp_edit::apply_sharpen;
use gtp_io::{encode_jpeg, encode_tiff16, JPEG_QUALITY};

use crate::jobs::JobCtx;
use crate::paths::{canonical_dir, display_path};
use crate::pipeline::{analyze, build_proxy, render, PipelineInput, RenderTarget, PROXY_EDGE};
use crate::services::counters;
use crate::services::develop::{read_ir_or_skip, read_scan};
use crate::services::library::Library;

/// Most frames one export may name.
pub const MAX_FRAMES: usize = 10_000;
/// The template used when none is given.
pub const DEFAULT_TEMPLATE: &str = "{roll}_{index:03}";

const STAGE: &str = "exporting";
/// Progress steps per frame: decode, render, write.
const STEPS: f32 = 3.0;
/// Longest file name stem kept, in characters.
const MAX_STEM_CHARS: usize = 150;
/// Most " (n)" suffixes tried before giving up on a name.
const MAX_SUFFIX: u32 = 10_000;

/// The file format of an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// 16-bit RGB, Deflate-compressed.
    Tiff16,
    /// 8-bit baseline JPEG without chroma subsampling.
    Jpeg,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Tiff16 => "tif",
            Self::Jpeg => "jpg",
        }
    }

    /// The name in `export_log`.
    fn logged(self) -> &'static str {
        match self {
            Self::Tiff16 => "tiff16",
            Self::Jpeg => "jpeg",
        }
    }
}

/// What to export and where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub space: OutputSpace,
    /// 1 to 100; used for JPEG only.
    pub jpeg_quality: u8,
    /// The folder to write into; created when it does not exist.
    pub dest_dir: String,
    /// The file name, without folders: `{roll}`, `{index}`, `{index:03}` (zero-padded to 3 digits,
    /// 1 to 9 allowed), `{frame}` and `{ext}`. The extension is added when the name lacks it.
    pub template: String,
}

/// The values a template can name.
struct NameParts<'a> {
    roll: &'a str,
    index: u32,
    frame: FrameId,
    ext: &'a str,
}

fn invalid(message: &str, details: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details.into())
}

/// The text of a template with its fields filled in. Folders and unknown fields are refused.
fn expand(template: &str, parts: &NameParts<'_>) -> Result<String, AppError> {
    if template.contains(['/', '\\']) {
        return Err(invalid(
            "A file name cannot contain folders.",
            template.to_owned(),
        ));
    }
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find(['{', '}']) {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        if tail.starts_with('}') {
            return Err(invalid("The name has a stray }.", template.to_owned()));
        }
        let Some(close) = tail.find('}') else {
            return Err(invalid(
                "The name has a { that is not closed.",
                template.to_owned(),
            ));
        };
        let field = &tail[1..close];
        match field {
            "roll" => out.push_str(parts.roll),
            "index" => out.push_str(&parts.index.to_string()),
            "frame" => out.push_str(&parts.frame.to_string()),
            "ext" => out.push_str(parts.ext),
            _ => {
                let width = field
                    .strip_prefix("index:0")
                    .and_then(|w| w.parse::<usize>().ok())
                    .filter(|w| (1..=9).contains(w));
                match width {
                    Some(width) => out.push_str(&format!("{:0width$}", parts.index)),
                    None => {
                        return Err(invalid(
                            "The name uses a field that does not exist.",
                            format!("{{{field}}}"),
                        ))
                    }
                }
            }
        }
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A file name that is safe on Windows and macOS: no characters they refuse, no reserved device
/// names, no trailing dots or spaces, a stem of reasonable length, and `ext` at the end.
fn file_name(template: &str, parts: &NameParts<'_>) -> Result<String, AppError> {
    let mut name = expand(template, parts)?;
    let suffix = format!(".{}", parts.ext);
    if !name.to_lowercase().ends_with(&suffix) {
        name.push_str(&suffix);
    }
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = cleaned[..cleaned.len() - suffix.len()]
        .trim_matches(|c: char| c == ' ' || c == '.')
        .chars()
        .take(MAX_STEM_CHARS)
        .collect::<String>();
    let stem = if stem.is_empty() {
        "frame".to_owned()
    } else if RESERVED.contains(&stem.to_uppercase().as_str()) {
        format!("_{stem}")
    } else {
        stem
    };
    Ok(format!("{stem}{suffix}"))
}

/// `dir/name`, or the first of `dir/stem (2).ext`, `dir/stem (3).ext`, ... that does not exist.
fn free_path(dir: &Path, name: &str) -> Result<PathBuf, AppError> {
    let first = dir.join(name);
    if !first.exists() {
        return Ok(first);
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 2..=MAX_SUFFIX {
        let candidate = dir.join(format!("{stem} ({n}).{ext}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::new(ErrorCode::Io, "No free file name was found.")
        .with_details(first.display().to_string()))
}

impl ExportOptions {
    /// Checks the options before any work starts, and creates the destination folder. Returns the\n    /// folder in its canonical form.
    pub fn validate(&self) -> Result<PathBuf, AppError> {
        if !JPEG_QUALITY.contains(&self.jpeg_quality) {
            return Err(invalid(
                "JPEG quality runs from 1 to 100.",
                self.jpeg_quality.to_string(),
            ));
        }
        if self.dest_dir.trim().is_empty() {
            return Err(invalid("Choose a folder to export into.", String::new()));
        }
        // A template that cannot make a name is found now, not on the first frame.
        file_name(
            &self.template,
            &NameParts {
                roll: "Roll",
                index: 1,
                frame: FrameId::new(),
                ext: self.format.extension(),
            },
        )?;
        let dir = PathBuf::from(&self.dest_dir);
        std::fs::create_dir_all(&dir).map_err(|err| AppError::from_io(&err))?;
        canonical_dir(&dir)
    }
}

/// How one frame ended, short of success.
enum FrameError {
    /// The frame is left out; the others carry on.
    Failed(AppError),
    /// The job was cancelled.
    Cancelled,
}

impl From<AppError> for FrameError {
    fn from(err: AppError) -> Self {
        if err.code == ErrorCode::Cancelled {
            Self::Cancelled
        } else {
            Self::Failed(err)
        }
    }
}

/// Renders one frame at full size and writes it; returns the file.
fn export_one(
    library: &Library,
    frame_id: FrameId,
    options: &ExportOptions,
    dust_removal: bool,
    dir: &Path,
    ctx: &JobCtx,
    progress: &dyn Fn(f32),
) -> Result<PathBuf, FrameError> {
    let (frame, sources, roll, saved) = library
        .read(|c| {
            let frame = c.get_frame(frame_id)?;
            let sources = c.sources_of(frame_id)?;
            let roll = c.get_roll(frame.roll_id)?;
            let saved = c.load_recipe(frame_id)?;
            Ok((frame, sources, roll, saved))
        })
        .map_err(FrameError::from)?;
    let name = file_name(
        &options.template,
        &NameParts {
            roll: &roll.name,
            index: frame.index_in_roll,
            frame: frame.id,
            ext: options.format.extension(),
        },
    )?;

    let full: ImageF32 = read_scan(library, &sources)?;
    // The infrared plane, at full size like the scan; a plane that cannot be used is skipped.
    let ir = read_ir_or_skip(library, &sources, &frame, (full.width, full.height))
        .filter(|_| dust_removal)
        .map(Arc::new);
    progress(1.0);
    ctx.check_cancelled()?;
    let recipe: Recipe = match saved {
        Some(recipe) => recipe,
        None => {
            // Never edited: the automatic recipe, found on a proxy (its values do not depend on
            // the size; the crop is in fractions).
            let proxy = PipelineInput {
                rgb: Arc::new(build_proxy(&full, PROXY_EDGE)?),
                ir: None,
                film_base: roll.film_base_rgb,
            };
            analyze(&proxy)?.auto_recipe
        }
    };
    let input = PipelineInput {
        rgb: Arc::new(full),
        ir,
        film_base: roll.film_base_rgb,
    };
    let mut image = render(&input, &recipe, RenderTarget::Full)?;
    drop(input);
    // Output sharpening belongs here, on the image at its final size, and nowhere else: the render
    // that previews and thumbnails use never sharpens.
    let sharpen = recipe.clone().clamped().output.sharpen;
    if sharpen.amount > 0.0 {
        apply_sharpen(&mut image, &sharpen)?;
    }
    progress(2.0);
    ctx.check_cancelled()?;

    let path = free_path(dir, &name)?;
    match options.format {
        ExportFormat::Tiff16 => encode_tiff16(&image, options.space, &path)?,
        ExportFormat::Jpeg => encode_jpeg(&image, options.space, options.jpeg_quality, &path)?,
    }
    progress(3.0);
    Ok(path)
}

/// Exports `frame_ids` as described in the module docs; `dust_removal` is the feature flag of that name. Ends with `Cancelled` when cancelled
/// (files already written stay), or with the error of the options or the library.
pub fn export_frames(
    library: &Library,
    frame_ids: &[FrameId],
    options: &ExportOptions,
    dust_removal: bool,
    ctx: &JobCtx,
) -> Result<ExportResult, AppError> {
    if frame_ids.is_empty() || frame_ids.len() > MAX_FRAMES {
        return Err(invalid(
            "Choose between 1 and 10,000 frames to export.",
            format!("{} frames", frame_ids.len()),
        ));
    }
    let dir = options.validate()?;
    let mut result = ExportResult::default();
    let total = frame_ids.len() as f32;
    for (i, frame_id) in frame_ids.iter().enumerate() {
        ctx.check_cancelled()?;
        let started = Instant::now();
        let progress = |step: f32| ctx.progress(STAGE, (i as f32 + step / STEPS) / total);
        progress(0.0);
        match export_one(
            library,
            *frame_id,
            options,
            dust_removal,
            &dir,
            ctx,
            &progress,
        ) {
            Ok(path) => {
                let shown = display_path(&path);
                // The file is written; a library that cannot take the log row (read-only) does not
                // undo it.
                if let Err(err) =
                    library.write(|c| c.log_export(*frame_id, options.format.logged(), &shown))
                {
                    tracing::warn!(frame = %frame_id, error = %err.message, "export not logged");
                }
                counters::count_export(library);
                result.files.push(ExportedFile {
                    frame_id: *frame_id,
                    path: shown,
                });
                result
                    .frame_ms
                    .push(u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX));
            }
            Err(FrameError::Cancelled) => {
                return Err(AppError::new(
                    ErrorCode::Cancelled,
                    "The job was cancelled.",
                ))
            }
            Err(FrameError::Failed(err)) => {
                tracing::warn!(frame = %frame_id, error = %err.message, "frame not exported");
                result.failed.push(FailedExport {
                    frame_id: *frame_id,
                    message: err.message,
                    details: err.details,
                });
            }
        }
    }
    ctx.progress(STAGE, 1.0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use gtp_core::testing::synthetic::{Params, Pattern};
    use gtp_core::{JobId, JobResult, SourceRole};
    use gtp_io::{icc_profile, read_tiff16, Limits};

    use super::*;
    use crate::jobs::{CancelFlag, JobSink};
    use crate::services::testkit::Fixture;

    fn parts(index: u32) -> NameParts<'static> {
        NameParts {
            roll: "Summer 26",
            index,
            frame: FrameId::new(),
            ext: "tif",
        }
    }

    fn options(dir: &Path, format: ExportFormat, space: OutputSpace) -> ExportOptions {
        ExportOptions {
            format,
            space,
            jpeg_quality: 90,
            dest_dir: dir.display().to_string(),
            template: DEFAULT_TEMPLATE.into(),
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        found
    }

    /// Whether the profile is in the file. Every profile the colour library writes carries its
    /// creation time in bytes 24 to 36, which differs between two calls, so those are skipped.
    fn contains(haystack: &[u8], profile: &[u8]) -> bool {
        const DATE: std::ops::Range<usize> = 24..36;
        haystack.windows(profile.len()).any(|w| {
            w[..DATE.start] == profile[..DATE.start] && w[DATE.end..] == profile[DATE.end..]
        })
    }

    fn run(
        t: &Fixture,
        ids: &[FrameId],
        options: &ExportOptions,
    ) -> Result<ExportResult, AppError> {
        let (ctx, _) = JobCtx::standalone();
        export_frames(&t.library, ids, options, true, &ctx)
    }

    /// Cancels the job once its progress reaches `at`.
    struct CancelAt {
        at: f32,
        flag: Mutex<Option<CancelFlag>>,
    }

    impl JobSink for CancelAt {
        fn progress(&self, _: JobId, _: &str, fraction: f32) {
            if fraction >= self.at {
                if let Some(flag) = self.flag.lock().unwrap().as_ref() {
                    flag.cancel();
                }
            }
        }
        fn done(&self, _: JobId, _: JobResult) {}
        fn error(&self, _: JobId, _: AppError) {}
    }

    fn run_cancelling_at(
        t: &Fixture,
        ids: &[FrameId],
        options: &ExportOptions,
        at: f32,
    ) -> Result<ExportResult, AppError> {
        let sink = Arc::new(CancelAt {
            at,
            flag: Mutex::new(None),
        });
        let (ctx, flag) = JobCtx::standalone_with(sink.clone());
        *sink.flag.lock().unwrap() = Some(flag);
        export_frames(&t.library, ids, options, true, &ctx)
    }

    #[test]
    fn names_come_from_the_template() {
        let name = |template: &str, index| file_name(template, &parts(index)).unwrap();
        assert_eq!(name(DEFAULT_TEMPLATE, 7), "Summer 26_007.tif");
        assert_eq!(name("{roll}-{index}", 12), "Summer 26-12.tif");
        assert_eq!(name("{index:05}_{roll}.{ext}", 3), "00003_Summer 26.tif");
        assert_eq!(name("Scan {index:02}.TIF", 4), "Scan 04.tif");
        assert_eq!(name("plain", 1), "plain.tif");
        let named = parts(1);
        assert_eq!(
            file_name("{frame}", &named).unwrap(),
            format!("{}.tif", named.frame)
        );
    }

    #[test]
    fn a_name_is_safe_on_windows_and_macos() {
        let name = |roll: &str, template: &str| {
            file_name(template, &NameParts { roll, ..parts(1) }).unwrap()
        };
        assert_eq!(name("a:b*c?", "{roll}"), "a_b_c_.tif");
        assert_eq!(name("Holiday <1>", "{roll}"), "Holiday _1_.tif");
        assert_eq!(name("  .. ", "{roll}"), "frame.tif");
        assert_eq!(name("nul", "{roll}"), "_nul.tif");
        assert_eq!(name("Con", "{roll}"), "_Con.tif");
        assert_eq!(name("end. ", "{roll}"), "end.tif");
        assert_eq!(name("tab\there", "{roll}"), "tab_here.tif");
        assert_eq!(
            name(&"x".repeat(400), "{roll}"),
            format!("{}.tif", "x".repeat(150))
        );
    }

    #[test]
    fn a_template_with_folders_or_unknown_fields_is_refused() {
        for template in [
            "../{roll}",
            "a/b",
            "a\\b",
            "{nope}",
            "{index:0}",
            "{index:010}",
            "{index:3}",
            "{roll",
            "roll}",
            "{",
        ] {
            let err = file_name(template, &parts(1)).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{template}");
        }
    }

    #[test]
    fn an_existing_name_gets_a_number_and_nothing_is_overwritten() {
        let dir = std::env::temp_dir().join(format!("gtp-export-free-{}", FrameId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.tif").unwrap(), dir.join("a.tif"));
        std::fs::write(dir.join("a.tif"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.tif").unwrap(), dir.join("a (2).tif"));
        std::fs::write(dir.join("a (2).tif"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.tif").unwrap(), dir.join("a (3).tif"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_options_are_checked_before_anything_is_done() {
        let t = Fixture::new("export-options");
        let id = t.frame(1, 200, 140);
        let good = options(&t.dir.join("out"), ExportFormat::Jpeg, OutputSpace::Srgb);
        for bad in [
            ExportOptions {
                jpeg_quality: 0,
                ..good.clone()
            },
            ExportOptions {
                jpeg_quality: 101,
                ..good.clone()
            },
            ExportOptions {
                dest_dir: "  ".into(),
                ..good.clone()
            },
            ExportOptions {
                template: "../x".into(),
                ..good.clone()
            },
        ] {
            let err = run(&t, &[id], &bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        }
        assert!(!t.dir.join("out").exists(), "nothing was created");
        for ids in [vec![], vec![id; MAX_FRAMES + 1]] {
            let err = run(&t, &ids, &good).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
        }
    }

    #[test]
    fn every_space_and_format_is_written_with_its_profile_and_logged() {
        let t = Fixture::new("export-profiles");
        let id = t.frame(3, 200, 140);
        for space in [
            OutputSpace::Srgb,
            OutputSpace::AdobeRgb,
            OutputSpace::Prophoto,
        ] {
            let profile = icc_profile(space).unwrap();
            for format in [ExportFormat::Tiff16, ExportFormat::Jpeg] {
                let out = t.dir.join(format!("out-{space:?}-{format:?}"));
                let result = run(&t, &[id], &options(&out, format, space)).unwrap();
                assert!(result.failed.is_empty(), "{:?}", result.failed);
                assert_eq!(result.files.len(), 1);
                assert_eq!(result.frame_ms.len(), 1);

                let expected = match format {
                    ExportFormat::Tiff16 => "Roll_003.tif",
                    ExportFormat::Jpeg => "Roll_003.jpg",
                };
                assert_eq!(names(&out), [expected], "{space:?} {format:?}");
                let bytes = std::fs::read(out.join(expected)).unwrap();
                assert!(
                    contains(&bytes, &profile),
                    "{space:?} {format:?}: profile embedded"
                );
                for other in [
                    OutputSpace::Srgb,
                    OutputSpace::AdobeRgb,
                    OutputSpace::Prophoto,
                ] {
                    if other != space {
                        assert!(
                            !contains(&bytes, &icc_profile(other).unwrap()),
                            "{space:?} {format:?} carries the profile of {other:?}"
                        );
                    }
                }
                match format {
                    ExportFormat::Tiff16 => {
                        let tiff = read_tiff16(&out.join(expected), &Limits::default()).unwrap();
                        assert_eq!(tiff.channels, 3);
                        assert!(tiff.width > 0 && tiff.width <= 200 && tiff.height <= 140);
                    }
                    ExportFormat::Jpeg => assert_eq!(bytes[..2], [0xFF, 0xD8]),
                }
                let written = Path::new(&result.files[0].path);
                assert!(written.is_absolute() && written.exists(), "{written:?}");
                assert_eq!(written.file_name().unwrap().to_str().unwrap(), expected);
            }
        }
        let logged = t.library.read(|c| c.exports_of(id)).unwrap();
        assert_eq!(logged.len(), 6);
        assert!(logged
            .iter()
            .any(|r| r.format == "tiff16" && r.path.ends_with("Roll_003.tif")));
        assert!(logged
            .iter()
            .any(|r| r.format == "jpeg" && r.path.ends_with("Roll_003.jpg")));
    }

    /// The pixels of a TIFF the export wrote.
    fn pixels(path: &Path) -> Vec<u16> {
        read_tiff16(path, &Limits::default()).unwrap().data
    }

    fn export_to(t: &Fixture, id: FrameId, name: &str) -> PathBuf {
        let out = t.dir.join(name);
        run(
            t,
            &[id],
            &options(&out, ExportFormat::Tiff16, OutputSpace::Srgb),
        )
        .unwrap();
        out.join("Roll_001.tif")
    }

    /// An export of one frame with the dust removal flag as given.
    fn export_dust(t: &Fixture, id: FrameId, name: &str, dust_removal: bool) -> PathBuf {
        let out = t.dir.join(name);
        let (ctx, _) = JobCtx::standalone();
        export_frames(
            &t.library,
            &[id],
            &options(&out, ExportFormat::Tiff16, OutputSpace::Srgb),
            dust_removal,
            &ctx,
        )
        .unwrap();
        let written = std::fs::read_dir(&out).unwrap().next().unwrap().unwrap();
        written.path()
    }

    #[test]
    fn dust_is_removed_from_the_file_only_when_the_flag_is_on_and_the_frame_has_a_plane() {
        let t = Fixture::new("export-dust");
        let params = Params {
            width: 400,
            height: 300,
            rebate: [28; 4],
            dust_specks: 6,
            with_ir: true,
            ..Params::default()
        };
        let id = t.frame_params(1, &params);
        let recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        t.library.write(|c| c.save_recipe(id, &recipe)).unwrap();
        let on = pixels(&export_dust(&t, id, "on", true));
        let off = pixels(&export_dust(&t, id, "off", false));
        assert_eq!(on.len(), off.len());
        assert_ne!(on, off, "the specks were filled");

        // Without the flag the file is what a frame with no plane gives.
        let no_plane = Params {
            with_ir: false,
            ..params
        };
        let bare = t.frame_params(2, &no_plane);
        t.library.write(|c| c.save_recipe(bare, &recipe)).unwrap();
        let plain = pixels(&export_dust(&t, bare, "bare-on", true));
        assert_eq!(plain, pixels(&export_dust(&t, bare, "bare-off", false)));
    }

    #[test]
    fn a_missing_infrared_file_does_not_stop_the_export() {
        let t = Fixture::new("export-dust-missing");
        let params = Params {
            width: 400,
            height: 300,
            rebate: [28; 4],
            dust_specks: 6,
            with_ir: true,
            ..Params::default()
        };
        let id = t.frame_params(1, &params);
        let recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        t.library.write(|c| c.save_recipe(id, &recipe)).unwrap();
        let sources = t.library.read(|c| c.sources_of(id)).unwrap();
        let ir = sources.iter().find(|s| s.role == SourceRole::Ir).unwrap();
        std::fs::remove_file(t.library.resolve(Path::new(&ir.rel_path)).unwrap()).unwrap();
        let dusty = pixels(&export_dust(&t, id, "missing", true));
        let off = pixels(&export_dust(&t, id, "off", false));
        assert_eq!(dusty, off, "exported as if there were no plane");
    }
    #[test]
    fn output_sharpening_changes_the_file_only_when_its_amount_is_above_zero() {
        let t = Fixture::new("export-sharpen");
        // Steps, so there are edges to sharpen.
        let id = t.frame_with(1, 200, 140, Pattern::Wedge);
        let mut recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        t.library.write(|c| c.save_recipe(id, &recipe)).unwrap();
        let plain = pixels(&export_to(&t, id, "plain"));

        // An amount of 0 is the same file, whatever the radius.
        recipe.output.sharpen.radius_px = 2.5;
        t.library.write(|c| c.save_recipe(id, &recipe)).unwrap();
        assert_eq!(pixels(&export_to(&t, id, "zero")), plain);

        recipe.output.sharpen.amount = 120.0;
        t.library.write(|c| c.save_recipe(id, &recipe)).unwrap();
        let sharp = pixels(&export_to(&t, id, "sharp"));
        assert_eq!(sharp.len(), plain.len());
        assert_ne!(sharp, plain);
        // More local contrast: the picture's pixels differ more from their neighbours.
        let activity = |data: &[u16]| -> u64 {
            data.chunks(3)
                .zip(data.chunks(3).skip(1))
                .map(|(a, b)| u64::from(a[1].abs_diff(b[1])))
                .sum()
        };
        assert!(activity(&sharp) > activity(&plain));
    }

    #[test]
    fn the_preview_render_never_sharpens() {
        let t = Fixture::new("export-sharpen-preview");
        let id = t.frame(1, 600, 400);
        let mut recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        let plain = t.service.render(&t.library, id, &recipe, 600, 1).unwrap();
        recipe.output.sharpen.amount = 150.0;
        recipe.output.sharpen.radius_px = 3.0;
        let sharp = t.service.render(&t.library, id, &recipe, 600, 2).unwrap();
        // The same picture (the header holds the sequence number, which differs).
        let header = crate::services::develop::HEADER_BYTES;
        assert_eq!(plain[header..], sharp[header..]);
    }

    #[test]
    fn out_of_range_sharpening_in_a_saved_recipe_is_clamped_not_refused() {
        let t = Fixture::new("export-sharpen-clamp");
        let id = t.frame(1, 200, 140);
        let mut recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        recipe.output.sharpen.amount = 9000.0;
        recipe.output.sharpen.radius_px = 90.0;
        t.library.write(|c| c.save_recipe(id, &recipe)).unwrap();
        let result = run(
            &t,
            &[id],
            &options(
                &t.dir.join("clamped"),
                ExportFormat::Tiff16,
                OutputSpace::Srgb,
            ),
        )
        .unwrap();
        assert!(result.failed.is_empty(), "{:?}", result.failed);
        assert_eq!(result.files.len(), 1);
    }

    #[test]
    fn a_saved_recipe_is_used_and_a_second_export_does_not_overwrite() {
        let t = Fixture::new("export-recipe");
        let id = t.frame(1, 200, 140);
        let out = t.dir.join("out");
        let opts = options(&out, ExportFormat::Tiff16, OutputSpace::Srgb);
        run(&t, &[id], &opts).unwrap();
        let auto = std::fs::read(out.join("Roll_001.tif")).unwrap();

        // A brighter edit gives a different file, written beside the first.
        let mut recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        recipe.tone.exposure_ev = 1.0;
        t.library.write(|c| c.save_recipe(id, &recipe)).unwrap();
        run(&t, &[id], &opts).unwrap();
        assert_eq!(names(&out), ["Roll_001 (2).tif", "Roll_001.tif"]);
        assert_eq!(
            std::fs::read(out.join("Roll_001.tif")).unwrap(),
            auto,
            "untouched"
        );
        assert_ne!(std::fs::read(out.join("Roll_001 (2).tif")).unwrap(), auto);
    }

    #[test]
    fn a_frame_that_cannot_be_exported_is_reported_and_the_others_carry_on() {
        let t = Fixture::new("export-failures");
        let good = t.frame(1, 200, 140);
        let no_scan = t.bare(2);
        let unknown = FrameId::new();
        let also_good = t.frame(4, 200, 140);
        let out = t.dir.join("out");
        let result = run(
            &t,
            &[good, no_scan, unknown, also_good],
            &options(&out, ExportFormat::Jpeg, OutputSpace::Srgb),
        )
        .unwrap();
        assert_eq!(
            result.files.iter().map(|f| f.frame_id).collect::<Vec<_>>(),
            [good, also_good]
        );
        assert_eq!(
            result.failed.iter().map(|f| f.frame_id).collect::<Vec<_>>(),
            [no_scan, unknown]
        );
        assert!(result.failed.iter().all(|f| !f.message.is_empty()));
        assert_eq!(names(&out), ["Roll_001.jpg", "Roll_004.jpg"]);
        assert!(t
            .library
            .read(|c| c.exports_of(no_scan))
            .unwrap()
            .is_empty());
        let counted = counters::read(&t.library).unwrap();
        assert_eq!(
            counted.frames_exported, 2,
            "only the files that were written"
        );
        assert!(counted.first_export_at.is_some());
    }

    #[test]
    fn a_cancel_between_frames_keeps_the_finished_files_and_nothing_else() {
        let t = Fixture::new("export-cancel-between");
        let ids: Vec<FrameId> = (1..=3).map(|i| t.frame(i, 200, 140)).collect();
        let out = t.dir.join("out");
        let opts = options(&out, ExportFormat::Tiff16, OutputSpace::Srgb);
        // The first frame is done at 1/3.
        let err = run_cancelling_at(&t, &ids, &opts, 0.33).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(names(&out), ["Roll_001.tif"]);
        assert_eq!(t.library.read(|c| c.exports_of(ids[0])).unwrap().len(), 1);
        assert!(t.library.read(|c| c.exports_of(ids[1])).unwrap().is_empty());
    }

    #[test]
    fn a_cancel_in_the_middle_of_a_frame_writes_no_partial_file() {
        let t = Fixture::new("export-cancel-inside");
        let ids: Vec<FrameId> = (1..=3).map(|i| t.frame(i, 200, 140)).collect();
        let out = t.dir.join("out");
        let opts = options(&out, ExportFormat::Jpeg, OutputSpace::Srgb);
        // The second frame has been rendered at 5/9 and is about to be written.
        let err = run_cancelling_at(&t, &ids, &opts, 0.55).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(
            names(&out),
            ["Roll_001.jpg"],
            "no second file, no temporary file"
        );
        assert!(t.library.read(|c| c.exports_of(ids[1])).unwrap().is_empty());
    }

    #[test]
    fn progress_runs_from_the_start_to_one() {
        struct Record(Mutex<Vec<f32>>);
        impl JobSink for Record {
            fn progress(&self, _: JobId, _: &str, fraction: f32) {
                self.0.lock().unwrap().push(fraction);
            }
            fn done(&self, _: JobId, _: JobResult) {}
            fn error(&self, _: JobId, _: AppError) {}
        }
        let t = Fixture::new("export-progress");
        let ids: Vec<FrameId> = (1..=2).map(|i| t.frame(i, 200, 140)).collect();
        let sink = Arc::new(Record(Mutex::new(Vec::new())));
        let (ctx, _) = JobCtx::standalone_with(sink.clone());
        export_frames(
            &t.library,
            &ids,
            &options(&t.dir.join("out"), ExportFormat::Jpeg, OutputSpace::Srgb),
            true,
            &ctx,
        )
        .unwrap();
        let seen = sink.0.lock().unwrap().clone();
        assert_eq!(seen.first(), Some(&0.0));
        assert_eq!(seen.last(), Some(&1.0));
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    }
}
