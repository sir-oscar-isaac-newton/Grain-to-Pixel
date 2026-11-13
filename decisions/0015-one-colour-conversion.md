# 0015. One colour conversion for screen and file

- Status: Accepted (issue P-30)
- Date: 2026-11-12

## Context

Two code paths turn the linear Rec.2020 working image into sRGB.

- **Screen** (`to_rgba8_srgb`, `crates/gtp-app/src/pipeline/display.rs`): a fixed 3x3 matrix
  `REC2020_TO_SRGB`, then each result channel clipped to [0, 1] (not finite becomes 0), then the
  sRGB curve to 8 bits by a threshold lookup. It draws the Develop preview, the detail view and
  the histogram.
- **File** (`to_output`, `crates/gtp-io/src/icc.rs`): each Rec.2020 channel clipped to [0, 1]
  first (NaN becomes 0), then a Little CMS transform (float in, 16 bits out, relative
  colorimetric, no cache) from the working profile to Little CMS's sRGB profile. The JPEG writer
  takes 16 bits to 8 by `(v + 128) / 257`. Thumbnails already use this path.

U-29 promises that the detail view shows what an export would write, so the screen should agree
with the file. It does not always. The measurements below compare the two, find where the
difference comes from and cost the ways of closing it. They come from throwaway
measurement code that changed no behaviour (the screen against the export, the causes, the hue
table, Little CMS's matrix and its 8-bit output, the cost of building the transform and of its
variants). That code is not kept, apart from the timing of both paths in
`crates/gtp-app/src/pipeline/display/measure.rs` (ignored); the tables below stay as the record,
and the checks that remain are the tests named under Decision.

### How far the screen is from the export

Difference per colour channel in 8-bit levels, screen against export (JPEG path, `(v + 128) / 257`).
"> 1" is the share of channels differing by 2 or more levels.

| Test set | Colours | Mean | p99 | Max | > 1 |
|---|---|---|---|---|---|
| a. in-gamut grid (64^3, even in the sRGB encoding) | 262,144 | 0.0000 | 0 | 0 | 0 % |
| b. Rec.2020 colours outside sRGB (65^3 grid, outside part) | 137,770 | 0.0000 | 0 | 1 | 0 % |
| c1. in-gamut colours times 1.0 to 4.0 (13 steps) | 425,984 | 16.32 | 213 | 250 | 23.1 % |
| c2. any Rec.2020 colour times 1.0 to 4.0 (13 steps) | 467,181 | 8.54 | 168 | 255 | 21.9 % |
| d1. near black, 0 to 0.01 (65^3) | 274,625 | 0.0000 | 0 | 1 | 0 % |
| d2. near white, 0.9 to 1.0 (65^3) | 274,625 | 0.0000 | 0 | 1 | 0 % |
| d3. neutral ramp, 4097 steps | 4,097 | 0.0000 | 0 | 0 | 0 % |
| e. render, gradient, automatic recipe | 1,151,108 | 0.0008 | 0 | 10 | 0.004 % |
| e. render, gradient, strong saturation | 1,151,108 | 1.35 | 31 | 100 | 13.7 % |
| e. render, gradient, strong saturation, +1.5 EV | 1,151,108 | 7.63 | 152 | 255 | 14.5 % |
| e. render, wedge, automatic | 1,151,108 | 0.0000 | 0 | 1 | 0 % |
| e. render, wedge, strong saturation | 1,151,108 | 0.0047 | 0 | 1 | 0 % |
| e. render, wedge, strong saturation, +1.5 EV | 1,151,108 | 0.0000 | 0 | 1 | 0 % |
| e. render, blobs, automatic | 1,151,108 | 0.0003 | 0 | 1 | 0 % |
| e. render, blobs, strong saturation | 1,151,108 | 0.58 | 17 | 74 | 4.6 % |
| e. render, blobs, strong saturation, +1.5 EV | 1,151,108 | 4.86 | 112 | 239 | 9.7 % |

The renders are the synthetic negatives of `gtp_core::testing::synthetic` at 1600 x 1067 with 2 %
noise, through the pipeline with the automatic recipe (from `analyze`) and with saturation 2, vibrance
100 and every mixer saturation at +100 (strong), and that plus +1.5 EV. In the gradient with the
automatic recipe 44 % of the pixels leave sRGB after the matrix, but only 1.4 % have a channel
above 1, and that is where the 10 levels come from. With strong saturation 66 % have a channel above
1. Dark colours, near-white colours and greys agree to the level.

The grids avoid exact rounding ties on purpose. A grid with 65 cells per axis has a colour at an
encoded 0.1 (25.5 levels), and there the two paths flip in 0.7 to 4 % of the channels for no
reason that matters; with 64 cells there is no tie and the in-gamut difference is zero.

### Where the difference comes from

Each variant changes one thing in the screen's way to the export's; the numbers are the share of
channels that differ from the export at all, and the maximum, on the set where it matters most.

| Variant (against the export) | c1 differs / max | e, gradient, strong saturation differs / max | e, blobs, strong saturation, +1.5 EV differs / max |
|---|---|---|---|
| Screen now | 25.3 % / 250 | 16.9 % / 100 | 9.9 % / 239 |
| B1: clip Rec.2020 first, same matrix and lookup | 0.002 % / 1 | 0.001 % / 1 | 0.001 % / 1 |
| B2: B1 with the matrix from the chromaticities in f64 | 0.000 % / 1 | 0.000 % / 1 | 0.000 % / 1 |
| B3: B2 with the curve in f64, 16 bits, `(v + 128) / 257` | 0.000 % / 0 | 0.000 % / 1 | 0.000 % / 1 |

1. **The order of clip and matrix is the whole difference.** The export clips Rec.2020 channels at
   1 before the matrix. The screen applies the matrix to values above 1 and clips afterwards, so a
   value above 1 moves the other channels through the matrix's negative terms. Switching the order
   alone takes every set to a maximum of 1 level and at most 0.002 % of channels (the worst is the
   in-gamut colours times 1 to 4: 0.002 %). Where the difference is large (c, the strong renders)
   it is only because values above 1 are common; it is zero in the in-gamut and near-black sets,
   where nothing is clipped.
2. **The matrix is the same.** The matrix Little CMS applies (measured float to float, from the
   working profile to a linear profile with the sRGB primaries) is, to 7 decimals, the matrix from
   the published chromaticities, and `REC2020_TO_SRGB` differs from it by at most 5.2e-7 per
   element. Little CMS's D50 connection space and its Bradford adaptation cancel, because both
   profiles are adapted the same way. The constants' rounding is worth at most one level on a
   handful of channels (0.002 % in b and c1, 0.001 % in the renders).
3. **The curve and the rounding are the same to the level.** Little CMS's 16-bit output is within one
   16-bit unit of the exact value (in-gamut grid: 98.9 % exact, the rest off by one; neutral ramp:
   99.85 % exact), so after `(v + 128) / 257` it agrees with the screen's 8-bit threshold lookup to
   at most one level on 0.000 to 0.002 % of channels. The remaining one-level cases are channels
   within a few thousandths of a rounding boundary.
4. **A smaller difference in robustness.** The screen turns a pixel black as soon as one matrix
   output is not finite, so one NaN or infinite channel blacks out all three; the export clips per
   channel before the matrix, so only that channel is affected and infinity becomes 1. Clipping first
   removes this.

### What Little CMS costs, and its 8-bit output

Median of seven runs, release build, rayon pools of 6 and 12 threads on a 12-thread machine, the
image already in memory, output as RGBA8 (alpha 255).

| Picture | Screen now, 6 / 12 threads | Matrix with clip first (B), 6 / 12 | Export path (`to_output`, a new transform per call, then RGBA8), 6 / 12 | Cached transform, `RGB_8` output, 6 / 12 |
|---|---|---|---|---|
| 1600 x 1067 preview | 5.9 / 4.6 ms | 6.1 / 4.7 ms | 74.4 / 61.2 ms | 69.0 / 55.6 ms |
| 2048 x 1365 preview | 9.0 / 8.2 ms | 9.4 / 8.1 ms | 121.9 / 97.2 ms | 114.0 / 82.5 ms |
| 6000 x 4000 detail | 83.9 / 70.1 ms | 90.6 / 71.0 ms | 1005 / 717 ms | 930 / 675 ms |

- Building the two profiles and the transform takes 0.68 ms (median of nine), so caching it saves
  almost nothing; the conversion of the pixels is the cost. A cached transform with `RGB_16` output
  plus `(v + 128) / 257` takes the same time as `RGB_8` (within 2 %).
- `RGB_8` output from Little CMS equals `(RGB_16 + 128) / 257` exactly on all 2,738,019 channels of a
  97^3 grid of Rec.2020 colours in [0, 1]. So option A could ask Little CMS for 8 bits directly.
- Little CMS costs about 12 times the matrix path, at both thread counts.
- Context from the existing timing run (`time_a_1600_px_preview`, 2048 x 1365 proxy to a 1600 px
  preview): the render is 52.7 ms with 12 threads and 59.5 ms with 6, and the present conversion
  4.0 / 4.9 ms.

### The budgets that apply (docs/specification.md section 11)

- **While dragging:** "Slider change to updated preview (1600 px during drag)" is at most 100 ms.
  That is the render plus the conversion (the existing run: 56.7 ms together with 12 threads, 64.4 ms
  with 6, the conversion being 4 to 5 ms).
- **Detail render:** "Detail view at 1:1: first region after a change (decode, render every tool at
  full size, sharpen, convert; 24 MP scan, 6 cores)" is at most 3 s. The specification records 2.79 s
  for render, sharpen and conversion with 6 threads, without the decode.
- A region from the picture already rendered (at most 50 ms) is a cut of the cached RGBA8 and is not
  affected by either option.

| | Drag, 1600 px (100 ms) | Detail 24 MP, 6 cores (3 s) |
|---|---|---|
| Today | about 64 ms: fits | 2.79 s: fits, 0.21 s to spare |
| A: Little CMS on screen | about 59.5 + 69 = 129 ms with 6 threads (53 + 56 = 109 ms with 12): misses | 2.79 - 0.08 + 0.93 = about 3.64 s: misses |
| B: matrix with the clip first | about 65 ms: fits | about 2.80 s: fits |

The figures for A are the render time of the existing run plus the measured conversion of this
run; they were not measured together in one run.

### How the export's clip changes the hue of bright saturated colours

The export clips each Rec.2020 channel at 1 before the matrix, so a channel above 1 stops at 1 while the others keep their size, and
the ratios between the channels change. A hue that was set by the bright channel moves towards
the neighbouring hue and loses saturation. Examples, linear Rec.2020 to sRGB bytes, screen order
(clip after) against export order (clip first): orange (1.8, 0.9, 0.2) gives (255, 230, 89) against
(255, 243, 95), and at three times that, (255, 255, 108) against (255, 255, 128), a paler yellow;
a green (0.4, 1.8, 0.6) gives (0, 255, 185) against (51, 255, 198), and (0.6, 2.7, 0.9) gives
(0, 255, 221) against (158, 255, 243), a green that has become a pale cyan-white; a magenta
(1.8, 0.3, 1.5) gives (255, 90, 255) against (255, 126, 255). A colour that is inside Rec.2020's
cube is not changed, and blues, where the Rec.2020 values stay under 1 longer, move by one or two
levels. In a picture the effect needs values above 1, which the automatic recipe produces in 1 %
of the pixels and strong saturation or a brighter exposure in most of them; it is a property of the
export as it stands, and this record does not propose to change it. (Clipping each channel, not
the colour as a whole, is what makes the hue move; a compression of the whole pixel by its largest
channel would keep the hue. That is a separate question for the colour pipeline.)

## Options

### A. The screen uses the export's Little CMS transform

Build the working-to-sRGB transform once (or per call, which costs 0.68 ms), ask Little CMS for
`RGB_8` (equal to the export's 8 bits on the 2.7 million channels checked), clip the input exactly as
the export does, expand to RGBA.

- *Exactness:* identical to the export by construction, including any future change to the
  profiles; no test set to maintain.
- *Speed:* 12 times the matrix: 55 to 69 ms for a 1600 px preview, 0.68 to 0.93 s for 24 MP. Misses
  the drag budget and the detail budget on the reference six cores as measured above. The conversion
  would have to be made faster (a lookup table of the transform, for instance, which gives up the
  identity with the export and needs its own proof) before it could be adopted.
- *Maintenance:* the app needs a second route to Little CMS (a cached transform, its lifetime, the
  thread-safety of the shared transform, which `to_output` already relies on). `gtp-app` gets a
  conversion that is not plain Rust arithmetic; `to_rgba8_srgb` and its lookup would be removed or kept
  only for the histogram.
- *Histogram and clipping overlay:* judge exactly what the file will contain.
- *Thumbnails:* unchanged (already the export path).

### B. A corrected matrix path

Clip each Rec.2020 channel to [0, 1] (not finite becomes 0) before the matrix, exactly as the export
does, keeping the matrix, the clip after it (it only catches rounding at the edge) and the
threshold lookup. The measurement shows nothing else is needed: the matrix constants and the curve
are within one level of Little CMS on 0.002 % of channels or fewer. A new proof replaces "the
conversion by lookup equals the formula" with "the screen is within 1 level of `to_output` on every
set above", as ordinary tests (not ignored): a few hundred thousand colours, in debug-build time, and
the renders of two recipes.

- *Exactness:* within 1 level on every set measured (maximum 1 level; at most 0.002 % of channels
  differ at all, none by 2 or more). Not identical, because the constants are rounded to six digits
  and the curve is evaluated in a different place. A future change to the export's profile or
  intent (for example black point compensation, or another working space) would silently open a gap
  unless the proof tests run against `to_output` and fail on it.
- *Speed:* the same as today within noise (6.1 ms against 5.9 ms at 1600 px with 6 threads; 90.6
  against 83.9 ms at 24 MP). Fits both budgets.
- *Maintenance:* one extra `clamp` line in the existing function, the existing tests, and the new
  comparison tests; no new dependency in `gtp-app`.
- *Histogram and clipping overlay:* they judge the same 8-bit values as the file within a level, so a
  pixel that the overlay shows as clipped is one the file clips (value 0 or 255) to the level.
- *Thumbnails:* unchanged (already the export path).

## Recommendation

**B.** The whole disagreement is the order of the clip and the matrix; fixing that one line puts
the screen within one level of the file on every set measured, at no change in speed. A costs a
factor of twelve in the conversion, which on the measured machine takes the drag past 100 ms and the
detail past 3 s on six cores, to remove a difference of at most one level on 0.002 % of channels. If
exact identity is wanted later (for instance when output profiles other than sRGB are shown on
screen), A is still available and its 8-bit output is proven equal to the export's.

## Decision

**B**, chosen by the owner on 2026-11-12. `to_rgba8_srgb` clips each Rec.2020 channel to [0, 1]
first, as `to_output` does (NaN to 0; infinity ends at the clip, so +infinity is 1 and -infinity 0,
as in the export), then applies the same matrix `REC2020_TO_SRGB`, clips the result to [0, 1] and
uses the same 8-bit threshold lookup. Reason: it puts the screen within 1 level of the sRGB export
on every set measured, at unchanged speed, while A, which is exact, misses the drag budget (about
129 ms against 100 ms with 6 threads) and the detail budget (about 3.64 s against 3 s).

The proof is ordinary tests, not ignored, that compare `to_rgba8_srgb` with `gtp_io::to_output`
(sRGB, then `(v + 128) / 257`) in `crates/gtp-app/src/pipeline/display/against_export.rs`. Each
asserts at most 1 level per channel and that at most 0.5 % of channels differ at all:

- colours inside sRGB (24^3 in the sRGB encoding): maximum 0;
- Rec.2020 colours outside sRGB (24^3 grid, the outside part): 0;
- values above 1, up to 4 times (in-gamut colours at seven scales, and a 12^3 grid at 4 times): 1,
  on 2 of 41,472 channels;
- near black (0 to 0.01) and near white (0.9 to 1.0), 17^3 each: 0;
- a neutral ramp of 1025 steps: 0;
- every combination of NaN, both infinities, negative zero, zero, -3, 0.5, 1 and 7 in the three
  channels: at most 1;
- renders of the synthetic gradient and blobs with strong saturation, at 0 and +1.5 EV (320 x 213,
  more than a tenth of the pixels above 1): 0 and 1, on at most 3 of 139,932 channels;
- the histogram of a render is the histogram of the pixels of the new conversion, and each
  channel's cumulative count at a level lies between the file's counts at the level below and the
  level above (so the clipped-to-white count is the file's, to the level); the histogram of
  `analyze` equals the histogram of `to_rgba8_srgb` on the render of its recipe.

The existing tests of the lookup (`whole_images_convert_to_the_same_bytes_as_before...`) keep
their reference, now with the Rec.2020 clip first. The ignored timing test
`time_screen_and_export` stays for the speed record.

The export's hue shift of bright saturated colours, described above, is out of scope here and may
become its own issue. Thumbnails are already on the export path and do not change; they are not
regenerated.

## Consequences

- The screen's highlights change where a Rec.2020 channel is above 1 (the strong and
  bright renders above change by up to 255 levels, on 5 to 25 % of the channels of the stress sets),
  in the direction of what the export already writes; the automatic recipe changes 0.08 % of
  the channels of the gradient render (4 in 100,000 by 2 levels or more). The histogram and the clipping overlay (which read the same bytes)
  change with it. The specification (sections 4.2 and 7) says that the preview, the detail view and
  the histogram convert as the export does, within 1 level, and the proof tests of the conversion
  stay against `to_output`. The line in ADR 0014 that says the preview converts "with a matrix and
  a clip, a file with the colour profiles" is left as the record of that day.
- A, exact by construction, stays available if output profiles other than sRGB are ever shown on
  screen, with its 8-bit output proven equal to the export's; it needs a faster conversion first.
- The export's per-channel clip in Rec.2020 stays as it is, and so does its hue
  behaviour described above; thumbnails and exports do not change.
