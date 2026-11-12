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
//!
//! The detail view (U-29) shows the picture at full size. The service keeps one such picture (the
//! frame being looked at, rendered as an export renders it, as RGBA8 sRGB) and cuts the regions the
//! view asks for out of it; see [`DevelopService::render_detail`].

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use gtp_core::recipe::{Color, Crop, Invert, BASE_MAX, BASE_MIN};
use gtp_core::{
    AppError, ErrorCode, FlatField, Frame, FrameId, ImageF32, Look, Recipe, RecipeField,
    SourceFile, SourceRole,
};
use gtp_edit::{neutral_balance, Area};
use gtp_io::{read_tiff16, to_f32_linear, Limits};
use gtp_negative::{levels_from_sample, sample_film_base, LevelPoint, Rect};

use crate::pipeline::{
    analyze_with_axes, build_plane_proxy, build_proxy, inverted_frame, normalized_frame,
    to_rgba8_srgb, Histogram, PipelineInput, RenderTarget, PROXY_EDGE,
};
use crate::services::counters;
use crate::services::export::{read_full, render_full};
use crate::services::flat_field::apply_flat_field;
use crate::services::library::Library;
use crate::services::roll_balance;
use crate::services::roll_frame::{hint_for_frame, record_frame_measurement};

/// Frames whose proxy is kept in memory (about 34 MB each).
pub const CACHE_FRAMES: usize = 4;
/// The smallest and largest long edge a preview may ask for.
pub const MIN_EDGE: u32 = 256;
pub const MAX_EDGE: u32 = 4096;
/// Bytes before the pixels of a rendered preview: `seq` (u64), `width` (u32), `height` (u32), all
/// little-endian.
pub const HEADER_BYTES: usize = 16;
/// Bytes before the pixels of a detail region (U-29), all little-endian: `seq` (u64), the region's
/// `width` and `height` (u32 each), the region's origin `x` and `y` in the full picture (u32 each),
/// and the full picture's `width` and `height` (u32 each).
pub const DETAIL_HEADER_BYTES: usize = 32;
/// The most pixels a detail region may have on a side.
pub const MAX_DETAIL_EDGE: u32 = 4096;
/// A detail picture nobody asked for in this many seconds is released.
pub const DETAIL_IDLE_SECS: u64 = 300;

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
    /// How sure the automatic measurement of `roll_base` was (0 to 1); `None` when it was picked by
    /// hand, or there is none. Below 0.5 the base is a guess.
    pub roll_base_confidence: Option<f32>,
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
    counters::count_convert(library);
    Ok(recipe)
}

/// Refuses an empty list of frames or of groups of settings.
fn check_apply_request(frames: &[FrameId], fields: &[RecipeField]) -> Result<(), AppError> {
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
    Ok(())
}

/// Copies the chosen groups of `from` onto each of `frames` (except `skip`, and repeated ones)
/// and saves them in one go; a frame without a saved recipe starts from the default.
fn copy_fields_onto(
    c: &mut gtp_catalog::Catalog,
    frames: &[FrameId],
    skip: Option<FrameId>,
    from: &Recipe,
    fields: &[RecipeField],
) -> Result<(), AppError> {
    let mut seen = std::collections::HashSet::new();
    let mut updated = Vec::new();
    for frame in frames {
        if Some(*frame) == skip || !seen.insert(*frame) {
            continue;
        }
        let mut recipe = c.load_recipe(*frame)?.unwrap_or_default();
        recipe.copy_fields_from(from, fields);
        updated.push((*frame, recipe));
    }
    c.save_recipes(&updated)
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
    check_apply_request(frames, fields)?;
    library.write(|c| {
        c.get_frame(source)?;
        let from = c.load_recipe(source)?.unwrap_or_default();
        copy_fields_onto(c, frames, Some(source), &from, fields)
    })
}

/// Like [`apply_recipe_to`], but the settings come from `recipe` (a saved Develop preset) and not
/// from a frame, so every listed frame is a target. A frame never edited starts from the default
/// recipe. The given `recipe` is clamped into range before its groups are copied.
pub fn apply_recipe_values_to(
    library: &Library,
    frames: &[FrameId],
    recipe: &Recipe,
    fields: &[RecipeField],
) -> Result<(), AppError> {
    check_apply_request(frames, fields)?;
    // The values come over IPC, so they are clamped here, as `save_recipe` does for an edit.
    let recipe = recipe.clone().clamped();
    library.write(|c| copy_fields_onto(c, frames, None, &recipe, fields))
}

/// `target` with the chosen groups of settings copied from `source`, clamped into range. Pure:
/// nothing is read or saved (the Develop view applies a preset to the open edit with it).
pub fn merge_recipe_fields(target: Recipe, source: &Recipe, fields: &[RecipeField]) -> Recipe {
    let mut merged = target;
    merged.copy_fields_from(source, fields);
    merged.clamped()
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
    /// The infrared plane at the size of `rgb`, when the frame has one that could be read.
    ir: Option<Arc<ImageF32>>,
    has_ir: bool,
    /// The long edge of the full scan the proxy was made from, so a render knows its scale.
    full_long_edge: u32,
    /// The roll's base as of the last open (the roll can get one after the proxy was built).
    film_base: Mutex<Option<[f32; 3]>>,
    /// The highest sequence number seen for this frame.
    latest: AtomicU64,
    /// One render at a time per frame, so a queue of superseded renders costs nothing.
    render_lock: Mutex<()>,
}

impl Entry {
    /// What a render starts from; the infrared plane only when `dust` (the `dust_removal` flag).
    fn input(&self, dust: bool) -> PipelineInput {
        PipelineInput {
            rgb: Arc::clone(&self.rgb),
            ir: self.ir.clone().filter(|_| dust),
            film_base: *self
                .film_base
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            full_long_edge: Some(self.full_long_edge),
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
    /// The `dust_removal` feature flag is off (see [`Self::set_dust_removal`]).
    dust_off: AtomicBool,
    /// The one full-size picture of the detail view, and the state of its requests.
    detail: DetailState,
}

/// A frame at full size as the detail view shows it.
struct DetailPicture {
    frame_id: FrameId,
    /// The clamped recipe it was rendered with.
    recipe: Recipe,
    /// What else the picture depends on: the roll's film base and the dust removal flag.
    context: DetailContext,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    /// When a request last used it.
    used: Instant,
}

/// What a detail picture depends on besides the frame and the recipe.
#[derive(Clone, Copy, PartialEq)]
struct DetailContext {
    film_base: Option<[f32; 3]>,
    dust_removal: bool,
}

/// The detail picture and the sequence numbers of the requests for it. These are separate from the
/// previews', so a detail request never cancels a preview and the reverse.
#[derive(Default)]
struct DetailState {
    picture: Mutex<Option<DetailPicture>>,
    /// The newest request per frame: its sequence number and its clamped recipe.
    latest: Mutex<HashMap<FrameId, (u64, Recipe)>>,
    /// One detail render at a time, so two requests for the same recipe render once.
    turn: Mutex<()>,
    /// Bumped by every release: a render that began before one does not keep its picture.
    generation: AtomicU64,
    /// How many full-size renders were made (for tests).
    renders: std::sync::atomic::AtomicUsize,
}

/// Converts fractions of the picture into whole pixels `(x, y, w, h)`: the origin is rounded down
/// and the far edge up, both clamped to the picture. NaN, negative values, an empty region or one
/// beyond the picture are `InvalidInput`, and so is one over [`MAX_DETAIL_EDGE`] on a side.
fn detail_pixels(region: Crop, width: u32, height: u32) -> Result<(u32, u32, u32, u32), AppError> {
    // A little slack, so fractions that were summed in f32 are not refused for rounding.
    const SLACK: f64 = 1e-4;
    let [x, y, w, h] = [region.x, region.y, region.w, region.h].map(f64::from);
    let valid = [x, y, w, h].iter().all(|v| v.is_finite())
        && x >= 0.0
        && y >= 0.0
        && w > 0.0
        && h > 0.0
        && x + w <= 1.0 + SLACK
        && y + h <= 1.0 + SLACK;
    if !valid {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The detail region must lie inside the picture.",
        )
        .with_details(format!("x {x} y {y} w {w} h {h}")));
    }
    let span = |origin: f64, size: f64, full: u32| {
        let full_f = f64::from(full);
        let first = (origin * full_f).floor().min(full_f - 1.0);
        let last = ((origin + size) * full_f).ceil().min(full_f);
        (first as u32, (last - first).max(1.0) as u32)
    };
    let (px, pw) = span(x, w, width);
    let (py, ph) = span(y, h, height);
    if pw > MAX_DETAIL_EDGE || ph > MAX_DETAIL_EDGE {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "A detail region may be at most 4096 pixels on a side.",
        )
        .with_details(format!("{pw}x{ph}")));
    }
    Ok((px, py, pw, ph))
}

impl DetailPicture {
    /// The header and pixels of the region `(x, y, w, h)`.
    fn cut(&self, seq: u64, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
        let mut out = Vec::with_capacity(DETAIL_HEADER_BYTES + (w * h * 4) as usize);
        out.extend_from_slice(&seq.to_le_bytes());
        for value in [w, h, x, y, self.width, self.height] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        let stride = self.width as usize * 4;
        for row in y..y + h {
            let start = row as usize * stride + x as usize * 4;
            out.extend_from_slice(&self.rgba[start..start + w as usize * 4]);
        }
        out
    }
}

fn superseded() -> AppError {
    AppError::new(
        ErrorCode::Cancelled,
        "A newer preview was requested, so this one was dropped.",
    )
}

/// Decodes a frame's picture at full size (linear f32, 3 channels): its RGB scan, or for a camera
/// triplet the red, green and blue originals, each a single-channel plane, put together and divided
/// by the roll's flat field when it has one.
pub(crate) fn read_scan(
    library: &Library,
    sources: &[SourceFile],
    flat: Option<&FlatField>,
) -> Result<ImageF32, AppError> {
    let Some(scan) = sources.iter().find(|s| s.role == SourceRole::Rgb) else {
        // Only camera triplets are corrected: the light they were made in is uneven. A flat field
        // of another size than the frame is skipped; the import says so (see `flat_field`).
        let mut image = read_triplet(library, sources)?;
        if let Some(flat) = flat {
            apply_flat_field(&mut image, flat)?;
        }
        return Ok(image);
    };
    let path = library.resolve(Path::new(&scan.rel_path))?;
    let tiff = read_tiff16(&path, &Limits::default())?;
    to_f32_linear(&tiff)
}

/// The red, green and blue originals of a camera triplet as one 3-channel picture.
fn read_triplet(library: &Library, sources: &[SourceFile]) -> Result<ImageF32, AppError> {
    let plane = |role: SourceRole| -> Result<ImageF32, AppError> {
        let source = sources
            .iter()
            .find(|s| s.role == role)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "This frame has no scan file."))?;
        let path = library.resolve(Path::new(&source.rel_path))?;
        let plane = to_f32_linear(&read_tiff16(&path, &Limits::default())?)?;
        if plane.channels != 1 {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "A triplet original must be a single channel.",
            )
            .with_details(format!("{} channels", plane.channels)));
        }
        Ok(plane)
    };
    let (r, g, b) = (
        plane(SourceRole::R)?,
        plane(SourceRole::G)?,
        plane(SourceRole::B)?,
    );
    let same = |p: &ImageF32| (p.width, p.height) == (r.width, r.height);
    if !same(&g) || !same(&b) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The three originals of this frame are not the same size.",
        )
        .with_details(format!(
            "{}x{}, {}x{}, {}x{}",
            r.width, r.height, g.width, g.height, b.width, b.height
        )));
    }
    let data = (0..r.data.len())
        .flat_map(|i| [r.data[i], g.data[i], b.data[i]])
        .collect();
    ImageF32::new(r.width, r.height, 3, data)
}

/// Decodes a frame's infrared plane at full size (linear f32, 1 channel), which must be as big as
/// the scan `like`.
pub(crate) fn read_ir(
    library: &Library,
    sources: &[SourceFile],
    like: (u32, u32),
) -> Result<ImageF32, AppError> {
    let source = sources
        .iter()
        .find(|s| s.role == SourceRole::Ir)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "This frame has no infrared file."))?;
    let path = library.resolve(Path::new(&source.rel_path))?;
    let plane = to_f32_linear(&read_tiff16(&path, &Limits::default())?)?;
    if plane.channels != 1 || (plane.width, plane.height) != like {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The infrared file does not fit the scan.",
        )
        .with_details(format!(
            "{}x{}x{} against {}x{}",
            plane.width, plane.height, plane.channels, like.0, like.1
        )));
    }
    Ok(plane)
}

/// The infrared plane of a frame, or `None` when it has none or it cannot be used: dust removal is
/// then skipped, and editing goes on (the problem is logged).
pub(crate) fn read_ir_or_skip(
    library: &Library,
    sources: &[SourceFile],
    frame: &Frame,
    like: (u32, u32),
) -> Option<ImageF32> {
    if !frame.has_ir {
        return None;
    }
    match read_ir(library, sources, like) {
        Ok(plane) => Some(plane),
        Err(err) => {
            tracing::warn!(frame = %frame.id, error = %err.message, "infrared plane not used");
            None
        }
    }
}

/// Decodes a frame's original and builds its proxy (and that of its infrared plane).
fn load(library: &Library, sources: &[SourceFile], frame: &Frame) -> Result<Entry, AppError> {
    let flat = library.read(|c| c.get_flat_field(frame.roll_id))?;
    let full = read_scan(library, sources, flat.as_ref())?;
    let proxy = build_proxy(&full, PROXY_EDGE)?;
    let ir = read_ir_or_skip(library, sources, frame, (full.width, full.height))
        .map(|plane| build_plane_proxy(&plane, PROXY_EDGE))
        .transpose()?
        .map(Arc::new);
    Ok(Entry {
        rgb: Arc::new(proxy),
        ir,
        has_ir: frame.has_ir,
        full_long_edge: full.width.max(full.height),
        film_base: Mutex::new(None),
        latest: AtomicU64::new(0),
        render_lock: Mutex::new(()),
    })
}

impl DevelopService {
    /// Turns dust removal on or off for the renders that follow (the `dust_removal` feature flag).
    pub fn set_dust_removal(&self, on: bool) {
        self.dust_off.store(!on, Ordering::Relaxed);
    }

    fn dust_removal(&self) -> bool {
        !self.dust_off.load(Ordering::Relaxed)
    }

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
            .map(|(_, entry)| Arc::clone(entry));
        let entry = match cached {
            Some(entry) => entry,
            None => Arc::new(load(library, &sources, &frame)?),
        };
        let mut input = entry.input(self.dust_removal());
        input.film_base = base;
        Ok(input)
    }

    /// Opens a frame for editing: decodes the original into the cache (when it is not there) and
    /// returns what the view needs. The roll's film base and the saved recipe are read fresh.
    pub fn open(&self, library: &Library, id: FrameId) -> Result<DevelopInfo, AppError> {
        self.release_idle_detail();
        let entry = self.ensure(library, id)?;
        let (frame, recipe, roll) = library.read(|c| {
            let frame = c.get_frame(id)?;
            let recipe = c.load_recipe(id)?;
            let roll = c.get_roll(frame.roll_id)?;
            Ok((frame, recipe, roll))
        })?;
        let base = roll.film_base_rgb;
        entry.set_base(base);
        Ok(DevelopInfo {
            width: frame.width,
            height: frame.height,
            has_ir: entry.has_ir,
            recipe: recipe.unwrap_or_default(),
            roll_base: base,
            roll_base_confidence: roll.film_base_confidence,
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
        self.analyze_with_look(library, id, base, Look::Neutral)
    }

    /// [`DevelopService::analyze`] whose automatic recipe also carries the values of `look`.
    pub fn analyze_with_look(
        &self,
        library: &Library,
        id: FrameId,
        base: Option<[f32; 3]>,
        look: Look,
    ) -> Result<DevelopAnalysis, AppError> {
        let entry = self.ensure(library, id)?;
        let mut input = entry.input(self.dust_removal());
        if let Some(base) = base {
            check_base(base)?;
            input.film_base = Some(base);
        }
        let (roll, hint) = hint_for_frame(library, id)?;
        // A scanner frame takes its roll's stored colour balance, so running auto again keeps it.
        let balance = roll_balance::frame_balance(library, id)?;
        let analysis = analyze_with_axes(&input, hint, look, balance)?;
        if let Some(size) = analysis.frame.measured_size.filter(|_| hint.is_some()) {
            record_frame_measurement(library, roll, size)?;
        }
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
        let frame = normalized_frame(&entry.input(self.dust_removal()), recipe)?;
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
        let frame = inverted_frame(&entry.input(self.dust_removal()), recipe)?;
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
        let image = crate::pipeline::render(
            &entry.input(self.dust_removal()),
            recipe,
            RenderTarget::Preview { max_edge },
        )?;
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

    /// Renders the frame at full size, as an export would (see `export::render_full`), and returns
    /// the part of it in `region` as the header and RGBA8 bytes described at
    /// [`DETAIL_HEADER_BYTES`]. `region` is in fractions of the whole rendered picture (each from
    /// 0 to 1), rounded outwards to whole pixels, and at most [`MAX_DETAIL_EDGE`] pixels a side.
    ///
    /// One picture is kept: while the frame and the (clamped) recipe stay the same, regions are cut
    /// from it; any other frame or recipe renders afresh and replaces it. A picture nobody asked for
    /// in [`DETAIL_IDLE_SECS`] is released. `seq` supersedes older requests for the same frame, which
    /// come back as `Cancelled`; a render that was already running finishes, and its picture is kept
    /// unless a newer request brought another recipe.
    pub fn render_detail(
        &self,
        library: &Library,
        id: FrameId,
        recipe: &Recipe,
        region: Crop,
        seq: u64,
    ) -> Result<Vec<u8>, AppError> {
        self.render_detail_at(library, id, recipe, region, seq, Instant::now())
    }

    /// [`Self::render_detail`] with the time given, so a test does not have to wait.
    fn render_detail_at(
        &self,
        library: &Library,
        id: FrameId,
        recipe: &Recipe,
        region: Crop,
        seq: u64,
        now: Instant,
    ) -> Result<Vec<u8>, AppError> {
        // Refused before anything is rendered; the size in pixels is checked once it is known.
        detail_pixels(region, 1, 1)?;
        let recipe = recipe.clone().clamped();
        let detail = &self.detail;
        let generation = detail.generation.load(Ordering::SeqCst);
        {
            let mut latest = detail.latest.lock().unwrap_or_else(PoisonError::into_inner);
            let entry = latest.entry(id).or_insert_with(|| (seq, recipe.clone()));
            if seq >= entry.0 {
                *entry = (seq, recipe.clone());
            }
        }
        let newest = || {
            detail
                .latest
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&id)
                .cloned()
        };
        let _turn = detail.turn.lock().unwrap_or_else(PoisonError::into_inner);
        if newest().is_some_and(|(n, _)| n > seq) {
            return Err(superseded());
        }
        let mut slot = detail
            .picture
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Self::release_if_idle(&mut slot, now);
        let context = DetailContext {
            film_base: library
                .read(|c| c.get_roll(c.get_frame(id)?.roll_id))?
                .film_base_rgb,
            dust_removal: self.dust_removal(),
        };
        let held = slot
            .as_ref()
            .is_some_and(|p| p.frame_id == id && p.recipe == recipe && p.context == context);
        if !held {
            // The old picture goes before the new one is made, so two never coexist.
            *slot = None;
            let picture = self.render_detail_picture(library, id, &recipe, context, now)?;
            detail.renders.fetch_add(1, Ordering::SeqCst);
            let (newest_seq, newest_recipe) = newest().unwrap_or((seq, recipe.clone()));
            // Kept unless another recipe was asked for meanwhile, or the view released it.
            let released = detail.generation.load(Ordering::SeqCst) != generation;
            if released {
                return Err(superseded());
            }
            if newest_recipe == recipe {
                *slot = Some(picture);
            }
            if newest_seq > seq || newest_recipe != recipe {
                return Err(superseded());
            }
        }
        let Some(picture) = slot.as_mut() else {
            return Err(superseded());
        };
        picture.used = now;
        let pixels = detail_pixels(region, picture.width, picture.height)?;
        Ok(picture.cut(seq, pixels))
    }

    /// Decodes and renders the frame at full size with `recipe`, the way an export does.
    fn render_detail_picture(
        &self,
        library: &Library,
        id: FrameId,
        recipe: &Recipe,
        context: DetailContext,
        now: Instant,
    ) -> Result<DetailPicture, AppError> {
        let (frame, sources) = library.read(|c| Ok((c.get_frame(id)?, c.sources_of(id)?)))?;
        let (full, ir) = read_full(library, &frame, &sources, context.dust_removal)?;
        let input = PipelineInput {
            rgb: Arc::new(full),
            ir: ir.map(Arc::new),
            film_base: context.film_base,
            full_long_edge: None,
        };
        let image = render_full(input, recipe, None)?;
        let rgba = to_rgba8_srgb(&image)?;
        Ok(DetailPicture {
            frame_id: id,
            recipe: recipe.clone(),
            context,
            width: image.width,
            height: image.height,
            rgba,
            used: now,
        })
    }

    fn release_if_idle(slot: &mut Option<DetailPicture>, now: Instant) {
        let idle = Duration::from_secs(DETAIL_IDLE_SECS);
        if slot
            .as_ref()
            .is_some_and(|p| now.saturating_duration_since(p.used) >= idle)
        {
            *slot = None;
        }
    }

    /// Releases the detail picture when nobody has used it for [`DETAIL_IDLE_SECS`]. Called when a
    /// frame is opened (`develop_open`) and before every detail request; there is no timer thread.
    pub fn release_idle_detail(&self) {
        self.release_idle_detail_at(Instant::now());
    }

    fn release_idle_detail_at(&self, now: Instant) {
        let mut slot = self
            .detail
            .picture
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Self::release_if_idle(&mut slot, now);
    }

    /// Memory held by the detail picture (0 when there is none).
    pub fn detail_bytes(&self) -> usize {
        self.detail
            .picture
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map_or(0, |p| p.rgba.len())
    }

    /// How many full-size detail renders have been made (for tests).
    pub fn detail_renders(&self) -> usize {
        self.detail.renders.load(Ordering::SeqCst)
    }

    /// The cached frames, most recently used first.
    pub fn cached(&self) -> Vec<FrameId> {
        self.cache().iter().map(|(id, _)| *id).collect()
    }

    /// Memory held by the cached proxies.
    pub fn cached_bytes(&self) -> usize {
        self.cache().iter().map(|(_, e)| e.bytes()).sum()
    }

    /// Drops the detail picture and forgets the detail requests' sequence numbers (the Develop view
    /// was left); the proxy cache stays. Safe with nothing cached. It does not wait for a render in
    /// flight: it bumps the release generation first, so a render that began before it discards its
    /// picture when it ends (and answers `Cancelled`), and then drops the picture if no render is
    /// holding it.
    pub fn release_detail(&self) {
        self.detail.generation.fetch_add(1, Ordering::SeqCst);
        self.detail
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        if let Ok(mut slot) = self.detail.picture.try_lock() {
            *slot = None;
        }
    }

    /// Forgets every proxy and the detail picture (another library was opened).
    pub fn clear(&self) {
        self.cache().clear();
        *self
            .detail
            .picture
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        self.detail
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use gtp_core::testing::synthetic::Pattern;
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
                    flag: gtp_core::FrameFlag::None,
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
    fn a_frame_with_an_infrared_plane_is_dusted_in_the_preview_unless_the_flag_is_off() {
        use gtp_core::testing::synthetic::Params;
        let t = Fixture::new("dust-preview");
        let dusty = Params {
            width: 400,
            height: 300,
            rebate: [28; 4],
            dust_specks: 8,
            with_ir: true,
            ..Params::default()
        };
        let id = t.frame_params(1, &dusty);
        let recipe = t.service.analyze(&t.library, id, None).unwrap().auto_recipe;
        let render = |seq: u64| {
            t.service.render(&t.library, id, &recipe, 400, seq).unwrap()[HEADER_BYTES..].to_vec()
        };
        let on = render(1);
        t.service.set_dust_removal(false);
        let off = render(2);
        assert_ne!(on, off, "the flag switches the stage");
        t.service.set_dust_removal(true);
        assert_eq!(render(3), on, "and back");

        // Dust removal off in the recipe has the same effect as the flag.
        let mut no_dust = recipe.clone();
        no_dust.dust.enabled = false;
        let bytes = t.service.render(&t.library, id, &no_dust, 400, 4).unwrap();
        assert_eq!(bytes[HEADER_BYTES..], off[..]);
    }

    #[test]
    fn an_infrared_file_that_does_not_fit_is_skipped_and_the_frame_still_opens() {
        use gtp_core::testing::synthetic::Params;
        let t = Fixture::new("dust-bad-plane");
        let params = Params {
            width: 200,
            height: 150,
            rebate: [14; 4],
            dust_specks: 4,
            with_ir: true,
            ..Params::default()
        };
        let id = t.frame_params(1, &params);
        let sources = t.library.read(|c| c.sources_of(id)).unwrap();
        let ir = sources.iter().find(|s| s.role == SourceRole::Ir).unwrap();
        let path = t.library.resolve(Path::new(&ir.rel_path)).unwrap();
        let small = gtp_io::Tiff16 {
            width: 10,
            height: 10,
            channels: 1,
            data: vec![0; 100],
        };
        gtp_io::write_tiff16(&path, &small, gtp_io::Compression::None).unwrap();
        let info = t.service.open(&t.library, id).unwrap();
        assert!(info.has_ir);
        t.service
            .render(&t.library, id, &Recipe::default(), 300, 1)
            .unwrap();
        assert!(t.service.get(id).unwrap().ir.is_none());
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
    fn the_cached_proxy_knows_the_long_edge_of_the_scan_it_came_from() {
        let t = Fixture::new("scale");
        let id = t.frame(1, 2400, 1600);
        let entry = t.service.ensure(&t.library, id).unwrap();
        let input = entry.input(false);
        assert_eq!(input.rgb.width.max(input.rgb.height), PROXY_EDGE);
        assert_eq!(input.full_long_edge, Some(2400));
        // So a preview of that proxy is rendered at a scale below 1, and thumbnails likewise.
        let scale = crate::pipeline::render_scale(PROXY_EDGE, &input);
        assert!((scale - PROXY_EDGE as f32 / 2400.0).abs() < 1e-6, "{scale}");
        let thumb = t.service.thumbnail_input(&t.library, id).unwrap();
        assert_eq!(thumb.full_long_edge, Some(2400));
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
    fn analysis_with_a_look_carries_its_curve_and_the_rest_is_the_same() {
        let t = Fixture::new("analyze-look");
        let id = t.frame(1, 1000, 700);
        let neutral = t.service.analyze(&t.library, id, None).unwrap();
        let looked = t
            .service
            .analyze_with_look(&t.library, id, None, Look::FilmLog)
            .unwrap();
        let default_master = Recipe::default().tone.curve.master;
        assert_eq!(neutral.auto_recipe.tone.curve.master, default_master);
        assert_eq!(looked.auto_recipe.tone.curve.master.len(), 16);
        let mut without = looked.auto_recipe.clone();
        without.tone.curve.master = default_master;
        assert_eq!(without, neutral.auto_recipe);
        assert_eq!(looked.frame_rect, neutral.frame_rect);
        assert_eq!(looked.base_rgb, neutral.base_rgb);
        let none = t
            .service
            .analyze_with_look(&t.library, id, None, Look::Neutral)
            .unwrap();
        assert_eq!(none.auto_recipe, neutral.auto_recipe);
    }

    #[test]
    fn running_auto_again_keeps_the_rolls_colour_balance_for_a_scanner_frame() {
        let t = Fixture::new("analyze-balance");
        let id = t.frame(1, 1000, 700);
        let plain = t.service.analyze(&t.library, id, None).unwrap();
        assert_eq!(
            plain.auto_recipe.color.red_cyan, 0.0,
            "no balance stored yet"
        );
        t.library
            .write(|c| c.store_roll_color_balance(t.roll, [4.0, -6.0, 9.0]))
            .unwrap();
        let balanced = t.service.analyze(&t.library, id, None).unwrap();
        let color = &balanced.auto_recipe.color;
        assert_eq!(
            [color.red_cyan, color.green_magenta, color.blue_yellow],
            [4.0, -6.0, 9.0]
        );
        // Nothing else of the recipe moved.
        let mut same = balanced.auto_recipe.clone();
        same.color = plain.auto_recipe.color;
        assert_eq!(same, plain.auto_recipe);
        assert_eq!(balanced.frame_rect, plain.frame_rect);
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
        let frame = inverted_frame(&entry.input(true), &recipe).unwrap();
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
        assert_eq!(counters::read(&t.library).unwrap().first_convert_at, None);
        let mut recipe = Recipe::default();
        recipe.tone.exposure_ev = 0.7;
        recipe.geometry.rot90 = 1;

        let stored = save_recipe(&t.library, id, recipe.clone()).unwrap();
        assert_eq!(stored, recipe, "in range already, so unchanged");
        assert_eq!(t.stored(id), Some(recipe.clone()));
        assert!(t.edited(id));
        assert!(counters::read(&t.library)
            .unwrap()
            .first_convert_at
            .is_some());
        // The Develop view gets it back when it opens the frame again.
        let scan = t.frame(2, 400, 300);
        save_recipe(&t.library, scan, recipe.clone()).unwrap();
        assert_eq!(t.service.open(&t.library, scan).unwrap().recipe, recipe);
    }

    // --- The detail view (U-29) ---

    const WHOLE: Crop = Crop {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };

    fn crop(x: f32, y: f32, w: f32, h: f32) -> Crop {
        Crop { x, y, w, h }
    }

    /// The header of a detail buffer: seq, then region size, region origin, full picture size.
    fn detail_header(bytes: &[u8]) -> (u64, [u32; 6]) {
        let word = |i: usize| u32::from_le_bytes(bytes[8 + i * 4..12 + i * 4].try_into().unwrap());
        (
            u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            [word(0), word(1), word(2), word(3), word(4), word(5)],
        )
    }

    fn detail_pixels_of(bytes: &[u8]) -> &[u8] {
        &bytes[DETAIL_HEADER_BYTES..]
    }

    /// The rectangle `(x, y, w, h)` of an RGBA picture `width` wide.
    fn rectangle(rgba: &[u8], width: u32, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
        (y..y + h)
            .flat_map(|row| {
                let start = (row * width + x) as usize * 4;
                rgba[start..start + w as usize * 4].iter().copied()
            })
            .collect()
    }

    #[test]
    fn a_detail_region_is_a_header_then_the_pixels_of_that_part_of_the_picture() {
        let t = Fixture::new("detail-region");
        let id = t.frame(1, 600, 400);
        let recipe = Recipe::default();
        let whole = t
            .service
            .render_detail(&t.library, id, &recipe, WHOLE, 1)
            .unwrap();
        let (seq, [w, h, x, y, full_w, full_h]) = detail_header(&whole);
        assert_eq!((seq, x, y), (1, 0, 0));
        assert_eq!((w, h), (full_w, full_h));
        assert_eq!(whole.len(), DETAIL_HEADER_BYTES + (w * h * 4) as usize);
        assert!(detail_pixels_of(&whole).chunks(4).all(|px| px[3] == 255));

        // A region inside, one touching the far corner, one touching the near corner.
        for (n, (region, pixels)) in [
            (
                crop(0.25, 0.5, 0.5, 0.25),
                (full_w / 4, full_h / 2, full_w / 2, full_h / 4),
            ),
            (
                crop(0.75, 0.75, 0.25, 0.25),
                (full_w * 3 / 4, full_h * 3 / 4, full_w / 4, full_h / 4),
            ),
            (crop(0.0, 0.0, 0.5, 0.5), (0, 0, full_w / 2, full_h / 2)),
        ]
        .into_iter()
        .enumerate()
        {
            let seq = 2 + n as u64;
            let cut = t
                .service
                .render_detail(&t.library, id, &recipe, region, seq)
                .unwrap();
            let (got, header) = detail_header(&cut);
            assert_eq!(got, seq);
            assert_eq!(
                header,
                [pixels.2, pixels.3, pixels.0, pixels.1, full_w, full_h]
            );
            assert_eq!(
                detail_pixels_of(&cut),
                rectangle(detail_pixels_of(&whole), full_w, pixels)
            );
        }
        // All of them came from the one picture.
        assert_eq!(t.service.detail_renders(), 1);
    }

    #[test]
    fn a_fractional_region_is_rounded_outwards_to_whole_pixels() {
        // 0.3125 and 0.625 of 16 are 5 and 10: pixels 5 to 9.
        assert_eq!(
            detail_pixels(crop(0.3125, 0.0, 0.3125, 1.0), 16, 4).unwrap(),
            (5, 0, 5, 4)
        );
        // 2.6 rounds down to 2, 7.6 up to 8.
        assert_eq!(
            detail_pixels(crop(0.26, 0.5, 0.5, 0.26), 10, 10).unwrap(),
            (2, 5, 6, 3)
        );
        // The far edge is clamped to the picture; a hair beyond 1 (f32 sums) is accepted.
        assert_eq!(
            detail_pixels(crop(0.875, 0.875, 0.125_01, 0.125), 8, 8).unwrap(),
            (7, 7, 1, 1)
        );
        // A region thinner than a pixel still covers one.
        assert_eq!(
            detail_pixels(crop(0.51, 0.51, 0.001, 0.001), 10, 10).unwrap(),
            (5, 5, 1, 1)
        );
    }

    #[test]
    fn a_region_outside_the_picture_or_empty_or_too_big_is_refused() {
        let invalid = |region: Crop, w: u32, h: u32| {
            let err = detail_pixels(region, w, h).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput, "{region:?}");
        };
        invalid(crop(f32::NAN, 0.0, 0.5, 0.5), 100, 100);
        invalid(crop(0.0, 0.0, f32::INFINITY, 0.5), 100, 100);
        invalid(crop(-0.1, 0.0, 0.5, 0.5), 100, 100);
        invalid(crop(0.0, 0.0, 0.0, 0.5), 100, 100);
        invalid(crop(0.0, 0.0, 0.5, -0.5), 100, 100);
        invalid(crop(0.6, 0.0, 0.5, 0.5), 100, 100);
        invalid(crop(0.0, 0.5, 0.5, 0.6), 100, 100);
        // The cap: 4096 is allowed, one more is not, on either side.
        assert!(detail_pixels(WHOLE, MAX_DETAIL_EDGE, 100).is_ok());
        invalid(WHOLE, MAX_DETAIL_EDGE + 1, 100);
        invalid(WHOLE, 100, MAX_DETAIL_EDGE + 1);
        assert!(detail_pixels(crop(0.0, 0.0, 0.5, 1.0), 2 * MAX_DETAIL_EDGE, 100).is_ok());

        // Through the service, before anything is rendered.
        let t = Fixture::new("detail-refused");
        let id = t.frame(1, 300, 200);
        let err = t
            .service
            .render_detail(
                &t.library,
                id,
                &Recipe::default(),
                crop(0.5, 0.5, 0.6, 0.2),
                1,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(t.service.detail_renders(), 0);
    }

    #[test]
    fn a_frame_without_a_scan_is_not_found_for_a_detail() {
        let t = Fixture::new("detail-missing");
        let id = t.bare(1);
        let err = t
            .service
            .render_detail(&t.library, id, &Recipe::default(), WHOLE, 1)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(t.service.detail_bytes(), 0);
    }

    #[test]
    fn the_same_frame_and_recipe_render_once_and_a_new_recipe_or_frame_replaces_the_picture() {
        let t = Fixture::new("detail-cache");
        let one = t.frame(1, 300, 200);
        let two = t.frame(2, 300, 200);
        let recipe = Recipe::default();
        let ask = |id, recipe: &Recipe, seq| {
            t.service
                .render_detail(&t.library, id, recipe, crop(0.0, 0.0, 0.5, 0.5), seq)
                .unwrap()
        };
        assert_eq!(t.service.detail_bytes(), 0);
        ask(one, &recipe, 1);
        let bytes = t.service.detail_bytes();
        assert!(bytes >= 300 * 200 * 4, "{bytes}");
        ask(one, &recipe, 2);
        // A recipe that clamps to the same values is the same recipe.
        let mut wild = recipe.clone();
        wild.tone.exposure_ev = 1000.0;
        let mut clamped = recipe.clone();
        clamped.tone.exposure_ev = wild.clone().clamped().tone.exposure_ev;
        ask(one, &wild, 3);
        ask(one, &clamped, 4);
        assert_eq!(
            t.service.detail_renders(),
            2,
            "the wild recipe is another one"
        );

        let mut brighter = recipe.clone();
        brighter.tone.exposure_ev = 0.5;
        let after = ask(one, &brighter, 5);
        assert_eq!(t.service.detail_renders(), 3);
        assert_eq!(t.service.detail_bytes(), bytes);
        let before = ask(one, &recipe, 6);
        assert_eq!(t.service.detail_renders(), 4);
        assert_ne!(detail_pixels_of(&after), detail_pixels_of(&before));

        ask(two, &recipe, 1);
        assert_eq!(t.service.detail_renders(), 5);
        ask(one, &recipe, 7);
        assert_eq!(t.service.detail_renders(), 6);
        assert_eq!(t.service.detail_bytes(), bytes);
    }

    #[test]
    fn a_new_film_base_or_the_dust_flag_renders_the_detail_again() {
        let t = Fixture::new("detail-context");
        let id = t.frame(1, 300, 200);
        let recipe = Recipe::default();
        let ask = |seq| {
            t.service
                .render_detail(&t.library, id, &recipe, WHOLE, seq)
                .unwrap()
        };
        ask(1);
        ask(2);
        assert_eq!(t.service.detail_renders(), 1);
        roll_set_film_base(&t.library, t.roll, Some([0.8, 0.5, 0.3])).unwrap();
        ask(3);
        assert_eq!(t.service.detail_renders(), 2);
        t.service.set_dust_removal(false);
        ask(4);
        assert_eq!(t.service.detail_renders(), 3);
    }

    #[test]
    fn releasing_drops_the_detail_picture_and_is_safe_with_nothing_cached() {
        let t = Fixture::new("detail-release");
        let id = t.frame(1, 300, 200);
        t.service.release_detail();
        assert_eq!(t.service.detail_bytes(), 0);
        t.service
            .render_detail(&t.library, id, &Recipe::default(), WHOLE, 1)
            .unwrap();
        t.service.open(&t.library, id).unwrap();
        assert!(t.service.detail_bytes() > 0);
        t.service.release_detail();
        assert_eq!(t.service.detail_bytes(), 0);
        // The proxy stays, and a later request renders again (its number may be lower: the
        // sequence numbers were forgotten).
        assert_eq!(t.service.cached(), vec![id]);
        t.service
            .render_detail(&t.library, id, &Recipe::default(), WHOLE, 1)
            .unwrap();
        assert!(t.service.detail_bytes() > 0);
        assert_eq!(t.service.detail_renders(), 2);
    }

    #[test]
    fn a_detail_render_that_began_before_a_release_does_not_keep_its_picture() {
        let t = Fixture::new("detail-release-flight");
        let id = t.frame(1, 300, 200);
        let recipe = Recipe::default();
        // Holding the turn stands in for a render that is running: the request has read the
        // generation and registered itself, and waits.
        let turn = t.service.detail.turn.lock().unwrap();
        let result = std::thread::scope(|scope| {
            let a = scope.spawn(|| t.service.render_detail(&t.library, id, &recipe, WHOLE, 1));
            while t.service.detail.latest.lock().unwrap().get(&id).is_none() {
                std::thread::yield_now();
            }
            t.service.release_detail();
            drop(turn);
            a.join().unwrap()
        });
        assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
        assert_eq!(t.service.detail_bytes(), 0);
        // A request made after the release is a new one and keeps its picture.
        t.service
            .render_detail(&t.library, id, &recipe, WHOLE, 1)
            .unwrap();
        assert!(t.service.detail_bytes() > 0);
    }

    #[test]
    fn the_detail_picture_is_released_after_the_idle_time_and_by_clear() {
        let t = Fixture::new("detail-idle");
        let id = t.frame(1, 300, 200);
        let recipe = Recipe::default();
        let start = Instant::now();
        let ask = |at: Instant, seq| {
            t.service
                .render_detail_at(&t.library, id, &recipe, WHOLE, seq, at)
                .unwrap()
        };
        ask(start, 1);
        assert!(t.service.detail_bytes() > 0);

        // Used again just before the limit: kept, and the clock starts over.
        let almost = start + Duration::from_secs(DETAIL_IDLE_SECS - 1);
        ask(almost, 2);
        t.service
            .release_idle_detail_at(almost + Duration::from_secs(DETAIL_IDLE_SECS - 1));
        assert!(t.service.detail_bytes() > 0);
        assert_eq!(t.service.detail_renders(), 1);

        // Idle for the limit: released, and a request after that renders again.
        let later = almost + Duration::from_secs(DETAIL_IDLE_SECS);
        t.service.release_idle_detail_at(later);
        assert_eq!(t.service.detail_bytes(), 0);
        ask(later, 3);
        assert_eq!(t.service.detail_renders(), 2);
        // Released by the request itself when nothing else did.
        ask(later + Duration::from_secs(DETAIL_IDLE_SECS), 4);
        assert_eq!(t.service.detail_renders(), 3);

        t.service.clear();
        assert_eq!(t.service.detail_bytes(), 0);
    }

    #[test]
    fn an_older_detail_request_is_cancelled_and_previews_are_not_touched() {
        let t = Fixture::new("detail-seq");
        let id = t.frame(1, 300, 200);
        let recipe = Recipe::default();
        let half = crop(0.0, 0.0, 0.5, 0.5);
        t.service
            .render_detail(&t.library, id, &recipe, half, 10)
            .unwrap();
        let err = t
            .service
            .render_detail(&t.library, id, &recipe, half, 9)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);

        // A preview numbered below the detail requests is rendered, and a detail request numbered
        // below a newer preview is too: the two counters are separate.
        t.service.render(&t.library, id, &recipe, 400, 3).unwrap();
        t.service
            .render_detail(&t.library, id, &recipe, half, 11)
            .unwrap();
        t.service.render(&t.library, id, &recipe, 400, 40).unwrap();
        t.service
            .render_detail(&t.library, id, &recipe, half, 12)
            .unwrap();
        // Another frame has its own numbers.
        let other = t.frame(2, 300, 200);
        t.service
            .render_detail(&t.library, other, &recipe, half, 1)
            .unwrap();
    }

    /// Runs two detail requests that queue behind a held turn, the older first, and releases the
    /// turn once both are waiting. Returns their results.
    fn queued_details(
        t: &Fixture,
        id: FrameId,
        older: &Recipe,
        newer: &Recipe,
    ) -> (Result<Vec<u8>, AppError>, Result<Vec<u8>, AppError>) {
        let half = crop(0.0, 0.0, 0.5, 0.5);
        let newest = |at: u64| {
            while t
                .service
                .detail
                .latest
                .lock()
                .unwrap()
                .get(&id)
                .is_none_or(|(n, _)| *n < at)
            {
                std::thread::yield_now();
            }
        };
        // Holding the turn stands in for a render that is running.
        let turn = t.service.detail.turn.lock().unwrap();
        std::thread::scope(|scope| {
            let a = scope.spawn(|| t.service.render_detail(&t.library, id, older, half, 1));
            newest(1);
            let b = scope.spawn(|| t.service.render_detail(&t.library, id, newer, half, 2));
            newest(2);
            drop(turn);
            (a.join().unwrap(), b.join().unwrap())
        })
    }

    #[test]
    fn a_detail_request_that_waits_its_turn_is_dropped_when_a_newer_one_arrives() {
        let t = Fixture::new("detail-queue");
        let id = t.frame(1, 300, 200);
        let recipe = Recipe::default();
        let (older, newer) = queued_details(&t, id, &recipe, &recipe);
        assert_eq!(older.unwrap_err().code, ErrorCode::Cancelled);
        assert!(newer.is_ok());
        // Two requests, one render.
        assert_eq!(t.service.detail_renders(), 1);

        let t = Fixture::new("detail-queue-recipe");
        let id = t.frame(1, 300, 200);
        let mut brighter = recipe.clone();
        brighter.tone.exposure_ev = 0.5;
        let (older, newer) = queued_details(&t, id, &recipe, &brighter);
        assert_eq!(older.unwrap_err().code, ErrorCode::Cancelled);
        assert!(newer.is_ok());
        assert_eq!(
            t.service.detail_renders(),
            1,
            "the dropped one never rendered"
        );
    }

    #[test]
    fn output_sharpening_and_noise_reduction_are_part_of_the_detail() {
        let t = Fixture::new("detail-stages");
        let id = t.frame_with(1, 300, 200, Pattern::Wedge);
        let plain = Recipe::default();
        let mut sharp = plain.clone();
        sharp.output.sharpen.amount = 100.0;
        let mut quiet = plain.clone();
        quiet.noise.luminance = 60.0;
        quiet.noise.color = 60.0;
        let get = |recipe: &Recipe, seq| {
            t.service
                .render_detail(&t.library, id, recipe, WHOLE, seq)
                .unwrap()
        };
        let base = get(&plain, 1);
        assert_ne!(
            detail_pixels_of(&get(&sharp, 2)),
            detail_pixels_of(&base),
            "output sharpening applies"
        );
        assert_ne!(
            detail_pixels_of(&get(&quiet, 3)),
            detail_pixels_of(&base),
            "noise reduction applies"
        );
    }

    /// Times the detail render of a 24 megapixel frame with every tool on, and a region cut from
    /// the picture afterwards; run with
    /// `cargo test -p gtp-app --release -- --ignored --nocapture time_a_detail_render`.
    /// The synthetic generator stops at 4096 px a side, so the picture is built in memory and goes
    /// through the shared full-size render (`export::render_full`), then the same conversion and cut
    /// the service makes; only the decode of the scan is left out. The budget is 3 s
    /// (docs/specification.md section 11).
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_a_detail_render() {
        let (width, height) = (6000u32, 4000u32);
        let data: Vec<f32> = (0..width * height * 3)
            .map(|i| {
                let px = i / 3;
                let (x, y) = (px % width, px / width);
                let noise = ((px.wrapping_mul(2_654_435_761) >> 16) & 0xff) as f32 / 4000.0;
                0.05 + 0.6 * (x as f32 / width as f32) * (0.5 + 0.5 * (y as f32 / height as f32))
                    + noise
            })
            .collect();
        let scan = ImageF32::new(width, height, 3, data).unwrap();
        let mut recipe = Recipe::default();
        recipe.noise.luminance = 50.0;
        recipe.noise.color = 50.0;
        recipe.color.mixer.hue[0] = 10.0;
        recipe.color.mixer.saturation[2] = 20.0;
        recipe.color.mixer.lightness[5] = -10.0;
        recipe.color.split_tone.shadow_hue = 220.0;
        recipe.color.split_tone.shadow_strength = 20.0;
        recipe.color.split_tone.highlight_hue = 40.0;
        recipe.color.split_tone.highlight_strength = 15.0;
        recipe.tone.vignette.amount = -30.0;
        recipe.geometry.angle_deg = 0.5;
        recipe.output.sharpen.amount = 60.0;
        let input = PipelineInput {
            rgb: Arc::new(scan),
            ir: None,
            film_base: None,
            full_long_edge: None,
        };
        let start = Instant::now();
        let image = render_full(input, &recipe, None).unwrap();
        let rendered = start.elapsed();
        let rgba = to_rgba8_srgb(&image).unwrap();
        let converted = start.elapsed();
        eprintln!(
            "detail {}x{}: render and sharpen {rendered:?}, with conversion to RGBA8 {converted:?}, {} MB held",
            image.width,
            image.height,
            rgba.len() / 1_000_000
        );
        let picture = DetailPicture {
            frame_id: FrameId::new(),
            recipe: recipe.clone(),
            context: DetailContext {
                film_base: None,
                dust_removal: true,
            },
            width: image.width,
            height: image.height,
            rgba,
            used: Instant::now(),
        };
        let start = Instant::now();
        let runs = 20;
        for _ in 0..runs {
            let cut = picture.cut(1, (1000, 1000, 2400, 1600));
            assert!(!cut.is_empty());
        }
        eprintln!(
            "a 2400x1600 region cut from the picture: {:?}",
            start.elapsed() / runs
        );
        eprintln!(
            "cores: {}",
            std::thread::available_parallelism().map_or(0, std::num::NonZero::get)
        );
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
    fn colour_and_split_toning_are_applied_independently() {
        let t = Fixture::new("apply-split-tone");
        let (source, target) = (t.bare(1), t.bare(2));
        let mut src = Recipe::default();
        src.color.temperature = 20.0;
        src.color.split_tone.highlight_strength = 60.0;
        src.color.split_tone.shadow_hue = 200.0;
        save_recipe(&t.library, source, src.clone()).unwrap();
        let mut own = Recipe::default();
        own.color.tint = -7.0;
        own.color.split_tone.balance = 25.0;
        save_recipe(&t.library, target, own.clone()).unwrap();

        // Split toning alone leaves the colour as it was.
        apply_recipe_to(&t.library, &[target], source, &[RecipeField::SplitTone]).unwrap();
        let after = t.stored(target).unwrap();
        assert_eq!(after.color.split_tone, src.color.split_tone);
        assert_eq!(after.color.tint, -7.0);
        assert_eq!(after.color.temperature, 0.0);

        // Colour alone leaves the split toning as it was.
        save_recipe(&t.library, target, own.clone()).unwrap();
        apply_recipe_to(&t.library, &[target], source, &[RecipeField::Color]).unwrap();
        let after = t.stored(target).unwrap();
        assert_eq!(after.color.temperature, 20.0);
        assert_eq!(after.color.tint, 0.0);
        assert_eq!(after.color.split_tone, own.color.split_tone);
    }

    #[test]
    fn colour_and_the_colour_mixer_are_applied_independently() {
        let t = Fixture::new("apply-mixer");
        let (source, target) = (t.bare(1), t.bare(2));
        let mut src = Recipe::default();
        src.color.temperature = 20.0;
        src.color.mixer.hue[3] = 40.0;
        src.color.mixer.saturation[0] = -25.0;
        save_recipe(&t.library, source, src.clone()).unwrap();
        let mut own = Recipe::default();
        own.color.tint = -7.0;
        own.color.mixer.lightness[6] = 12.0;
        save_recipe(&t.library, target, own.clone()).unwrap();

        // The mixer alone leaves the colour as it was.
        apply_recipe_to(&t.library, &[target], source, &[RecipeField::Mixer]).unwrap();
        let after = t.stored(target).unwrap();
        assert_eq!(after.color.mixer, src.color.mixer);
        assert_eq!(after.color.tint, -7.0);
        assert_eq!(after.color.temperature, 0.0);

        // Colour alone leaves the mixer as it was.
        save_recipe(&t.library, target, own.clone()).unwrap();
        apply_recipe_to(&t.library, &[target], source, &[RecipeField::Color]).unwrap();
        let after = t.stored(target).unwrap();
        assert_eq!(after.color.temperature, 20.0);
        assert_eq!(after.color.tint, 0.0);
        assert_eq!(after.color.mixer, own.color.mixer);
    }

    #[test]
    fn noise_reduction_is_applied_on_its_own() {
        let t = Fixture::new("apply-noise");
        let (source, target) = (t.bare(1), t.bare(2));
        let mut src = Recipe::default();
        src.noise.luminance = 40.0;
        src.noise.color = 70.0;
        src.tone.exposure_ev = 0.5;
        save_recipe(&t.library, source, src.clone()).unwrap();
        let mut own = Recipe::default();
        own.output.sharpen.amount = 30.0;
        save_recipe(&t.library, target, own.clone()).unwrap();

        // The noise part alone leaves the rest as it was.
        apply_recipe_to(&t.library, &[target], source, &[RecipeField::Noise]).unwrap();
        let after = t.stored(target).unwrap();
        assert_eq!(after.noise, src.noise);
        assert_eq!(after.tone, own.tone);
        assert_eq!(after.output, own.output);

        // Other parts alone leave the target's noise reduction as it was.
        save_recipe(&t.library, target, own.clone()).unwrap();
        apply_recipe_to(
            &t.library,
            &[target],
            source,
            &[RecipeField::Tone, RecipeField::Output],
        )
        .unwrap();
        let after = t.stored(target).unwrap();
        assert_eq!(after.tone, src.tone);
        assert_eq!(after.noise, own.noise);
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

    #[test]
    fn values_are_applied_to_every_listed_frame_and_a_frame_without_a_recipe_starts_from_default() {
        let t = Fixture::new("apply-values");
        let (edited, fresh) = (t.bare(1), t.bare(2));
        let mut own = Recipe::default();
        own.color.temperature = 15.0;
        own.geometry.angle_deg = -1.0;
        save_recipe(&t.library, edited, own.clone()).unwrap();
        assert!(!t.edited(fresh));

        apply_recipe_values_to(
            &t.library,
            &[edited, fresh, fresh],
            &source_recipe(),
            &[RecipeField::Tone, RecipeField::Output],
        )
        .unwrap();

        let want = |base: &Recipe| {
            let mut r = base.clone();
            r.tone = source_recipe().tone;
            r.output = source_recipe().output;
            r
        };
        assert_eq!(t.stored(edited), Some(want(&own)));
        assert_eq!(t.stored(fresh), Some(want(&Recipe::default())));
        assert!(t.edited(edited) && t.edited(fresh), "targets are stamped");
    }

    #[test]
    fn applying_values_needs_frames_and_fields_and_known_frames() {
        let t = Fixture::new("apply-values-bad");
        let (a, b) = (t.bare(1), t.bare(2));
        let recipe = source_recipe();
        let code = |r: Result<(), AppError>| r.unwrap_err().code;
        assert_eq!(
            code(apply_recipe_values_to(
                &t.library,
                &[],
                &recipe,
                &[RecipeField::Tone]
            )),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(apply_recipe_values_to(&t.library, &[a], &recipe, &[])),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(apply_recipe_values_to(
                &t.library,
                &[a, FrameId::new(), b],
                &recipe,
                &[RecipeField::Tone]
            )),
            ErrorCode::NotFound
        );
        assert_eq!((t.stored(a), t.stored(b)), (None, None));
    }

    #[test]
    fn applying_values_is_refused_in_a_read_only_library() {
        let temp = crate::services::testkit::TempDir::new("develop-values-read-only");
        let dir = temp.path();
        let library = Library::create(dir, "Develop").unwrap();
        let roll = crate::services::roll_frame::roll_create(
            &library,
            crate::services::roll_frame::RollInput {
                name: "Roll".into(),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        let frame = crate::services::testkit::add_frame(&library, roll, 1, 64, 48);
        drop(library);
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(dir.join(crate::library_fs::DB_FILE))
                .unwrap();
            file.seek(SeekFrom::Start(60)).unwrap();
            file.write_all(&(gtp_catalog::SCHEMA_VERSION + 1).to_be_bytes())
                .unwrap();
        }
        let library = Library::open(dir).unwrap();
        assert!(library.is_read_only());
        let err =
            apply_recipe_values_to(&library, &[frame], &source_recipe(), &[RecipeField::Tone])
                .unwrap_err();
        assert_eq!(err.code, ErrorCode::LibraryTooNew);
    }

    #[test]
    fn merging_copies_only_the_chosen_parts_and_clamps() {
        let mut source = source_recipe();
        source.color.temperature = 20.0;
        source.color.split_tone.highlight_strength = 60.0;
        let mut target = Recipe::default();
        target.color.split_tone.shadow_hue = 100.0;

        // Colour alone leaves the split toning, tone and geometry of the target.
        let merged = merge_recipe_fields(target.clone(), &source, &[RecipeField::Color]);
        assert_eq!(merged.color.temperature, 20.0);
        assert_eq!(merged.color.split_tone, target.color.split_tone);
        assert_eq!(merged.tone, target.tone);
        assert_eq!(merged.geometry, target.geometry);

        // Split toning alone leaves the rest of the colour.
        let merged = merge_recipe_fields(target.clone(), &source, &[RecipeField::SplitTone]);
        assert_eq!(merged.color.split_tone, source.color.split_tone);
        assert_eq!(merged.color.temperature, target.color.temperature);

        let merged = merge_recipe_fields(target.clone(), &source, &[RecipeField::Tone]);
        assert_eq!(merged.tone, source.tone);
        assert_eq!(merged.color, target.color);
        assert_eq!(merged.output, target.output);

        let merged = merge_recipe_fields(target.clone(), &source, &[RecipeField::Geometry]);
        assert_eq!(merged.geometry.angle_deg, 2.0);
        assert_eq!(merged.tone, target.tone);

        // No parts: the target comes back as it was.
        assert_eq!(merge_recipe_fields(target.clone(), &source, &[]), target);

        // An out-of-range value is clamped.
        let mut wild = Recipe::default();
        wild.tone.exposure_ev = 5000.0;
        let merged = merge_recipe_fields(Recipe::default(), &wild, &[RecipeField::Tone]);
        assert_ne!(merged.tone.exposure_ev, 5000.0);
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
