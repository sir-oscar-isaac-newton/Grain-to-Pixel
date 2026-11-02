//! Triplet grouping (specification section 6, `triplet_group`): reads the capture time and the
//! colour of the preview of the chosen camera files and groups them into R/G/B candidates. It changes nothing on disk; importing
//! a triplet is a separate job.

use std::collections::HashSet;
use std::path::PathBuf;

use rayon::prelude::*;

use gtp_core::{AppError, ErrorCode};
use gtp_ingest::{group_triplets, read_camera_file, TripletCandidate};

/// Most files one grouping may name.
pub const MAX_FILES: usize = 10_000;

/// Groups the files at `paths` (duplicates counted once) into triplet candidates. Every path must
/// be an existing file; a file without a readable capture time ends up in a flagged candidate
/// instead of failing the call.
pub fn triplet_group(paths: &[PathBuf]) -> Result<Vec<TripletCandidate>, AppError> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Choose between 1 and 10,000 files to group.",
        )
        .with_details(format!("{} files", paths.len())));
    }
    let mut seen = HashSet::new();
    let mut unique = Vec::with_capacity(paths.len());
    for path in paths {
        if !path.is_file() {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "One of the chosen files does not exist.",
            )
            .with_details(path.display().to_string()));
        }
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }
    // Finding the preview reads the whole file, so the files are read side by side.
    let files: Vec<_> = unique
        .par_iter()
        .map(|path| read_camera_file(path))
        .collect();
    Ok(group_triplets(&files))
}

#[cfg(test)]
mod tests {
    use gtp_core::FrameId;
    use gtp_ingest::testing::{nef_with_preview, tiff_with_capture_time};
    use gtp_ingest::{TripletChannel, TripletProblem};

    use super::*;

    fn folder(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gtp-triplets-{label}-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A camera file shot at `12:00:{seconds}`.
    fn nef(dir: &std::path::Path, name: &str, seconds: u32) -> PathBuf {
        let path = dir.join(name);
        let date = format!("2026:10:06 12:{:02}:{:02}", seconds / 60, seconds % 60);
        std::fs::write(&path, tiff_with_capture_time(Some(&date), Some("50"), 0)).unwrap();
        path
    }

    #[test]
    fn camera_files_are_grouped_by_their_exif_time_whatever_their_names() {
        let dir = folder("group");
        let paths = vec![
            // Named in the wrong order on purpose: only the capture time counts.
            nef(&dir, "DSC_0003.NEF", 4),
            nef(&dir, "DSC_0001.NEF", 0),
            nef(&dir, "DSC_0002.NEF", 2),
            nef(&dir, "DSC_0004.NEF", 20),
            nef(&dir, "DSC_0005.NEF", 22),
        ];
        let candidates = triplet_group(&paths).unwrap();
        assert_eq!(candidates.len(), 2);
        let names = |c: &TripletCandidate| -> Vec<String> {
            c.files
                .iter()
                .map(|f| f.path.rsplit(['\\', '/']).next().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            names(&candidates[0]),
            ["DSC_0001.NEF", "DSC_0002.NEF", "DSC_0003.NEF"]
        );
        assert_eq!(candidates[0].problem, None);
        assert_eq!(
            candidates[0].files[0].captured.as_deref(),
            Some("2026-10-06T12:00:00.500")
        );
        assert_eq!(
            candidates[1].problem,
            Some(TripletProblem::MissingShots { missing: 1 })
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A camera file shot at `12:00:{seconds}` whose preview is lit in `rgb`.
    fn lit(dir: &std::path::Path, name: &str, seconds: u32, rgb: [f32; 3]) -> PathBuf {
        let path = dir.join(name);
        let date = format!("2026:10:06 12:00:{seconds:02}");
        std::fs::write(&path, nef_with_preview(&date, rgb)).unwrap();
        path
    }

    #[test]
    fn the_colour_of_each_preview_decides_which_file_is_red_green_and_blue() {
        let dir = folder("colour");
        let (red, green, blue) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        // Shot blue, red, green, but the colours say which is which.
        let paths = [
            lit(&dir, "a.NEF", 0, blue),
            lit(&dir, "b.NEF", 2, red),
            lit(&dir, "c.NEF", 4, green),
        ];
        let candidate = triplet_group(&paths).unwrap().remove(0);
        assert_eq!(candidate.problem, None);
        let files: Vec<_> = candidate
            .files
            .iter()
            .map(|f| {
                (
                    f.path.rsplit(['\\', '/']).next().unwrap().to_owned(),
                    f.channel,
                )
            })
            .collect();
        assert_eq!(
            files,
            [
                ("b.NEF".to_owned(), Some(TripletChannel::Red)),
                ("c.NEF".to_owned(), Some(TripletChannel::Green)),
                ("a.NEF".to_owned(), Some(TripletChannel::Blue)),
            ]
        );

        // Two reds: not a triplet.
        let twice = [
            lit(&dir, "d.NEF", 20, red),
            lit(&dir, "e.NEF", 22, red),
            lit(&dir, "f.NEF", 24, blue),
        ];
        assert_eq!(
            triplet_group(&twice).unwrap()[0].problem,
            Some(TripletProblem::ChannelMismatch)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_listed_twice_counts_once_and_one_without_exif_is_flagged() {
        let dir = folder("dupes");
        let a = nef(&dir, "a.NEF", 0);
        let b = nef(&dir, "b.NEF", 2);
        let c = nef(&dir, "c.NEF", 4);
        let plain = dir.join("plain.NEF");
        std::fs::write(&plain, b"not a camera file").unwrap();
        let candidates = triplet_group(&[a.clone(), b, a, c, plain]).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].files.len(), 3);
        assert_eq!(candidates[1].problem, Some(TripletProblem::NoCaptureTime));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_number_of_files_and_their_existence_are_checked() {
        assert_eq!(
            triplet_group(&[]).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let many = vec![PathBuf::from("x.NEF"); MAX_FILES + 1];
        assert_eq!(
            triplet_group(&many).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let dir = folder("missing");
        let missing = dir.join("nothing.NEF");
        let err = triplet_group(std::slice::from_ref(&missing)).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(err.details, Some(missing.display().to_string()));
        // A folder is not a file.
        assert_eq!(
            triplet_group(std::slice::from_ref(&dir)).unwrap_err().code,
            ErrorCode::NotFound
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
