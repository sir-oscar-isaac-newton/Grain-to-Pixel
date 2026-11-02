//! Merging the three exposures of an RGB triplet into one picture (specification section 4.2 step
//! 1). Each exposure is lit by one LED colour, so only its lit channel carries the picture: red
//! from the red-lit exposure, green from the green-lit one, blue from the blue-lit one. The other
//! two channels of each exposure are what leaked past the colour filters and are dropped.
//!
//! The three files are decoded side by side. The merge also checks the shots against each other
//! (the same exposure time, ISO and aperture) and against clipping, and says so in warnings; it
//! stores nothing.

use std::path::Path;

use gtp_core::{AppError, ErrorCode, ImageF32};
use gtp_io::{demosaic_channel, Limits, RawDecoder};

use crate::capture::{read_exposure_info, ExifInfo};
use crate::channel::TripletChannel;

/// A sample at or above this fraction of full scale counts as clipped.
pub const CLIP_LEVEL: f32 = 0.99;
/// More than this fraction of clipped samples in a lit channel is worth a warning.
pub const CLIP_FRACTION: f32 = 0.005;

/// Something the user should know about a merged triplet.
#[derive(Debug, Clone, PartialEq)]
pub enum TripletWarning {
    /// The three shots used different shutter times.
    ExposureTimeMismatch,
    IsoMismatch,
    ApertureMismatch,
    /// More than [`CLIP_FRACTION`] of the lit channel is at full scale: `fraction` of it.
    Clipped {
        channel: TripletChannel,
        fraction: f32,
    },
}

/// The merged picture and the warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct TripletMerge {
    /// Linear RGB, 3 channels, 0 to 1 over the range of the sensor.
    pub image: ImageF32,
    pub warnings: Vec<TripletWarning>,
}

fn all_same<T: Copy>(values: [Option<T>; 3], same: impl Fn(T, T) -> bool) -> bool {
    match values {
        [Some(a), Some(b), Some(c)] => same(a, b) && same(a, c),
        // What a file does not say cannot differ.
        _ => true,
    }
}

/// Warnings for shots that were not made alike: the exposure time, ISO and aperture of the red,
/// green and blue shot must be equal (specification section 6.1). A value a file does not have is
/// not compared.
pub fn check_exif(info: &[ExifInfo; 3]) -> Vec<TripletWarning> {
    let mut warnings = Vec::new();
    if !all_same(info.map(|i| i.exposure_time), |a, b| a.same_as(b)) {
        warnings.push(TripletWarning::ExposureTimeMismatch);
    }
    if !all_same(info.map(|i| i.iso), |a, b| a == b) {
        warnings.push(TripletWarning::IsoMismatch);
    }
    if !all_same(info.map(|i| i.f_number), |a, b| a.same_as(b)) {
        warnings.push(TripletWarning::ApertureMismatch);
    }
    warnings
}

/// Warnings for lit channels with clipped highlights: `planes` are the lit channel of the red,
/// green and blue exposure.
pub fn check_clipping(planes: [&ImageF32; 3]) -> Vec<TripletWarning> {
    let mut warnings = Vec::new();
    for (plane, channel) in planes.into_iter().zip(TripletChannel::ALL) {
        if plane.data.is_empty() {
            continue;
        }
        let clipped = plane.data.iter().filter(|v| **v >= CLIP_LEVEL).count();
        let fraction = clipped as f32 / plane.data.len() as f32;
        if fraction > CLIP_FRACTION {
            warnings.push(TripletWarning::Clipped { channel, fraction });
        }
    }
    warnings
}

/// Decodes the red-, green- and blue-lit exposure (`r`, `g`, `b`) and merges them. The three must be
/// the same size. Ends with the first error of the three files.
pub fn merge_triplet(
    decoder: &dyn RawDecoder,
    r: &Path,
    g: &Path,
    b: &Path,
    limits: &Limits,
) -> Result<TripletMerge, AppError> {
    let paths = [r, g, b];
    let results: Vec<Result<(ImageF32, ExifInfo), AppError>> = std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(channel, path)| {
                scope.spawn(move || {
                    let raw = decoder.decode(path, limits)?;
                    let plane = demosaic_channel(&raw, channel)?;
                    Ok((plane, read_exposure_info(path)))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err(AppError::internal("a triplet merge thread panicked")))
            })
            .collect()
    });
    let mut planes = Vec::with_capacity(3);
    let mut info = Vec::with_capacity(3);
    for result in results {
        let (plane, exif) = result?;
        planes.push(plane);
        info.push(exif);
    }
    let (width, height) = (planes[0].width, planes[0].height);
    if planes
        .iter()
        .any(|p| (p.width, p.height) != (width, height))
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The three exposures are not the same size.",
        )
        .with_details(
            planes
                .iter()
                .map(|p| format!("{}x{}", p.width, p.height))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }

    let mut data = Vec::with_capacity(planes[0].data.len() * 3);
    for i in 0..planes[0].data.len() {
        data.extend([planes[0].data[i], planes[1].data[i], planes[2].data[i]]);
    }
    let mut warnings = check_exif(&[info[0], info[1], info[2]]);
    warnings.extend(check_clipping([&planes[0], &planes[1], &planes[2]]));
    Ok(TripletMerge {
        image: ImageF32::new(width, height, 3, data)?,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use gtp_io::{Cfa, RawLinear};

    use super::*;
    use crate::capture::testing::tiff_with_exposure;
    use crate::capture::Ratio;

    /// Decoder that hands out constructed mosaics by file name.
    struct Fake(HashMap<PathBuf, Result<RawLinear, AppError>>);

    impl RawDecoder for Fake {
        fn decode(&self, path: &Path, _: &Limits) -> Result<RawLinear, AppError> {
            self.0
                .get(path)
                .cloned()
                .unwrap_or_else(|| Err(AppError::new(ErrorCode::NotFound, "no such fake file")))
        }
    }

    /// A 16 x 12 RGGB mosaic whose red, green and blue photosites hold `levels`.
    fn mosaic(levels: [u16; 3]) -> RawLinear {
        let (w, h) = (16u32, 12u32);
        let data = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| levels[Cfa::RGGB.color_at(x as usize, y as usize)])
            .collect();
        RawLinear {
            width: w,
            height: h,
            cfa: Cfa::RGGB,
            black: [0.0; 4],
            white: 10_000.0,
            data,
        }
    }

    fn files(mosaics: [RawLinear; 3]) -> (Fake, [PathBuf; 3]) {
        let paths = ["r.NEF", "g.NEF", "b.NEF"].map(PathBuf::from);
        let map = paths
            .iter()
            .cloned()
            .zip(mosaics)
            .map(|(p, m)| (p, Ok(m)))
            .collect();
        (Fake(map), paths)
    }

    fn merge(fake: &Fake, p: &[PathBuf; 3]) -> Result<TripletMerge, AppError> {
        merge_triplet(fake, &p[0], &p[1], &p[2], &Limits::default())
    }

    #[test]
    fn red_comes_from_the_red_lit_exposure_green_from_the_green_and_blue_from_the_blue() {
        // Each exposure has its lit colour bright and the other two dim, as with real LEDs.
        let (fake, paths) = files([
            mosaic([6_000, 300, 100]),
            mosaic([200, 4_000, 400]),
            mosaic([50, 150, 2_000]),
        ]);
        let merged = merge(&fake, &paths).unwrap();
        assert_eq!(
            (
                merged.image.width,
                merged.image.height,
                merged.image.channels
            ),
            (16, 12, 3)
        );
        for pixel in merged.image.data.as_chunks::<3>().0 {
            assert_eq!(*pixel, [0.6, 0.4, 0.2], "no trace of the unlit channels");
        }
        assert!(merged.warnings.is_empty());
    }

    #[test]
    fn three_exposures_of_different_sizes_are_refused() {
        let mut small = mosaic([1_000; 3]);
        small.width = 8;
        small.data.truncate(8 * 12);
        let (fake, paths) = files([mosaic([1_000; 3]), small, mosaic([1_000; 3])]);
        let err = merge(&fake, &paths).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(err.details.as_deref(), Some("16x12, 8x12, 16x12"));
    }

    #[test]
    fn a_file_that_cannot_be_decoded_fails_the_merge_with_its_error() {
        let (mut fake, paths) = files([mosaic([1_000; 3]), mosaic([1_000; 3]), mosaic([1_000; 3])]);
        fake.0.insert(
            paths[1].clone(),
            Err(AppError::new(
                ErrorCode::UnsupportedFormat,
                "not a camera file",
            )),
        );
        assert_eq!(
            merge(&fake, &paths).unwrap_err().code,
            ErrorCode::UnsupportedFormat
        );
        fake.0.remove(&paths[2]);
        let err = merge(&fake, &paths).unwrap_err();
        assert!(matches!(
            err.code,
            ErrorCode::UnsupportedFormat | ErrorCode::NotFound
        ));
    }

    #[test]
    fn a_lit_channel_with_clipped_highlights_is_reported_with_its_share() {
        let clipped = |share_clipped: usize| {
            let mut m = mosaic([5_000, 100, 100]);
            // Clip some red photosites of the red-lit exposure.
            let mut left = share_clipped;
            for i in 0..m.data.len() {
                if left > 0 && Cfa::RGGB.color_at(i % 16, i / 16) == 0 {
                    m.data[i] = 10_000;
                    left -= 1;
                }
            }
            m
        };
        // 16 x 12 has 48 red photosites; the interpolated plane clips wherever a clipped photosite
        // or its neighbours are. One red photosite is nowhere near 0.5 % of the plane after
        // interpolation, but 40 of 48 clearly is.
        let (fake, paths) = files([
            clipped(40),
            mosaic([100, 5_000, 100]),
            mosaic([100, 100, 5_000]),
        ]);
        let merged = merge(&fake, &paths).unwrap();
        let [TripletWarning::Clipped { channel, fraction }] = merged.warnings.as_slice() else {
            unreachable!("{:?}", merged.warnings);
        };
        assert_eq!(*channel, TripletChannel::Red);
        assert!(*fraction > 0.5 && *fraction <= 1.0, "{fraction}");
        let (fake, paths) = files([
            clipped(0),
            mosaic([100, 5_000, 100]),
            mosaic([100, 100, 5_000]),
        ]);
        assert!(merge(&fake, &paths).unwrap().warnings.is_empty());
    }

    #[test]
    fn the_clipping_threshold_is_half_a_percent_of_the_plane() {
        let plane = |clipped: usize| {
            let mut data = vec![0.5_f32; 1_000];
            data[..clipped].fill(1.0);
            ImageF32::new(1_000, 1, 1, data).unwrap()
        };
        let ok = plane(0);
        assert!(check_clipping([&ok, &ok, &ok]).is_empty());
        let five = plane(5);
        assert!(
            check_clipping([&five, &five, &five]).is_empty(),
            "exactly 0.5 % is fine"
        );
        let six = plane(6);
        let warnings = check_clipping([&ok, &six, &ok]);
        assert_eq!(
            warnings,
            [TripletWarning::Clipped {
                channel: TripletChannel::Green,
                fraction: 0.006
            }]
        );
    }

    #[test]
    fn shots_made_alike_give_no_warning_and_each_difference_gives_its_own() {
        let info = |time: (u32, u32), iso: u32, f: (u32, u32)| ExifInfo {
            exposure_time: Some(Ratio {
                num: time.0,
                den: time.1,
            }),
            iso: Some(iso),
            f_number: Some(Ratio { num: f.0, den: f.1 }),
        };
        let a = info((1, 60), 200, (56, 10));
        assert!(check_exif(&[a, a, a]).is_empty());
        assert!(
            check_exif(&[a, info((2, 120), 200, (56, 10)), a]).is_empty(),
            "1/60 is 2/120"
        );
        assert_eq!(
            check_exif(&[a, a, info((1, 30), 200, (56, 10))]),
            [TripletWarning::ExposureTimeMismatch]
        );
        assert_eq!(
            check_exif(&[a, info((1, 60), 400, (56, 10)), a]),
            [TripletWarning::IsoMismatch]
        );
        assert_eq!(
            check_exif(&[info((1, 60), 200, (80, 10)), a, a]),
            [TripletWarning::ApertureMismatch]
        );
        let all = [info((1, 30), 400, (80, 10)), a, a];
        assert_eq!(check_exif(&all).len(), 3);
        // A value a file does not have is not compared.
        let none = ExifInfo::default();
        assert!(check_exif(&[a, none, a]).is_empty());
        assert!(check_exif(&[none; 3]).is_empty());
    }

    #[test]
    fn the_exposure_of_the_real_files_is_compared_when_they_say_it() {
        let dir = std::env::temp_dir().join(format!("gtp-merge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, exposure: (u32, u32)| {
            let path = dir.join(name);
            std::fs::write(&path, tiff_with_exposure(exposure, 200, (56, 10))).unwrap();
            path
        };
        let paths = [
            write("r.NEF", (1, 60)),
            write("g.NEF", (1, 60)),
            write("b.NEF", (1, 30)),
        ];
        let map = paths
            .iter()
            .cloned()
            .zip([
                mosaic([6_000, 300, 100]),
                mosaic([200, 4_000, 400]),
                mosaic([50, 150, 2_000]),
            ])
            .map(|(p, m)| (p, Ok(m)))
            .collect();
        let merged = merge(&Fake(map), &paths).unwrap();
        assert_eq!(merged.warnings, [TripletWarning::ExposureTimeMismatch]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Merges the first real triplet of the private D600 session (red-, green- and blue-lit) and
    /// records the time against the 10 s budget. Run by hand with
    /// `cargo test -p gtp-ingest --release -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the private D600 samples"]
    fn a_real_d600_triplet_merges_within_the_budget() {
        use gtp_io::RawlerDecoder;

        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/private/Mamiya 645 Kodak Gold 200 120 Trichrome RGB");
        let [r, g, b] = ["DSC_4652.NEF", "DSC_4653.NEF", "DSC_4654.NEF"].map(|n| dir.join(n));
        if !r.is_file() {
            eprintln!("no private samples; skipped");
            return;
        }
        for run in 1..=3 {
            let started = std::time::Instant::now();
            let merged = merge_triplet(&RawlerDecoder, &r, &g, &b, &Limits::default()).unwrap();
            let took = started.elapsed();
            eprintln!(
                "run {run}: merged in {took:?}, warnings {:?}",
                merged.warnings
            );
            assert!(took.as_secs_f64() <= 10.0, "over the 10 s budget: {took:?}");
            assert_eq!((merged.image.width, merged.image.height), (6016, 4016));
            assert!(merged.image.data.iter().all(|v| v.is_finite() && *v >= 0.0));
        }
        let merged = merge_triplet(&RawlerDecoder, &r, &g, &b, &Limits::default()).unwrap();
        let mut means = [0.0_f64; 3];
        for pixel in merged.image.data.as_chunks::<3>().0 {
            for (mean, v) in means.iter_mut().zip(pixel) {
                *mean += f64::from(*v);
            }
        }
        let n = (merged.image.data.len() / 3) as f64;
        eprintln!(
            "mean R {:.4} G {:.4} B {:.4}",
            means[0] / n,
            means[1] / n,
            means[2] / n
        );
        assert!(
            means.iter().all(|m| *m / n > 0.0005),
            "every channel has light"
        );
    }
}
