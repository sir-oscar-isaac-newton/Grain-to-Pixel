# 0008. The Film log look: a log start for editing

- Status: Accepted (the author approved the design on 2026-11-02, issue P-19 / GitHub #186)
- Date: 2026-11-02

## Context

The inversion's output `P` is linear in density between the black and white points. Density is the
logarithm of exposure (times the film's gamma), so `P` is already a log-like rendering of the scene,
and shown as if it were display-ready it looks washed out. The author shoots much Vision3 film
developed in ECN-2, which is designed for scanning, and wants a deliberate flat, wide-latitude
starting point to edit from, encoded with the Blackmagic Film Gen 5 log curve.

The recipe has a 16-point master tone curve that works in linear light, after the tone sliders and
before the display or export encoding; it was chosen to carry the look so that the recipe schema
and version do not change (a look is a set of values `analyze` writes, not a stage).

## Decision

1. **A look is a named set of values** (`gtp_core::Look`: `Neutral`, `FilmLog`) that
   `analyze_with_look` writes into the automatic recipe, as the master curve. `Neutral` is the
   default and adds nothing, so every existing conversion and golden image is unchanged. The look
   is chosen by the Starting look setting, applied to frames as they are imported (the stored recipe
   keeps it); Develop has a button that sets one frame's master curve to the look's.
2. **Film model.** Scene light is `s = 0.18 * 2^((P - 0.5) * R)`: the middle of the frame's black to
   white range is middle grey, and `R = (white - black) / (gamma * log10 2)` stops span the range,
   using the frame's own automatic levels (mean of the channels, kept between 2 and 16 stops) and
   one fixed `gamma = 0.6`.
3. **Encoding.** The code is `gen5_encode(s)`, the Blackmagic Film Generation 5 transfer function
   (constants in `gtp-edit`, named, checked against the published example values: 18 % grey
   encodes to 0.38356 and the two branches meet at 0.13388). The master curve is
   `srgb_decode(code)` through 16 points spaced evenly in `P`, so that after the display encoding
   the picture shows the code values, as log footage looks on a monitor with no LUT. The curve
   follows the exact function within 0.005 in code value between the black and white points, and
   within 0.02 in the 10 % above the white point, where a spline can only continue with its end
   slope (both are tested).

## Consequences

- **The gamma is an estimate.** Kodak's VISION3 data sheets give the sensitometric curves only as
  graphs, so 0.6 (about right for camera negative in its straight-line part) is an engineering
  value, not a measurement. Differences between stocks, pushes and pulls move middle grey; the
  exposure slider covers them. A different value is a one-constant change (`FILM_GAMMA`).
- **The Gen 5 constants were not read from the whitepaper.** They come from the open-source
  `colour` library's implementation of Blackmagic's published function. The tests pin published
  example values; comparing the constants with Blackmagic's PDF would close the question.
- **The sliders act before the curve.** Tone and colour controls work in linear light and the log
  curve comes after them, so they behave as before but their effect on a flat picture is
  compressed. Grading inside log space (a working-space stage so that the sliders act in log, as in
  DaVinci Resolve) would change the stage order and need a recipe version; it is a separate issue.
- **A frame already imported keeps its recipe** when the setting changes; the Develop button
  applies the look to it, and the same button with the setting on Neutral returns the curve to the
  plain line.
- Looks are data, so another look (the Frontier style of P-20, a Noritsu look) is another variant
  and a fit, not a new mechanism. A log-encoded export would be an output space, not a look.
