# 0023. The tone stage fades to the mapped grey below a luma of 1e-4

- Status: Accepted (issue P-38 / GitHub #405)
- Date: 2026-11-25

## Context

The tone stage (steps 1 to 4 of `apply_tone` in `crates/gtp-edit/src/tone.rs`) maps the Rec.2020
luma `Y` to `Y'` and scales the pixel by `Y' / Y`, which keeps hue and saturation. The scale is not
defined at `Y = 0` and grows without bound near it, so the code switched at `Y = 1e-6`
(`LUMA_EPSILON`): above it the pixel was scaled, below it the pixel became the grey `[Y'; 3]`.

That switch is a jump in the output. With `tone.blacks` = +100, `Y'` at black is about 0.048, so a
saturated pixel goes from full colour to grey at `Y = 1e-6`: about 62 levels of 8 bits for pure red
or green and about 170 for pure blue. The review of ADR 0022 found it (see "An exception to the
8-bit limit" there). It needs a luma within a hair of 1e-6, so it is rare, but it is a tone error
that the stage should not have.

## Decision

A named constant `NEAR_BLACK_LUMA = 1e-4` (`T`) replaces `LUMA_EPSILON`.

- For `Y >= T` the code is unchanged, bit for bit: `c * (Y' / Y)`, clamped to `[0, MAX_SAMPLE]`.
- For `Y < T`: `c' = Y' * (1 + (c - Y) / T)`, clamped to `[0, MAX_SAMPLE]`.

Why this form:

- It meets the scaling at `Y = T` (`Y' * (1 + (c - T) / T) = c * Y' / T`), so the output is
  continuous there.
- At `Y = 0` every channel is 0, so the result is exactly the grey `[Y'; 3]`: the same as before for
  a zero pixel.
- It is linear in `c`, and the luma of the result is `Y'` (the luma weights add up to 1), so the
  stage still sets the luma to what the tone map says.
- `c >= 0` after the input clamp, so `c - Y >= -Y > -T` and the factor is positive: the result
  cannot go negative. It never divides by `Y`.
- The weight is linear in `Y`. The slope of the output with respect to the luma is bounded by
  `Y'_slope * max(1, r) + Y'_max * |r - 1| / T`, with `r = c / Y`.

Input handling does not change: samples are clamped to `[0, MAX_SAMPLE]` and a value that is not
finite becomes 0, so zero, negative and non-finite pixels give the same output as before. The
identity early return stays.

## Alternatives considered

**A smoothstep weight on the same blend** (`w = smoothstep(Y / T)` between the grey and the
scaling). It has no kink at `T`, but the slope at 0 is 0 and the extra cost and complexity buy
nothing visible: a kink in the first derivative at `T` is not a jump in the picture. Rejected for
simplicity.

**Other values of T.** Larger values give a smaller step per luma change but lose more colour in
the deepest shadows; smaller values do the reverse. 8-bit levels, from a model that applies the sRGB
curve to Rec.2020 values without the gamut matrix, with a step per luma change of 1.5e-6, each cell
for pure red / pure blue:

| T | Step, Blacks +100 | Change at EV +5 | Change at contrast -100 |
|---|---|---|---|
| 3e-5 | 8.0 / 21 | 2 / 10 | 8 / 23 |
| **1e-4** | **2.4 / 7.1** | **6 / 18** | **11 / 30** |
| 3e-4 | 0.8 / 2.5 | 11 / 28 | 14 / 38 |
| 1e-3 | 0.2 / 0.8 | 22 / 47 | 18 / 49 |

1e-4 keeps the largest step at a few levels (red) and 7 (blue) at the worst slider setting, and the
colour lost below `T` stays in the deepest shadow.

## Consequences

- Below a luma of 1e-4 the colour relative to the luma is reduced by `Y / T` against the old
  scaling, whatever the sliders. It shows mainly under strong lifts (Blacks above 0, exposure up),
  where deep shadow colour is weaker. Pixels with `0 < Y <= 1e-6`, grey before, now keep a little
  colour.
- At and above `T` the output is bit for bit what it was; below `T` it changes only for pixels
  whose old output was a scaled colour with `Y` in `(1e-6, 1e-4)` or a grey that now keeps some
  colour.
- Goldens: unchanged, ΔE2000 maximum 0.000 on all six (`cargo run -p gtp-benchmark -- golden`).
- Cost: none measurable. The colour and tone stages on a 21-megapixel picture took 212 to 215 ms
  before and after (three release runs each of `time_a_21_megapixel_image`).
- Real scans and synthetic negatives: the 60 cases of ADR 0022's measurement (`measure_cbrt`),
  rendered before and after this change and compared. No 8-bit value changes, in any case. With
  the neutral recipe nothing changes at all. The 18 cases that change use the steep recipe
  (exposure, contrast and a steep curve), which lifts pixels from below a luma of 1e-4: at most
  0.18 % of the 16-bit values of a full-size real scan change by more than 1 level, by up to 48
  levels of 16 bits in Adobe RGB and 14 in ProPhoto at old values below about 160, where those
  encodings are steepest; sRGB moves by at most 2 levels. A synthetic scene of saturated colours
  changes 1.5 % of its Adobe RGB values. None of the cases sets Blacks, so they do not show the old
  jump itself.
- Tests in `tone.rs` check the step between neighbouring lumas against the bound above, bit equality
  with the old formula at `T` and above, the unchanged output of zero, negative and non-finite
  pixels, and the luma below `T`.
