//! The metadata an export writes into its files when "Write metadata" is on (docs/specification.md
//! sections 6 and 9). It is built only from what the user typed about the roll and the frame, plus
//! the app name and version. Nothing else is written: no exposure, aperture, lens, GPS, paths, or
//! machine and user names.

use gtp_core::model::{partial_date_span, Frame, Roll};
use gtp_io::{ExifDate, ExportMetadata};

/// The metadata of one exported frame. `version` is the app version, as in `Grain to Pixel 1.2.3`.
pub(crate) fn build(roll: &Roll, frame: &Frame, version: &str) -> ExportMetadata {
    let camera = roll
        .camera
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    ExportMetadata {
        software: Some(format!("Grain to Pixel {version}")),
        description: Some(description(roll)),
        make: camera
            .and_then(|c| c.split_whitespace().next())
            .map(str::to_owned),
        model: camera.map(str::to_owned),
        // The speed the film was exposed at; the box speed is not written.
        iso: roll.iso_shot,
        date_original: roll.shot_from.as_deref().and_then(full_date),
        user_comment: comment(&frame.note, &roll.notes),
    }
}

/// `Film stock (at ISO 800), Roll name`, leaving out what is empty. The speed is the one the film
/// was shot at; the box speed is not written.
fn description(roll: &Roll) -> String {
    let stock = roll
        .film_stock
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let speeds = roll
        .iso_shot
        .map(|s| format!("(at ISO {s})"))
        .unwrap_or_default();
    let film = [stock, speeds.as_str()]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    [film.as_str(), roll.name.trim()]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// The frame's note, a blank line, then the roll's notes; either alone when the other is empty.
fn comment(frame_note: &str, roll_notes: &str) -> Option<String> {
    let parts: Vec<&str> = [frame_note.trim(), roll_notes.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// A roll's `shot_from` is a partial ISO date (`2026`, `2026-07` or `2026-07-14`). Only a full,
/// valid day becomes a date: a month or a year alone would claim a precision the user did not give.
fn full_date(text: &str) -> Option<ExifDate> {
    let text = text.trim();
    let (first, last) = partial_date_span(text).ok()?;
    if first != text || last != text {
        return None;
    }
    let mut parts = text.split('-');
    Some(ExifDate {
        year: parts.next()?.parse().ok()?,
        month: parts.next()?.parse().ok()?,
        day: parts.next()?.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use gtp_core::model::FilmFormat;
    use gtp_core::{FrameFlag, FrameId, FrameKind, RollId};

    use super::*;

    fn roll() -> Roll {
        Roll {
            id: RollId::new(),
            name: "Roll 12".into(),
            film_stock: Some("Kodak Portra 400".into()),
            format: FilmFormat::Mm35,
            camera: Some("Nikon F3".into()),
            iso_box: Some(400),
            iso_shot: Some(800),
            shot_from: Some("2026-07-14".into()),
            shot_to: Some("2026-07-20".into()),
            notes: "Rain all week".into(),
            film_base_rgb: None,
            film_base_confidence: None,
            frame_size: None,
            has_flat_field: false,
            color_balance: None,
            created_at: String::new(),
        }
    }

    fn frame(note: &str) -> Frame {
        Frame {
            id: FrameId::new(),
            roll_id: RollId::new(),
            index_in_roll: 1,
            kind: FrameKind::ScanRgbIr,
            width: 10,
            height: 10,
            has_ir: false,
            note: note.into(),
            flag: FrameFlag::None,
            created_at: String::new(),
            edited_at: None,
        }
    }

    #[test]
    fn a_full_roll_gives_every_field() {
        let m = build(&roll(), &frame("Harbour"), "1.2.3");
        assert_eq!(m.software.as_deref(), Some("Grain to Pixel 1.2.3"));
        assert_eq!(
            m.description.as_deref(),
            Some("Kodak Portra 400 (at ISO 800), Roll 12")
        );
        assert_eq!(m.make.as_deref(), Some("Nikon"));
        assert_eq!(m.model.as_deref(), Some("Nikon F3"));
        assert_eq!(m.iso, Some(800));
        assert_eq!(
            m.date_original,
            Some(ExifDate {
                year: 2026,
                month: 7,
                day: 14
            })
        );
        assert_eq!(m.user_comment.as_deref(), Some("Harbour\n\nRain all week"));
    }

    #[test]
    fn the_description_variants() {
        let mut r = roll();
        r.iso_shot = None;
        assert_eq!(description(&r), "Kodak Portra 400, Roll 12", "no box ISO");
        r.iso_box = None;
        r.iso_shot = Some(800);
        assert_eq!(description(&r), "Kodak Portra 400 (at ISO 800), Roll 12");
        r.film_stock = None;
        assert_eq!(description(&r), "(at ISO 800), Roll 12");
        r.iso_shot = None;
        assert_eq!(description(&r), "Roll 12");
        r.film_stock = Some("  ".into());
        assert_eq!(description(&r), "Roll 12");
        r.film_stock = Some("Ilford HP5".into());
        assert_eq!(description(&r), "Ilford HP5, Roll 12");
    }

    #[test]
    fn make_is_the_first_word_and_model_the_whole_text() {
        let mut r = roll();
        r.camera = Some("  Canon  AE-1 Program ".into());
        let m = build(&r, &frame(""), "1");
        assert_eq!(m.make.as_deref(), Some("Canon"));
        assert_eq!(m.model.as_deref(), Some("Canon  AE-1 Program"));
        r.camera = Some("Leica".into());
        let m = build(&r, &frame(""), "1");
        assert_eq!(m.make.as_deref(), Some("Leica"));
        assert_eq!(m.model.as_deref(), Some("Leica"));
        for none in [None, Some("   ".to_owned())] {
            r.camera = none;
            let m = build(&r, &frame(""), "1");
            assert_eq!((m.make, m.model), (None, None));
        }
    }

    #[test]
    fn only_a_full_valid_date_is_written() {
        for bad in [
            "2026",
            "2026-07",
            "2026-02-30",
            "2026-13-01",
            "soon",
            "",
            "2026-7-4",
        ] {
            let mut r = roll();
            r.shot_from = Some(bad.into());
            assert_eq!(build(&r, &frame(""), "1").date_original, None, "{bad:?}");
        }
        let mut r = roll();
        r.shot_from = None;
        assert_eq!(build(&r, &frame(""), "1").date_original, None);
    }

    #[test]
    fn comments_join_with_a_blank_line_and_vanish_when_empty() {
        assert_eq!(comment("a", "b").as_deref(), Some("a\n\nb"));
        assert_eq!(comment("a", " ").as_deref(), Some("a"));
        assert_eq!(comment("", "b").as_deref(), Some("b"));
        assert_eq!(comment(" ", ""), None);
    }

    #[test]
    fn the_box_speed_never_becomes_the_iso_tag() {
        let mut r = roll();
        r.iso_shot = None;
        assert_eq!(build(&r, &frame(""), "1").iso, None);
    }
}
