# 0009. The Frontier-style look, fitted to Trichroma Lab

- Status: Accepted (the author approved the design on 2026-11-02, issue P-20 / GitHub #187)
- Date: 2026-11-02

## Context

Q-02 measured the automatic conversion of the author's 15 camera triplets against Trichroma Lab's
renders: a median ΔE2000 of 17.2 against a target of 6, mostly in tone. Trichroma Lab imitates the
Fuji Frontier SP-3000 minilab, a contrasty lab-print rendition with raised saturation, so matching
it means matching a look, not accuracy to the scene. [ADR 0008](0008-film-log-look.md) made a look a
named set of values that `analyze` writes into the automatic recipe, with no change to the recipe
schema.

## Decision

1. **A third look, `Frontier`, writes two values:** the master tone curve and `color.saturation`.
   Neutral stays the default and Film log is unchanged, so the golden images do not change. The
   look is chosen by the Starting look setting, like the others.
2. **The curve has three numbers and the look a fourth** (`gtp-edit/src/frontier.rs`). With `P` the
   inversion's output (0 at the frame's black point, 1 at its white point, so one curve serves every
   frame): a sigmoid `s(P) = 1 / (1 + exp(-slope * (P - pivot)))`, rescaled to 0 and 1 at the ends,
   raised to `gamma`, then `srgb_decode`d so the display shows it. It is written as 16 points, closer
   together near the ends, and follows the exact function within 0.01 in code value (tested). The
   fourth number is the saturation.
3. **The numbers come from `benchmark fit`.** Each candidate is rendered through the real pipeline,
   so the curve, the LUT and the colour stage are what the app uses. The objective is the mean
   ΔE2000 (each pixel capped at 40) plus the absolute difference of our mean chroma and the
   reference's: without the chroma term the search turned the saturation down to hide colour
   casts it could not fix. The search is Nelder-Mead. Ten frames are fitted and five held out (the
   3rd, 6th, 9th, 12th and 15th), and the held-out result is the one that counts.
4. **The levels stay as they are** (0.5 and 99.5 percentiles): four settings were fitted and none
   won clearly (see `docs/research/benchmark.md`).
5. **The fitted values:** slope 4.918, pivot 0.525, gamma 1.069, saturation 0.465.

## Consequences

- **The target is missed, and the look is still a gain.** On the 15 frames the median ΔE falls from
  17.2 to 10.0 (p95 44.8 to 22.0); on the fitted ten the median of the frames' medians is 9.0 and on
  the five held out **13.2**, against a target of 6. The author is to judge the renders by eye; the
  numbers do not say the look is right, only that it is closer to Trichroma Lab's.
- **The saturation of 0.465 is not a desaturated look.** The master curve acts on each channel, so
  adding contrast adds chroma, and the saturation takes it back; the net chroma matches the
  reference. A reader who sees 0.465 and expects "more saturated" should read this paragraph.
- **One film, fifteen frames.** Four numbers fitted to one stock cannot follow a lab's per-hue,
  per-density colour rendition, and the held-out result moves by about a unit between runs of the
  search. What is left is mostly colour (casts of up to 11 in b\*, and frame 15's sky), so the
  next step is an automatic colour balance, not a richer curve.
- **A global curve cannot reproduce local adjustments** the lab's rendition may include.
- Changing the fit is cheap: `benchmark fit` prints the four numbers, ready to paste into the
  named constants. Another reference look (Nikon Scan's, a Noritsu look) is another variant and a
  fit on its own pairs.
