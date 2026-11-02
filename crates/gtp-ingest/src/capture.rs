//! Capture time of a camera file, from its EXIF (`DateTimeOriginal` and `SubSecTimeOriginal`).
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
fn capture_time_in(bytes: Vec<u8>) -> Option<CaptureTime> {
    let exif = Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .ok()?;
    capture_time(&exif)
}

/// The capture time of the file at `path`; `None` when it has none or cannot be read.
pub fn read_capture_time(path: &Path) -> Option<CaptureTime> {
    let file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut prefix = Vec::new();
    file.take(PREFIX_BYTES).read_to_end(&mut prefix).ok()?;
    if let Some(time) = capture_time_in(prefix) {
        return Some(time);
    }
    if length <= PREFIX_BYTES {
        return None;
    }
    capture_time_in(std::fs::read(path).ok()?)
}

/// `path` with its capture time, ready for [`crate::group_triplets`].
pub fn read_exif_file(path: &Path) -> ExifFile {
    ExifFile {
        path: path.to_path_buf(),
        captured: read_capture_time(path),
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

    /// The private D600 session: 45 files in 15 triplets and one more triplet in a subfolder. Run
    /// by hand with `cargo test -p gtp-ingest -- --ignored`; skips when the samples are missing.
    #[test]
    #[ignore = "reads the private D600 samples"]
    fn the_real_d600_session_groups_into_clean_triplets() {
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
            files.iter().map(|p| read_exif_file(p)).collect()
        };
        let started = std::time::Instant::now();
        let session = nefs(&root);
        eprintln!("read {} files in {:?}", session.len(), started.elapsed());
        assert!(
            session.iter().all(|f| f.captured.is_some()),
            "every NEF has a time"
        );
        let candidates = group_triplets(&session);
        assert_eq!(candidates.len(), session.len() / 3);
        assert!(candidates
            .iter()
            .all(|c| c.problem.is_none() && c.files.len() == 3));

        let barelight = nefs(&root.join("Barelight RGB Capture"));
        let single = group_triplets(&barelight);
        assert_eq!(single.len(), barelight.len() / 3);
        assert!(single.iter().all(|c| c.problem.is_none()));
    }
}
