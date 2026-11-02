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
| Images | `tiff` (16-bit TIFF), `rawler` (NEF input for triplets; confirm D600 support in M5), `kamadak-exif` (capture time of camera files, BSD-2-Clause), `jpeg-decoder` (the colour of a NEF's embedded preview, MIT OR Apache-2.0), `lcms2` (ICC), `fast_image_resize`, `rayon` | pure f32 buffers, CPU only |
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
              Roll *─* Tag   (a roll's tags count for every frame on it)
```

| Entity | Fields (summary) |
|---|---|
| **Library** | `schema_version`, `created_at`, `name` |
| **Roll** | `id`, `name`, `film_stock?`, `format` (`35mm`), `camera?`, `iso_box?` (rated), `iso_shot?` (exposed at; differs when pushed or pulled), `shot_from?` / `shot_to?` (partial ISO dates `2026`, `2026-07` or `2026-07-14`; the range may not end before it starts), `notes`, `film_base_rgb?` (measured, linear), `film_base_confidence?` (0 to 1 for an automatic measurement, none for a base set by hand; below 0.5 the UI asks for a manual pick), `created_at` |
| **Frame** | `id`, `roll_id`, `index_in_roll`, `kind` (`ScanRgbIr` \| `RgbTriplet`), `width`, `height`, `has_ir`, `note`, `created_at`, `edited_at` |
| **SourceFile** | `frame_id`, `role` (`rgb`, `ir`, `r`, `g`, `b`), `rel_path`, `sha256`, `bytes` |
| **Recipe** | `frame_id`, `schema_v`, `json` (see below), `updated_at` |
| **Tag** | `id`, `name` (trimmed, inner spaces collapsed, spelling kept); unique by its key, the name lower-cased across Unicode (`Été` = `ÉTÉ`, but `Ete` ≠ `Été`). On frames and on rolls; a roll's tags are not copied to its frames but count for them, also for frames imported later |
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
  "output":   { "space": "srgb", "sharpen": { "amount": 0, "radius_px": 1.0 } }
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
| `output.sharpen.amount` / `radius_px` | 0 to 150 percent (0, which changes nothing) / 0.3 to 3 pixels of the exported image (1.0) |

### 4.2 Processing pipeline (order is fixed, see `rules/colour-pipeline.md`)

1. **Load** 16-bit source to linear f32. Triplet: decode each NEF to linear (black level subtracted, no white balance or tone curve), demosaic it, then take R from the red-lit exposure, G from the green, B from the blue. The D600's 6016 x 4016 sensor at about 1:1 on a 36 mm frame gives roughly 4,200 ppi, comparable to the LS-50. Capture order is R, G, B by default and can be reordered in the import dialog. Exposures may use different shutter speeds or ISO per colour (typical when balancing the LED channels); the per-channel film-base division in step 4 removes the gain difference, so the only requirement is that no exposure clips.
2. **IR dust removal** (scans with an IR plane): threshold the normalized IR plane to a mask, dilate by `grow_px`, fill masked pixels from surrounding unmasked pixels (iterative neighbour-weighted fill). Runs before inversion. The plane is read relative to its own clear level (its 90th percentile): a pixel is dust when it reads below `clear * (1 - threshold)`, so a higher threshold masks fewer pixels and a larger `grow_px` more. The mask grows by a square of side `2 * grow_px + 1`, and the fill closes in from the edge of a spot to its centre. Spots (8-connected runs of masked pixels) larger than 0.05 % of the picture, such as the dark band the film holder makes at the edge of the plane, are not dust and are left alone; hairs and scratches are small enough to be removed. A plane that masks more than a quarter of the picture is not showing dust and is ignored. Previews shrink the plane with the scan; the export uses it at full size. A missing or unusable plane skips the stage.
3. **Crop** to the frame area (auto-detected, user adjustable).
4. **Film-base normalize:** `T = I / base` per channel.
5. **Invert:** density `D = -log10(T)` (`T` floored at 1e-6); per-channel black/white densities map `D` to the positive value `P = max(0, (D - black) / (white - black))`, linear in density, with no upper clamp so highlights keep headroom for the tone stage (the output stage clips). The points are auto-filled from the 0.5th and 99.5th percentiles of `D` over the cropped frame; a value the user sets in the recipe is used as is. The white and black pickers set one end from the density of the median of a clicked or dragged area (`levels_from_sample`); the end being set gives way so white stays at least 0.001 above black, and a black or non-finite area is rejected.
6. **Colour:** the red/cyan, green/magenta and blue/yellow axes, temperature and tint as per-channel gains in stops (each control raises its channel(s) and lowers the others so the gains multiply to 1: an axis of +-100 is +-1 stop on its channel); then black-and-white (when enabled: `Y = mix . rgb` in all channels, and saturation and vibrance are skipped); then saturation around the Rec.2020 luma; then vibrance (the same, weighted by `1 - ` the pixel's own saturation). The neutral picker (`neutral_balance`) takes the median of an area sampled before this stage and returns the three axes that equalize its channels at the same brightness (temperature and tint already set supply their share; the axes take `2/3` of the remaining stops); a sample that would need an axis beyond +-100 is rejected.
7. **Tone:** on the Rec.2020 luma, with the pixel scaled by `Y'/Y` so hue is kept: exposure (`2^EV`), contrast (a power curve pivoting on mid-grey 0.18), highlights and shadows (up to 1 stop, weighted by distance from mid-grey; mid-grey is untouched and the mapping stays monotone), whites and blacks (move the white and black points). Then dehaze, global: positive removes a veil estimated as the 0.5th percentile of each pixel's darkest channel and lifts saturation slightly; negative adds a veil. Then curves (monotone cubic, master then R, G, B). Every constant is named in `gtp-edit` and calibrated against reference renders in Q-03.
8. **Geometry:** crop (fractions of the source, rounded to whole pixels), then straighten by `angle_deg` (positive turns the picture clockwise) around the crop centre, then `rot90` quarter turns clockwise, then flips in the final orientation. The straightened output is the largest rectangle with the crop's aspect ratio that fits inside the rotated crop, so there are no empty corners (a few degrees cost a few percent of the frame; `output_size` reports it). Angle 0 copies pixels exactly; any other angle samples each output pixel once, with Catmull-Rom bicubic, at the position of the composed transform. Turns and flips never cause a second resample.

**Pipeline (`gtp-app`):** `render(input, recipe, target)` runs the stages in this fixed order on a clamped copy of the recipe: IR dust (hook; P-12), crop (`geometry.crop`, an exact pixel copy, so later stages and auto levels see only the frame), normalize, invert, colour, tone, geometry (straighten, turns, flips; one resample at most). The film base is the recipe's `base.rgb` when `base.source` is `manual`, otherwise the roll's measured base, falling back to `base.rgb`. `render` never computes automatic values; `analyze` does (on a 1600 px proxy: base unless the roll has one, frame detection on the normalized scan, auto levels on the cropped frame, a render for the histogram) and returns them as a recipe. `Preview { max_edge }` downscales first with an area filter (no overshoot at the rebate edge); the Develop view keeps a 2048 px proxy and previews at 1600 px. Results are linear Rec.2020; `to_rgba8_srgb` converts to sRGB primaries and the sRGB curve for display, and the histogram (256 bins per channel plus luma) is taken from those display values. Per-pixel stages run in parallel over rows (`rayon`) with results identical to a single thread.
9. **Output transform** to the export space with ICC embedded, preceded at export by **output sharpening** (`output.sharpen`): an unsharp mask on the luminance of the image at its final size. The Rec.2020 luma `Y` is encoded as `E = Y^(1/2.4)` (edges are what the eye sees in an encoded image; in linear light the halo would be strong on the bright side and weak on the dark one), blurred with a Gaussian of standard deviation `radius_px` (edges repeated, kernel reaching 3 standard deviations), and `E2 = max(0, E + amount/100 * (E - blur))`; each pixel is then scaled by `E2^2.4 / Y`, so hue and saturation are kept and no colour fringes appear. An amount of 0 changes nothing at all and a flat image stays flat. It runs once, in the export job, after the full-size render and only when the amount is above 0; `render` never sharpens, so previews and thumbnails are never sharpened. The radius is in pixels of the image it is given and is not scaled: a larger export wants a larger radius for the same look, and a future resize option must sharpen after it. Measured on 22 MP frames (release build, this machine): 0.12 to 0.15 s at amount 100, whatever the radius from 0.3 to 3, against an export of 4.2 to 4.4 s.

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
├─ thumbs/<frame_id>.jpg         512 px, rebuildable cache of the frame as its saved recipe renders it
└─ exports/                      default export destination (changeable)
```

- Originals are written once, as **lossless-compressed 16-bit TIFF** (Deflate with predictor), then marked read-only. RAW triplet inputs are converted to single-channel 16-bit TIFFs on import so the library has one format.
- Thumbnails are the only persisted cache; the editing proxy (2048 px linear f32) is built on frame open and kept in an in-memory LRU of 4 frames.
- Backup = copy the library folder (documented in the README). No cloud features.
- Size planning: an LS-50 frame at 4000 dpi is about 5670 x 3780 px, roughly 128 MB of 16-bit RGB (+43 MB IR) before compression. Expect ~60-100 MB per frame on disk after compression, so ~2.5-4 GB per 40-frame roll. The first-run screen shows free space and the chosen location.

### 5.2 Schema (initial migration)

```sql
CREATE TABLE roll   (id TEXT PRIMARY KEY, name TEXT NOT NULL, film_stock TEXT, format TEXT NOT NULL DEFAULT '35mm',
                     camera TEXT, iso_box INTEGER, iso_shot INTEGER, shot_from TEXT, shot_to TEXT,
                     notes TEXT NOT NULL DEFAULT '', film_base_r REAL, film_base_g REAL, film_base_b REAL,
                     created_at TEXT NOT NULL,
                     film_base_confidence REAL);  -- added by migration 0004 (L-10)
CREATE TABLE frame  (id TEXT PRIMARY KEY, roll_id TEXT NOT NULL REFERENCES roll(id) ON DELETE CASCADE,
                     index_in_roll INTEGER NOT NULL, kind TEXT NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL,
                     has_ir INTEGER NOT NULL, note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, edited_at TEXT);
CREATE TABLE source_file (frame_id TEXT NOT NULL REFERENCES frame(id) ON DELETE CASCADE, role TEXT NOT NULL,
                     rel_path TEXT NOT NULL, sha256 TEXT NOT NULL, bytes INTEGER NOT NULL, PRIMARY KEY (frame_id, role));
CREATE TABLE recipe (frame_id TEXT PRIMARY KEY REFERENCES frame(id) ON DELETE CASCADE,
                     schema_v INTEGER NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE tag    (id TEXT PRIMARY KEY, name TEXT NOT NULL, name_key TEXT NOT NULL UNIQUE);  -- key: Unicode lower case
CREATE TABLE frame_tag (frame_id TEXT NOT NULL REFERENCES frame(id) ON DELETE CASCADE, tag_id TEXT NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
                     PRIMARY KEY (frame_id, tag_id));
CREATE TABLE roll_tag  (roll_id TEXT NOT NULL REFERENCES roll(id) ON DELETE CASCADE, tag_id TEXT NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
                     PRIMARY KEY (roll_id, tag_id));
CREATE TABLE export_log (id INTEGER PRIMARY KEY, frame_id TEXT, format TEXT, path TEXT, created_at TEXT);
CREATE TABLE meta   (key TEXT PRIMARY KEY, value TEXT);          -- library name, first_export_at, counters
CREATE VIRTUAL TABLE search USING fts5(frame_id UNINDEXED, text, tokenize = 'unicode61 remove_diacritics 2');
                     -- roll name, stock, camera, ISOs, dates, notes; frame note; frame and roll tags
CREATE INDEX idx_frame_roll ON frame(roll_id, index_in_roll);
CREATE INDEX idx_frame_tag_tag ON frame_tag(tag_id);
CREATE INDEX idx_roll_tag_tag ON roll_tag(tag_id);
```

`search.text` is maintained by the catalog crate in the same transaction as any change to a roll, frame note or tag (frame or roll). Search ignores accents (`ete` finds `Été`); tag identity does not.

### 5.3 Migrations and integrity

- Forward-only migrations in `gtp-catalog` (`rusqlite_migration`). Before migrating, copy `library.db` to `library.db.bak-v<N>`.
- `PRAGMA foreign_keys=ON; journal_mode=WAL; synchronous=NORMAL`. Run `PRAGMA integrity_check` on open when the previous session ended uncleanly.
- On import, every original is verified by `sha256` after the copy; a *Verify library* action in settings (`library_verify() -> JobId`, a cancellable job) re-checks every original and lists the missing, changed (SHA-256), wrong-size and unreadable ones with their frame and reason. It only reports: it also counts frames without a thumbnail (a rebuildable cache) and lists non-empty folders under `originals/` that no frame owns, and never changes or deletes anything. It works on a read-only library; the time and problem count of the last run are kept in the library (`last_verify_at`, `last_verify_problems`).
- A library from a newer app version opens read-only with a message: `library_info` reports `read_only: true`, reads use a read-only connection, and every write returns `LibraryTooNew`.

### 5.4 App-level settings

`settings.json` in the OS app-config dir: last library path, window size, feature flags, theme, and the keyboard shortcuts the user changed (`keys`, action name to key: only the changed ones are stored, and an empty map is the defaults; at most 64, action names of letters, digits, `.`, `-` and `_`, keys of 1 to 32 printable characters, and no two actions on one key, compared without case). Per-library values live in the `meta` table.

## 6. Public interface (Tauri commands and events)

All commands return `Result<T, AppError>`. `AppError { code, message, details? }` with codes: `NotFound`, `InvalidInput`, `Io`, `Db`, `UnsupportedFormat`, `ScannerUnavailable`, `ScanFailed`, `Cancelled`, `LibraryTooNew`, `Internal`. The UI shows `message` and logs `code`.

| Area | Command | Purpose |
|---|---|---|
| Library | `library_create(path, name)`, `library_open(path)`, `library_info()`, `library_verify()`, `folder_check(path)` | lifecycle; `folder_check` describes a picked folder (exists, empty, holds a library, free space) for the first-run dialog without creating anything |
| Rolls | `roll_create(input)`, `roll_update(id, patch)`, `roll_list()`, `roll_delete(id)`, `roll_set_film_base(id, rgb)` | `roll_set_film_base` changes how every frame that follows the roll's base looks (a frame with a base picked by hand does not), so those frames' thumbnails are regenerated in the background, as after a saved recipe |
| Ingest | `scanner_list()`, `scan_start(params) -> JobId`, `import_files(paths, roll_id, kind) -> JobId`, `triplet_group(paths) -> [TripletCandidate]` | `kind`: `tiff_scan` \| `rgb_triplet`; `triplet_group` reads each camera file's EXIF capture time (`DateTimeOriginal` + `SubSecTimeOriginal`, from the first megabyte) and returns candidates in capture order, red first, then green, then blue. Shots more than 5 s apart start a new triplet (a D600 session with the Scanlight: 2 to 3 s inside a triplet, 8 s and more between). A run that is not a whole number of triplets is one candidate flagged `missing_shots` or `extra_shots`; files without a time form one candidate flagged `no_capture_time`. It also looks at the colour of each file: the dominant colour of the full-size JPEG preview embedded in the NEF (decoded at 1/8 size) says which light it was shot in. A file whose colour is clear (the brightest of R, G, B at least 3 times the next and at least 2 of 255; on the author's D600 files 13 times or more) is placed in its own channel whatever the capture order; unclear files (a dark frame, a bare light source, no preview) fill the free places in capture order; two files of one colour flag the candidate `channel_mismatch`. Each file in the result carries its `channel` (`red`, `green`, `blue` or none). The preview is the camera's own processing, so it tells the channels apart but says nothing about exposure. It changes nothing; reordering a candidate is `TripletCandidate::reordered` (the import UI uses it). The guided capture (§6.1) uses the same check on each shot to confirm the LED that was on |
| Capture | `light_list()`, `light_connect(port?)`, `light_set(levels)`, `light_off()`, `camera_list()`, `capture_profile_get()`, `capture_profile_set(patch)`, `capture_test(channel) -> TestShot`, `capture_triplet(roll_id) -> JobId` | guided RGB capture (§6.1); `capture_triplet` captures one frame and returns the new `frame_id` in `job://done` |
| Jobs | `job_cancel(job_id)` | |
| Frames | `frame_list(roll_id?, page)`, `frame_get(id)`, `frame_thumbnail(id) -> bytes`, `frame_delete(ids)`, `frame_set_note(id, text)` | `frame_thumbnail` returns the thumbnail JPEG as raw bytes through `tauri::ipc::Response` (no JSON); a missing file is rebuilt first from the frame's saved recipe (the auto recipe when it has none), one rebuild at a time, and `NotFound` remains when the frame or its original is gone (a read-only library is never written to, so nothing is rebuilt there); the UI makes an object URL and keeps recent ones in a small cache. It is not in the generated bindings (specta cannot describe raw bytes) and is called by name from `shared/ipc.ts` |
| Develop | `develop_open(frame_id) -> DevelopInfo`, `develop_analyze(frame_id, base?) -> DevelopAnalysis`, `develop_sample_base(frame_id, region) -> [f32; 3]`, `develop_render(frame_id, recipe, max_edge, seq) -> bytes`, `recipe_save(frame_id, recipe)`, `recipe_apply_to(frame_ids, source_frame_id, fields)` | `DevelopInfo` = original width and height, `has_ir`, the saved recipe (the default one when never edited) and the roll's film base. `DevelopAnalysis` = detected frame rectangle and confidence, film base and confidence, whether the base was given rather than measured (`base_given`), the histogram of the auto render, and the auto recipe. With `base` (three linear scan values above 0, such as one picked on the rebate) the analysis uses that base instead of the roll's or a measured one, so the automatic levels fit it. `develop_sample_base` is the per-channel median of an area of the whole scan, given as fractions of it (x, y, width, height, each 0 to 1) and read from the cached proxy; it is how a film base is picked on the rebate, and an area that is too dark to be film base, outside the picture, or without size is refused as `InvalidInput`. `develop_sample_levels(frame_id, recipe, region, point)` returns the recipe's levels with the white or black point set from an area (`levels_from_sample`): `region` is in fractions of the cropped frame as shown without turns, flips or straightening, the recipe's own base and crop are used (the Crop and Normalize stages run on the cached proxy), and a black, outside or empty area is refused as `InvalidInput`. `develop_sample_neutral(frame_id, recipe, region)` returns the recipe's colour block with the three colour axes set so that the area becomes grey (`neutral_balance`): the area is measured after the Invert stage and before the Color stage, with the recipe's base, crop and levels, on the same frame view as `develop_sample_levels`; an area that is black or too far from neutral for the axes (+-100) is refused as `InvalidInput`. `develop_open` decodes the original once into a 2048 px proxy (linear f32) kept in an in-memory LRU of 4 frames (cleared when another library opens); analysis and renders use it, loading it again when the frame was evicted. `develop_render` returns raw bytes through `tauri::ipc::Response`, called by name like `frame_thumbnail`: a 16-byte little-endian header (`seq` u64, `width` u32, `height` u32) then `width * height * 4` bytes of RGBA8 sRGB. `max_edge` must be 256 to 4096 (a preview is never larger than the proxy) and the recipe is clamped on the way in. Per frame only the newest `seq` counts: a render superseded before it starts, or while it runs, returns `Cancelled` (a running render finishes first, its result is dropped). `recipe_save` clamps the recipe into range, stores it, stamps the frame's `edited_at` and returns the recipe as stored; a recipe the pipeline could not render (a tone curve with a single point; no curve at all means "no change") is refused as `InvalidInput`. `recipe_apply_to(frame_ids, source_frame_id, fields)` copies the chosen groups of the source's recipe onto each frame, all or nothing in one transaction, keeping whatever else a target has; `fields` are `Geometry`, `Base` (the base source with its value), `Invert`, `Color` (colour balance and the black-and-white conversion), `Tone`, `Dust` and `Output`, and at least one frame and one field are required. A frame that was never edited counts as having the default recipe, as a source or as a target; the source itself, if listed, is skipped. Both send `library://changed` and queue the affected frames' thumbnails for regeneration: they are re-rendered in the background from the saved recipe (512 px, JPEG quality 85, sRGB, as at import), the command never waits, requests for the same frame within 500 ms become one render, a batch is worked through one frame at a time, and when it is done `library://changed` is sent again with `frame_ids` listing the frames whose thumbnails changed, so the grid reloads just those pictures. A thumbnail is rendered from the open frame's cached proxy, otherwise from the original decoded for the purpose without touching the cache; a failure keeps the old thumbnail and is logged |
| Tags | `tag_list()`, `roll_tags(roll_id)`, `frames_own_tags(frame_ids)`, `tag_apply(frame_ids, add[], remove[])`, `tag_apply_roll(roll_id, add[], remove[])`, `tag_rename(id, name)`, `tag_delete(id)` | `add` and `remove` are tag names; missing tags in `add` are created, unknown names in `remove` are skipped, all or nothing. A roll's tags count for all its frames. `roll_tags` lists a roll's tags; `frames_own_tags` lists the tags put on any of the given frames themselves (not those from their rolls), each once, in name order, for batch editing. Mutations send `library://changed` |
| Search | `search(query, page, page_size) -> FramePage` | `query`: `{text?, tags_all[] (tag ids), roll_id?, date_from?, date_to?, film_stock?, camera?, iso?}`, every part optional, all must match. `text`: every word, as a word start, accents ignored. `film_stock`/`camera`: equal ignoring case. `iso`: box or shot. Dates are partial; a roll matches when its shooting dates overlap the range, and a roll without dates never matches. Best text match first, otherwise newest roll first. `page_size` 1 to 500 |
| Export | `export_start(frame_ids, options) -> JobId` | `options`: format (`tiff16`, `jpeg`), space, `jpeg_quality` (1 to 100), `dest_dir` (created if missing, canonicalized), `template` (default `{roll}_{index:03}`; fields `{roll}`, `{index}`, `{index:0N}` with N 1 to 9, `{frame}`, `{ext}`; folders and unknown fields are refused, characters Windows and macOS refuse become `_`, the extension is added when missing). One frame at a time: decode the original, render at full size with the saved recipe (the automatic one for a frame never edited), convert to the space with its ICC profile, write through a temporary file and a rename, and insert an `export_log` row (`frame_id`, `format`, `path`, `created_at`; a library that cannot be written to skips the row). An existing file is never overwritten: the name gets ` (2)`, ` (3)`, and so on. A frame that cannot be exported (no scan file, deleted) is listed under `failed` in the `Exported` result and the others carry on; a cancel stops before the next frame or before a write and the finished files stay. Measured on 23.5 MP scans (release build, this machine): decode 0.13 to 0.15 s (1.6 s cold from disk), analysis on the proxy 0.17 to 0.20 s, full render 0.17 to 0.19 s, TIFF16 encode 3.7 to 3.9 s, JPEG encode 0.95 to 0.99 s; a whole export 4.2 to 4.4 s as TIFF16 and 1.4 to 1.5 s as JPEG (budget 10 s) |
| System | `settings_get()`, `settings_defaults()` (the settings before anyone changes them, for the default of each feature flag), `settings_set(patch)`, `diagnostics_export(dest)`, `counters_get()` (the activation counters of the open library, or none), `log_client_event(level, msg)` | |
| Updates | `update_check(manual)`, `update_install()`, `update_restart()` | `update_check` answers `disabled` (the flag is off: nothing was asked), `not_due` (the launch check runs at most once a day, `manual` ignores that), `up_to_date` or `available` with the version, the running version and the release notes; an unreachable server is an `Io` error. `update_install` installs the update the last check found, after checking its minisign signature (and that it was signed for the version the manifest announces); a package that does not verify is an `InvalidInput` error and is never installed. Windows: the installer takes over and closes the app; macOS: `update_restart` starts the new version. The updater plugin's own commands are not reachable from the page (no `updater:` permission). |

**Events** (backend to UI): `job://progress {id, stage, fraction}`, `job://done {id, result}`, `job://error {id, error}`, `library://changed {roll_id, frame_ids}` (`frame_ids` is empty unless thumbnails were regenerated), `capture://step {channel, state}` (`lit`, `fired`, `received`), `light://status {temp_c, vbus_mv}`.

**Scanner abstraction (`gtp-ingest`):**

```rust
pub trait ScannerSource {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>>;
    /// Scans `p.frames` in order (1..=40), calling `on_frame` as each completes so it is stored immediately.
    fn scan(&self, p: &ScanParams, on_frame: &mut dyn FnMut(u32, RawScan) -> Result<()>,
            progress: &dyn Fn(f32), cancel: &CancelToken) -> Result<()>;
}
// ScanParams: device_id, frames (first..=last, 1..=40), dpi (1000, 2000 or 4000; default 4000), samples (1/2/4/8/16), ir: bool, autofocus: bool
// Roll scans take 40+ minutes: every finished frame is committed to the library at once, so cancel or error
// keeps completed frames and the user can resume from the next frame number.
// RawScan: rgb16 (w*h*3), ir16 (w*h, optional), dpi, metadata
```

Implementations: `NkscanSource` (real), `FixtureSource` (replays TIFFs from `samples/`, used by tests and development without hardware). `ScanParams::validate` refuses (`InvalidInput`) an empty device id, frames outside 1 to 40 or last before first, a dpi other than 1000, 2000 or 4000, and sample counts other than 1, 2, 4, 8 and 16. `FixtureSource` plays the `n`-th TIFF of its folder (file-name order) as frame `n`, so a resumed scan gets the right file; a 4-channel RGBI file or a `<name>_IR.tif` beside the scan gives the infrared plane; asking for a frame the folder does not have fails with `ScanFailed` after the earlier frames were delivered; a cancel (`Cancelled`) is seen before each frame. `GTP_FAKE_HW=1` makes the app start with fixture devices (`gtp_app::Hardware`, logged at start): the fixture scanner replays the folder named by `GTP_FIXTURE_DIR`, by default `samples/private` of the repository the build was made in; without it a real scanner adapter is used, and one that does not exist yet is absent.

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
- **Develop:** large canvas, filmstrip, right panel with Crop/Geometry, Base & Invert, Colour, Tone (curve), Dust. Before/after toggle. Histogram. A frame is opened with a double-click, or Enter, on it in the Library; the filmstrip lists the frames of the Library's current view (the roll, or the search results) and `[` and `]` move through it. `\` toggles "before", which is the automatic conversion (the recipe `develop_analyze` suggests), against the recipe being edited; Escape returns to the Library. The canvas draws the `develop_render` bytes: a 1600 px preview while a control is dragged and a 2048 px one on release, each request numbered so that older pictures are dropped. The zoom runs from fit to 400% of the 2048 px proxy (past 100% pixels are enlarged, not smoothed, since there is no more detail than the proxy holds): the wheel zooms around the cursor, dragging pans, `0` fits, `1` shows 100%, `+` and `-` step. The histogram (red, green, blue and luma, with the share lost to pure black and white) is computed from the pixels of the picture on screen. **Geometry panel:** `Edit crop` switches the canvas to the whole scan in its scanned orientation (crop and geometry forced to identity for the picture only, as for the pickers) with the crop drawn over it and the rest dimmed. The crop is dragged to move it and by its sides and corners to resize it (pointer, pen or touch); the arrow keys move it when it has focus (0.5%, or 5% with Shift); four sliders (left, top, right, bottom, in percent) set the sides one by one, each stopping 1% from the opposite one (the recipe's minimum). Nothing is drawn while the crop is edited, since the picture on screen is the whole scan; the edit is drawn once when editing ends (Escape, or `Edit crop` again). `Use the detected frame` applies the rectangle `develop_analyze` found (`frame_rect`), and `Use the whole scan` the full frame; when `frame_confidence` is below 0.5 (the backend's limit, in which case `frame_rect` is already the full frame) a hint says the edges could not be found reliably and the crop should be adjusted by hand. `Rotate left` and `Rotate right` (quarter turns, kept in 0 to 3), the two flips (pressed buttons) and a `Straighten` slider (-45 to 45 degrees, steps of 0.1) change the recipe's geometry with a live preview and autosave; while the straighten slider has focus a thirds grid is drawn over the picture to judge a horizon. `Reset turns and straighten` keeps the crop. The crop is free-form until an **Aspect ratio** is chosen: Free, the scan's own ratio, 3:2, 4:3, 5:4, 1:1, 16:9 or a custom width and height (positive numbers, at most 20 to 1 either way). The ratio is that of the finished picture, after the quarter turns, so it is converted to the scan's own orientation and pixel shape (a ratio times scan height over scan width, as fractions); straightening keeps the crop's ratio, so it needs nothing more. Choosing a ratio fits the largest crop of it around the centre of the current crop. While it is locked the corner and side handles, the four side sliders and `Use the detected frame` / `Use the whole scan` all keep it (the last two take the largest crop of the ratio inside the detected frame or the scan), moving the crop changes nothing, `Swap landscape and portrait` turns the ratio the other way up, `Fit the largest crop` fits it again, and a quarter turn refits the crop so that the finished picture keeps its ratio. The lock is an editing aid of this frame: it is not stored and is free again when another frame opens. **Dust panel:** for frames that have an infrared plane only (it is not shown otherwise), after the Base and Invert panel. A `Remove dust` switch (`dust.enabled`); `Sensitivity`, the recipe's threshold turned around (100 % minus the threshold, so more is more dust found); and `Widen spots by`, `dust.grow_px` in whole pixels from 0 to 10; the sliders are off while the switch is. Dust removal changes the picture, so a change draws the preview again. The note says the preview uses a smaller copy of the plane, so very fine specks may only disappear in the exported file, and, when the `dust_removal` flag is off in Settings (read when the view opens), that the controls have no effect until it is switched on. `Reset dust removal` restores on, 50 % and 2 px.

**Sharpen panel:** the last section of the side column. Amount (0 to 150 percent, steps of 1) and Radius (0.3 to 3 pixels, steps of 0.1) set the recipe's `output.sharpen`, saved by the autosave like the other panels; `Reset sharpening` returns to amount 0 and a radius of 1. Sharpening is applied when the frame is exported, to the image at its final size, and never to the preview, so moving a slider asks for no new preview, and the panel says so and that the radius is in pixels of the exported image. **Base and Invert panel:** shows where the film base comes from (the roll's, or picked on this frame) and its values. "Pick film base" switches the canvas to the whole scan (crop and geometry forced to identity for the picture only, since the rebate lies outside the crop); a click then takes a small square around it (2% of the picture's width) and a dragged box takes the box, each sampled by `develop_sample_base`, and the result becomes this frame's base (`base.source = manual`); Escape cancels picking before it leaves the view. "Use the roll's base" goes back to the roll's, and "Use as the roll's default" sets the roll's base from the frame's picked one (the frames that follow the roll are re-rendered and their thumbnails regenerated). The black and white points of the inversion have a slider per channel, which overrides the automatic values, and "Re-run auto levels" finds them again for the base in use (the picked one, else the roll's). Every change goes through `recipe_save` once it has been left alone for 500 ms, and a change still waiting is saved when another frame is opened or the view closes; the panel says when it is saving, saved, or could not save. `Pick white point` and `Pick black point` switch the canvas to the frame without turns, flips or straightening (crop kept, for the picture only, so a click is a position in the cropped frame); a click or dragged box is sampled by `develop_sample_levels` and sets that end of the levels in all three channels; Escape cancels, and choosing the other picker switches without drawing the picture again. **Colour panel:** sliders for the three colour axes (cyan to red, magenta to green, yellow to blue), temperature, tint, saturation (0 to 2) and vibrance, all previewed live and autosaved like the levels; Page Up and Page Down move a slider 10 steps and the arrow keys one, which is what the shortcuts of U-13 will drive. `Pick neutral grey` shows the frame without turns, flips or straightening and takes a click or dragged box, sampled by `develop_sample_neutral`; it sets only the three axes and keeps temperature, tint and the rest. The Black and white switch turns on the channel mix (three weights from -2 to 2, starting at the Rec.2020 luma) and hides saturation and vibrance, which that mode skips; their values stay in the recipe for when it is switched off. **Tone panel:** exposure (-5 to 5 EV, steps of 0.01) and contrast, highlights, shadows, whites, blacks and dehaze (-100 to 100), previewed live and autosaved, with a Reset tone button. **Curve editor:** four curves (master, red, green, blue), each a smooth monotone cubic through 2 to 16 points; the master starts as the straight line and a colour curve with no points changes nothing, which is how it starts. Points move with the arrow keys (0.01, or 0.1 with Shift), the pointer or touch; Delete removes one (never below two); Add point inserts one on the curve in the widest gap, and so does a double click on the plot; Reset curve restores the straight line (master) or no points (colour). The line on screen is computed in TypeScript with the same Fritsch-Carlson cubic as the backend, and both are tested against `crates/gtp-edit/tests/fixtures/curve-cases.json`. A library from a newer version shows the panel but cannot be edited.
- **Settings:** a fourth view in the navigation bar. It shows the open library's name, folder and free space (asked for again when the view opens, since it changes), says when the library is read-only (made by a newer version), and says so when none is open. It shows when the library was last verified and with how many problems (or that it never was), and a `Verify library` section: a button that starts the verify job (off while it runs), its progress with Cancel, and then what it found: the files checked, each problem with its file and the reason in words (missing, changed, wrong size, could not be read), the thumbnails that are missing (made again when needed), the stray folders (never deleted), or that nothing was found; the text says that verifying only reports and works in a read-only library. The check lives in the job list of the library store, so leaving Settings and coming back shows the same check, and the Library view's job list shows it too. The five runtime feature flags of section 10 are listed as switches with what they do and their default; a flag is saved with `settings_set` the moment it is switched, goes back with the message if saving fails, and is marked when it is not at its default; `Reset flags to their defaults` restores those that differ. The flags are saved and listed, and each feature reads its flag as it arrives. There is no theme control (the app has one dark theme) and no separate update-check switch: the update check is the `update_check` flag in the list above. An `Updates` section above the flags has a `Check for updates now` button (off while the flag is off) that asks at once, ignoring the once-a-day limit, and says that the newest version is installed, offers a newer one, or says why the check failed.
- **Apply settings dialog:** `Apply settings…` in the Library toolbar (enabled with at least two frames selected, and in a library that can be written to) and in the Develop toolbar (the open frame is the source). It copies chosen groups of the source frame's recipe to other frames with `recipe_apply_to`: Colour (with black and white), Tone and curves, Crop and geometry, Film base, Levels, Dust removal and Output sharpening (the whole output block: the sharpening, and the colour space the recipe carries, which the export dialog overrides for each export), of which Colour and Tone start ticked because crop, base and levels usually differ from frame to frame. In the Library the source is chosen among the first 50 selected frames; the targets are the other selected frames or all other frames of the source's roll (read in pages of 500), and in Develop the Library's selection, less the open frame, or the rest of the roll. The button names what it will do (`Apply colour and tone to 11 frames`), a line says it replaces those settings on those frames and cannot be undone, and a summary follows; the thumbnails refresh through `library://changed`.
- **Export dialog:** reached with `Export…` from the Library toolbar (the selected frames; disabled with none) and from the Develop toolbar (the open frame). It asks for the file format (TIFF 16-bit or JPEG), the colour space (sRGB, Adobe RGB, ProPhoto RGB, each with a line on what it is for), the JPEG quality (a slider, JPEG only), the destination folder (OS dialog) and the file name template, with an example name for roll `Roll`, frame 1. Export stays disabled until there is a folder and a name without `/` or `\`, which show their message. The last options are remembered in the window's local storage (ignored when unusable or unavailable). Starting calls `export_start`; the dialog then shows the job's progress bar, Cancel, and at the end a summary (`N files written to <folder>`) with the frames that failed and why; `Export more` returns to the form. Closing the dialog does not stop the job: the Library's Jobs panel lists it, and reopening the dialog shows the running export. A cancelled job says that files already written stay (the job reports no count).
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
| End-to-end | WebDriver through msedgedriver on WebView2's debugging port (Windows CI, job `e2e-windows`; ADR 0006) | create library, import fixture, convert, tag, search, export |
| macOS | build + launch smoke test in CI; manual checklist per release | WebKit has no WebDriver |
| Hardware | manual checklist in `docs/research/nkscan.md` | LS-50 roll scan (several frames, IR on) on Windows and macOS before each release that touches ingest || Benchmarks | `criterion` in crates; `tools/benchmark` quality report | enforce §11 budgets; report ΔE vs reference renders |

**CI gates (every PR):** `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `pnpm lint`, `pnpm typecheck`, `pnpm test`, `cargo deny check`, ipc-types up to date (regenerate and diff).
**Sample data:** the project has few colour-negative samples (known risk). The author's reference pairs (raw scan or NEF triplet + Nikon Scan 4 / Trichroma Lab render) live in `samples/private/reference/` (or `samples/public/reference/` if shareable) with a manifest row each; `tools/benchmark` reads them. Every sample gets a `samples/public/MANIFEST.md` row; generate synthetic negatives (known density wedges) for unit tests so correctness does not depend on real film.
**Definition of done:** tests added, budgets still met, docs/ADR updated if behaviour or structure changed, no new clippy/lint warnings.

## 9. Security and privacy

- **Local only.** The app makes no network calls except the update check to GitHub Releases (user can disable). No telemetry, no accounts.
- **Tauri hardening:** strict CSP (`default-src 'self'`), no remote content, capabilities limited to what commands need; filesystem access only to the library folder and paths chosen through the OS dialog; no shell plugin (if the nkscan CLI fallback is ever used it is an allowlisted sidecar with fixed arguments).
- **Untrusted files:** images from other people are parsed with memory-safe Rust crates; enforce max dimensions (20,000 px per side) and max decoded bytes (2 GB) before allocating. Paths received from the UI are canonicalized and must resolve inside the library or a user-selected directory.
- **Updates:** Tauri updater verifies a minisign signature (public key embedded; private key held only as a GitHub Actions secret). `requireSignedVersion` is on, so a manifest cannot pair a new version number with the signed package of an older release (a forced downgrade); the update endpoint must be `https`. The text of the release notes is shown as plain text, never as markup. The `update_check` flag (section 10) switches the only network call off.
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
| `update_check` | on | ask GitHub Releases for a newer version at launch (at most once a day); off means no network request at all |

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
- **Diagnostics export:** `diagnostics_export` zips logs, app/OS/version info, DB schema version, flags, local counters (§14) and the last job errors. No images. Friends send the zip manually; nothing is uploaded automatically. The user picks the `.zip` path in the OS save dialog. Logs are cut to their last 2 MB each (16 MB in all), and the library folder and the home folder are replaced by `<library>` and `<home>` in everything copied. The job errors are the last 20 of this session, kept in memory (cancellations are not errors); `details` and messages are scrubbed the same way.
- **Dev overlay** (flag `dev_overlay`): live stage timings and memory for tuning against §11.

## 13. Internationalization and accessibility

**i18n:** all user-visible strings use `t('key')` from `packages/ui/src/i18n/en.json`; dates/numbers via `Intl`. Only English ships in the MVP, so adding a language later is a JSON file. No string concatenation for sentences; units are formatted, not hard-coded.

**Accessibility (target: WCAG 2.1 AA for application chrome):**
- Everything operable by keyboard (grid navigation with arrows, `[`/`]` previous/next frame, slider keys, curve points movable with arrow keys); visible focus rings; logical tab order.
- **Colour keys** in the Develop view: `Q` adds red, `A` cyan, `W` green, `S` magenta, `E` blue, `D` yellow. A press moves the matching slider by 10 steps, Shift+press by 1. Tone sliders have no keys yet. All shortcuts are customizable: the key map is a user setting (action name to key), these keys are only the defaults, and a key assigned to two actions is rejected with a message. The actions are the eight of the Develop view (before/after `\`, previous and next frame `[` and `]`, back to the Library `Escape`, fit `0`, actual size `1`, zoom `+` and `-`) and the six colour keys. A key is one printable character other than space (letters are compared without case, so Shift never changes which action a key means), or the default `Escape`; Settings has a Keyboard section where *Change* takes the next key pressed (Escape cancels, a combination with Ctrl, Alt or Command is refused) and *Reset keyboard to defaults* clears the changes. A change is saved at once and applies the next time the Develop view opens. Shortcuts are ignored while typing in a field and while a dialog is open (a held colour key draws rough previews and the sharp one when it is let go), and every action stays reachable without its key.
- Radix primitives for dialogs, menus and sliders (labelled, ARIA-correct); every control has an accessible name; status/progress uses `aria-live`.
- Contrast >= 4.5:1 for text; colour is never the only signal; respects `prefers-reduced-motion`; UI scales with the OS text-size setting.
- `eslint-plugin-jsx-a11y` in lint, `vitest-axe` on key components, manual screen-reader pass (NVDA, VoiceOver) before v0.1.0.
- The image canvas gets a text alternative (frame name, tags) and is not otherwise inspectable by screen reader.

## 14. Analytics

**No telemetry and no network analytics.** To measure the *activation* metric without sending data, the app keeps local counters in the `meta` table: `first_import_at`, `first_convert_at`, `first_export_at`, `frames_imported`, `frames_exported`, `scans_failed`. Users can view them in Settings and include them in the diagnostics zip when they choose to share. `first_import_at`, `first_export_at` and the frame counts are set per frame as the import and export jobs finish it (a cancelled job keeps what it finished); `first_convert_at` is the first saved recipe; `scans_failed` is counted by the scan jobs (H-03). A counter that cannot be written (a read-only library) never fails the work it counts. Activation = `first_export_at` within the first session. If remote analytics are ever wanted, that is a new ADR with explicit opt-in.

## 15. Deployment and releases

- **Targets:** Windows x64 NSIS installer (`.exe`) and macOS arm64 `.dmg` (ad-hoc signed). No native Windows ARM64 build until someone with an ARM device can test it; the x64 installer is the stopgap there.
- **Installers:** built by the `Bundle` workflow (`bundle.yml`) with `tauri build --bundles nsis` on Windows and `--bundles dmg --features tether` on macOS, and kept as artifacts. The NSIS installer installs per user (no administrator rights) and must be at most 40 MB (the workflow fails above that); the dmg size is reported only. `pnpm check:bundle` keeps the settings in `tauri.conf.json` from drifting. Install steps for the unsigned builds (SmartScreen, Gatekeeper) are in [install.md](install.md).
- **Versioning:** SemVer, `CHANGELOG.md` kept from conventional commits. One channel (stable); prereleases use `-beta.N` tags and are not offered by the updater unless the user opts in.
- **Pipeline:** push tag `vX.Y.Z` triggers GitHub Actions: lint/test, then `tauri build` on `windows-latest` and `macos-15`, then a **draft** GitHub Release with installers and the signed `latest.json` for the updater. The author reviews and publishes.
- **Updates:** Tauri updater checks `latest.json` (`releases/latest/download/latest.json`, which skips drafts and prereleases) at launch, at most once a day (the time of the last answer is kept in `settings.json`), and shows a notice with the version and the release notes; Install and Later are the user's choice, nothing is installed silently, and a failed launch check says nothing. A tampered package or manifest does not verify and is not installed (tests with a throwaway key pair in `apps/desktop/src-tauri/tests/fixtures/updater`). To be verified with two real releases (human): updater behaviour on unsigned macOS builds (quarantine flag on the downloaded bundle); the answer goes into `docs/install.md`.
- **Releases:** the steps (version, tag, draft, publish) and the dry run on a prerelease tag are in [release.md](release.md). `release.yml` builds with `createUpdaterArtifacts` (`tauri.release.conf.json`), so pull request builds need no signing key.
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
