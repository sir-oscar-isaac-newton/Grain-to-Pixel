//! A library of synthetic frames for trying the Library view at its design size (5,000 frames
//! scroll smoothly). The frames have no original files, only a small thumbnail each, so the grid,
//! search and tags can be felt in the real app without scanning anything.

use std::path::Path;

use gtp_app::library_fs::thumb_rel;
use gtp_app::services::library::Library;
use gtp_app::services::roll_frame::{roll_create, RollInput};
use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, Frame, FrameId, FrameKind, ImageF32};
use gtp_io::encode_jpeg;

/// Frames in the default fixture.
pub const DEFAULT_FRAMES: u32 = 5_000;
/// Rolls in the default fixture.
pub const DEFAULT_ROLLS: u32 = 100;

const THUMB_WIDTH: u32 = 240;
const THUMB_HEIGHT: u32 = 160;
const FILMS: [&str; 4] = [
    "Kodak Portra 400",
    "Kodak Ektar 100",
    "Ilford HP5 Plus",
    "Fuji Pro 400H",
];
const CAMERAS: [&str; 3] = ["Nikon F3", "Leica M6", "Pentax K1000"];

/// What was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Built {
    pub rolls: u32,
    pub frames: u32,
}

/// A thumbnail-sized gradient; `seed` shifts its colours so frames are not all the same.
fn gradient(seed: u32) -> Result<ImageF32, AppError> {
    let mut data = Vec::with_capacity((THUMB_WIDTH * THUMB_HEIGHT * 3) as usize);
    let shift = (seed % 7) as f32 / 7.0;
    for y in 0..THUMB_HEIGHT {
        for x in 0..THUMB_WIDTH {
            data.push((x as f32 / THUMB_WIDTH as f32 + shift).fract());
            data.push(y as f32 / THUMB_HEIGHT as f32);
            data.push(1.0 - x as f32 / THUMB_WIDTH as f32);
        }
    }
    ImageF32::new(THUMB_WIDTH, THUMB_HEIGHT, 3, data)
}

/// Creates a library in `dir` (which must not hold one) with `rolls` rolls sharing `frames`
/// frames as evenly as they divide, each frame with a thumbnail.
pub fn build(dir: &Path, frames: u32, rolls: u32) -> Result<Built, AppError> {
    if frames == 0 || rolls == 0 || rolls > frames {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "A fixture needs at least one frame, and no more rolls than frames.",
        ));
    }
    let library = Library::create(dir, "Benchmark fixture")?;

    // One JPEG per colour shift, copied for every frame: encoding 5,000 times would be the slow part.
    let templates = dir.join("fixture-thumbs");
    std::fs::create_dir_all(&templates).map_err(|e| AppError::from_io(&e))?;
    let mut shifted = Vec::new();
    for seed in 0..7 {
        let path = templates.join(format!("{seed}.jpg"));
        encode_jpeg(&gradient(seed)?, OutputSpace::Srgb, 80, &path)?;
        shifted.push(path);
    }

    let mut made = 0;
    for r in 0..rolls {
        let share = frames / rolls + u32::from(r < frames % rolls);
        let roll = roll_create(
            &library,
            RollInput {
                name: format!("Roll {:03}", r + 1),
                film_stock: Some(FILMS[r as usize % FILMS.len()].into()),
                camera: Some(CAMERAS[r as usize % CAMERAS.len()].into()),
                iso_box: Some(400),
                shot_from: Some(format!("{}-{:02}", 2020 + r % 6, 1 + r % 12)),
                ..RollInput::default()
            },
        )?;
        for index in 1..=share {
            let frame = library.write(|c| {
                c.insert_frame(&Frame {
                    id: FrameId::new(),
                    roll_id: roll.id,
                    index_in_roll: index,
                    kind: FrameKind::ScanRgbIr,
                    width: 5616,
                    height: 3744,
                    has_ir: true,
                    note: String::new(),
                    created_at: String::new(),
                    edited_at: None,
                })
            })?;
            let thumb = library.resolve(&thumb_rel(frame.id))?;
            if let Some(parent) = thumb.parent() {
                std::fs::create_dir_all(parent).map_err(|e| AppError::from_io(&e))?;
            }
            let template = &shifted[made as usize % shifted.len()];
            std::fs::copy(template, &thumb).map_err(|e| AppError::from_io(&e))?;
            made += 1;
        }
    }
    drop(library);
    std::fs::remove_dir_all(&templates).map_err(|e| AppError::from_io(&e))?;
    Ok(Built {
        rolls,
        frames: made,
    })
}

#[cfg(test)]
mod tests {
    use gtp_app::services::roll_frame::{frame_list, frame_thumbnail, FrameQuery};

    use super::*;

    fn temp(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "gtp-fixture-{label}-{}-{}",
            std::process::id(),
            FrameId::new()
        ))
    }

    #[test]
    fn builds_a_library_whose_frames_all_have_a_thumbnail() {
        let dir = temp("ok");
        let built = build(&dir, 31, 4).unwrap();
        assert_eq!(
            built,
            Built {
                rolls: 4,
                frames: 31
            }
        );

        let library = Library::open(&dir).unwrap();
        let page = frame_list(
            &library,
            FrameQuery {
                roll_id: None,
                page: 0,
                page_size: 100,
            },
        )
        .unwrap();
        assert_eq!(page.total, 31);
        assert!(page.items.iter().all(|f| f.has_thumb));
        let bytes = frame_thumbnail(&library, page.items[0].id).unwrap();
        assert_eq!(&bytes[..2], [0xFF, 0xD8], "a JPEG");
        assert!(
            !dir.join("fixture-thumbs").exists(),
            "scratch files are removed"
        );

        drop(library);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shares_frames_over_rolls_as_evenly_as_they_divide() {
        let dir = temp("share");
        build(&dir, 10, 3).unwrap();
        let library = Library::open(&dir).unwrap();
        let mut sizes: Vec<u32> = gtp_app::services::roll_frame::roll_list(&library)
            .unwrap()
            .iter()
            .map(|r| {
                frame_list(
                    &library,
                    FrameQuery {
                        roll_id: Some(r.id),
                        page: 0,
                        page_size: 50,
                    },
                )
                .unwrap()
                .total
            })
            .collect();
        sizes.sort_unstable();
        assert_eq!(sizes, [3, 3, 4]);
        drop(library);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_nonsense_sizes_and_an_existing_library() {
        let dir = temp("bad");
        for (frames, rolls) in [(0, 1), (5, 0), (2, 3)] {
            assert_eq!(
                build(&dir, frames, rolls).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
        assert!(!dir.exists(), "nothing is created for a refused size");
        build(&dir, 2, 1).unwrap();
        assert_eq!(
            build(&dir, 2, 1).unwrap_err().code,
            ErrorCode::InvalidInput,
            "a library already there is not overwritten"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
