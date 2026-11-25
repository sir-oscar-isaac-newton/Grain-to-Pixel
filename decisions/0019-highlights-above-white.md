# 0019. Colours above white keep their hue and roll off to white

- Status: Accepted (issue P-32)
- Date: 2026-11-24

## Context

The output conversion clipped each linear Rec.2020 channel to [0, 1] on its own before the matrix
or the Little CMS transform. A colour with one or two channels above 1 lost the ratio between its
channels: it turned paler and moved towards the neighbouring hue. ADR 0015 measured it. A green
(0.6, 2.7, 0.9) became (158, 255, 243) in sRGB, a pale cyan-white; an orange (1.8, 0.9, 0.2)
became (255, 243, 95), yellower. Since P-30 the screen does the same, so a file and the preview
agree, both wrongly.

Three places clipped: `to_output` and `to_grey_output` in `crates/gtp-io/src/icc.rs`, and
`to_rgba8_srgb` in `crates/gtp-app/src/pipeline/display.rs`. The histogram and the clipping
overlay are computed from the 8-bit result of the screen conversion, so they judge whatever rule
those places use.

The figures below come from ignored tests that stay in the tree
(`crates/gtp-app/src/pipeline/display/measure_highlights.rs` and `measure_time_export` in
`crates/gtp-io/src/icc.rs`). Run them with `--ignored --nocapture`.

### How the figures are taken

- The rules are applied to a linear Rec.2020 pixel before the matrix to sRGB, as the screen does.
  The sRGB bytes are what the screen shows.
- The "true hue" of a pixel is the source colour scaled so that its largest channel is 1, shown
  the same way. Hue shift is the Oklab hue angle of the result minus the true one, in degrees.
  Chroma change is the Oklab chroma of the result minus the true chroma.
- Pixels whose true Oklab chroma is under 0.02, or whose result has under 0.005, have no stable
  hue and are left out of the hue figures. The hue figures count only pixels with a channel above
  1.
- The Oklab hue is not the hue of the linear values. The roll-off keeps the ratios between the
  channels exactly (the HSV hue of the linear values moves by less than 0.1 degree in the tests),
  but a mix with white in linear light does not stay on a line of constant Oklab hue, so the
  roll-off shows a few degrees of Oklab shift even though the ratios are kept.

## Options

Three rules, all of which leave a pixel with no channel above 1 as it is (apart from NaN and
negative values going to 0):

- **A. Clip each channel** (the rule until P-32).
- **B. Scale by the largest channel**: divide the pixel by its largest channel `m` when `m` is
  above 1, so the largest channel is 1 and the ratios stay.
- **C. Soft roll-off**: scale as in B, then move each channel towards 1 by
  `t = (m - 1) / (m - 1 + K)`. Saturation falls towards white as `m` grows. A neutral pixel above
  1 reaches exactly white.

### The test colours of ADR 0015, sRGB bytes

Hue shift in degrees, chroma change in Oklab units (result minus true).

| Colour | A clip | B scale | C, K = 0.5 | C, K = 1 | C, K = 2 |
|---|---|---|---|---|---|
| orange (1.8, 0.9, 0.2) | (255, 243, 95), +32.8 deg, +0.014 | (255, 177, 67), 0, 0 | (255, 229, 209), -11.2 deg, -0.112 | (255, 216, 183), -9.2 deg, -0.089 | (255, 204, 155), -6.8 deg, -0.065 |
| green (0.4, 1.8, 0.6) | (51, 255, 198), +14.5 deg, -0.051 | (0, 255, 141), 0, 0 | (191, 255, 220), +7.1 deg, -0.144 | (151, 255, 202), +7.1 deg, -0.101 | (94, 255, 184), +7.1 deg, -0.055 |
| green (0.6, 2.7, 0.9) | (158, 255, 243), +32.1 deg, -0.130 | (0, 255, 141), 0, 0 | (220, 255, 235), +7.2 deg, -0.179 | (194, 255, 222), +7.2 deg, -0.147 | (155, 255, 204), +7.1 deg, -0.105 |
| magenta (1.8, 0.3, 1.5) | (255, 126, 255), -3.5 deg, -0.063 | (255, 68, 243), 0, 0 | (255, 209, 251), -1.5 deg, -0.207 | (255, 183, 248), -1.2 deg, -0.164 | (255, 155, 247), -0.9 deg, -0.117 |
| orange x2 (2.0, 1.0, 0.22) | (255, 255, 100), +37.4 deg, +0.022 | (255, 177, 66), 0, 0 | (255, 233, 216), -11.8 deg, -0.118 | (255, 221, 192), -10.0 deg, -0.097 | (255, 207, 164), -7.6 deg, -0.073 |
| orange x3 | (255, 255, 137), +36.9 deg, -0.011 | (255, 177, 66), 0, 0 | (255, 242, 233), -13.0 deg, -0.132 | (255, 233, 216), -11.8 deg, -0.118 | (255, 221, 192), -10.0 deg, -0.097 |
| green x2 (0.44, 2.0, 0.66) | (87, 255, 208), +17.1 deg, -0.070 | (0, 255, 141), 0, 0 | (201, 255, 225), +7.3 deg, -0.156 | (165, 255, 208), +7.2 deg, -0.116 | (114, 255, 189), +7.2 deg, -0.069 |
| green x3 | (176, 255, 254), +42.0 deg, -0.147 | (0, 255, 141), 0, 0 | (225, 255, 238), +7.3 deg, -0.185 | (201, 255, 225), +7.3 deg, -0.156 | (165, 255, 208), +7.2 deg, -0.116 |
| magenta x2 | (255, 138, 255), -3.7 deg, -0.079 | (255, 70, 243), 0, 0 | (255, 216, 251), -1.6 deg, -0.216 | (255, 193, 249), -1.3 deg, -0.177 | (255, 165, 247), -1.0 deg, -0.131 |
| magenta x3 | (255, 178, 255), -4.3 deg, -0.146 | (255, 70, 243), 0, 0 | (255, 233, 253), -1.7 deg, -0.243 | (255, 216, 251), -1.6 deg, -0.216 | (255, 193, 249), -1.3 deg, -0.177 |
| blue x2 (0.2, 0.8, 2.0) | (0, 240, 255), -36.7 deg, -0.015 | (0, 176, 255), 0, 0 | (206, 232, 255), +4.8 deg, -0.122 | (175, 220, 255), +2.4 deg, -0.096 | (134, 207, 255), -0.6 deg, -0.064 |
| blue x3 | (0, 255, 255), -44.7 deg, -0.009 | (0, 176, 255), 0, 0 | (227, 242, 255), +6.2 deg, -0.140 | (206, 232, 255), +4.8 deg, -0.122 | (175, 220, 255), +2.4 deg, -0.096 |

At x1 (largest channel exactly 1) all five columns give the same bytes. The orange base is
(1.0, 0.5, 0.11), the green (0.22, 1.0, 0.33), the magenta (1.0, 0.17, 0.83) and the blue
(0.1, 0.4, 1.0); the multiples scale all three channels.

### Synthetic renders

The renders ADR 0015 used: `gtp_core::testing::synthetic` gradient and blobs at 320 x 213 with
strong saturation (saturation 2, vibrance 100, every mixer saturation +100), at 0 and +1.5 EV.
Mean and p99 hue shift in degrees (absolute) over the pixels with a channel above 1, and the mean
Oklab lightness L of those pixels.

| Render | Above 1 | A clip | B scale | C, K = 0.5 | C, K = 1 | C, K = 2 |
|---|---|---|---|---|---|---|
| gradient, 0 EV | 66.3 % | 9.39 / 26.01, L 0.784 | 0 / 0, L 0.765 | 5.72 / 16.11, L 0.887 | 4.56 / 15.59, L 0.853 | 3.17 / 9.12, L 0.821 |
| gradient, +1.5 EV | 95.7 % | 19.57 / 43.72, L 0.834 | 0 / 0, L 0.787 | 7.24 / 16.36, L 0.961 | 7.10 / 16.29, L 0.938 | 6.79 / 16.19, L 0.906 |
| blobs, 0 EV | 31.4 % | 5.28 / 23.98, L 0.756 | 0 / 0, L 0.737 | 7.20 / 17.78, L 0.827 | 5.68 / 16.97, L 0.791 | 4.17 / 15.74, L 0.765 |
| blobs, +1.5 EV | 74.5 % | 17.47 / 61.20, L 0.825 | 0 / 0, L 0.757 | 9.98 / 18.60, L 0.934 | 9.24 / 18.44, L 0.902 | 8.26 / 18.12, L 0.865 |

### Real renders

Scans from the private sample set (Q-11), rendered with the automatic recipe and with strong
saturation at a 1600 px preview. Share of pixels above 1, then mean / p99 hue shift in degrees
over the pixels with a hue.

| Scan | Recipe | Above 1 | A clip | B scale | C, K = 0.5 | C, K = 1 | C, K = 2 |
|---|---|---|---|---|---|---|---|
| Ektar 100 | automatic | 0.9 % | 7.02 / 30.56 | 0 / 0 | 0.02 / 0.12 | 0.01 / 0.06 | 0.00 / 0.03 |
| Ektar 100 | strong | 25.0 % | 7.94 / 31.39 | 0 / 0 | 6.95 / 11.46 | 4.79 / 10.75 | 2.62 / 6.46 |
| Vision3 250D | automatic | 1.1 % | 7.22 / 29.72 | 0 / 0 | 0.03 / 0.22 | 0.02 / 0.12 | 0.01 / 0.06 |
| Vision3 250D | strong | 4.1 % | 9.67 / 53.12 | 0 / 0 | 0.78 / 5.11 | 0.49 / 3.66 | 0.28 / 2.35 |
| Ektacolor 400 | automatic | 0.8 % | 6.56 / 27.61 | 0 / 0 | 0.01 / 0.05 | 0.01 / 0.03 | 0.00 / 0.02 |
| Ektacolor 400 | strong | 7.1 % | 7.88 / 32.92 | 0 / 0 | 1.22 / 5.99 | 0.74 / 4.75 | 0.41 / 2.73 |
| Ektacolor 160 | automatic | 0.9 % | 2.87 / 16.49 | 0 / 0 | 0.01 / 0.12 | 0.01 / 0.07 | 0.00 / 0.03 |
| Ektacolor 160 | strong | 22.9 % | 12.42 / 28.02 | 0 / 0 | 4.99 / 10.92 | 5.59 / 11.32 | 4.13 / 10.60 |
| Vision3 500T | automatic | 0.8 % | no pixel with a hue | | | | |
| Vision3 500T | strong | 5.0 % | 6.84 / 41.47 | 0 / 0 | 3.62 / 17.79 | 2.55 / 16.98 | 1.66 / 15.75 |

The mean Oklab lightness of the pixels above 1, for K = 1 against A and B, is 0.992 against 0.996
and 0.992 (Ektar 100, automatic) and 0.806 against 0.814 and 0.784 (Ektar 100, strong). With the
automatic recipe about 1 % of the pixels are above 1 and most of them are near neutral, so the
rule changes few visible hues; with strong saturation it changes 4 to 25 %.

### Cost in time

Release build, median of seven, 12 threads (a 12-thread machine). The old clip is a local copy in
the measure code. "Half" is a picture whose brighter half of the columns has red and green tripled.

| Conversion | Picture | A clip (old) | C (new) |
|---|---|---|---|
| `to_rgba8_srgb`, 2048 x 1365 | no pixel above 1 | 7.69 ms | 7.95 ms |
| `to_rgba8_srgb`, 2048 x 1365 | half above 1 | 7.71 ms | 8.93 ms |
| `to_output` sRGB, 6000 x 4000 | no pixel above 1 | 660 ms | 665 ms |
| `to_output` sRGB, 6000 x 4000 | half above 1 | 657 ms | 666 ms |

B was timed on the screen conversion in a second run of the same test, through a copy of
`to_rgba8_srgb` with B in place of the rule. That run gave, for no pixel and half the pixels above
1: A 7.87 and 8.62 ms, B 7.92 and 9.41 ms, C 7.84 and 9.28 ms. B and C cost the same within the
noise between runs, since the division by `m` is most of the work and the blend adds a multiply
and an add. B was not timed on the 24 MP export; the Little CMS transform is most of that time,
and A and C differ there by under 10 ms. The existing timing of the whole preview (`time_a_1600_px_preview`, with the new rule)
gives 53 ms for the render and conversion of a 2048 x 1365 frame to a 1600 px preview and 81 ms
for a 2048 x 2048 one, against the 100 ms budget for a slider change. The 24 MP conversion takes
0.67 s against the 10 s budget for a 21 MP export (specification section 11). The rule adds under
2 ms to the preview conversion and under 10 ms to the 24 MP one.

### Summary of the options

| | A clip | B scale | C soft roll-off |
|---|---|---|---|
| Hue shift (Oklab, pixels above 1) | 3 to 20 deg mean, p99 up to 61 deg | none by construction | 0 to 9 deg mean, p99 0.03 to 18 deg (K = 1) |
| Highlights | pale, towards the neighbouring hue; a green becomes cyan-white | a bright colour looks the same as at 1: no highlight, darker than A (L 0.76 against 0.78 on the gradient) | a bright colour is paler as it gets brighter and reaches white; lighter than A and B on the synthetic renders (L 0.85 on the same gradient), a little under A on the real strong renders |
| Neutral above 1 | white | white | white |
| Cost | the reference | as C on 2048 x 1365 (within the noise between runs) | +0.3 to 1.2 ms on 2048 x 1365; +5 to 9 ms on 24 MP |
| Values already in range | unchanged | unchanged | unchanged |

### Adobe RGB and ProPhoto

The rule works in linear Rec.2020, before the profile conversion, so it decides what happens
above white for every output space alike. Its result is inside [0, 1] in each Rec.2020 channel, so
ProPhoto RGB, whose gamut is wider than Rec.2020's, holds every result of the rule. A colour that
is in range but outside a narrower space, such as saturated Rec.2020 colours in sRGB or Adobe RGB,
is still clipped per channel by Little CMS. That is gamut mapping, a separate question that this
rule does not change. The grey export converts the luminance of the result of the rule, so a
colour that rolls off to white gives white there too.

### The choice of K

K sets how soon a bright colour turns white: at `m = 1 + K` the colour is half way to white. K = 2
keeps more saturation and a lower lightness, with a smaller Oklab hue shift on most sets
(for example 2.6 against 4.8 degrees mean on Ektar 100 with strong saturation); K = 0.5 goes to
white sooner and shifts the hue more. No figure favours one of them clearly over K = 1: the
differences are a few degrees of Oklab hue and a few hundredths of lightness, and no set puts K = 1
out of line. K = 1 is the default and is kept; it is a named constant (`HIGHLIGHT_KNEE`) that can
be tuned later with the measure code that stays in the tree.

## Recommendation

**C with K = 1.** A is the rule that causes the problem: on the strongly saturated sets its hue
shift at p99 is 1.4 to 14 times that of C (for example 61 against 18 degrees on the blobs at
+1.5 EV, and 53 against 3.7 on Vision3 250D with strong saturation), and a bright green
turns cyan-white. A's mean is not always the larger: on the blobs at 0 EV it is 5.3 against C's
5.7 degrees. B keeps the hue exactly but never lets a bright colour rise above the colour it
has at 1, so a highlight looks dimmer than with A (L 0.76 against 0.78 on the gradient) and keeps
its full saturation however bright it is, whereas film highlights go to white. C keeps the ratios
between the channels and lets saturation fall towards white. Its bright colours are lighter than
A's on the synthetic renders and a little darker on the real strong ones. Its Oklab hue shift is
small on real renders and up to about 10
degrees on the most saturated synthetic ones, because a mix with white in linear light is not a line
of constant Oklab hue; that is the price of the roll-off, and B is the choice if a measured hue
shift of zero matters more than how highlights look. C costs under 2 ms on the preview and under
10 ms on 24 MP, well inside the budgets.

## Decision

**C with K = 1**, as the owner approved on 2026-11-24.

`gtp_core::bring_into_range` (`crates/gtp-core/src/highlights.rs`) is the one rule. NaN and
negative values (and negative infinity) go to 0 and positive infinity to the largest finite
value. If no channel is above 1 the cleaned pixel is returned as it is, which is the per-channel
clip bit for bit. Otherwise the pixel is divided by its largest channel `m` and each channel `c`
becomes `c + (1 - c) * t` with `t = (m - 1) / (m - 1 + K)` and `K = HIGHLIGHT_KNEE = 1.0`. The
result is finite and inside [0, 1], the largest channel is exactly 1 before the blend, a neutral
pixel above 1 gives exactly white, the rule is continuous at `m = 1`, and scaling a pixel up never
darkens a channel. Raising one channel alone can lower another a little, since the pixel is divided
by its largest channel; B does the same.

`to_output` and `to_grey_output` apply it per pixel before the Little CMS transform, and
`to_rgba8_srgb` applies it before the matrix, so the screen and the file agree within 1 level
on the same test sets as before (ADR 0015). The later per-row clip after the matrix stays.

## Consequences

- Pixels with no channel above 1 give the same bytes as before in every output space and in the
  grey output. Tests compare `to_output` (sRGB, Adobe RGB, ProPhoto) and `to_grey_output` with a
  reference that clips each channel, bit for bit, on a grid that includes NaN, negative values,
  zero, negative zero and values just below 1. The unit tests of the rule do the same on over a
  million pixels.
- Only colours with a channel above 1 change, on screen and in files. The golden cases of
  `gtp-benchmark golden` compare the linear render, before the output rule, and they do not change
  (all six cases: max ΔE2000 0.000). `tools/benchmark/tests/export_colour.rs` passes unchanged.
- Thumbnails made before keep their pixels and no regeneration is needed. A thumbnail is a derived
  copy, rebuilt whenever the frame's recipe changes, and nothing reads it back as data. The change
  touches only pixels with a channel above 1, which in an old thumbnail are a little paler
  or yellower than a new one would be.
- The histogram and the clipping overlay are computed from the 8-bit result of the screen
  conversion, so they judge the result of the rule: a colour above 1 that rolls to white counts as
  clipped, and one that keeps a channel below 255 does not. Specification section 7 says so.
- Tests that changed, and why:
  - `out_of_range_and_non_finite_values_are_clipped` (`display.rs`): `[-1, NaN, +inf]` was
    `[0, 0, 255, 255]` and is now `[255, 255, 255, 255]`. Infinity became 1 in one channel and the
    others 0; now it becomes the largest finite value, the pixel rolls all the way to white, and
    infinitely bright blue is white.
  - `whole_images_convert_to_the_same_bytes_as_before_also_across_task_boundaries` (`display.rs`):
    its reference `reference_rgba` used `clip_input` and now uses the shared rule. It checks that
    the lookup equals the formula, which the rule does not change; the test body is unchanged.
  - `out_of_range_and_nan_values_are_clipped` (`icc.rs`): it asserted that the green output of
    `[5, -1, NaN]` was 0; red 5 now keeps its hue and rolls towards white, so green is above 0.
    The test now asserts that green is above 0 and below red.
- Spaces wider than sRGB get the same rule. What it leaves in range but outside sRGB or Adobe RGB is
  still clipped by Little CMS per channel; gamut mapping stays out of scope.
