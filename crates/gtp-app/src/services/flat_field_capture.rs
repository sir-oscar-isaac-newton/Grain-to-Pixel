//! Capturing a roll's flat field (H-24): the red, green and blue exposures of the bare light, shot
//! as a frame is, merged and built into the roll's flat field by the code of P-21
//! ([`attach_merged`]), with the three NEF files kept in the library for rebuilding it later.
//!
//! A capture whose merge shows a clipped channel, or three different apertures, attaches nothing and
//! keeps no files: such a flat field would correct wrongly. The light is off again when the sequence
//! ends, whichever way (see `CaptureSequence`).

use std::path::Path;

use gtp_capture::{CameraSource, CaptureSequence, LightSource};
use gtp_core::{AppError, ErrorCode, FlatFieldInfo, FrameId, RollId, TripletChannel};
use gtp_ingest::{merge_triplet, TripletWarning};
use gtp_io::{mark_read_only, Limits, RawDecoder};

use super::capture::{safe_name, Staging};
use super::flat_field::{attach_merged, exposure_of_files, remove_kept_files};
use crate::jobs::JobCtx;
use crate::library_fs::flat_field_rel;
use crate::services::library::Library;

/// Steps of the job for progress: three pictures, then merging and storing.
const CAPTURE_STEPS: f32 = 3.0;
const STORE_STEPS: f32 = 2.0;

/// What the job says while it shoots and stores (the calibration before it has its own words).
const STAGE: &str = "Capturing the flat field";

const COLOURS: [&str; 3] = ["red", "green", "blue"];

fn colour(channel: TripletChannel) -> &'static str {
    COLOURS[channel.index()]
}

/// The refusal for a capture that cannot make a flat field, in words, or `None` when it can.
fn refusal(warnings: &[TripletWarning]) -> Option<AppError> {
    let clipped: Vec<(TripletChannel, f32)> = warnings
        .iter()
        .filter_map(|w| match w {
            TripletWarning::Clipped { channel, fraction } => Some((*channel, *fraction)),
            _ => None,
        })
        .collect();
    if !clipped.is_empty() {
        let names: Vec<&str> = clipped.iter().map(|(c, _)| colour(*c)).collect();
        let shares: Vec<String> = clipped
            .iter()
            .map(|(c, f)| format!("{} {:.1}%", colour(*c), f * 100.0))
            .collect();
        return Some(
            AppError::new(
                ErrorCode::InvalidInput,
                format!(
                    "The {} picture is too bright: part of it is at full scale, so it cannot make a flat field. Lower the {} level or the exposure and capture again.",
                    names.join(" and "),
                    names.join(" and "),
                ),
            )
            .with_details(format!("clipped: {}", shares.join(", "))),
        );
    }
    warnings
        .iter()
        .any(|w| matches!(w, TripletWarning::ApertureMismatch))
        .then(|| {
            AppError::new(
                ErrorCode::InvalidInput,
                "The three pictures were taken at different apertures, so they cannot make a flat field. Leave the camera's aperture as it is and capture again.",
            )
        })
}

/// Where a flat field capture reports its progress: `job`, from `from` (the share of the job already
/// done, 0 to 1, such as a calibration before it) to 1.
pub struct FlatFieldProgress<'a> {
    pub job: &'a JobCtx,
    pub from: f32,
}

/// Captures a flat field into `roll`: runs `sequence` on `light` and `camera`, merges the three
/// pictures read by `decoder`, keeps them under `originals/<roll>/flat-field/` and attaches the flat
/// field built from them, replacing the roll's one (and removing the files it kept). Nothing is
/// attached and nothing kept when the capture clips, differs in aperture, does not fit the roll's
/// frames, fails or is cancelled. The caller makes the roll's frames and thumbnails follow (see
/// `after_flat_field_change`).
pub fn capture_flat_field(
    library: &Library,
    roll: RollId,
    sequence: &CaptureSequence<'_>,
    light: &mut dyn LightSource,
    camera: &mut dyn CameraSource,
    decoder: &dyn RawDecoder,
    progress: &FlatFieldProgress<'_>,
) -> Result<FlatFieldInfo, AppError> {
    library.read(|c| c.get_roll(roll))?;
    let FlatFieldProgress { job: ctx, from } = *progress;
    let total = CAPTURE_STEPS + STORE_STEPS;
    let report = |done: f32| ctx.progress(STAGE, from + (1.0 - from) * done);
    report(0.0);
    let mut taken = 0.0;
    let outcome = sequence.run(light, camera, &|| ctx.is_cancelled(), &mut |_| {
        taken += 1.0;
        report(taken / total);
    })?;
    ctx.check_cancelled()?;

    let staging = Staging::create(FrameId::new(), &outcome.files)?;
    let [r, g, b] = &staging.paths;
    let merge = merge_triplet(decoder, r, g, b, &Limits::default())?;
    if let Some(err) = refusal(&merge.warnings) {
        return Err(err);
    }
    report((CAPTURE_STEPS + 1.0) / total);
    ctx.check_cancelled()?;

    // The kept files: one folder for the roll, names that cannot meet those of an earlier flat field.
    // The end of an id is the random part (the start is the clock), so two captures never share it.
    let token: String = FrameId::new().to_string().chars().rev().take(8).collect();
    let dir = flat_field_rel(roll);
    let rels: Vec<String> = outcome
        .files
        .iter()
        .zip(["r", "g", "b"])
        .map(|(file, prefix)| {
            let name = format!("{prefix}-{}-{}", token, safe_name(&file.name));
            dir.join(name).to_string_lossy().replace('\\', "/")
        })
        .collect();
    let sources: Vec<String> = rels
        .iter()
        .map(|rel| {
            Path::new(rel)
                .file_name()
                .map_or_else(|| rel.clone(), |n| n.to_string_lossy().into_owned())
        })
        .collect();
    let exposure = exposure_of_files([r, g, b].map(|p| p.as_path()));

    let attached = keep_files(library, &staging, &rels)
        .and_then(|()| attach_merged(library, roll, &merge.image, sources, exposure, rels.clone()));
    match attached {
        Ok(info) => {
            report(1.0);
            tracing::info!(roll = %roll, "flat field captured");
            Ok(info)
        }
        Err(err) => {
            remove_kept_files(library, roll, &rels);
            Err(err)
        }
    }
}

/// Copies the staged pictures to `rels` in the library and marks them read-only, as originals are.
fn keep_files(library: &Library, staging: &Staging, rels: &[String]) -> Result<(), AppError> {
    for (from, rel) in staging.paths.iter().zip(rels) {
        let to = library.resolve(Path::new(rel))?;
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|err| AppError::from_io(&err))?;
        }
        std::fs::copy(from, &to).map_err(|err| AppError::from_io(&err))?;
        mark_read_only(&to)?;
    }
    Ok(())
}
