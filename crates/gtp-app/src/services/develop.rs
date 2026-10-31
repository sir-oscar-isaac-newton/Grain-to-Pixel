//! The Develop service (P-11): the backend of interactive editing.
//!
//! Opening a frame decodes its original once and keeps a [`PROXY_EDGE`] proxy (linear f32) in an
//! in-memory LRU of [`CACHE_FRAMES`] frames; renders, analysis and the Develop view's previews all
//! work on that proxy. The original is never touched. A preview travels as one buffer: a
//! [`HEADER_BYTES`]-byte header followed by RGBA8 sRGB pixels (no JSON, no base64).
//!
//! Dragging a slider sends many renders. Each carries a sequence number, and per frame only the
//! newest counts: a render that is superseded before it starts, or while it runs, comes back as
//! `Cancelled` and its pixels are thrown away. (A render already running finishes first; stopping
//! it midway would mean threading cancellation through every pipeline stage.) The recipe is clamped
//! by the pipeline, so values from the UI can never reach the stages out of range.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use gtp_core::recipe::{Color, Crop, Invert, BASE_MAX, BASE_MIN};
use gtp_core::{
    AppError, ErrorCode, Frame, FrameId, ImageF32, Recipe, RecipeField, SourceFile, SourceRole,
};
use gtp_edit::{neutral_balance, Area};
use gtp_io::{read_tiff16, to_f32_linear, Limits};
use gtp_negative::{levels_from_sample, sample_film_base, LevelPoint, Rect};

use crate::pipeline::{
    analyze, build_proxy, inverted_frame, normalized_frame, to_rgba8_srgb, Histogram,
    PipelineInput, RenderTarget, PROXY_EDGE,
};
use crate::services::library::Library;

/// Frames whose proxy is kept in memory (about 34 MB each).
pub const CACHE_FRAMES: usize = 4;
/// The smallest and largest long edge a preview may ask for.
pub const MIN_EDGE: u32 = 256;
pub const MAX_EDGE: u32 = 4096;
/// Bytes before the pixels of a rendered preview: `seq` (u64), `width` (u32), `height` (u32), all
/// little-endian.
pub const HEADER_BYTES: usize = 16;

/// Which end of the levels a picked area sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum LevelsPoint {
    White,
    Black,
}

impl From<LevelsPoint> for LevelPoint {
    fn from(point: LevelsPoint) -> Self {
        match point {
            LevelsPoint::White => LevelPoint::White,
            LevelsPoint::Black => LevelPoint::Black,
        }
    }
}

/// What the Develop view needs to open a frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DevelopInfo {
    /// Size of the original, not of the proxy.
    pub width: u32,
    pub height: u32,
    pub has_ir: bool,
    /// The frame's saved recipe, or the default one when it was never edited.
    pub recipe: Recipe,
    /// The roll's measured film base, if it has one.
    pub roll_base: Option<[f32; 3]>,
}

/// 256-bin histograms of the display values, one list of 256 counts per channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HistogramData {
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub luma: Vec<u32>,
}

impl From<Histogram> for HistogramData {
    fn from(h: Histogram) -> Self {
        Self {
            red: h.red.to_vec(),
            green: h.green.to_vec(),
            blue: h.blue.to_vec(),
            luma: h.luma.to_vec(),
        }
    }
}

/// What the automatic analysis of a frame found.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DevelopAnalysis {
    /// The detected frame as fractions of the scan; the whole scan when not confident.
    pub frame_rect: Crop,
    /// 0 to 1.
    pub frame_confidence: f32,
    pub base_rgb: [f32; 3],
    /// 0 to 1; low means the base should be sampled by hand.
    pub base_confidence: f32,
    /// True when the base was given (the roll's, or one picked on the frame) rather than measured
    /// on this scan.
    pub base_given: bool,
    pub histogram: HistogramData,
    /// A recipe with the automatic crop, base and levels, everything else at its default.
    pub auto_recipe: Recipe,
}

/// Saves a frame's recipe, clamped into range, and stamps the frame as edited. Returns the recipe
/// as stored. A recipe the pipeline could not render is refused instead: a tone curve with a single
/// point (no curve at all means "no change").
pub fn save_recipe(library: &Library, frame: FrameId, recipe: Recipe) -> Result<Recipe, AppError> {
    let recipe = recipe.clamped();
    for curve in [
        &recipe.tone.curve.master,
        &recipe.tone.curve.r,
        &recipe.tone.curve.g,
        &recipe.tone.curve.b,
    ] {
        gtp_edit::Curve::new(curve)?;
    }
    library.write(|c| c.save_recipe(frame, &recipe))?;
    Ok(recipe)
}

/// Copies the chosen groups of settings from `source`'s recipe onto each of `frames`, all or
/// nothing; whatever else a target frame has is kept. A frame that was never edited counts as
/// having the default recipe, both as a source and as a target. The source itself, if listed, is
/// skipped.
pub fn apply_recipe_to(
    library: &Library,
    frames: &[FrameId],
    source: FrameId,
    fields: &[RecipeField],
) -> Result<(), AppError> {
    if frames.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Select at least one frame to apply the settings to.",
        ));
    }
    if fields.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Choose at least one group of settings to copy.",
        ));
    }
    library.write(|c| {
        c.get_frame(source)?;
        let from = c.load_recipe(source)?.unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let mut updated = Vec::new();
        for frame in frames {
            if *frame == source || !seen.insert(*frame) {
                continue;
            }
            let mut recipe = c.load_recipe(*frame)?.unwrap_or_default();
            recipe.copy_fields_from(&from, fields);
            updated.push((*frame, recipe));
        }
        c.save_recipes(&updated)
    })
}

/// A film base must be finite and within the range a recipe holds.
fn check_base(base: [f32; 3]) -> Result<(), AppError> {
    if base
        .iter()
        .all(|v| v.is_finite() && (BASE_MIN..=BASE_MAX).contains(v))
    {
        return Ok(());
    }
    Err(
        AppError::new(ErrorCode::InvalidInput, "That film base is not usable.")
            .with_details(format!("{base:?}")),
    )
}

/// The pixels of `image` that a region given in fractions of it covers: from the pixel the region
/// starts in to the one it ends in, so a small region is never empty.
fn pixel_area(region: Crop, image: &ImageF32) -> Result<Rect, AppError> {
    let inside = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    if !(inside(region.x)
        && inside(region.y)
        && inside(region.x + region.w)
        && inside(region.y + region.h))
        || region.w <= 0.0
        || region.h <= 0.0
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Choose an area inside the picture.",
        )
        .with_details(format!("{region:?}")));
    }
    let span = |start: f32, length: f32, size: u32| {
        let first = ((start * size as f32).floor() as u32).min(size.saturating_sub(1));
        let last = (((start + length) * size as f32).ceil() as u32).clamp(first + 1, size);
        (first, last - first)
    };
    let (x, w) = span(region.x, region.w, image.width);
    let (y, h) = span(region.y, region.h, image.height);
    Ok(Rect { x, y, w, h })
}

/// One frame's proxy and the state of its renders.
struct Entry {
    rgb: Arc<ImageF32>,
    has_ir: bool,
    /// The roll's base as of the last open (the roll can get one after the proxy was built).
    film_base: Mutex<Option<[f32; 3]>>,
    /// The highest sequence number seen for this frame.
    latest: AtomicU64,
    /// One render at a time per frame, so a queue of superseded renders costs nothing.
    render_lock: Mutex<()>,
}

impl Entry {
    fn input(&self) -> PipelineInput {
        PipelineInput {
            rgb: Arc::clone(&self.rgb),
            ir: None,
            film_base: *self
                .film_base
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        }
    }

    fn set_base(&self, base: Option<[f32; 3]>) {
        *self
            .film_base
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = base;
    }

    fn bytes(&self) -> usize {
        self.rgb.data.len() * std::mem::size_of::<f32>()
    }
}

/// The frames open for editing. One per app (it lives in the `AppContext`).
#[derive(Default)]
pub struct DevelopService {
    /// Most recently used first.
    cache: Mutex<Vec<(FrameId, Arc<Entry>)>>,
}

fn superseded() -> AppError {
    AppError::new(
        ErrorCode::Cancelled,
        "A newer preview was requested, so this one was dropped.",
    )
}

/// Decodes a frame's original and builds its proxy.
fn load(library: &Library, sources: &[SourceFile], frame: &Frame) -> Result<Entry, AppError> {
    let scan = sources
        .iter()
        .find(|s| s.role == SourceRole::Rgb)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "This frame has no scan file."))?;
    let path = library.resolve(Path::new(&scan.rel_path))?;
    let tiff = read_tiff16(&path, &Limits::default())?;
    let full = to_f32_linear(&tiff)?;
    drop(tiff);
    let proxy = build_proxy(&full, PROXY_EDGE)?;
    Ok(Entry {
        rgb: Arc::new(proxy),
        has_ir: frame.has_ir,
        film_base: Mutex::new(None),
        latest: AtomicU64::new(0),
        render_lock: Mutex::new(()),
    })
}

impl DevelopService {
    fn cache(&self) -> std::sync::MutexGuard<'_, Vec<(FrameId, Arc<Entry>)>> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cached entry, moved to the front.
    fn get(&self, id: FrameId) -> Option<Arc<Entry>> {
        let mut cache = self.cache();
        let at = cache.iter().position(|(f, _)| *f == id)?;
        let hit = cache.remove(at);
        let entry = Arc::clone(&hit.1);
        cache.insert(0, hit);
        Some(entry)
    }

    /// Adds an entry at the front, evicting the oldest beyond [`CACHE_FRAMES`]. When another
    /// thread cached the same frame meanwhile, that entry wins and is returned.
    fn put(&self, id: FrameId, entry: Entry) -> Arc<Entry> {
        let mut cache = self.cache();
        if let Some(at) = cache.iter().position(|(f, _)| *f == id) {
            let hit = cache.remove(at);
            let existing = Arc::clone(&hit.1);
            cache.insert(0, hit);
            return existing;
        }
        let entry = Arc::new(entry);
        cache.insert(0, (id, Arc::clone(&entry)));
        cache.truncate(CACHE_FRAMES);
        entry
    }

    /// The frame's entry, decoding its original when it is not cached.
    fn ensure(&self, library: &Library, id: FrameId) -> Result<Arc<Entry>, AppError> {
        if let Some(entry) = self.get(id) {
            return Ok(entry);
        }
        let (frame, sources, base) = library.read(|c| {
            let frame = c.get_frame(id)?;
            let sources = c.sources_of(id)?;
            let base = c.get_roll(frame.roll_id)?.film_base_rgb;
            Ok((frame, sources, base))
        })?;
        let entry = load(library, &sources, &frame)?;
        entry.set_base(base);
        Ok(self.put(id, entry))
    }

    /// What a thumbnail is rendered from: the cached proxy when the frame is open, otherwise the
    /// original decoded just for this and not cached, so a batch of thumbnails cannot push the
    /// frame being edited out of the cache. The roll's film base is read fresh.
    pub(crate) fn thumbnail_input(
        &self,
        library: &Library,
        id: FrameId,
    ) -> Result<PipelineInput, AppError> {
        let (frame, sources, base) = library.read(|c| {
            let frame = c.get_frame(id)?;
            let sources = c.sources_of(id)?;
            let base = c.get_roll(frame.roll_id)?.film_base_rgb;
            Ok((frame, sources, base))
        })?;
        let cached = self
            .cache()
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(_, entry)| Arc::clone(&entry.rgb));
        let rgb = match cached {
            Some(rgb) => rgb,
            None => load(library, &sources, &frame)?.rgb,
        };
        Ok(PipelineInput {
            rgb,
            ir: None,
            film_base: base,
        })
    }

    /// Opens a frame for editing: decodes the original into the cache (when it is not there) and
    /// returns what the view needs. The roll's film base and the saved recipe are read fresh.
    pub fn open(&self, library: &Library, id: FrameId) -> Result<DevelopInfo, AppError> {
        let entry = self.ensure(library, id)?;
        let (frame, recipe, base) = library.read(|c| {
            let frame = c.get_frame(id)?;
            let recipe = c.load_recipe(id)?;
            let base = c.get_roll(frame.roll_id)?.film_base_rgb;
            Ok((frame, recipe, base))
        })?;
        entry.set_base(base);
        Ok(DevelopInfo {
            width: frame.width,
            height: frame.height,
            has_ir: entry.has_ir,
            recipe: recipe.unwrap_or_default(),
            roll_base: base,
        })
    }

    /// Finds the frame, film base and levels on the cached proxy (loading it when needed). With
    /// `base` (linear scan values above 0, such as a base picked on the rebate) that base is used
    /// instead of the roll's or a measured one, so the automatic levels fit it.
    pub fn analyze(
        &self,
        library: &Library,
        id: FrameId,
        base: Option<[f32; 3]>,
    ) -> Result<DevelopAnalysis, AppError> {
        let entry = self.ensure(library, id)?;
        let mut input = entry.input();
        if let Some(base) = base {
            check_base(base)?;
            input.film_base = Some(base);
        }
        let analysis = analyze(&input)?;
        Ok(DevelopAnalysis {
            frame_rect: analysis.frame.rect,
            frame_confidence: analysis.frame.confidence,
            base_rgb: analysis.base.rgb,
            base_confidence: analysis.base.confidence,
            base_given: analysis.base_from_roll,
            histogram: analysis.histogram.into(),
            auto_recipe: analysis.auto_recipe,
        })
    }

    /// The film base from an area of the scan, for picking it on the rebate: the per-channel median
    /// of the area, in linear scan values. `region` is in fractions of the whole scan (x, y, width,
    /// height, each from 0 to 1) and is taken from the cached proxy. An area that is too dark to be
    /// film base is refused.
    pub fn sample_base(
        &self,
        library: &Library,
        id: FrameId,
        region: Crop,
    ) -> Result<[f32; 3], AppError> {
        let entry = self.ensure(library, id)?;
        let area = pixel_area(region, &entry.rgb)?;
        sample_film_base(&entry.rgb, area)
    }

    /// The levels of `recipe` with their white or black point set from an area of the frame: the
    /// density of the area's median, per channel. `region` is in fractions of the cropped frame
    /// (before straighten, turns and flips, which come after the Invert stage), and the base and
    /// crop are the recipe's. Taken from the cached proxy. A black area is refused.
    pub fn sample_levels(
        &self,
        library: &Library,
        id: FrameId,
        recipe: &Recipe,
        region: Crop,
        point: LevelsPoint,
    ) -> Result<Invert, AppError> {
        let entry = self.ensure(library, id)?;
        let frame = normalized_frame(&entry.input(), recipe)?;
        let area = pixel_area(region, &frame)?;
        levels_from_sample(&frame, area, point.into(), &recipe.clone().clamped().invert)
    }

    /// The recipe's colour settings with the three colour axes set so that the area becomes
    /// neutral: the area is taken after the Invert stage and before the colour stage, so the
    /// recipe's film base, crop and levels count, and its temperature, tint and the rest are kept.
    /// `region` is in fractions of the cropped frame, as for [`Self::sample_levels`]. An area that
    /// is black, or too far from neutral for the axes to correct, is refused.
    pub fn sample_neutral(
        &self,
        library: &Library,
        id: FrameId,
        recipe: &Recipe,
        region: Crop,
    ) -> Result<Color, AppError> {
        let entry = self.ensure(library, id)?;
        let frame = inverted_frame(&entry.input(), recipe)?;
        let Rect { x, y, w, h } = pixel_area(region, &frame)?;
        neutral_balance(&frame, Area { x, y, w, h }, &recipe.clone().clamped().color)
    }

    /// Renders a preview whose long edge is at most `max_edge` (never above the proxy's) and
    /// returns it as the header and RGBA8 bytes described at [`HEADER_BYTES`]. Returns `Cancelled`
    /// when a newer sequence number arrived for this frame before the render started or while it
    /// ran.
    pub fn render(
        &self,
        library: &Library,
        id: FrameId,
        recipe: &Recipe,
        max_edge: u32,
        seq: u64,
    ) -> Result<Vec<u8>, AppError> {
        if !(MIN_EDGE..=MAX_EDGE).contains(&max_edge) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "A preview must be between 256 and 4096 pixels on its long edge.",
            )
            .with_details(max_edge.to_string()));
        }
        let entry = self.ensure(library, id)?;
        entry.latest.fetch_max(seq, Ordering::SeqCst);
        let _turn = entry
            .render_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if entry.latest.load(Ordering::SeqCst) > seq {
            return Err(superseded());
        }
        let image =
            crate::pipeline::render(&entry.input(), recipe, RenderTarget::Preview { max_edge })?;
        let rgba = to_rgba8_srgb(&image)?;
        if entry.latest.load(Ordering::SeqCst) > seq {
            return Err(superseded());
        }
        let mut out = Vec::with_capacity(HEADER_BYTES + rgba.len());
        out.extend_from_slice(&seq.to_le_bytes());
        out.extend_from_slice(&image.width.to_le_bytes());
        out.extend_from_slice(&image.height.to_le_bytes());
        out.extend_from_slice(&rgba);
        Ok(out)
    }

    /// The cached frames, most recently used first.
    pub fn cached(&self) -> Vec<FrameId> {
        self.cache().iter().map(|(id, _)| *id).collect()
    }

    /// Memory held by the cached proxies.
    pub fn cached_bytes(&self) -> usize {
        self.cache().iter().map(|(_, e)| e.bytes()).sum()
    }

    /// Forgets every proxy (another library was opened).
    pub fn clear(&self) {
        self.cache().clear();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use gtp_core::FrameKind;

    use super::*;
    use crate::services::roll_frame::roll_set_film_base;
    use crate::services::testkit::Fixture;

    fn header(bytes: &[u8]) -> (u64, u32, u32) {
        (
            u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        )
    }

    #[test]
    fn opening_a_frame_reports_the_original_and_caches_a_proxy() {
        let t = Fixture::new("open");
        let id = t.frame(1, 2400, 1600);
        assert!(t.service.cached().is_empty());

        let info = t.service.open(&t.library, id).unwrap();
        assert_eq!((info.width, info.height), (2400, 1600));
        assert!(!info.has_ir);
        assert_eq!(info.recipe, Recipe::default(), "never edited");
        assert_eq!(info.roll_base, None);
        assert_eq!(t.service.cached(), [id]);
        // 2048 x 1365 pixels of three f32.
        assert_eq!(t.service.cached_bytes(), 2048 * 1365 * 3 * 4);
    }

    #[test]
    fn open_returns_the_saved_recipe_and_the_rolls_current_base() {
        let t = Fixture::new("fresh");
        let id = t.frame(1, 700, 500);
        t.service.open(&t.library, id).unwrap();

        let mut saved = Recipe::default();
        saved.tone.exposure_ev = 0.5;
        t.library.write(|c| c.save_recipe(id, &saved)).unwrap();
        roll_set_film_base(&t.library, t.roll, Some([0.8, 0.5, 0.3])).unwrap();

        // Cached already, yet the recipe and base are read again.
        let info = t.service.open(&t.library, id).unwrap();
        assert_eq!(info.recipe, saved);
        assert_eq!(info.roll_base, Some([0.8, 0.5, 0.3]));
        assert_eq!(t.service.cached().len(), 1, "no second copy");
    }

    #[test]
    fn a_missing_frame_or_scan_is_not_found() {
        let t = Fixture::new("missing");
        let err = t.service.open(&t.library, FrameId::new()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);

        let bare = FrameId::new();
        t.library
            .write(|c| {
                c.insert_frame(&Frame {
                    id: bare,
                    roll_id: t.roll,
                    index_in_roll: 9,
                    kind: FrameKind::ScanRgbIr,
                    width: 10,
                    height: 10,
                    has_ir: false,
                    note: String::new(),
                    created_at: String::new(),
                    edited_at: None,
                })
            })
            .unwrap();
        let err = t.service.open(&t.library, bare).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(t.service.cached().is_empty());
    }

    #[test]
    fn a_preview_is_a_header_then_rgba8_pixels() {
        let t = Fixture::new("render");
        let id = t.frame(1, 1200, 800);
        t.service.open(&t.library, id).unwrap();

        let bytes = t
            .service
            .render(&t.library, id, &Recipe::default(), 600, 42)
            .unwrap();
        let (seq, w, h) = header(&bytes);
        assert_eq!(seq, 42);
        assert!(w.max(h) <= 600 && w > 0 && h > 0);
        assert_eq!(bytes.len(), HEADER_BYTES + (w * h * 4) as usize);
        assert!(
            bytes[HEADER_BYTES..].chunks(4).all(|px| px[3] == 255),
            "opaque"
        );

        // The recipe changes the picture.
        let mut brighter = Recipe::default();
        brighter.tone.exposure_ev = 1.0;
        let other = t
            .service
            .render(&t.library, id, &brighter, 600, 43)
            .unwrap();
        assert_ne!(bytes[HEADER_BYTES..], other[HEADER_BYTES..]);
    }

    #[test]
    fn a_preview_is_never_larger_than_the_proxy() {
        let t = Fixture::new("cap");
        let id = t.frame(1, 2400, 1600);
        let bytes = t
            .service
            .render(&t.library, id, &Recipe::default(), MAX_EDGE, 1)
            .unwrap();
        let (_, w, h) = header(&bytes);
        assert!(w.max(h) <= PROXY_EDGE, "{w}x{h}");
    }

    #[test]
    fn the_preview_size_is_validated() {
        let t = Fixture::new("edge");
        let id = t.frame(1, 700, 500);
        for edge in [0, MIN_EDGE - 1, MAX_EDGE + 1] {
            let err = t
                .service
                .render(&t.library, id, &Recipe::default(), edge, 1)
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{edge}");
        }
        for edge in [MIN_EDGE, MAX_EDGE] {
            t.service
                .render(&t.library, id, &Recipe::default(), edge, 1)
                .unwrap();
        }
    }

    #[test]
    fn the_recipe_is_clamped_on_the_way_in_and_a_bad_one_is_refused() {
        let t = Fixture::new("clamp");
        let id = t.frame(1, 700, 500);
        let mut wild = Recipe::default();
        wild.tone.exposure_ev = 1000.0;
        wild.tone.contrast = -5000.0;
        let from_wild = t.service.render(&t.library, id, &wild, 400, 1).unwrap();
        let from_clamped = t
            .service
            .render(&t.library, id, &wild.clone().clamped(), 400, 2)
            .unwrap();
        assert_eq!(from_wild[HEADER_BYTES..], from_clamped[HEADER_BYTES..]);

        let mut broken = Recipe::default();
        broken.tone.curve.master = vec![[0.5, 0.5]];
        let err = t
            .service
            .render(&t.library, id, &broken, 400, 3)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn only_the_newest_request_for_a_frame_gets_a_picture() {
        let t = Fixture::new("stale");
        let id = t.frame(1, 700, 500);
        let recipe = Recipe::default();

        t.service.render(&t.library, id, &recipe, 400, 10).unwrap();
        // An older request arriving late is dropped; the same number is not "older".
        let late = t
            .service
            .render(&t.library, id, &recipe, 400, 9)
            .unwrap_err();
        assert_eq!(late.code, ErrorCode::Cancelled);
        t.service.render(&t.library, id, &recipe, 400, 10).unwrap();
        t.service.render(&t.library, id, &recipe, 400, 11).unwrap();
    }

    #[test]
    fn a_render_that_waits_its_turn_is_dropped_when_a_newer_one_arrives() {
        let t = Fixture::new("queue");
        let id = t.frame(1, 700, 500);
        t.service.open(&t.library, id).unwrap();
        let entry = t.service.get(id).unwrap();

        // The frame is busy: hold its render turn, queue a request, then a newer one is seen.
        let busy = entry.render_lock.lock().unwrap();
        let result = std::thread::scope(|scope| {
            let queued =
                scope.spawn(|| t.service.render(&t.library, id, &Recipe::default(), 400, 1));
            entry.latest.store(2, Ordering::SeqCst);
            drop(busy);
            queued.join().unwrap()
        });
        assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    }

    #[test]
    fn requests_for_different_frames_do_not_supersede_each_other() {
        let t = Fixture::new("two");
        let (a, b) = (t.frame(1, 700, 500), t.frame(2, 700, 500));
        let recipe = Recipe::default();
        t.service.render(&t.library, a, &recipe, 400, 50).unwrap();
        t.service.render(&t.library, b, &recipe, 400, 1).unwrap();
    }

    #[test]
    fn the_cache_keeps_four_frames_and_drops_the_least_recently_used() {
        let t = Fixture::new("lru");
        let ids: Vec<FrameId> = (1..=5).map(|n| t.frame(n, 640, 480)).collect();
        for id in &ids[..4] {
            t.service.open(&t.library, *id).unwrap();
        }
        assert_eq!(t.service.cached(), [ids[3], ids[2], ids[1], ids[0]]);

        // Using the oldest makes it the newest, so the next one to go is the second oldest.
        t.service.open(&t.library, ids[0]).unwrap();
        t.service.open(&t.library, ids[4]).unwrap();
        assert_eq!(t.service.cached(), [ids[4], ids[0], ids[3], ids[2]]);
        assert!(t.service.cached_bytes() <= CACHE_FRAMES * 640 * 480 * 3 * 4);

        // An evicted frame renders again, by decoding its original once more.
        t.service
            .render(&t.library, ids[1], &Recipe::default(), 400, 1)
            .unwrap();
        assert_eq!(t.service.cached()[0], ids[1]);
        assert_eq!(t.service.cached().len(), CACHE_FRAMES);

        t.service.clear();
        assert!(t.service.cached().is_empty());
    }

    #[test]
    fn analysis_finds_the_frame_and_base_and_makes_an_auto_recipe() {
        let t = Fixture::new("analyze");
        let id = t.frame(1, 1000, 700);
        let found = t.service.analyze(&t.library, id, None).unwrap();
        assert!(found.frame_confidence > 0.5, "{}", found.frame_confidence);
        assert!(found.base_confidence > 0.5, "{}", found.base_confidence);
        assert!(!found.base_given);
        assert_eq!(found.auto_recipe.geometry.crop, found.frame_rect);
        assert_eq!(found.histogram.red.len(), 256);
        assert!(found.histogram.luma.iter().sum::<u32>() > 0);

        roll_set_film_base(&t.library, t.roll, Some(found.base_rgb)).unwrap();
        t.service.open(&t.library, id).unwrap();
        assert!(t.service.analyze(&t.library, id, None).unwrap().base_given);
    }

    #[test]
    fn analysis_with_a_given_base_uses_it_and_the_levels_fit_it() {
        let t = Fixture::new("analyze-base");
        let id = t.frame(1, 1000, 700);
        let measured = t.service.analyze(&t.library, id, None).unwrap();
        assert!(!measured.base_given);

        // The base the scan really has gives the same picture as measuring it.
        let same = t
            .service
            .analyze(&t.library, id, Some(measured.base_rgb))
            .unwrap();
        assert!(same.base_given);
        assert_eq!(same.base_rgb, measured.base_rgb);
        assert_eq!(same.auto_recipe.base.rgb, measured.base_rgb);
        assert_eq!(same.auto_recipe.invert, measured.auto_recipe.invert);

        // Another base is used as given, and the levels follow it.
        let darker = measured.base_rgb.map(|v| v * 0.8);
        let other = t.service.analyze(&t.library, id, Some(darker)).unwrap();
        assert_eq!(other.base_rgb, darker);
        assert_eq!(other.auto_recipe.base.rgb, darker);
        assert_ne!(other.auto_recipe.invert, measured.auto_recipe.invert);
    }

    #[test]
    fn a_base_that_is_not_usable_is_refused() {
        let t = Fixture::new("analyze-bad-base");
        let id = t.frame(1, 400, 300);
        for bad in [
            [f32::NAN, 0.5, 0.5],
            [0.5, f32::INFINITY, 0.5],
            [0.5, 0.5, 0.0],
            [-0.1, 0.5, 0.5],
            [0.5, 0.5, 1.0e9],
        ] {
            let err = t.service.analyze(&t.library, id, Some(bad)).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{bad:?}");
        }
    }

    #[test]
    fn the_base_is_sampled_from_an_area_of_the_scan_given_in_fractions() {
        let t = Fixture::new("sample");
        let id = t.frame(1, 1000, 700);
        let measured = t.service.analyze(&t.library, id, None).unwrap().base_rgb;

        // The rebate is a band of 1/14 of the width down the left side: a click and a box on it.
        let click = Crop {
            x: 0.02,
            y: 0.45,
            w: 0.02,
            h: 0.03,
        };
        let drag = Crop {
            x: 0.005,
            y: 0.1,
            w: 0.05,
            h: 0.8,
        };
        for region in [click, drag] {
            let picked = t.service.sample_base(&t.library, id, region).unwrap();
            for (got, want) in picked.iter().zip(measured) {
                assert!(
                    (got - want).abs() / want < 0.02,
                    "{picked:?} vs {measured:?}"
                );
            }
        }
        assert_eq!(
            t.service.cached(),
            [id],
            "the proxy is kept for the next pick"
        );
    }

    #[test]
    fn an_area_outside_the_picture_or_without_size_is_refused() {
        let t = Fixture::new("sample-bad");
        let id = t.frame(1, 400, 300);
        let area = |x, y, w, h| Crop { x, y, w, h };
        for region in [
            area(0.5, 0.5, 0.6, 0.1),
            area(-0.1, 0.0, 0.2, 0.2),
            area(0.1, 0.1, 0.0, 0.2),
            area(0.1, 0.1, 0.2, -0.2),
            area(f32::NAN, 0.1, 0.2, 0.2),
            area(0.1, 0.1, f32::INFINITY, 0.2),
        ] {
            let err = t.service.sample_base(&t.library, id, region).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{region:?}");
        }
        let err = t
            .service
            .sample_base(&t.library, FrameId::new(), area(0.1, 0.1, 0.2, 0.2))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn a_picked_white_or_black_sets_its_end_from_the_density_under_the_area() {
        let t = Fixture::new("levels");
        let id = t.frame(1, 1000, 700);
        let auto = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        let corner = |x: f32, y: f32| Crop {
            x,
            y,
            w: 0.05,
            h: 0.05,
        };

        // The synthetic picture runs from black at its top left to white at its bottom right.
        let white = t
            .service
            .sample_levels(
                &t.library,
                id,
                &auto,
                corner(0.95, 0.95),
                LevelsPoint::White,
            )
            .unwrap();
        let black = t
            .service
            .sample_levels(&t.library, id, &auto, corner(0.0, 0.0), LevelsPoint::Black)
            .unwrap();
        for c in 0..3 {
            let span = auto.invert.white[c] - auto.invert.black[c];
            assert!(
                (white.white[c] - auto.invert.white[c]).abs() < 0.1 * span,
                "{white:?} vs {:?}",
                auto.invert
            );
            assert!(
                (black.black[c] - auto.invert.black[c]).abs() < 0.1 * span,
                "{black:?} vs {:?}",
                auto.invert
            );
        }
        assert_eq!(white.black, auto.invert.black, "the other end is kept");
        assert_eq!(black.white, auto.invert.white, "the other end is kept");
    }

    #[test]
    fn a_picked_level_follows_the_recipes_film_base() {
        let t = Fixture::new("levels-base");
        let id = t.frame(1, 1000, 700);
        let mut recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        recipe.base.source = gtp_core::recipe::BaseSource::Manual;
        let area = Crop {
            x: 0.4,
            y: 0.4,
            w: 0.1,
            h: 0.1,
        };
        let before = t
            .service
            .sample_levels(&t.library, id, &recipe, area, LevelsPoint::White)
            .unwrap();
        // A base twice as bright halves every transmittance: 0.30103 more density.
        recipe.base.rgb = recipe.base.rgb.map(|v| v * 2.0);
        let after = t
            .service
            .sample_levels(&t.library, id, &recipe, area, LevelsPoint::White)
            .unwrap();
        for c in 0..3 {
            assert!(
                (after.white[c] - before.white[c] - std::f32::consts::LOG10_2).abs() < 1e-3,
                "{before:?} {after:?}"
            );
        }
    }

    #[test]
    fn a_picked_neutral_makes_the_area_grey_and_keeps_the_other_colour_settings() {
        let t = Fixture::new("neutral");
        let id = t.frame(1, 1000, 700);
        let mut recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        recipe.color.temperature = 12.0;
        recipe.color.saturation = 1.2;
        // Off the diagonal of the synthetic gradient, so the area is clearly tinted.
        let region = Crop {
            x: 0.7,
            y: 0.2,
            w: 0.05,
            h: 0.05,
        };
        let color = t
            .service
            .sample_neutral(&t.library, id, &recipe, region)
            .unwrap();
        assert_eq!((color.temperature, color.saturation), (12.0, 1.2));
        assert!(color.red_cyan != 0.0 || color.green_magenta != 0.0 || color.blue_yellow != 0.0);

        // What the Color stage receives, balanced by those colours: channels equal within 2%.
        let entry = t.service.ensure(&t.library, id).unwrap();
        let frame = inverted_frame(&entry.input(), &recipe).unwrap();
        let area = pixel_area(region, &frame).unwrap();
        let gains = gtp_edit::channel_gains(&color);
        let mut sums = [0.0f64; 3];
        for y in area.y..area.y + area.h {
            for x in area.x..area.x + area.w {
                let at = (y as usize * frame.width as usize + x as usize) * 3;
                for c in 0..3 {
                    sums[c] += f64::from(frame.data[at + c] * gains[c]);
                }
            }
        }
        let top = sums.iter().cloned().fold(f64::MIN, f64::max);
        let bottom = sums.iter().cloned().fold(f64::MAX, f64::min);
        assert!(top / bottom < 1.02, "{sums:?}");
    }

    #[test]
    fn picking_a_neutral_refuses_a_black_area_bad_areas_and_unknown_frames() {
        let t = Fixture::new("neutral-bad");
        let id = t.frame(1, 400, 300);
        let mut recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        let area = Crop {
            x: 0.4,
            y: 0.4,
            w: 0.1,
            h: 0.1,
        };
        // Levels above every density turn the whole frame black.
        recipe.invert.black = [5.0; 3];
        recipe.invert.white = [6.0; 3];
        let err = t
            .service
            .sample_neutral(&t.library, id, &recipe, area)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);

        let ok = Recipe::default();
        for region in [
            Crop { w: 0.0, ..area },
            Crop { x: 0.95, ..area },
            Crop {
                x: f32::NAN,
                ..area
            },
        ] {
            let err = t
                .service
                .sample_neutral(&t.library, id, &ok, region)
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{region:?}");
        }
        let err = t
            .service
            .sample_neutral(&t.library, FrameId::new(), &ok, area)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn picking_levels_refuses_bad_areas_and_unknown_frames() {
        let t = Fixture::new("levels-bad");
        let id = t.frame(1, 400, 300);
        let recipe = Recipe::default();
        let area = |x, y, w, h| Crop { x, y, w, h };
        for region in [
            area(0.5, 0.5, 0.6, 0.1),
            area(0.1, 0.1, 0.0, 0.2),
            area(f32::NAN, 0.1, 0.2, 0.2),
        ] {
            let err = t
                .service
                .sample_levels(&t.library, id, &recipe, region, LevelsPoint::Black)
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{region:?}");
        }
        let err = t
            .service
            .sample_levels(
                &t.library,
                FrameId::new(),
                &recipe,
                area(0.1, 0.1, 0.2, 0.2),
                LevelsPoint::White,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn fractions_map_to_the_pixels_they_cover() {
        let image = ImageF32::zeros(100, 80, 3).unwrap();
        let area = |x, y, w, h| pixel_area(Crop { x, y, w, h }, &image).unwrap();
        assert_eq!(
            area(0.0, 0.0, 1.0, 1.0),
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 80
            }
        );
        assert_eq!(
            area(0.5, 0.5, 0.25, 0.25),
            Rect {
                x: 50,
                y: 40,
                w: 25,
                h: 20
            }
        );
        // A region smaller than a pixel still covers one, and one at the far edge stays inside.
        assert_eq!(
            area(0.2, 0.2, 1e-6, 1e-6),
            Rect {
                x: 20,
                y: 16,
                w: 1,
                h: 1
            }
        );
        let edge = area(0.99, 0.99, 0.005, 0.005);
        assert_eq!(
            edge,
            Rect {
                x: 99,
                y: 79,
                w: 1,
                h: 1
            }
        );
        assert!(edge.x + edge.w <= 100 && edge.y + edge.h <= 80);
    }

    #[test]
    fn a_saved_recipe_reloads_the_same_and_marks_the_frame_edited() {
        let t = Fixture::new("save");
        let id = t.bare(1);
        assert!(!t.edited(id));
        let mut recipe = Recipe::default();
        recipe.tone.exposure_ev = 0.7;
        recipe.geometry.rot90 = 1;

        let stored = save_recipe(&t.library, id, recipe.clone()).unwrap();
        assert_eq!(stored, recipe, "in range already, so unchanged");
        assert_eq!(t.stored(id), Some(recipe.clone()));
        assert!(t.edited(id));
        // The Develop view gets it back when it opens the frame again.
        let scan = t.frame(2, 400, 300);
        save_recipe(&t.library, scan, recipe.clone()).unwrap();
        assert_eq!(t.service.open(&t.library, scan).unwrap().recipe, recipe);
    }

    #[test]
    fn a_recipe_is_clamped_before_it_is_stored() {
        let t = Fixture::new("save-clamp");
        let id = t.bare(1);
        let mut wild = Recipe::default();
        wild.tone.exposure_ev = 1000.0;
        wild.color.saturation = -4.0;
        wild.geometry.crop.w = 7.0;

        let stored = save_recipe(&t.library, id, wild.clone()).unwrap();
        assert_eq!(stored, wild.clamped());
        assert_ne!(stored.tone.exposure_ev, 1000.0);
        assert_eq!(t.stored(id), Some(stored));
    }

    #[test]
    fn a_recipe_that_could_not_be_rendered_is_refused_and_nothing_is_stored() {
        let t = Fixture::new("save-bad");
        let id = t.bare(1);
        let mut one_point = Recipe::default();
        one_point.tone.curve.r = vec![[0.5, 0.5]];
        let err = save_recipe(&t.library, id, one_point).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(t.stored(id), None);
        assert!(!t.edited(id));

        // No curve at all means "no change", and is fine.
        let mut none = Recipe::default();
        none.tone.curve.master = Vec::new();
        save_recipe(&t.library, id, none).unwrap();

        let err = save_recipe(&t.library, FrameId::new(), Recipe::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    /// The source for the copy tests: tone and geometry edited, colour left alone.
    fn source_recipe() -> Recipe {
        let mut r = Recipe::default();
        r.tone.exposure_ev = 0.8;
        r.geometry.angle_deg = 2.0;
        r.output.space = gtp_core::recipe::OutputSpace::Prophoto;
        r
    }

    #[test]
    fn applying_copies_only_the_chosen_groups_and_keeps_the_rest_of_each_target() {
        let t = Fixture::new("apply");
        let (source, edited, fresh) = (t.bare(1), t.bare(2), t.bare(3));
        save_recipe(&t.library, source, source_recipe()).unwrap();
        let mut own = Recipe::default();
        own.color.temperature = 15.0;
        own.geometry.angle_deg = -1.0;
        save_recipe(&t.library, edited, own.clone()).unwrap();
        assert!(!t.edited(fresh));

        apply_recipe_to(
            &t.library,
            &[edited, fresh],
            source,
            &[RecipeField::Tone, RecipeField::Output],
        )
        .unwrap();

        let want = |base: &Recipe| {
            let mut r = base.clone();
            r.tone = source_recipe().tone;
            r.output = source_recipe().output;
            r
        };
        // The target's own colour and geometry stay; a never-edited target starts from the default.
        assert_eq!(t.stored(edited), Some(want(&own)));
        assert_eq!(t.stored(fresh), Some(want(&Recipe::default())));
        assert_eq!(t.stored(edited).unwrap().geometry.angle_deg, -1.0);
        assert!(t.edited(edited) && t.edited(fresh), "targets are stamped");
        assert_eq!(
            t.stored(source),
            Some(source_recipe()),
            "the source is untouched"
        );
    }

    #[test]
    fn a_never_edited_source_counts_as_the_default_recipe() {
        let t = Fixture::new("apply-default");
        let (source, target) = (t.bare(1), t.bare(2));
        save_recipe(&t.library, target, source_recipe()).unwrap();

        apply_recipe_to(&t.library, &[target], source, &[RecipeField::Tone]).unwrap();
        let after = t.stored(target).unwrap();
        assert_eq!(after.tone, Recipe::default().tone, "reset to the default");
        assert_eq!(after.geometry.angle_deg, 2.0);
        assert_eq!(t.stored(source), None, "a source never edited stays so");
        assert!(!t.edited(source));
    }

    #[test]
    fn the_source_and_repeated_frames_in_the_list_are_harmless() {
        let t = Fixture::new("apply-dupes");
        let (source, target) = (t.bare(1), t.bare(2));
        save_recipe(&t.library, source, source_recipe()).unwrap();
        apply_recipe_to(
            &t.library,
            &[source, target, target],
            source,
            &[RecipeField::Geometry],
        )
        .unwrap();
        assert_eq!(t.stored(target).unwrap().geometry.angle_deg, 2.0);

        // Only the source: nothing to do, and no error.
        apply_recipe_to(&t.library, &[source], source, &[RecipeField::Tone]).unwrap();
    }

    #[test]
    fn applying_needs_frames_and_fields_and_known_ones() {
        let t = Fixture::new("apply-bad");
        let (source, a, b) = (t.bare(1), t.bare(2), t.bare(3));
        save_recipe(&t.library, source, source_recipe()).unwrap();
        let code = |r: Result<(), AppError>| r.unwrap_err().code;

        assert_eq!(
            code(apply_recipe_to(
                &t.library,
                &[],
                source,
                &[RecipeField::Tone]
            )),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(apply_recipe_to(&t.library, &[a], source, &[])),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(apply_recipe_to(
                &t.library,
                &[a],
                FrameId::new(),
                &[RecipeField::Tone]
            )),
            ErrorCode::NotFound
        );

        // An unknown target changes none of the others.
        assert_eq!(
            code(apply_recipe_to(
                &t.library,
                &[a, FrameId::new(), b],
                source,
                &[RecipeField::Tone]
            )),
            ErrorCode::NotFound
        );
        assert_eq!((t.stored(a), t.stored(b)), (None, None));
        assert!(!t.edited(a) && !t.edited(b));
    }

    /// A 1600 px preview of a 2048 px proxy, as the Develop view asks for while a slider moves, for
    /// a 3:2 frame (35 mm) and a square one (6x6); run with
    /// `cargo test -p gtp-app --release -- --ignored --nocapture time_a_develop_render`.
    /// The budget is 100 ms (docs/specification.md section 11).
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_a_develop_render() {
        let mut recipe = Recipe::default();
        recipe.tone.exposure_ev = 0.3;
        recipe.tone.contrast = 20.0;
        recipe.tone.shadows = 20.0;
        recipe.color.vibrance = 15.0;
        recipe.geometry.angle_deg = 1.0;

        // The synthetic generator stops at 4096 px a side, so these originals are smaller than a
        // real 21 MP scan; the proxy, and so the render, is the same size.
        for (label, width, height) in [("3:2", 4096, 2731), ("square", 4096, 4096)] {
            let t = Fixture::new("time");
            let id = t.frame(1, width, height);
            let start = std::time::Instant::now();
            t.service.open(&t.library, id).unwrap();
            eprintln!(
                "{label} {width}x{height}: open (decode, build the proxy) {:?}",
                start.elapsed()
            );
            t.service.render(&t.library, id, &recipe, 1600, 0).unwrap();
            let runs = 10;
            let start = std::time::Instant::now();
            for seq in 1..=runs {
                t.service
                    .render(&t.library, id, &recipe, 1600, u64::from(seq))
                    .unwrap();
            }
            eprintln!(
                "{label}: develop_render at 1600 px {:?} per render",
                start.elapsed() / runs
            );
        }
    }
}
