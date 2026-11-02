//! What the EXIF of a camera file says: when it was shot (`DateTimeOriginal` and
//! `SubSecTimeOriginal`) and with which exposure, ISO and aperture.
//!
//! Only the start of a NEF is read: its EXIF sits in the first few kilobytes and the rest is
//! previews and sensor data, 25 MB a file. A file whose time is not in the start is read whole.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use exif::{In, Reader, Tag, Value};

use crate::triplet::{CaptureTime, ExifFile};

/// How much of a file is read first.
const PREFIX_BYTES: u64 = 1024 * 1024;

/// The first ASCII string of an EXIF field.
fn ascii(exif: &exif::Exif, tag: Tag) -> Option<String> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(strings) => strings
            .first()
            .map(|bytes| String::from_utf8_lossy(bytes).trim().to_owned()),
        _ => None,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY:MM:DD HH:MM:SS` plus the sub-second digits (`"20"` is 0.20 s) as a [`CaptureTime`].
pub(crate) fn parse_capture_time(date_time: &str, sub_second: Option<&str>) -> Option<CaptureTime> {
    let numbers: Vec<i64> = date_time
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let &[year, month, day, hour, minute, second] = numbers.as_slice() else {
        return None;
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
        || year < 1970
    {
        return None;
    }
    let millis = sub_second
        .map(|digits| {
            let digits: String = digits
                .chars()
                .filter(char::is_ascii_digit)
                .take(3)
                .collect();
            format!("{digits:0<3}").parse::<i64>().unwrap_or(0)
        })
        .unwrap_or(0);
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Some(CaptureTime(seconds * 1000 + millis))
}

fn capture_time(exif: &exif::Exif) -> Option<CaptureTime> {
    let date_time = ascii(exif, Tag::DateTimeOriginal)?;
    parse_capture_time(&date_time, ascii(exif, Tag::SubSecTimeOriginal).as_deref())
}

/// The capture time in `bytes`, which are the start of a TIFF-based file.
#[cfg(test)]
fn capture_time_in(bytes: Vec<u8>) -> Option<CaptureTime> {
    pick_in(bytes, &capture_time)
}

/// What `pick` takes from the EXIF in `bytes`, the start of a TIFF-based file.
fn pick_in<T>(bytes: Vec<u8>, pick: &dyn Fn(&exif::Exif) -> Option<T>) -> Option<T> {
    let exif = Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()?;
    pick(&exif)
}

/// What `pick` takes from the EXIF of the file at `path`: from the first megabyte, or from the
/// whole file when the start does not have it.
fn read_pick<T>(path: &Path, pick: &dyn Fn(&exif::Exif) -> Option<T>) -> Option<T> {
    let file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut prefix = Vec::new();
    file.take(PREFIX_BYTES).read_to_end(&mut prefix).ok()?;
    if let Some(found) = pick_in(prefix, pick) {
        return Some(found);
    }
    if length <= PREFIX_BYTES {
        return None;
    }
    pick_in(std::fs::read(path).ok()?, pick)
}

/// The capture time of the file at `path`; `None` when it has none or cannot be read.
pub fn read_capture_time(path: &Path) -> Option<CaptureTime> {
    read_pick(path, &capture_time)
}

/// A fraction from the EXIF, as the camera wrote it (an exposure of 1/60 s is 1 over 60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ratio {
    pub num: u32,
    pub den: u32,
}

impl Ratio {
    /// True when both are the same fraction (1/60 and 2/120 are).
    pub fn same_as(self, other: Ratio) -> bool {
        u64::from(self.num) * u64::from(other.den) == u64::from(other.num) * u64::from(self.den)
    }
}

/// How an exposure was made, as far as the EXIF says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExifInfo {
    /// The shutter time in seconds.
    pub exposure_time: Option<Ratio>,
    pub iso: Option<u32>,
    /// The aperture, as an f-number.
    pub f_number: Option<Ratio>,
}

fn ratio(exif: &exif::Exif, tag: Tag) -> Option<Ratio> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(values) => values.first().map(|v| Ratio {
            num: v.num,
            den: v.denom,
        }),
        _ => None,
    }
}

fn exposure_info(exif: &exif::Exif) -> Option<ExifInfo> {
    let iso = exif
        .get_field(Tag::PhotographicSensitivity, In::PRIMARY)
        .and_then(|field| field.value.get_uint(0));
    let info = ExifInfo {
        exposure_time: ratio(exif, Tag::ExposureTime),
        iso,
        f_number: ratio(exif, Tag::FNumber),
    };
    (info != ExifInfo::default()).then_some(info)
}

/// The exposure, ISO and aperture of the file at `path`; every field `None` when it has none or
/// cannot be read.
pub fn read_exposure_info(path: &Path) -> ExifInfo {
    read_pick(path, &exposure_info).unwrap_or_default()
}

/// `path` with its capture time, ready for [`crate::group_triplets`].
pub fn read_exif_file(path: &Path) -> ExifFile {
    ExifFile {
        path: path.to_path_buf(),
        captured: read_capture_time(path),
        channel: None,
    }
}

/// `path` with its capture time and the channel its colour says, ready for
/// [`crate::group_triplets`]. Reads the whole file to find the preview, so it is slower than
/// [`read_exif_file`].
pub fn read_camera_file(path: &Path) -> ExifFile {
    ExifFile {
        channel: crate::channel::dominant_channel(path).channel,
        ..read_exif_file(path)
    }
}

/// A tiny TIFF with EXIF, for tests here and in crates that group files (feature `testing`).
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    /// A little-endian TIFF whose Exif IFD holds `DateTimeOriginal` and/or `SubSecTimeOriginal`, with the date string `gap` bytes later than it would otherwise be.
    pub fn tiff_with_capture_time(date: Option<&str>, sub: Option<&str>, gap: usize) -> Vec<u8> {
        let entries = usize::from(date.is_some()) + usize::from(sub.is_some());
        let exif_ifd = 26usize;
        let strings = exif_ifd + 2 + entries * 12 + 4;
        let mut out = b"II*\0".to_vec();
        out.extend(8u32.to_le_bytes());
        // IFD0: one entry, the pointer to the Exif IFD.
        out.extend(1u16.to_le_bytes());
        out.extend(0x8769u16.to_le_bytes());
        out.extend(4u16.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend((exif_ifd as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        // Exif IFD.
        out.extend((entries as u16).to_le_bytes());
        let mut tail = Vec::new();
        if let Some(date) = date {
            let mut text = date.as_bytes().to_vec();
            text.push(0);
            out.extend(0x9003u16.to_le_bytes());
            out.extend(2u16.to_le_bytes());
            out.extend((text.len() as u32).to_le_bytes());
            out.extend(((strings + gap + tail.len()) as u32).to_le_bytes());
            tail.extend(text);
        }
        if let Some(sub) = sub {
            let mut text = sub.as_bytes().to_vec();
            text.push(0);
            out.extend(0x9291u16.to_le_bytes());
            out.extend(2u16.to_le_bytes());
            out.extend((text.len() as u32).to_le_bytes());
            assert!(text.len() <= 4, "kept inline in these tests");
            text.resize(4, 0);
            out.extend(text);
        }
        out.extend(0u32.to_le_bytes());
        out.resize(out.len() + gap, 0);
        out.extend(tail);
        out
    }
    /// A little-endian TIFF whose Exif IFD holds an exposure time, an ISO and an f-number.
    pub fn tiff_with_exposure(exposure: (u32, u32), iso: u16, f_number: (u32, u32)) -> Vec<u8> {
        let exif_ifd = 26usize;
        let data = exif_ifd + 2 + 3 * 12 + 4;
        let mut out = b"II*\0".to_vec();
        out.extend(8u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(0x8769u16.to_le_bytes());
        out.extend(4u16.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend((exif_ifd as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(3u16.to_le_bytes());
        for (tag, at) in [(0x829Au16, data), (0x829Du16, data + 8)] {
            out.extend(tag.to_le_bytes());
            out.extend(5u16.to_le_bytes());
            out.extend(1u32.to_le_bytes());
            out.extend((at as u32).to_le_bytes());
        }
        out.extend(0x8827u16.to_le_bytes());
        out.extend(3u16.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(iso.to_le_bytes());
        out.extend([0u8, 0]);
        out.extend(0u32.to_le_bytes());
        for (num, den) in [exposure, f_number] {
            out.extend(num.to_le_bytes());
            out.extend(den.to_le_bytes());
        }
        out
    }
    // A test aid: a failure here is a broken test, not an app error.
    #[allow(clippy::expect_used)]
    /// A camera-file lookalike: a TIFF with `date` as its capture time followed by a flat JPEG
    /// preview of `rgb` (each 0 to 1, linear), 600 x 500 pixels.
    pub fn nef_with_preview(date: &str, rgb: [f32; 3]) -> Vec<u8> {
        use std::sync::atomic::{AtomicU32, Ordering};

        use gtp_core::recipe::OutputSpace;
        use gtp_core::ImageF32;

        static NEXT: AtomicU32 = AtomicU32::new(0);
        let (width, height) = (600u32, 500u32);
        let data: Vec<f32> = (0..width * height).flat_map(|_| rgb).collect();
        let image = ImageF32::new(width, height, 3, data).expect("a flat image");
        let path = std::env::temp_dir().join(format!(
            "gtp-preview-{}-{}.jpg",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        gtp_io::encode_jpeg(&image, OutputSpace::Srgb, 90, &path).expect("a JPEG");
        let jpeg = std::fs::read(&path).expect("the JPEG");
        let _ = std::fs::remove_file(&path);
        let mut bytes = tiff_with_capture_time(Some(date), None, 0);
        bytes.extend(jpeg);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::testing::tiff_with_capture_time as tiff;
    use super::*;

    fn time_of(date: Option<&str>, sub: Option<&str>) -> Option<i64> {
        capture_time_in(tiff(date, sub, 0)).map(|t| t.0)
    }

    const NOON: i64 = 1_791_288_000_000; // 2026-10-06 12:00:00

    #[test]
    fn the_capture_time_comes_from_date_time_original_and_the_sub_second_digits() {
        assert_eq!(
            time_of(Some("2026:10:06 12:00:00"), None),
            Some(NOON),
            "no sub-second field"
        );
        assert_eq!(
            time_of(Some("2026:10:06 12:00:00"), Some("20")),
            Some(NOON + 200)
        );
        assert_eq!(
            time_of(Some("2026:10:06 12:00:00"), Some("5")),
            Some(NOON + 500)
        );
        assert_eq!(
            time_of(Some("2026:10:06 12:00:00"), Some("123")),
            Some(NOON + 123)
        );
        assert_eq!(
            parse_capture_time("2026:10:06 12:00:00", Some("9999")),
            Some(CaptureTime(NOON + 999)),
            "only the first three digits count"
        );
        assert_eq!(
            time_of(Some("2026:10:06 12:00:01"), Some("00")).unwrap()
                - time_of(Some("2026:10:06 12:00:00"), Some("90")).unwrap(),
            100
        );
    }

    #[test]
    fn the_exposure_iso_and_aperture_are_read_and_fractions_compare_by_value() {
        use super::testing::tiff_with_exposure;

        let dir = std::env::temp_dir().join(format!("gtp-exposure-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.NEF");
        std::fs::write(&path, tiff_with_exposure((1, 60), 200, (56, 10))).unwrap();
        let info = read_exposure_info(&path);
        assert_eq!(info.exposure_time, Some(Ratio { num: 1, den: 60 }));
        assert_eq!(info.iso, Some(200));
        assert_eq!(info.f_number, Some(Ratio { num: 56, den: 10 }));

        assert!(Ratio { num: 1, den: 60 }.same_as(Ratio { num: 2, den: 120 }));
        assert!(!Ratio { num: 1, den: 60 }.same_as(Ratio { num: 1, den: 125 }));

        // A file without EXIF, and one that is not there, say nothing.
        let plain = dir.join("b.NEF");
        std::fs::write(&plain, b"nothing").unwrap();
        assert_eq!(read_exposure_info(&plain), ExifInfo::default());
        assert_eq!(
            read_exposure_info(&dir.join("missing.NEF")),
            ExifInfo::default()
        );
        // A file with a capture time but no exposure tags says nothing about the exposure.
        let dated = dir.join("c.NEF");
        std::fs::write(&dated, tiff(Some("2026:10:06 12:00:00"), None, 0)).unwrap();
        assert_eq!(read_exposure_info(&dated), ExifInfo::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_or_nonsense_time_gives_none() {
        assert_eq!(time_of(None, Some("20")), None);
        assert_eq!(time_of(Some("not a date"), None), None);
        assert_eq!(time_of(Some("2026:13:06 12:00:00"), None), None);
        assert_eq!(time_of(Some("2026:10:06 25:00:00"), None), None);
        assert_eq!(time_of(Some("0000:00:00 00:00:00"), None), None);
        assert_eq!(capture_time_in(b"just some text".to_vec()), None);
        assert_eq!(capture_time_in(Vec::new()), None);
    }

    #[test]
    fn dates_across_a_midnight_and_a_leap_day_count_correctly() {
        let a = parse_capture_time("2028:02:28 23:59:59", None).unwrap();
        let b = parse_capture_time("2028:03:01 00:00:01", None).unwrap();
        assert_eq!(b.0 - a.0, 86_400_000 + 2_000, "2028 has a 29th of February");
        assert_eq!(
            parse_capture_time("1970:01:01 00:00:00", None),
            Some(CaptureTime(0))
        );
    }

    #[test]
    fn a_file_is_read_by_path_and_an_unreadable_one_has_no_time() {
        let dir = std::env::temp_dir().join(format!("gtp-exif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("a.NEF");
        std::fs::write(&good, tiff(Some("2026:10:06 12:00:00"), Some("20"), 0)).unwrap();
        let junk = dir.join("b.NEF");
        std::fs::write(&junk, b"nothing here").unwrap();

        assert_eq!(read_capture_time(&good), Some(CaptureTime(NOON + 200)));
        assert_eq!(read_exif_file(&good).path, good);
        assert_eq!(read_capture_time(&junk), None);
        assert_eq!(read_capture_time(&dir.join("missing.NEF")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_time_beyond_the_first_megabyte_is_still_found() {
        // A real NEF has its EXIF up front; this file keeps the date string past the first
        // megabyte, so the prefix read cannot find it and the whole file is read.
        let dir = std::env::temp_dir().join(format!("gtp-exif-late-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = tiff(
            Some("2026:10:06 12:00:00"),
            None,
            PREFIX_BYTES as usize + 64,
        );
        let path = dir.join("late.NEF");
        assert_eq!(
            capture_time_in(bytes[..PREFIX_BYTES as usize].to_vec()),
            None
        );
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_capture_time(&path), Some(CaptureTime(NOON)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The private D600 session: 45 files in 15 triplets and one more triplet in a subfolder. Every
    /// file gets the channel its place in the triplet predicts, and putting the files in order by
    /// colour gives the same order as the capture times. Run by hand with
    /// `cargo test -p gtp-ingest -- --ignored --nocapture`; skips when the samples are missing.
    #[test]
    #[ignore = "reads the private D600 samples"]
    fn the_real_d600_session_groups_into_clean_triplets_with_the_right_colours() {
        use crate::channel::TripletChannel;
        use crate::triplet::group_triplets;

        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../samples/private/Mamiya 645 Kodak Gold 200 120 Trichrome RGB");
        if !root.is_dir() {
            eprintln!("no private samples; skipped");
            return;
        }
        let nefs = |dir: &Path| -> Vec<ExifFile> {
            let mut files: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nef")))
                .collect();
            files.sort();
            files.iter().map(|p| read_camera_file(p)).collect()
        };
        // `all_clear`: every preview must give a clear channel. The bare-light triplet is the
        // exception, shot without film: its three previews are not clearly one colour each, so the
        // guess must stay unknown or be right, never wrong, and the capture order still groups it.
        let check = |dir: &Path, all_clear: bool| {
            let started = std::time::Instant::now();
            let session = nefs(dir);
            eprintln!("read {} files in {:?}", session.len(), started.elapsed());
            assert!(
                session.iter().all(|f| f.captured.is_some()),
                "every NEF has a time"
            );
            if all_clear {
                let unclear: Vec<_> = session
                    .iter()
                    .filter(|f| f.channel.is_none())
                    .map(|f| {
                        (
                            f.path.clone(),
                            crate::channel::dominant_channel(&f.path).ratio,
                        )
                    })
                    .collect();
                assert!(unclear.is_empty(), "every preview is clear: {unclear:?}");
            }
            let candidates = group_triplets(&session);
            assert_eq!(candidates.len(), session.len() / 3);
            for candidate in &candidates {
                assert_eq!(candidate.problem, None);
                for (place, file) in candidate.files.iter().enumerate() {
                    assert_eq!(
                        file.channel.map(TripletChannel::index).unwrap_or(place),
                        place,
                        "{}",
                        file.path
                    );
                }
            }
            // Without the colours the capture times alone give the same files in the same places.
            let timed_only: Vec<_> = session
                .iter()
                .map(|f| ExifFile {
                    channel: None,
                    ..f.clone()
                })
                .collect();
            let paths = |cs: &[crate::triplet::TripletCandidate]| -> Vec<Vec<String>> {
                cs.iter()
                    .map(|c| c.files.iter().map(|f| f.path.clone()).collect())
                    .collect()
            };
            assert_eq!(paths(&candidates), paths(&group_triplets(&timed_only)));
        };
        check(&root, true);
        check(&root.join("Barelight RGB Capture"), false);
    }
}
