# Grain to Pixel: Technical Specification (MVP)

- Status: Draft v0.3, 2026-10-29 (guided RGB capture added)
- Inputs: [product/idea.md](product/idea.md), [structure.md](structure.md), [ADR 0001](../decisions/0001-monorepo.md), [ADR 0002](../decisions/0002-tech-stack.md), [ADR 0003](../decisions/0003-ls-50-via-nkscan.md)
- Principle: KISS. One user at a time, local only, no servers, no accounts. Add complexity only when a measured problem demands it.

## 1. Scope

**Problem:** one app to scan, convert, edit and organize film negatives, replacing paid tools (Nikon Scan 4, Trichroma Lab) for the author and a few friends.

**Core workflow (MVP):**
1. **Ingest:** scan a whole roll (up to 40 frames) on the author's modified Nikon Coolscan LS-50 (RGB + IR), or import R/G/B exposure triplets photographed with a camera (Nikon D600, NEF). Add tags.
2. **Prepare:** detect the frame area, measure the film base.
3. **Convert:** negative to positive.
4. **Edit:** colour balance, tone, geometry, IR dust removal.
5. **Organize:** tagged, searchable archive; export.

**Success metrics**
- *Activation:* a new user completes ingest, convert and export on their first session (measured locally, see §14).
- *Quality:* results comparable to Nikon Scan 4 / Trichroma Lab. Proposed measure: on at least 10 reference frames, median ΔE2000 between our auto-converted output and the reference render is at most 6, and at most 3 after manual tweaks, plus a blind side-by-side check by the author. Calibrate the numbers in M1.

### 1.1 Decisions confirmed

| Topic | Decision |
|---|---|
| Camera input | Three exposures per frame under R, G and B light (Trichroma-style), shot on a Nikon D600 (24 MP, 14-bit NEF) with a 60 mm f/2.8 AF-D macro. Each exposure contributes the channel matching its light. Fixed tripod and light table assumed (no image registration in MVP). |
| Light source | Jack's Scanlight V4: narrowband deep-red, green and deep-blue LEDs (plus white), USB control, and automated shutter triggering for separate R, G and B exposures (per the [jackw01/scanlight](https://github.com/jackw01/scanlight) project description; confirm the V4 specifics). Narrowband light is why each exposure contributes only its matching channel. |
| Guided RGB capture | **In the MVP.** The app drives the Scanlight V4 over its USB serial interface (one LED colour at a time, R then G then B, same exposure time for all three) and captures each exposure with the D600 through one of two camera backends, then stores the triplet as one frame. Details: [ADR 0004](../decisions/0004-guided-rgb-capture.md) and §6.1. |
| Camera backends | **macOS is the primary platform for tethered capture**: libgphoto2 (the app triggers and downloads over USB; D600 support to be verified in the spike). **Windows** uses [digiCamControl](https://digicamcontrol.com/) as the tether tool: the Scanlight's `SHUTTER_PULSE` fires the D600 through the author's remote cable, digiCamControl downloads each NEF to a session folder, and the app watches that folder. Importing existing NEFs stays available on both. |
| Scanlight hardware | PCB **rev B** (26a902b): isolated shutter and focus drivers, so `SET_FOCUS` is available. The author already has the D600 remote cable. |
| Library | **Managed library folder**: the app copies originals into one folder it owns, plus a SQLite catalog. |
| Distribution | GitHub Releases, **unsigned** builds (macOS ad-hoc signed), auto-update with Tauri updater. |
| Platforms | Windows 10/11 x64 and macOS 13+ on Apple Silicon (arm64). Windows on ARM can run the x64 installer under Windows 11 emulation, but that is untested and unsupported (no ARM device to test on); a native ARM64 build waits until a tester exists. Intel Mac and Linux are unsupported. |
| Scanner | The author's LS-50 is **modified to scan whole rolls (up to 40 frames)** with [coolscan-mods](https://github.com/kosma/coolscan-mods): modified firmware re-enables the scanner's built-in SA-30 roll-adapter logic, the SA-21 is jumpered to identify as an SA-30, and the rear is opened for long strips. Validated in Nikon Scan 4, VueScan and Negpy. The scanner therefore presents a standard SA-30 roll adapter, and whether nkscan supports that holder type is the main M0 question. If it does, the MVP has a roll-scan job; otherwise it imports TIFFs scanned in Nikon Scan 4 / VueScan. Flashing the firmware is a one-time external step, not part of the app. |
| Reference data | The author will supply 10+ frames scanned with Nikon Scan 4 / Trichroma Lab, with the raw scans, for the quality benchmark. |
| Exports | sRGB JPEG and 16-bit TIFF (sRGB, Adobe RGB, ProPhoto) are sufficient. |
| Languages | English only; strings externalized. |

### 1.2 Non-goals

- Masking, local adjustments, AI generation or AI retouching, healing/cloning.
- Digital-camera RAW editing as a general feature (RAW is accepted only as input for RGB triplets).
- Support for other scanners or other LS-50 modifications (only the author's whole-roll LS-50 is targeted).
- Cloud sync, accounts, multi-user, sharing links, social features.
- Printing, soft-proofing, HDR, plugins/extensions.
- Crash/usage reporting to any server; telemetry.
- Camera live view, camera exposure/focus control, automatic film advance for camera capture, image registration between the three exposures.
- Scanlight firmware flashing, preset saving, or configuration beyond LED levels and shutter pulse (use its own web app for that).
- Mobile and Linux builds. Monetization or licensing.

## 2. Technical stack

| Layer | Choice | Notes |
|---|---|---|
| App shell | Tauri 2 | small installers, Rust backend |
| Core language | Rust (stable, pinned in `rust-toolchain.toml`) | Cargo workspace in `crates/` |
| UI | React 18 + TypeScript (strict) + Vite | `apps/desktop/src` |
| UI state | Zustand | no router: view state is `Library | Develop | Import` |
| UI primitives | Radix UI + plain CSS (variables, CSS modules) | accessible sliders, dialogs, menus |
| IPC types | `tauri-specta` generating `packages/ipc-types` | single source of truth is Rust |
| DB | SQLite via `rusqlite` (bundled, FTS5) + `rusqlite_migration` | one file per library |
| Images | `tiff` (16-bit TIFF), `rawler` (NEF input for triplets; confirm D600 support in M5), `lcms2` (ICC), `fast_image_resize`, `rayon` | pure f32 buffers, CPU only |
| Scanner | `nkscan` crate (CLI fallback) | [ADR 0003](../decisions/0003-ls-50-via-nkscan.md) |
| Scanlight control | `serialport` crate (USB CDC serial, binary protocol) | no driver needed on Windows 10+ or macOS; [ADR 0004](../decisions/0004-guided-rgb-capture.md) |
| Camera tether | libgphoto2 via a Rust binding behind the `tether` Cargo feature (macOS builds); `notify` crate for the watched folder | libgphoto2 is LGPL, linked dynamically |
| Logging | `tracing` + `tracing-subscriber` + `tracing-appender` | |
| i18n | `i18next` + `react-i18next`, `en.json` only | |
| Package managers | cargo, pnpm 9, Node 20 LTS | `.nvmrc` committed |
| CI/CD | GitHub Actions | Windows + macOS-arm64 matrix |

**Deliberately not used (KISS):** GPU compute/WebGL pipeline, Redux/React Query, a plugin system, a server, an ORM, a feature-flag SaaS. Revisit GPU only if the §11 budgets are missed on CPU.

## 3. Architecture

```
┌──────────────────────── Tauri app (single process pair) ─────────────────────────┐
│  WebView (React UI)                    Rust backend (apps/desktop/src-tauri)       │
│  ┌──────────────┐  invoke / events    ┌───────────────────────────────────────┐   │
│  │ Library view │ ◄─────────────────► │ commands/  (thin, validate, delegate) │   │
│  │ Develop view │  binary preview     │ jobs/      (scan, import, export)     │   │
│  │ Import view  │                     │ state/     (open Library, LRU cache)  │   │
│  └──────────────┘                     └──────┬──────────────────────────────┬─┘   │
│                                              │ calls                         │     │
│                    gtp-app (services, JobManager, pipeline composition)            │
│   gtp-ingest  gtp-capture  gtp-negative  gtp-edit  gtp-catalog        gtp-io        │
│          \         |            |           |        /                 /            │
│                                    gtp-core (types)                                 │
└────────────────────────────────────────────────────────────────────────────────────┘
   │ nkscan (USB)    │ USB serial   │ USB (PTP) or     │ SQLite file + files on disk
   Nikon LS-50       Scanlight V4   │ shutter pulse    │ (library folder)
                                    Nikon D600
```

**Rules** (also in [structure.md](structure.md) §4 and `rules/coding.md`)
- Feature crates depend on `gtp-core`/`gtp-io` only, never on each other. The application layer crate `gtp-app` (ADR 0005) composes them: use-case services, `JobManager` and the render pipeline live there. `src-tauri` is a thin adapter over `gtp-app`.
- The UI never touches pixels. It sends a `Recipe` and receives a rendered preview.
- Commands are thin: validate input, call a `gtp-app` service, map errors. No business logic in `src-tauri`.
- Long work (scan, import, export) runs as **jobs** on a worker thread/rayon pool, reports progress by events, and is cancellable.

**Preview transport:** `develop_render` returns raw RGBA8 bytes through `tauri::ipc::Response` (no JSON/base64). While a slider is dragged the UI asks for a 1600 px long-edge render; on release it requests 2048 px. Requests carry a sequence number, and stale results are dropped.

**Concurrency:** one write connection (`Mutex<Connection>`, WAL mode) plus one read connection. A single render at a time per open frame (latest request wins). Export uses a rayon pool sized to `cores - 1`.

## 4. Domain model

```
Library 1─* Roll 1─* Frame 1─* SourceFile
                      Frame 1─1 Recipe
                      Frame *─* Tag
```

| Entity | Fields (summary) |
|---|---|
| **Library** | `schema_version`, `created_at`, `name` |
| **Roll** | `id`, `name`, `film_stock?`, `format` (`35mm`), `shot_date?`, `notes`, `film_base_rgb?` (measured, linear), `created_at` |
| **Frame** | `id`, `roll_id`, `index_in_roll`, `kind` (`ScanRgbIr` \| `RgbTriplet`), `width`, `height`, `has_ir`, `note`, `created_at`, `edited_at` |
| **SourceFile** | `frame_id`, `role` (`rgb`, `ir`, `r`, `g`, `b`), `rel_path`, `sha256`, `bytes` |
| **Recipe** | `frame_id`, `schema_v`, `json` (see below), `updated_at` |
| **Tag** | `id`, `name` (unique, case-insensitive, normalized) |
| **Export (job log)** | `frame_id`, `format`, `path`, `created_at` |

IDs are ULIDs (sortable, collision-safe). Originals are **immutable**; every edit lives in the Recipe.

### 4.1 Recipe (versioned JSON, stored in SQLite)

```jsonc
{
  "v": 1,
  "geometry": { "crop": {"x":0,"y":0,"w":1,"h":1}, "angle_deg": 0, "rot90": 0, "flip_h": false, "flip_v": false },
  "base":     { "source": "roll" | "manual", "rgb": [r,g,b] },     // film base in linear scan values
  "invert":   { "black": [r,g,b], "white": [r,g,b] },              // per-channel density range; auto-filled
  "color":    { "red_cyan": 0, "green_magenta": 0, "blue_yellow": 0, "temperature": 0, "tint": 0, "saturation": 1.0, "vibrance": 0 },
  "mono":     { "enabled": false, "mix": [0.2627, 0.6780, 0.0593] },    // black and white; mix defaults to Rec.2020 luma
  "tone":     { "exposure_ev": 0, "contrast": 0, "highlights": 0, "shadows": 0, "whites": 0, "blacks": 0, "dehaze": 0, "curve": { "master": [[0,0],[1,1]], "r": [], "g": [], "b": [] } },
  "dust":     { "enabled": true, "threshold": 0.5, "grow_px": 2 },
  "output":   { "space": "srgb" }
}
```

Rules: unknown fields are ignored; a missing field means default; `v` bumps only with a migration function in `gtp-core`. "Auto" is just a function that fills the recipe (never a hidden state).

**Ranges.** `Recipe::clamp()` pulls every value into its range (non-finite values fall back to the default) and is idempotent; `migrate` clamps on load and rejects unknown versions with `UnsupportedFormat`.

| Field | Range (default) |
|---|---|
| `geometry.crop` x, y, w, h | fractions of the frame, 0 to 1; w and h at least 0.01; the box stays inside the frame (0, 0, 1, 1) |
| `geometry.angle_deg` | -45 to 45 (0) |
| `geometry.rot90` | quarter turns, wrapped to 0 to 3 (0) |
| `base.rgb` | 1e-6 to 65535, linear scan values (1, 1, 1) |
| `invert.black` / `white` | densities, 0 to 10; white at least 0.001 above black (0 / 1) |
| `color.red_cyan`, `green_magenta`, `blue_yellow`, `temperature`, `tint` | -100 to 100 (0). Positive `red_cyan` adds red, negative adds cyan; likewise green/magenta and blue/yellow, as in darkroom filtration |
| `color.saturation` | 0 to 2 (1) |
| `color.vibrance` | -100 to 100 (0); saturation that spares colours which are already vivid |
| `mono.mix` | each weight -2 to 2 (Rec.2020 luma 0.2627, 0.6780, 0.0593); `mono.enabled` false. Weights summing to 1 keep the brightness; a heavy red weight acts like a red filter |
| `tone.exposure_ev` | -5 to 5 (0) |
| `tone.contrast`, `highlights`, `shadows`, `whites`, `blacks`, `dehaze` | -100 to 100 (0). Highlights and shadows act on the bright and dark parts of the tonal range; whites and blacks move the white and black points; dehaze removes (positive) or adds (negative) a uniform veil |
| `tone.curve.*` | points in the unit square, sorted and unique by input, at most 16 per curve; empty means no change |
| `dust.threshold` | 0 to 1 (0.5) |
| `dust.grow_px` | 0 to 10 (2) |

### 4.2 Processing pipeline (order is fixed, see `rules/colour-pipeline.md`)

1. **Load** 16-bit source to linear f32. Triplet: decode each NEF to linear (black level subtracted, no white balance or tone curve), demosaic it, then take R from the red-lit exposure, G from the green, B from the blue. The D600's 6016 x 4016 sensor at about 1:1 on a 36 mm frame gives roughly 4,200 ppi, comparable to the LS-50. Capture order is R, G, B by default and can be reordered in the import dialog. Exposures may use different shutter speeds or ISO per colour (typical when balancing the LED channels); the per-channel film-base division in step 4 removes the gain difference, so the only requirement is that no exposure clips.
2. **IR dust removal** (scans with an IR plane): threshold the normalized IR plane to a mask, dilate by `grow_px`, fill masked pixels from surrounding unmasked pixels (iterative neighbour-weighted fill). Runs before inversion.
3. **Crop** to the frame area (auto-detected, user adjustable).
4. **Film-base normalize:** `T = I / base` per channel.
5. **Invert:** density `D = -log10(T)` (`T` floored at 1e-6); per-channel black/white densities map `D` to the positive value `P = max(0, (D - black) / (white - black))`, linear in density, with no upper clamp so highlights keep headroom for the tone stage (the output stage clips). The points are auto-filled from the 0.5th and 99.5th percentiles of `D` over the cropped frame; a value the user sets in the recipe is used as is.
6. **Colour:** the red/cyan, green/magenta and blue/yellow axes, temperature and tint as per-channel gains in stops (each control raises its channel(s) and lowers the others so the gains multiply to 1: an axis of +-100 is +-1 stop on its channel); then black-and-white (when enabled: `Y = mix . rgb` in all channels, and saturation and vibrance are skipped); then saturation around the Rec.2020 luma; then vibrance (the same, weighted by `1 - ` the pixel's own saturation).
7. **Tone:** on the Rec.2020 luma, with the pixel scaled by `Y'/Y` so hue is kept: exposure (`2^EV`), contrast (a power curve pivoting on mid-grey 0.18), highlights and shadows (up to 1 stop, weighted by distance from mid-grey; mid-grey is untouched and the mapping stays monotone), whites and blacks (move the white and black points). Then dehaze, global: positive removes a veil estimated as the 0.5th percentile of each pixel's darkest channel and lifts saturation slightly; negative adds a veil. Then curves (monotone cubic, master then R, G, B). Every constant is named in `gtp-edit` and calibrated against reference renders in Q-03.
8. **Geometry:** rotate/flip/straighten (resample once, at the end).
9. **Output transform** to the export space with ICC embedded.

**Colour management (MVP):** internal working space is linear Rec.2020-primaries f32. Scanner values are assumed linear in that space (no input profile); white balance compensates. Preview assumes an sRGB display. Export spaces: sRGB (default), Adobe RGB, ProPhoto. OS display profiles are post-MVP.

**Frame detection:** on the base-normalized scan the unexposed rebate is about 1.0 and picture pixels are below it, so a pixel counts as rebate when its luminance is within 0.06 of 1.0 (a brightness-step search fails on shadows and on pictures with hard steps). Per axis, the median luminance profile of the columns and of the rows is searched for a rebate band in the first and last quarter; the frame edge is the inner end of each band, and a side without a band keeps the full extent, so one to four rebate sides work. The result is a crop rectangle (the recipe's `geometry.crop`) with a confidence: the share of the outside area that is rebate times the share of the inside area that is not. Confidence below 0.5 selects the full frame and shows a hint.

**Film base:** auto-sample the brightest unexposed border region of the roll's first frame, or the user clicks a rebate pixel. The result is stored on the Roll and used by all frames; a frame can override it (`base.source = manual`).

## 5. Storage

### 5.1 Library folder (user chosen; created by the app)

```
<Library>/
├─ library.db            SQLite (WAL); the source of truth for metadata and recipes
├─ originals/<roll_id>/<frame_id>/
│    scan.tif  ir.tif            (scanner)   or   r.tif  g.tif  b.tif   (triplet)
├─ thumbs/<frame_id>.jpg         512 px, rebuildable cache
└─ exports/                      default export destination (changeable)
```

- Originals are written once, as **lossless-compressed 16-bit TIFF** (Deflate with predictor), then marked read-only. RAW triplet inputs are converted to single-channel 16-bit TIFFs on import so the library has one format.
- Thumbnails are the only persisted cache; the editing proxy (2048 px linear f32) is built on frame open and kept in an in-memory LRU of 4 frames.
- Backup = copy the library folder (documented in the README). No cloud features.
- Size planning: an LS-50 frame at 4000 dpi is about 5670 x 3780 px, roughly 128 MB of 16-bit RGB (+43 MB IR) before compression. Expect ~60-100 MB per frame on disk after compression, so ~2.5-4 GB per 40-frame roll. The first-run screen shows free space and the chosen location.

### 5.2 Schema (initial migration)

```sql
CREATE TABLE roll   (id TEXT PRIMARY KEY, name TEXT NOT NULL, film_stock TEXT, format TEXT NOT NULL DEFAULT '35mm',
                     shot_date TEXT, notes TEXT NOT NULL DEFAULT '', film_base_r REAL, film_base_g REAL, film_base_b REAL,
                     created_at TEXT NOT NULL);
CREATE TABLE frame  (id TEXT PRIMARY KEY, roll_id TEXT NOT NULL REFERENCES roll(id) ON DELETE CASCADE,
                     index_in_roll INTEGER NOT NULL, kind TEXT NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL,
                     has_ir INTEGER NOT NULL, note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, edited_at TEXT);
CREATE TABLE source_file (frame_id TEXT NOT NULL REFERENCES frame(id) ON DELETE CASCADE, role TEXT NOT NULL,
                     rel_path TEXT NOT NULL, sha256 TEXT NOT NULL, bytes INTEGER NOT NULL, PRIMARY KEY (frame_id, role));
CREATE TABLE recipe (frame_id TEXT PRIMARY KEY REFERENCES frame(id) ON DELETE CASCADE,
                     schema_v INTEGER NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE tag    (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
CREATE TABLE frame_tag (frame_id TEXT REFERENCES frame(id) ON DELETE CASCADE, tag_id TEXT REFERENCES tag(id) ON DELETE CASCADE,
                     PRIMARY KEY (frame_id, tag_id));
CREATE TABLE export_log (id INTEGER PRIMARY KEY, frame_id TEXT, format TEXT, path TEXT, created_at TEXT);
CREATE TABLE meta   (key TEXT PRIMARY KEY, value TEXT);          -- library name, first_export_at, counters
CREATE VIRTUAL TABLE search USING fts5(frame_id UNINDEXED, text);  -- roll name, stock, notes, frame note, tags
CREATE INDEX idx_frame_roll ON frame(roll_id, index_in_roll);
CREATE INDEX idx_frame_tag_tag ON frame_tag(tag_id);
```

`search.text` is maintained by the catalog crate in the same transaction as any change to a roll, frame note or tag.

### 5.3 Migrations and integrity

- Forward-only migrations in `gtp-catalog` (`rusqlite_migration`). Before migrating, copy `library.db` to `library.db.bak-v<N>`.
- `PRAGMA foreign_keys=ON; journal_mode=WAL; synchronous=NORMAL`. Run `PRAGMA integrity_check` on open when the previous session ended uncleanly.
- On import, every original is verified by `sha256` after the copy; a *Verify library* action in settings re-checks all files and lists missing/corrupt ones.
- A library from a newer app version opens read-only with a message.

### 5.4 App-level settings

`settings.json` in the OS app-config dir: last library path, window size, feature flags, theme. Per-library values live in the `meta` table.

## 6. Public interface (Tauri commands and events)

All commands return `Result<T, AppError>`. `AppError { code, message, details? }` with codes: `NotFound`, `InvalidInput`, `Io`, `Db`, `UnsupportedFormat`, `ScannerUnavailable`, `ScanFailed`, `Cancelled`, `LibraryTooNew`, `Internal`. The UI shows `message` and logs `code`.

| Area | Command | Purpose |
|---|---|---|
| Library | `library_create(path, name)`, `library_open(path)`, `library_info()`, `library_verify()` | lifecycle |
| Rolls | `roll_create(input)`, `roll_update(id, patch)`, `roll_list()`, `roll_delete(id)`, `roll_set_film_base(id, rgb)` | |
| Ingest | `scanner_list()`, `scan_start(params) -> JobId`, `import_files(paths, roll_id, kind) -> JobId`, `triplet_group(paths) -> [TripletCandidate]` | `kind`: `tiff_scan` \| `rgb_triplet` |
| Capture | `light_list()`, `light_connect(port?)`, `light_set(levels)`, `light_off()`, `camera_list()`, `capture_profile_get()`, `capture_profile_set(patch)`, `capture_test(channel) -> TestShot`, `capture_triplet(roll_id) -> JobId` | guided RGB capture (§6.1); `capture_triplet` captures one frame and returns the new `frame_id` in `job://done` |
| Jobs | `job_cancel(job_id)` | |
| Frames | `frame_list(roll_id?, page)`, `frame_get(id)`, `frame_delete(ids)`, `frame_set_note(id, text)` | |
| Develop | `develop_open(frame_id) -> DevelopInfo`, `develop_analyze(frame_id) -> Analysis`, `develop_render(frame_id, recipe, max_edge, seq) -> bytes`, `recipe_save(frame_id, recipe)`, `recipe_apply_to(frame_ids, source_frame_id, fields)` | `Analysis` = crop, film base, histogram, auto recipe |
| Tags | `tag_list()`, `tag_apply(frame_ids, add[], remove[])`, `tag_rename(id, name)`, `tag_delete(id)` | |
| Search | `search(query, page) -> FrameSummary[]` | `query`: `{text?, tags_all[], roll_id?, date_from?, date_to?, film_stock?}` |
| Export | `export_start(frame_ids, options) -> JobId` | `options`: format (`tiff16`, `jpeg`), space, quality, dest dir, naming template |
| System | `settings_get()`, `settings_set(patch)`, `diagnostics_export(dest)`, `log_client_event(level, msg)` | |

**Events** (backend to UI): `job://progress {id, stage, fraction}`, `job://done {id, result}`, `job://error {id, error}`, `library://changed {kind, ids}`, `capture://step {channel, state}` (`lit`, `fired`, `received`), `light://status {temp_c, vbus_mv}`.

**Scanner abstraction (`gtp-ingest`):**

```rust
pub trait ScannerSource {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>>;
    /// Scans `p.frames` in order (1..=40), calling `on_frame` as each completes so it is stored immediately.
    fn scan(&self, p: &ScanParams, on_frame: &mut dyn FnMut(u32, RawScan) -> Result<()>,
            progress: &dyn Fn(f32), cancel: &CancelToken) -> Result<()>;
}
// ScanParams: device_id, frames (first..=last), dpi (default 4000), samples (1/2/4/8/16), ir: bool, autofocus: bool
// Roll scans take 40+ minutes: every finished frame is committed to the library at once, so cancel or error
// keeps completed frames and the user can resume from the next frame number.
// RawScan: rgb16 (w*h*3), ir16 (w*h, optional), dpi, metadata
```

Implementations: `NkscanSource` (real), `FixtureSource` (replays TIFFs from `samples/`, used by tests and development without hardware).

### 6.1 Guided RGB capture (`gtp-capture`)

The Scanlight's serial interface (documented in its repository at `automation/bsl_control_interface.md`, firmware 1.2) is a binary protocol over USB CDC serial: packets are `0xFE`, header byte, data-length byte, data. The commands we use:

| Header | Command | Data | Use |
|---|---|---|---|
| 0 | `SET_COLOR` | R, G, B, W, IR (0-255) and a save flag | light one colour at a time; **always save flag 0** (saving writes non-volatile memory). IR is ignored on v4 |
| 2 | `GET_FW_VERSION` | none | handshake and device identification (the interface defines no VID/PID, so we probe serial ports); accept firmware IDs 1 and 2 (v4 rev A/B), warn on others |
| 3 | `SHUTTER_PULSE` | length in 10 ms units | fire the camera through the 3.5 mm output |
| 7 | `SET_FOCUS` | 0/1 | hold the focus signal; only on PCB 26a902b |

The device streams `LED_TEMP` (millidegrees C) and `VBUS` (mV) every 200 ms. Host commands are not acknowledged, and the firmware has no sequence command, so **the app does the sequencing** and treats the link as open-loop (verified by the camera actually delivering a file).

```rust
pub trait LightSource {                    // impl: ScanlightV4 (serial), FakeLight (tests)
    fn connect(&mut self, port: Option<&str>) -> Result<LightInfo>;   // probes with GET_FW_VERSION
    fn set_levels(&mut self, rgb: [u8; 3]) -> Result<()>;              // SET_COLOR, save=0
    fn shutter_pulse(&mut self, tens_of_ms: u8) -> Result<()>;
    fn status(&self) -> LightStatus;                                   // last LED_TEMP / VBUS
}
pub trait CameraSource {                   // impl: GphotoCamera (feature `tether`), WatchedFolder, FakeCamera
    fn arm(&mut self) -> Result<()>;                                   // ready to receive one exposure
    fn trigger(&mut self, light: &mut dyn LightSource) -> Result<()>;  // gphoto: PTP capture; folder: Scanlight pulse
    fn wait_for_image(&mut self, timeout: Duration) -> Result<CapturedFile>;  // the NEF bytes + EXIF
}
```

**Per-frame sequence** (`capture_triplet`, cancellable, one job per frame):
1. Check the light and camera are connected and the temperature is under the warn limit.
2. For each channel in order R, G, B: `set_levels` with only that channel at its calibrated level; wait `settle_ms` (default 200); `arm`; `trigger`; `wait_for_image` (timeout 15 s); emit `capture://step`.
3. `light_off` (all zeros, never saved).
4. Validate: three NEFs received; EXIF exposure time, ISO and aperture identical (warn if not); no exposure clipped (histogram check on the matching channel; warn if more than 0.1 percent of its photosites are saturated).
5. Store as an `RgbTriplet` frame (originals converted to 16-bit TIFFs per §5.1) and hash-verify. The UI then shows the merged frame and waits for the user to advance the film by hand and press Next (or Space).

**Capture profile** (stored in `settings.json`): `levels {r,g,b}` (LED balance so the three exposures are similar at the fixed exposure time), `settle_ms`, `pulse_10ms` (default 10, so 100 ms), `focus_signal` (bool), `camera_backend` (`gphoto` | `folder`), `watch_folder`, `temp_warn_c`, `temp_abort_c` (defaults 60 and 75, tunable; the Scanlight docs give no thresholds). A **test shot** command lights one channel, fires, and returns the histogram so levels can be balanced by hand.

**Safety rules:** the app never writes presets or trim (SET_COLOR save flag always 0, no SET_TRIM), turns the light off on cancel, error, panic and exit, and aborts a sequence if `LED_TEMP` exceeds `temp_abort_c`.

**Camera backends**
- `GphotoCamera` (macOS arm64 builds, Cargo feature `tether`): libgphoto2 over USB/PTP; the app sets the colour, triggers capture, and downloads the NEF directly, so no shutter cable is needed. Exposure settings stay under the camera's own manual controls. Known risk: macOS's own camera daemon can claim the USB device (to be handled in the spike). libgphoto2 and its camera/port driver libraries must ship inside the app bundle.
- `WatchedFolder` (Windows via digiCamControl; macOS fallback): the Scanlight `SHUTTER_PULSE` fires the D600 through the author's remote cable (the profile's `focus_signal` option uses `SET_FOCUS`, available on rev B); digiCamControl, connected to the camera over USB, saves each NEF to `watch_folder`; the app waits for new files (`notify`, file-size-stable check). digiCamControl must only download, not also trigger captures. Any other tool that writes NEFs to a folder works the same way.

## 7. UI

Three views, keyboard first:
- **Library:** roll sidebar, thumbnail grid (virtualized), search bar with tag chips, multi-select with batch tag/export.
- **Develop:** large canvas, filmstrip, right panel with Crop/Geometry, Base & Invert, Colour, Tone (curve), Dust. Before/after toggle. Histogram.
- **Import:** pick source (Scanner / Files), choose roll, enter tags, progress list.
- **Capture** (part of the Import view): connection status for the Scanlight and camera, LED level sliders per colour, test shot with histogram, a large "Next frame" button (Space), the merged result of the last frame, and a running frame counter for the roll. The light is switched off whenever the view closes.

Neutral dark grey surround for colour judgement; no coloured chrome around the image.

## 8. Testing strategy

| Level | Tooling | What |
|---|---|---|
| Unit (Rust) | `cargo test` | each pipeline stage on small synthetic buffers; recipe (de)serialization and migrations; catalog queries on in-memory SQLite |
| Golden images | custom ΔE2000 comparator in `tools/benchmark`, fixtures in `samples/public` | full pipeline output vs stored expected TIFF within tolerance; regenerated only by an explicit command + review |
| Property tests | `proptest` (few) | invert/normalize monotonicity, crop never out of bounds, triplet merge channel mapping |
| Ingest | `FixtureSource` | scan job progress, cancel, error paths without hardware |
| Capture | `FakeLight` + `FakeCamera`; byte-exact packet tests built from the Scanlight interface doc | sequence order R, G, B; light-off on cancel/error/timeout; temperature abort; mismatched EXIF warning; clipping warning; the real `ScanlightV4` encoder/decoder against recorded packets |
| UI components | Vitest + Testing Library + `vitest-axe` | panels, tag chips, search bar, a11y violations |
| End-to-end | `tauri-driver` + WebDriver (Windows CI) | create library, import fixture, convert, tag, search, export |
| macOS | build + launch smoke test in CI; manual checklist per release | WebKit has no WebDriver |
| Hardware | manual checklist in `docs/research/nkscan.md` | LS-50 roll scan (several frames, IR on) on Windows and macOS before each release that touches ingest || Benchmarks | `criterion` in crates; `tools/benchmark` quality report | enforce §11 budgets; report ΔE vs reference renders |

**CI gates (every PR):** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `pnpm lint`, `pnpm typecheck`, `pnpm test`, `cargo deny check`, ipc-types up to date (regenerate and diff).
**Sample data:** the project has few colour-negative samples (known risk). The author's reference pairs (raw scan or NEF triplet + Nikon Scan 4 / Trichroma Lab render) live in `samples/private/reference/` (or `samples/public/reference/` if shareable) with a manifest row each; `tools/benchmark` reads them. Every sample gets a `samples/public/MANIFEST.md` row; generate synthetic negatives (known density wedges) for unit tests so correctness does not depend on real film.
**Definition of done:** tests added, budgets still met, docs/ADR updated if behaviour or structure changed, no new clippy/lint warnings.

## 9. Security and privacy

- **Local only.** The app makes no network calls except the update check to GitHub Releases (user can disable). No telemetry, no accounts.
- **Tauri hardening:** strict CSP (`default-src 'self'`), no remote content, capabilities limited to what commands need; filesystem access only to the library folder and paths chosen through the OS dialog; no shell plugin (if the nkscan CLI fallback is ever used it is an allowlisted sidecar with fixed arguments).
- **Untrusted files:** images from other people are parsed with memory-safe Rust crates; enforce max dimensions (20,000 px per side) and max decoded bytes (2 GB) before allocating. Paths received from the UI are canonicalized and must resolve inside the library or a user-selected directory.
- **Updates:** Tauri updater verifies a minisign signature (public key embedded; private key held only as a GitHub Actions secret).
- **Supply chain:** lockfiles committed, `cargo-deny` + `cargo audit` + `pnpm audit` in CI, Dependabot weekly, pin `nkscan` exactly.
- **Data safety:** originals read-only and hash-verified; migrations back up first; destructive actions (delete frame/roll) require confirmation and delete files only after the DB transaction commits.
- **Unsigned builds:** documented install steps (Windows SmartScreen "More info, Run anyway"; macOS first launch via right-click Open). macOS builds use ad-hoc signing (`signingIdentity: "-"`), which Apple Silicon requires to run at all.
- **Serial and camera access:** the Scanlight is opened only through the Rust backend (no Tauri serial plugin, no WebSerial) on a port the user selected or the app probed; all bytes read from it are length-checked and unknown packets are dropped. libgphoto2 is bundled as dynamic libraries with their licence texts. Watched folders are user-chosen and files are validated like any import (§9 untrusted files).
- Scanner on Windows: the coolscan-mods firmware flasher needs VueScan's USB drivers, so the driver stack nkscan can use alongside them (VueScan's driver, or WinUSB via Zadig) must be confirmed in the M0 spike and documented in the user guide.

## 10. Feature flags

KISS: two mechanisms, no external service.
- **Compile-time Cargo features:** `tether` (libgphoto2; on for macOS builds, off for Windows, which uses the watched-folder backend).
- **Runtime flags** in `settings.json` (`flags: { name: bool }`), read through `settings_get`, with defaults in code:

| Flag | Default | Purpose |
|---|---|---|
| `dust_removal` | on | disable if IR output misbehaves on real scans |
| `triplet_import` | on | allow disabling until M5 verified |
| `guided_capture` | on | Scanlight control and camera capture; disable if hardware misbehaves |
| `dev_overlay` | off | per-stage timings and memory in the Develop view |
| `experimental_auto_levels` | off | alternative auto black/white-point algorithm |

Each flag has an owner issue and is deleted after one release in which it was fully on.

## 11. Performance budgets

Reference machine: 4-core/8 GB laptop on SSD (2020-class). Measured in CI benchmarks where possible and by hand otherwise; a miss blocks release.

| Metric | Budget |
|---|---|
| Cold start to interactive Library view | <= 2 s |
| Idle memory | <= 300 MB |
| Installer size | <= 40 MB |
| Open a frame in Develop (decode 21 MP TIFF, build proxy) | <= 1.5 s |
| Slider change to updated preview (1600 px during drag) | <= 100 ms |
| Auto analysis (frame detect + base + levels) on proxy | <= 500 ms |
| Full-res export of one 21 MP frame (incl. dust removal) | <= 10 s (4-core), <= 6 s (8-core) |
| Peak memory during export | <= 1.5 GB (single f32 working buffer, in-place stages) |
| Import (copy + compress + hash + thumbnail) per frame | <= 8 s, excluding scan time |
| Triplet merge per frame (3 NEF decodes + demosaic + store) | <= 10 s |
| Guided capture, Next click to stored triplet (excluding exposure time) | <= 15 s |
| Colour switch (command write to LED lit) | <= 50 ms, plus the configured `settle_ms` |
| Library grid scrolling, 5,000 frames | 60 fps, thumbnails lazy-loaded |
| Search over 50,000 frames | <= 100 ms |

The scan itself is hardware-bound and not budgeted, but the UI must stay responsive and cancellable throughout. Implementation hints: `rayon` over rows, in-place f32 buffers, `fast_image_resize` for proxies, reuse buffers between renders. If CPU cannot meet the preview budget, first reduce the preview size, then consider GPU, and only then add complexity.

## 12. Observability and logging

- **Logs:** `tracing` with a rolling JSON file in the OS log dir (daily, keep 7 days, cap 50 MB) and human-readable stderr in dev. Levels: `error` (user-visible failures), `warn`, `info` (job start/finish, library open, migrations), `debug` (off by default).
- **Spans** around each pipeline stage and job with durations. Fields include `frame_id`, `job_id`, `stage`, `ms`. Never log pixel data; avoid full file paths at `info`.
- **Frontend errors:** React error boundary and `window.onerror` call `log_client_event`; the user sees a recoverable error screen.
- **Panics:** a panic hook writes a crash note to the log dir and shows a "something went wrong, export diagnostics" dialog on next launch.
- **Diagnostics export:** `diagnostics_export` zips logs, app/OS/version info, DB schema version, flags, local counters (§14) and the last job errors. No images. Friends send the zip manually; nothing is uploaded automatically.
- **Dev overlay** (flag `dev_overlay`): live stage timings and memory for tuning against §11.

## 13. Internationalization and accessibility

**i18n:** all user-visible strings use `t('key')` from `packages/ui/src/i18n/en.json`; dates/numbers via `Intl`. Only English ships in the MVP, so adding a language later is a JSON file. No string concatenation for sentences; units are formatted, not hard-coded.

**Accessibility (target: WCAG 2.1 AA for application chrome):**
- Everything operable by keyboard (grid navigation with arrows, `[`/`]` previous/next frame, slider keys, curve points movable with arrow keys); visible focus rings; logical tab order.
- **Colour keys** in the Develop view: `Q` adds red, `A` cyan, `W` green, `S` magenta, `E` blue, `D` yellow. A press moves the matching slider by 10 steps, Shift+press by 1. Tone sliders have no keys yet. All shortcuts are customizable: the key map is a user setting (action name to key), these keys are only the defaults, and a key assigned to two actions is rejected with a message.
- Radix primitives for dialogs, menus and sliders (labelled, ARIA-correct); every control has an accessible name; status/progress uses `aria-live`.
- Contrast >= 4.5:1 for text; colour is never the only signal; respects `prefers-reduced-motion`; UI scales with the OS text-size setting.
- `eslint-plugin-jsx-a11y` in lint, `vitest-axe` on key components, manual screen-reader pass (NVDA, VoiceOver) before v0.1.0.
- The image canvas gets a text alternative (frame name, tags) and is not otherwise inspectable by screen reader.

## 14. Analytics

**No telemetry and no network analytics.** To measure the *activation* metric without sending data, the app keeps local counters in the `meta` table: `first_import_at`, `first_convert_at`, `first_export_at`, `frames_imported`, `frames_exported`, `scans_failed`. Users can view them in Settings and include them in the diagnostics zip when they choose to share. Activation = `first_export_at` within the first session. If remote analytics are ever wanted, that is a new ADR with explicit opt-in.

## 15. Deployment and releases

- **Targets:** Windows x64 NSIS installer (`.exe`) and macOS arm64 `.dmg` (ad-hoc signed). No native Windows ARM64 build until someone with an ARM device can test it; the x64 installer is the stopgap there.
- **Versioning:** SemVer, `CHANGELOG.md` kept from conventional commits. One channel (stable); prereleases use `-beta.N` tags and are not offered by the updater unless the user opts in.
- **Pipeline:** push tag `vX.Y.Z` triggers GitHub Actions: lint/test, then `tauri build` on `windows-latest` and `macos-15`, then a **draft** GitHub Release with installers and the signed `latest.json` for the updater. The author reviews and publishes.
- **Updates:** Tauri updater checks `latest.json` at launch (once per day) and prompts; never installs silently. To be verified in the M7 spike: updater behaviour on unsigned macOS builds (quarantine flag on the downloaded bundle).
- **Rollback:** previous releases stay on GitHub; the library is forward-compatible within a major version (§5.3), and migrations back up the DB so a downgrade is a restore.
- **Environments:** dev (`pnpm dev`) and release only. No staging.
- **User docs:** `docs/` contains install notes (SmartScreen/Gatekeeper), library backup, scanner setup (Windows driver), and known limitations.

## 16. Roadmap

| Milestone | Deliverable | Exit criteria |
|---|---|---|
| **M0 Foundations** | Tauri app skeleton in `apps/desktop`, CI (fmt, clippy, test, build on both OS), `tauri-specta` wiring, **nkscan hardware spike** ([research/nkscan.md](research/nkscan.md)), ADR 0002/0003 accepted | app launches on Windows x64 and macOS; spike answers: LS-50 detected and identified as an SA-30 roll adapter, 16-bit RGB + IR obtained, **and the modified roll transport advances across at least 3 frames**, on each OS. If advance fails, decide: contribute to nkscan, or rely on importing Nikon Scan 4 / VueScan TIFFs. **Capture spikes** ([research/scanlight-capture.md](research/scanlight-capture.md)): open the Scanlight over serial and light each colour; fire the D600 with `SHUTTER_PULSE` and choose the cable; capture and download a NEF with libgphoto2 on the Mac |
| **M1 Pipeline on files** | `gtp-io`, `gtp-negative`, `gtp-edit` basics: load TIFF, film base, invert, colour, tone; golden tests; `tools/benchmark` ΔE report | quality metric calibrated; budgets for open/render/export met on fixtures |
| **M2 Library** | `gtp-catalog`, library create/open, rolls, TIFF import with hashing + thumbnails, tags, FTS search, Library view | create library, import 36 frames, tag, search under budget |
| **M3 Develop UI** | Develop view with all recipe panels, crop with auto-detect, before/after, copy settings to roll, export (TIFF16/JPEG + ICC) | full file-based workflow usable end to end |
| **M4 LS-50 ingest** | `NkscanSource`, roll-scan job (1-40 frames, per-frame commit, cancel/resume), roll-level film base, IR dust removal, Import view | scan a full roll with IR on each OS; frames land in the library as they finish; dust visibly removed |
| **M5 RGB triplets (files)** | `triplet_group`, NEF/TIFF triplet merge, import UI | a D600 NEF triplet converts correctly; D600 files decode in `rawler` (else fall back to a LibRaw binding) |
| **M6 Guided RGB capture** | `gtp-capture`: Scanlight V4 serial driver (byte-exact protocol tests), `FakeLight`/`FakeCamera`, `WatchedFolder` backend, `GphotoCamera` backend (macOS), capture profile + test shot + Capture view, per-frame sequence with validation | on the author's rig: **macOS first**: a full roll captured frame by frame with libgphoto2 (no cable), then Windows with digiCamControl (shutter pulse + watched folder); light always ends off. Build order inside M6: Scanlight driver and fakes, `GphotoCamera` on the Mac, then `WatchedFolder`. **Cut line:** if libgphoto2 bundling or D600 support blocks, make `WatchedFolder` (with the remote cable) the capture path on macOS too and move `GphotoCamera` to post-MVP |
| **M7 MVP release v0.1.0** | packaging, updater, diagnostics export, a11y pass, user docs, benchmark vs Nikon Scan 4 / Trichroma references | all §11 budgets met; quality metric met; release checklist passed |
| **Post-MVP** | camera live view / focus assist, auto-balance of LED levels, motorized film advance, triplet registration, display ICC profile, per-frame undo history, batch sync | prioritized after friend feedback |

## 17. Risks

| Risk | Mitigation |
|---|---|
| nkscan LS-50 support is Linux-tested and partly inferred; its docs describe roll transport as inferred, and the SA-30 roll-adapter path enabled by the firmware mod is unproven there | M0 hardware spike including multi-frame advance; `ScannerSource` trait; fallback to importing TIFFs scanned in Nikon Scan 4 / VueScan |
| Friends on Windows ARM cannot be supported (no test device) | x64 installer under emulation, documented as unsupported; native ARM64 build once a tester appears |
| Few colour-negative samples | synthetic density-wedge fixtures; sample manifest; collect friends' scans (consented) into `samples/private` |
| CPU preview too slow | smaller drag preview, buffer reuse; GPU only if needed |
| RGB triplet misregistration (focus shift or camera movement) | document fixed-setup requirement; registration is a post-MVP candidate |
| Library size (GBs per roll) | lossless compression, free-space display, thumbnails only as cache |
| Unsigned installers frighten friends | install guide with screenshots; consider signing if the audience grows |
| libgphoto2: D600 support unverified; bundling its libraries in the Apple Silicon app; macOS may claim the camera over USB | M0 spike on the Mac; `tether` feature; `WatchedFolder` backend is the fallback and the Windows path |
| Scanlight protocol has no acknowledgements, no VID/PID, no sequence command, and may change with firmware | open-loop sequencing verified by received files; probe ports with `GET_FW_VERSION`; accept firmware IDs 1-2 only; pin the protocol doc revision in the research note |
| Shutter pulse length and focus signal for the D600 over the author's remote cable | tune `pulse_10ms` and `focus_signal` in the M0 spike; the gphoto backend avoids the cable on macOS |
| digiCamControl on Windows: third-party, D600 support and maintenance status unverified | confirm in the M0 spike; the watched folder accepts any NEF-saving tool |
| Light left on or hot after a crash | lights-off on every exit path; temperature warn/abort limits; physical power switch is the last resort |

## 18. Open questions

**Resolved (2026-10-29):** camera is a Nikon D600 with 60 mm f/2.8 AF-D, NEF; light is Jack's Scanlight V4; reference renders will be supplied; export defaults confirmed; the LS-50 is whole-roll modified via coolscan-mods firmware plus an SA-21 jumpered as SA-30 (validated in Nikon Scan 4, VueScan, Negpy); no Windows ARM device is available, so ARM is unsupported in the MVP.

Also resolved: the Scanlight fires in R, G, B order and all three exposure times are the same; guided capture (Scanlight control plus camera tether from the app) is in the MVP (§6.1, M6).

**Remaining (non-blocking):**
None for capture. Resolved: the Scanlight is rev B (focus signal available), the author has the D600 remote cable, tethered capture is developed on macOS first (libgphoto2), and Windows uses digiCamControl with a watched folder.

Everything else is a spike item: SA-30 recognition and frame advance in nkscan, the Windows driver stack, D600 support in `rawler` and libgphoto2 (see §16).
