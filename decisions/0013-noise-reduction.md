# 0013. Noise reduction is a guided filter in Oklab, between the inversion and the colour stage

- Status: Accepted (the owner approved the design, issue P-27 / GitHub #304)
- Date: 2026-11-11

## Context

Issue P-27 asks for noise reduction on the positive: a luminance amount and a colour amount, with
no learned or AI model. Film scans carry grain in the lightness and colour speckle in the chroma;
the first should be smoothed gently, the second strongly, and neither may blur an edge. The open
questions are which filter keeps edges at a cost that suits a 24 megapixel export, in which space
the planes are filtered, where the stage sits in the pipeline, and how a preview, which is made
from a smaller proxy, can show something close to the full-size result.

## Decision

1. **Method: a guided filter** (He, Sun, Tang). For a plane `p`, a guide `I`, a radius `r` and a
   regulariser `eps`, with `mean` a box average over the `(2r+1)^2` window clipped at the border
   (divided by the real number of pixels in the window): `a_k = (mean(I p) - mean(I) mean(p)) /
   (mean(I I) - mean(I)^2 + eps)`, `b_k = mean(p) - a_k mean(I)`, `q = mean(a_k) I + mean(b_k)`.
2. **Space: Oklab** (the planes `L`, `a`, `b` of the colour mixer's conversions, ADR 0012), so the
   lightness and the colour are separate planes and the filter strength means the same thing in
   dark and bright areas.
3. **Luminance**: `L' = guided(p = L, I = L, r_l, e_l)`, with `r_l = NR_LUMA_RADIUS_PX * s` and
   `e_l = (luminance / 100 * NR_LUMA_MAX_SIGMA * s)^2`. **Colour**: `a' = guided(p = a, I = L',
   r_c, e_c)` and likewise `b'` (the guide is the unfiltered `L` when the luminance is 0), with
   `r_c = NR_CHROMA_RADIUS_PX * s` and `e_c = (color / 100 * NR_CHROMA_MAX_SIGMA * s)^2`. Radii are
   rounded to whole pixels, at least 1. The constants are 3 px, 8 px, 0.04 and 0.06 (Oklab units);
   they are named `pub const`s in `gtp-edit`, to be tuned on real scans like the other edit
   constants.
4. **Scale `s`**: the size of the image being processed relative to the full scan, 1 at export and
   below 1 for a preview: the long edge of the uncropped image the stages start with over the long
   edge of the full scan. `PipelineInput` (an internal type, not an IPC type) gains
   `full_long_edge: Option<u32>`: the Develop service sets it from the scan when it builds the
   proxy (and so previews and thumbnails get it); export and every input that is the scan itself
   leave it `None`.
5. **Box averages in constant time per pixel**: a sliding window sum, in `f64`, along each row
   (rows in parallel), then along the columns in fixed bands of rows (bands in parallel). Every
   output sample depends only on its own row or column band, summed in a fixed order, so the
   result is identical on any number of threads, as `rules/colour-pipeline.md` requires.
6. **Recipe**: a top-level block `noise: { luminance, color }`, both 0 to 100, default 0, clamped
   (non-finite values become 0). The version stays 1; a recipe saved before it loads with zeros.
7. **Stage**: a stage of its own after the inversion and before the colour stage. Both amounts at 0
   return the image untouched, bit for bit. A pixel that is not finite is left as it is and enters
   the averages as 0. The stage is not part of the normalized or the inverted frame, so the pickers
   and the automatic balance do not see it; `analyze` leaves both amounts at 0.
8. **Apply settings and presets**: the noise block is a part of its own (`Noise`, listed after Dust
   removal, before Output). It is unticked by default in Apply settings and in a Develop preset.
9. **Detail panel**: the Sharpening panel becomes the Detail panel, with a Noise reduction section
   (Luminance, Colour) above the Output sharpening section.

## Rejected

- **Bilateral filter.** Its cost grows with the square of the radius (every pixel weighs every
  neighbour in the window), which is too slow for the chroma radius of 8 pixels on a 24 megapixel
  export unless it is approximated, and the approximations add code.
- **Non-local means.** It finds similar patches over a large search area; the cost is far too high
  for a 24 megapixel export.
- **Wavelet shrinkage.** It keeps the texture of grain better, but it needs much more code and
  tuning (a transform, thresholds per band and level). Kept for a later issue (P-29).
- **A plain blur of the colour.** It is cheap, but it bleeds colour across edges: a red next to a
  blue turns purple along the border.

## Consequences

- **Cost.** About 1.1 s for a 6000 x 4000 image with both amounts at 50, in a release build on this
  development machine (12 threads, 6 cores), single pass at scale 1. The cost does not depend on
  the radius. The working planes (about a dozen of `f32`, 96 MB each at this size) come on top of
  the image.
- **A preview can only approximate the full-size result.** The proxy is made with an area filter,
  which already averages away the finest grain, and the radii become whole pixels at the preview's
  scale. Moving a slider shows the trend; the grain itself shows in the exported file, and the
  panel says so.
- **Not clamped.** The result is converted back to linear Rec.2020 and can leave the gamut by a
  rounding; the tone stage and the output transform deal with that as they do for other stages.
- **It smooths grain, it does not keep its texture.** At high amounts the flat areas look clean
  and a little plastic; that is what the guided filter does, and the wavelet approach is the answer
  if that proves a problem.
- **The constants are a first guess**, to be tuned on real scans.
