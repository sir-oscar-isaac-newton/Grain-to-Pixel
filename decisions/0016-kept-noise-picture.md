# 0016. The detail view keeps the picture after noise reduction, so an edit to a later stage skips the decode and the first five stages

- Status: Accepted (the owner approved the plan and the memory on 2026-11-14, issue P-33 / GitHub #355)
- Date: 2026-11-14

## Context

The detail view ([ADR 0014](0014-detail-view.md)) renders the whole frame at full size through the
export's code path and keeps the result as RGBA8. Any edit to the recipe makes that picture stale, so
the next region starts a full render again: decode of the scan and its infrared plane, then every
stage. Measured for P-31 (24 megapixels, every tool on, 6 threads), the stages up to and including
noise reduction (Dust, Crop, Normalize, Invert, Noise) and the decode take about 1.7 s of the 2.98 s,
and most edits made at 1:1 (colour, tone, curves, vignette, split toning, straighten, sharpening)
touch only later stages. Those first stages depend on the scan, the roll and five parts of the
recipe, and on nothing else (`pipeline::stages::run_stage`; only the Noise stage uses the render's
scale, which is 1 at full size).

[Convention 11](../docs/convention.md) says: GPU, caching layers and extra threads need a measured
budget miss and an ADR. This is a caching layer.

## Decision

1. **Split the render in two, exactly.** `render_traced` is now `render_up_to_noise` (Dust, Crop,
   Normalize, Invert, Noise: the first five stages of `STAGE_ORDER`) followed by `render_from_color`
   (Color, Mixer, Tone, SplitTone, Geometry, Vignette), with the same clamped recipe, the same input
   and the scale measured once at the start. The later stages take an image and the recipe and
   nothing else (`run_late_stage` has no input and no scale to receive), so a render from a copy of
   the picture after noise reduction runs the same code on the same numbers: the pixels are the same
   bit for bit. `render_full_from_noise` (export module) is `render_full`'s tail on such a picture:
   the stages from Color on, then the optional resize and the output sharpening, and `render_full`
   itself is unchanged for the export.
2. **Keep one picture after noise reduction**, linear f32, 3 channels: 12 bytes a pixel, 288 MB for
   24 megapixels (6000 x 4000, measured as 288 MB), beside the 96 MB RGBA8 detail picture (4 bytes a
   pixel). It is made on the first full-size render of a frame, by a copy of the picture just before
   the stages from Color on run on the original; it is the copy that is kept. An edit whose key
   (below) is the kept key starts from a copy of the kept picture and runs from Color on, then the
   sharpening and the conversion as before; any other request reads the scan, makes the picture
   afresh and replaces the old one, which is dropped before the new one is made, so two are never
   held.
3. **The key** holds exactly what the first five stages read, by value:
   - the frame id, the frame's `has_ir`, and the catalog's record of each original (`SourceFile`:
     role, path, SHA-256, size): which scan and which infrared plane are decoded;
   - the length and the modification time of each original file as the file system reports them at
     the request. Originals are immutable by design, but a file replaced in place under the same
     record would decode to another picture; it changes at least one of the two. An unreadable file
     compares as nothing and so differs from a readable one. A change nobody could see (a copy
     that keeps content and size but gets a new time) only costs a render, never a wrong picture. No
     file is hashed: the catalog's SHA-256 is of the import, not a check of the file as it is now, and
     reading 100 MB of scan to compare it would cost what the keep saves;
   - the roll's flat field, the value itself (`Option<FlatField>`: a small grid of light values and
     the names of its files, compared, not a version), which is what `read_scan` divides a camera
     triplet by, read in the same catalog read as the sources and used for the decode, so key and
     decode cannot disagree;
   - the roll's film base passed in (Normalize with `base.source` = roll) and whether dust removal
     is enabled (the feature flag: without it the infrared plane is not decoded);
   - from the clamped recipe: `dust` (Dust), `geometry.crop` (Crop), `base` (Normalize: its source
     and value), `invert` (Invert) and `noise` (Noise).

   Not in it: `geometry` other than `crop` (straighten, turns and flips come after noise
   reduction), `color` (including `split_tone` and `mixer`), `mono`, `tone` (curves and vignette
   included) and `output`. A crop edit therefore renders afresh, because Crop precedes Noise (the
   noise reduction sees the cropped frame, and the crop is an exact copy), while straightening,
   quarter turns and flips do not, since the Geometry stage is after Noise. Nothing in the key is
   large data: the flat field is a few hundred values and the paths are short.
4. **Release by the rules of the detail picture, no others.** After `DETAIL_IDLE_SECS` = 300 s
   without a request (a request that cut a region from the detail picture counts), on
   `release_detail` (the Develop view is left) and on `clear` (another library), and when a request
   for another frame arrives, even if that request then fails. A render that began before a release
   discards the kept picture when it ends (the release generation is checked after the render, and
   the same check that stops the detail picture from being stored empties the kept one). The picture
   and the kept picture are locked in that order everywhere, so a release never waits on a render.
5. **If the copy cannot be allocated, nothing is kept.** The copy is made with
   `Vec::try_reserve_exact`; when it fails the render goes on from the picture it has, exactly as it
   did before this ADR, and nothing is kept. A later edit then reads the file again. When the copy a
   later edit would start from fails, the kept picture is dropped and that edit reads the file.
6. **The export does not use it.** `export_one` still calls `read_full` and `render_full` and has no
   access to the detail view's state; `read_full_with` (the same function with the flat field given)
   is what the detail view calls so its key and its decode use one value.

## Memory

12 bytes a pixel is 288 MB for 24 megapixels, held in addition to the 96 MB detail picture while the
detail view is in use: 384 MB, against 96 MB before. The peak during a render is not higher than
before: the scan is dropped once the first five stages are done (it used to be kept to the end of the
render), so the kept picture takes its place beside the working image. The memory budget row
"Idle memory <= 300 MB" is for the app with nothing open at 1:1; the pictures are released after five
minutes idle and when Develop is left.

## Rejected

- **Keep the decoded scan** (and its infrared plane) instead: for similar memory (288 MB for the RGB
  scan, 96 MB more for the plane) it would save only the decode and the copy the render starts with,
  about 115 ms after P-31, and every edit would still run the noise reduction, which is most of the
  1.7 s.
- **Keep a picture later in the pipeline** (after Color, say): an edit to the colour, the most
  common one, would miss it. After noise reduction is the last point every later edit passes.
- **Hash the files** for the key: see above.
- **Resize or compress the kept picture**: it would change the pixels, which must stay what they are.

## Consequences

- Measured in release mode on the 12-core desktop with a 6000 x 4000 scan, every tool on, a deflate
  scan and infrared plane, median of 5 runs (`time_a_detail_render_from_the_kept_picture`), `render_detail`
  with a small region: first render of the frame 3.12 s on 6 threads and 2.44 s on 12; a render after a
  colour edit 1.82 s and 1.25 s, after a tone edit (with the vignette) 1.80 s and 1.24 s. The same
  edits with nothing kept read the file again and take 3.05 s and 2.41 s. The keep costs the first
  render about 55 ms (the copy: 3.07 s and 2.38 s without it).
- That is the measured budget miss the convention asks for: the first-region budget of 3 s for the
  detail view is met by 20 ms in P-31's run (2.98 s) and missed in this one (3.07 s on 6 threads
  without the keep), P-31's aim of 2 s was not reached, and every edit costs the whole of it.
  With the kept picture an edit to a later stage costs 1.8 s on 6 threads, under both. The first render
  after a change to the crop, the base, the invert, the noise, the dust settings, the film base, the
  flat field or another frame is as slow as before (and 55 ms slower).
- 288 MB more is held while the detail view is in use (see Memory).
- Every key part is covered by a test that changes it on its own and expects a read of the scan.
