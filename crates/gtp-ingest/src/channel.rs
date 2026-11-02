//! Which channel an exposure of a triplet is, from its colour. Each exposure is lit by one LED
//! colour, so the dominant colour of the picture says whether it is the red, green or blue one.
//!
//! The picture used is the full-size JPEG preview a camera embeds in its raw file: it is found by
//! looking for embedded JPEGs in the file, and decoded at one eighth of its size, which is plenty
//! for a mean colour. The preview is processed by the camera, so it tells the channels apart but
//! says nothing reliable about exposure.
//!
//! On the author's D600 files the dominant channel is 13 times the next one or more (blue is the
//! weakest), so a ratio under [`MIN_RATIO`] means the frame is not what it should be.

use std::fs;
use std::path::Path;

use jpeg_decoder::{Decoder, PixelFormat};
use serde::Serialize;

/// A brightest mean colour (of 255) below this is a dark frame: nothing can be told.
pub const MIN_LEVEL: f64 = 2.0;
/// The brightest mean colour must be at least this many times the next one.
pub const MIN_RATIO: f64 = 3.0;
/// Embedded JPEGs with fewer pixels than this are thumbnails, not the preview.
const MIN_PREVIEW_PIXELS: u64 = 250_000;

/// The light an exposure was taken in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum TripletChannel {
    Red,
    Green,
    Blue,
}

impl TripletChannel {
    pub const ALL: [TripletChannel; 3] = [
        TripletChannel::Red,
        TripletChannel::Green,
        TripletChannel::Blue,
    ];

    /// 0 for red, 1 for green, 2 for blue: the position in a triplet.
    pub fn index(self) -> usize {
        match self {
            TripletChannel::Red => 0,
            TripletChannel::Green => 1,
            TripletChannel::Blue => 2,
        }
    }
}

/// What the colour of a picture says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelGuess {
    /// `None` when the picture is dark or no colour clearly leads.
    pub channel: Option<TripletChannel>,
    /// The brightest mean colour divided by the next one (0 when there was no picture).
    pub ratio: f64,
}

impl ChannelGuess {
    const UNKNOWN: Self = Self {
        channel: None,
        ratio: 0.0,
    };
}

/// The guess for mean red, green and blue values (0 to 255) of a picture.
pub fn guess_from_means(means: [f64; 3]) -> ChannelGuess {
    let top = (0..3)
        .max_by(|&a, &b| means[a].total_cmp(&means[b]))
        .unwrap_or(0);
    let second = (0..3)
        .filter(|&i| i != top)
        .map(|i| means[i])
        .fold(0.0_f64, f64::max);
    let ratio = means[top] / second.max(0.01);
    let clear = means[top] >= MIN_LEVEL && ratio >= MIN_RATIO;
    ChannelGuess {
        channel: clear.then_some(TripletChannel::ALL[top]),
        ratio,
    }
}

/// The offset of the embedded JPEG with the most pixels, if any is large enough to be a preview.
fn largest_preview(bytes: &[u8]) -> Option<usize> {
    let mut best: Option<(u64, usize)> = None;
    let mut at = 0;
    while let Some(found) = bytes[at..].windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) {
        let start = at + found;
        let mut decoder = Decoder::new(&bytes[start..]);
        if decoder.read_info().is_ok() {
            if let Some(info) = decoder.info() {
                let pixels = u64::from(info.width) * u64::from(info.height);
                if pixels >= MIN_PREVIEW_PIXELS && best.is_none_or(|(most, _)| pixels > most) {
                    best = Some((pixels, start));
                }
            }
        }
        at = start + 1;
    }
    best.map(|(_, start)| start)
}

/// The dominant channel of the preview inside `bytes`, the contents of a camera file.
pub fn dominant_channel_in(bytes: &[u8]) -> ChannelGuess {
    let Some(start) = largest_preview(bytes) else {
        return ChannelGuess::UNKNOWN;
    };
    let mut decoder = Decoder::new(&bytes[start..]);
    if decoder.read_info().is_err() {
        return ChannelGuess::UNKNOWN;
    }
    let Some(info) = decoder.info() else {
        return ChannelGuess::UNKNOWN;
    };
    // The largest reduction, 1/8, keeps a 24 MP preview at 750 x 502 pixels.
    if decoder.scale(info.width / 8, info.height / 8).is_err() {
        return ChannelGuess::UNKNOWN;
    }
    let Ok(pixels) = decoder.decode() else {
        return ChannelGuess::UNKNOWN;
    };
    if decoder.info().map(|i| i.pixel_format) != Some(PixelFormat::RGB24) || pixels.is_empty() {
        return ChannelGuess::UNKNOWN;
    }
    let mut sums = [0.0_f64; 3];
    let (triples, _) = pixels.as_chunks::<3>();
    for pixel in triples {
        for (sum, value) in sums.iter_mut().zip(pixel) {
            *sum += f64::from(*value);
        }
    }
    let count = triples.len() as f64;
    guess_from_means(sums.map(|sum| sum / count))
}

/// The dominant channel of the camera file at `path`; unknown when it cannot be read, has no
/// preview, or the picture is dark or unclear.
pub fn dominant_channel(path: &Path) -> ChannelGuess {
    fs::read(path).map_or(ChannelGuess::UNKNOWN, |bytes| dominant_channel_in(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::testing::nef_with_preview;

    #[test]
    fn the_brightest_colour_leads_when_it_is_clearly_ahead() {
        // The weakest real case on the D600 session: blue 27.2 against green 2.1.
        for (means, channel) in [
            ([94.8, 0.8, 0.6], TripletChannel::Red),
            ([0.2, 29.0, 0.2], TripletChannel::Green),
            ([1.5, 2.1, 27.2], TripletChannel::Blue),
        ] {
            let guess = guess_from_means(means);
            assert_eq!(guess.channel, Some(channel), "{means:?}");
            assert!(guess.ratio > 10.0);
        }
    }

    #[test]
    fn a_dark_or_unclear_picture_is_unknown() {
        assert_eq!(guess_from_means([0.0, 0.0, 0.0]).channel, None, "black");
        assert_eq!(
            guess_from_means([1.9, 0.0, 0.0]).channel,
            None,
            "too dark to say"
        );
        assert_eq!(
            guess_from_means([100.0, 100.0, 100.0]).channel,
            None,
            "grey"
        );
        assert_eq!(
            guess_from_means([60.0, 30.0, 10.0]).channel,
            None,
            "only twice the next"
        );
        assert_eq!(
            guess_from_means([90.0, 30.0, 10.0]).channel,
            Some(TripletChannel::Red),
            "3 times"
        );
        assert_eq!(
            guess_from_means([2.0, 0.0, 0.0]).channel,
            Some(TripletChannel::Red),
            "just bright enough"
        );
    }

    #[test]
    fn a_ratio_is_reported_even_when_the_guess_is_unknown() {
        let guess = guess_from_means([60.0, 30.0, 10.0]);
        assert!((guess.ratio - 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_preview_inside_a_file_says_which_light_it_was_taken_in() {
        for (rgb, channel) in [
            ([1.0, 0.0, 0.0], TripletChannel::Red),
            ([0.0, 1.0, 0.0], TripletChannel::Green),
            ([0.0, 0.0, 1.0], TripletChannel::Blue),
        ] {
            let file = nef_with_preview("2026:10:06 12:00:00", rgb);
            let guess = dominant_channel_in(&file);
            assert_eq!(guess.channel, Some(channel), "{rgb:?}");
            assert!(guess.ratio > 10.0, "{}", guess.ratio);
        }
    }

    #[test]
    fn a_dark_preview_a_mixed_one_and_a_file_without_a_preview_are_unknown() {
        assert_eq!(
            dominant_channel_in(&nef_with_preview("2026:10:06 12:00:00", [0.0; 3])).channel,
            None
        );
        let white = nef_with_preview("2026:10:06 12:00:00", [1.0; 3]);
        assert_eq!(dominant_channel_in(&white).channel, None);
        assert_eq!(dominant_channel_in(b"no picture in here").channel, None);
        assert_eq!(dominant_channel_in(&[]).channel, None);
        assert_eq!(
            dominant_channel_in(&[0xFF, 0xD8, 0xFF]).channel,
            None,
            "a cut-off header"
        );
    }

    #[test]
    fn a_file_is_read_by_path_and_a_missing_one_is_unknown() {
        let dir = std::env::temp_dir().join(format!("gtp-channel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.NEF");
        std::fs::write(
            &path,
            nef_with_preview("2026:10:06 12:00:00", [0.0, 1.0, 0.0]),
        )
        .unwrap();
        assert_eq!(dominant_channel(&path).channel, Some(TripletChannel::Green));
        assert_eq!(dominant_channel(&dir.join("missing.NEF")).channel, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn channels_know_their_place_in_a_triplet() {
        assert_eq!(TripletChannel::ALL.map(TripletChannel::index), [0, 1, 2]);
    }
}
