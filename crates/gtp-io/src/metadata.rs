//! The Exif metadata an export may carry (docs/specification.md section 9, "Metadata in exported
//! files"). [`ExportMetadata`] is plain data from the caller; this module cleans it up under the
//! Exif text rules and turns it into an Exif block for JPEG. The TIFF writer in `export.rs` uses
//! the same cleaned [`Fields`].

use std::io::Cursor;

use exif::experimental::Writer;
use exif::{Field, In, Tag, Value};
use gtp_core::{AppError, ErrorCode};

/// Longest ASCII text kept, in characters. Exif has no limit, but a JPEG Exif block must fit in
/// one 64 KiB segment.
const MAX_ASCII_CHARS: usize = 1_000;
/// Longest user comment kept, in characters (at most 2 bytes each in UTF-16 plus the code).
const MAX_COMMENT_CHARS: usize = 20_000;
/// The Exif version written next to the other Exif tags: 2.3.0.
pub(crate) const EXIF_VERSION: &[u8; 4] = b"0230";

/// A calendar day, written as `DateTimeOriginal` at 00:00:00.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExifDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// What to write into an exported file. Every field is optional; an empty or blank text, an ISO
/// above 65535 and an impossible date write no tag. Nothing here is read from the pixels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportMetadata {
    /// `Software`.
    pub software: Option<String>,
    /// `ImageDescription`.
    pub description: Option<String>,
    /// `Make`.
    pub make: Option<String>,
    /// `Model`.
    pub model: Option<String>,
    /// `PhotographicSensitivity` (ISO). Exif holds it as a 16-bit number, so a larger value is
    /// left out rather than clipped.
    pub iso: Option<u32>,
    /// `DateTimeOriginal`, at 00:00:00.
    pub date_original: Option<ExifDate>,
    /// `UserComment`: free text, ASCII or Unicode as needed.
    pub user_comment: Option<String>,
}

/// The cleaned values, ready to be written.
#[derive(Debug, Default)]
pub(crate) struct Fields {
    pub software: Option<String>,
    pub description: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub iso: Option<u16>,
    /// `YYYY:MM:DD 00:00:00`.
    pub date_original: Option<String>,
    pub user_comment: Option<String>,
}

/// Exif ASCII: trimmed, control characters turned into spaces and each other non-ASCII character
/// into `?`. `None` when nothing is left.
fn ascii_text(text: Option<&str>) -> Option<String> {
    let cleaned: String = text?
        .trim()
        .chars()
        .take(MAX_ASCII_CHARS)
        .map(|c| match c {
            c if c.is_ascii_control() => ' ',
            c if c.is_ascii() => c,
            _ => '?',
        })
        .collect();
    let cleaned = cleaned.trim().to_owned();
    (!cleaned.is_empty()).then_some(cleaned)
}

impl ExportMetadata {
    /// The values as they will be written, with every rule applied.
    pub(crate) fn fields(&self) -> Fields {
        let comment: String = self
            .user_comment
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .chars()
            .take(MAX_COMMENT_CHARS)
            .filter(|&c| c != '\0')
            .collect();
        Fields {
            software: ascii_text(self.software.as_deref()),
            description: ascii_text(self.description.as_deref()),
            make: ascii_text(self.make.as_deref()),
            model: ascii_text(self.model.as_deref()),
            iso: self
                .iso
                .filter(|&iso| iso > 0)
                .and_then(|iso| u16::try_from(iso).ok()),
            date_original: self.date_original.and_then(format_date),
            user_comment: (!comment.trim().is_empty()).then_some(comment),
        }
    }
}

fn format_date(date: ExifDate) -> Option<String> {
    let valid = (1..=9999).contains(&date.year)
        && (1..=12).contains(&date.month)
        && (1..=31).contains(&date.day);
    valid.then(|| {
        format!(
            "{:04}:{:02}:{:02} 00:00:00",
            date.year, date.month, date.day
        )
    })
}

impl Fields {
    /// Whether any tag of the Exif sub-IFD (not the baseline tags) is to be written.
    pub fn has_exif_tags(&self) -> bool {
        self.iso.is_some() || self.date_original.is_some() || self.user_comment.is_some()
    }

    /// Whether nothing at all is to be written.
    pub fn is_empty(&self) -> bool {
        self.software.is_none()
            && self.description.is_none()
            && self.make.is_none()
            && self.model.is_none()
            && !self.has_exif_tags()
    }

    /// The `UserComment` value: the 8-byte character code, then the text.
    pub fn comment_bytes(&self, big_endian: bool) -> Option<Vec<u8>> {
        self.user_comment
            .as_deref()
            .map(|text| user_comment_bytes(text, big_endian))
    }
}

/// `ASCII\0\0\0` and the text when it is ASCII, else `UNICODE\0` and the text in UTF-16.
pub(crate) fn user_comment_bytes(text: &str, big_endian: bool) -> Vec<u8> {
    if text.is_ascii() {
        let mut out = b"ASCII\0\0\0".to_vec();
        out.extend_from_slice(text.as_bytes());
        out
    } else {
        let mut out = b"UNICODE\0".to_vec();
        for unit in text.encode_utf16() {
            let bytes = if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            };
            out.extend_from_slice(&bytes);
        }
        out
    }
}

fn ascii_field(tag: Tag, text: &str) -> Field {
    Field {
        tag,
        ifd_num: In::PRIMARY,
        value: Value::Ascii(vec![text.as_bytes().to_vec()]),
    }
}

/// The Exif block for a JPEG (the bytes after `Exif\0\0`), or `None` when nothing is to be
/// written. Uses the writer under `exif::experimental`.
pub(crate) fn jpeg_exif_block(metadata: &ExportMetadata) -> Result<Option<Vec<u8>>, AppError> {
    let fields = metadata.fields();
    if fields.is_empty() {
        return Ok(None);
    }
    let mut list: Vec<Field> = Vec::new();
    for (tag, text) in [
        (Tag::ImageDescription, &fields.description),
        (Tag::Make, &fields.make),
        (Tag::Model, &fields.model),
        (Tag::Software, &fields.software),
        (Tag::DateTimeOriginal, &fields.date_original),
    ] {
        if let Some(text) = text {
            list.push(ascii_field(tag, text));
        }
    }
    if let Some(iso) = fields.iso {
        list.push(Field {
            tag: Tag::PhotographicSensitivity,
            ifd_num: In::PRIMARY,
            value: Value::Short(vec![iso]),
        });
    }
    // The block is big-endian, so the UTF-16 comment is too.
    if let Some(bytes) = fields.comment_bytes(true) {
        list.push(Field {
            tag: Tag::UserComment,
            ifd_num: In::PRIMARY,
            value: Value::Undefined(bytes, 0),
        });
    }
    if fields.has_exif_tags() {
        list.push(Field {
            tag: Tag::ExifVersion,
            ifd_num: In::PRIMARY,
            value: Value::Undefined(EXIF_VERSION.to_vec(), 0),
        });
    }
    let mut writer = Writer::new();
    for field in &list {
        writer.push_field(field);
    }
    let mut buffer = Cursor::new(Vec::new());
    writer.write(&mut buffer, false).map_err(|err| {
        AppError::new(ErrorCode::Io, "The metadata could not be written.")
            .with_details(err.to_string())
    })?;
    Ok(Some(buffer.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_rules_apply() {
        assert_eq!(ascii_text(Some("  Grain  ")), Some("Grain".into()));
        assert_eq!(ascii_text(Some("   ")), None);
        assert_eq!(ascii_text(None), None);
        assert_eq!(ascii_text(Some("Zürich ☃")), Some("Z?rich ?".into()));
        assert_eq!(ascii_text(Some("a\nb\0c")), Some("a b c".into()));
    }

    #[test]
    fn iso_and_date_limits() {
        let fields = |iso, date| {
            ExportMetadata {
                iso,
                date_original: date,
                ..ExportMetadata::default()
            }
            .fields()
        };
        assert_eq!(fields(Some(65_535), None).iso, Some(65_535));
        assert_eq!(fields(Some(65_536), None).iso, None);
        assert_eq!(fields(Some(0), None).iso, None);
        let day = |year, month, day| Some(ExifDate { year, month, day });
        assert_eq!(
            fields(None, day(2026, 7, 4)).date_original.as_deref(),
            Some("2026:07:04 00:00:00")
        );
        assert_eq!(fields(None, day(2026, 13, 4)).date_original, None);
        assert_eq!(fields(None, day(0, 1, 1)).date_original, None);
    }

    #[test]
    fn the_comment_code_follows_the_text() {
        assert_eq!(user_comment_bytes("hi", true), b"ASCII\0\0\0hi");
        assert_eq!(
            user_comment_bytes("é", true),
            [b"UNICODE\0".as_slice(), &[0, 0xE9]].concat()
        );
        assert_eq!(
            user_comment_bytes("é", false),
            [b"UNICODE\0".as_slice(), &[0xE9, 0]].concat()
        );
    }

    #[test]
    fn nothing_to_write_gives_no_block() {
        assert_eq!(jpeg_exif_block(&ExportMetadata::default()).unwrap(), None);
    }
}
