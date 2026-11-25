# 0022. The Oklab conversion uses our own cube root, the same on every platform

- Status: Accepted (issue P-34 / GitHub #356; the owner accepted the change on 2026-11-25 with the tolerance below)
- Date: 2026-11-25

## Context

Noise reduction (ADR 0013, 0020) and the colour mixer (ADR 0012) convert every pixel to Oklab, which
takes a cube root of each of three cone responses. The platform's maths library computes it, and
P-31 found it the largest single part of what could not get faster under the bit-for-bit rule:
about 0.37 s each for the noise stage and the mixer on 6 threads at 24 MP. The library's result is
also not the same on every system, so a picture cannot be promised to be the same on Windows and
macOS. Issue P-34 asks whether a faster cube root with a stated error may replace it, within a
tolerance the owner sets, and if so to do it.

**The tolerance.** The first tolerance was: no 16-bit export value changes by more than 1 level, and
no 8-bit value by more than 1 level. It was set before anything was measured. Measured, 38 of the 60
renders broke it (in 16 bits). On 2026-11-25 the owner accepted the change with a new tolerance that
replaces the first one: in every measured render, no 8-bit value changes by more than 1 level, and at
most 0.02 % of the 16-bit values (per space) change by more than 1 level. The new limit was fitted to
the results, not set before them: the largest measured share is 0.0107 %, and the largest changes are
7 levels in sRGB, 55 in Adobe RGB, 3 in ProPhoto and 1 in 8 bits.

## Options

Each was measured over every `f32` in [1, 8) (25 165 824 values; 8 stands for all normals, see
"Bit-identical"), on Windows x86-64, release build. The error is in units in the last place (ulp) of
the `f32` result against the true root, which is `f64` Newton iteration to the last bit, a method
unrelated to the candidates. "Not correctly rounded" is decided in exact integer arithmetic (the
root lies between the midpoints to the neighbours of the result exactly when the input lies between
their cubes), which does not call any cube root. The candidates are test-only code in
`crates/gtp-edit/src/cbrt.rs`; B is the one adopted. B, C and E share the seed below; D has its own reciprocal seed.

| Candidate | Largest error | Mean error | Not correctly rounded | Time per value |
|---|---|---|---|---|
| A: the library, `f32::cbrt` | 1.8207 ulp | 0.3799 ulp | 30.28 % | 28.4 ns |
| B: two Halley steps in `f64`, one rounding (adopted) | 0.5000 ulp | 0.2500 ulp | 0 | 2.6 ns (3.1 ns with the edge handling of `cbrt`) |
| C: two Halley steps in `f32` only (correction form) | 0.9566 ulp | 0.2697 ulp | 11.31 % | 2.3 ns |
| D: division-free, three Newton steps on the reciprocal root in `f64` | 0.5070 ulp | 0.2500 ulp | 0.12 % | 13.2 ns |
| E: first step in `f32`, second in `f64` | 0.5000 ulp | 0.2500 ulp | 0 | 2.8 ns |

A prototype run by the orchestrating agent (not by the owner) gave the same for A (1.82, 0.380,
30.28 %), B and C (0.96 ulp, 2.3 ns). It gave 4.0 ns for D at 0.501 ulp, 3.5 ns for B and 3.5 ns for E;
the D in the tree is a plain version of the idea (three steps, no tuning, which is where its
0.507 ulp comes from), so its figures differ from the prototype's and say little about the best
division-free method. Times are one thread on independent values, minimum of seven runs. A, B, C and
E move by some tenths of a nanosecond between runs; D moved more (13.2 ns here, 9.0 ns in the
review's rerun). Commands, from the repository root:

- `cargo test --release -p gtp-edit --lib error_of_each -- --ignored --nocapture` (errors),
- `cargo test --release -p gtp-edit --lib time_of_each -- --ignored --nocapture` (time),
- `cargo test --release -p gtp-edit --lib every_float -- --ignored --nocapture` (B over all of [1, 8): 0 not correctly rounded, largest error 0.500000 ulp, mean 0.249999 ulp),
- `cargo test --release -p gtp-edit --lib how_the_library_differs -- --ignored --nocapture` (A against B).

B and E are the same in accuracy; B is simpler (one extra division in `f64` instead of a mixed
pair) and a little faster, so B. C is the fastest but is wrong by up to 0.96 ulp on more than a tenth
of the values, an error of the order of the library's. D is the slowest of the four replacements.

## Decision

`cbrt` (`crates/gtp-edit/src/cbrt.rs`, private to `gtp-edit`) is the cube root of `rgb_to_oklab`, so noise
reduction, the colour mixer, the preview, the detail view and the export all use it (ADR 0015 is
kept: one function everywhere). It is B:

1. Zero, an infinity or NaN is returned as it came (zero keeps its sign; a signalling NaN too).
2. A subnormal is multiplied by 2^24 (exact) and the result divided by 2^8 (exact) at the end.
3. The seed is `f32::from_bits(bits(|x|) / 3 + CBRT_SEED_OFFSET)`.
4. Two Halley steps in `f64`: `t' = t (2a + t^3) / (a + 2 t^3)`; one step maps a relative error
   `e` to about `2/3 e^3`, so a seed within 3.2 % gives about 2e-5 after one step and 6e-15 after
   two.
5. One rounding to `f32`; the sign of `x` is put back by its bit.

Why the rounding is correct: not by a margin. The largest two-step error measured is 7.52e-15
relative (2^-46.92, at 1.101681), but the true root comes as close as 1.69e-15 (2^-49.07) to a
midpoint between two `f32` values (input `0x400353B5`, root `0x3FA2A7BC`, the hardest input, pinned in
the fixed bits). The worst error and the closest approach fall at different inputs, so correct
rounding holds because the exhaustive test checked every float of [1, 8), not because of the bound.
Changing the seed, the number of steps or the order of the operations needs that test run again (per
the review: the first guess `0x2A555555` gives 29 wrong values in [1, 8); offsets of +0x1000 and
+0x10000 added to the constant give 0, +0x40000 gives 19 and +0x80000 gives 644). Every finite input
is covered: [1, 8) by the exhaustive test, the other normals by the scaling argument below, and all
8 388 607 subnormals were checked directly in the review (0 wrong).

Only integer operations and IEEE `+`, `*` and `/` are used: no `mul_add`, no maths-library call.
Nothing else is replaced: `atan2`, `sin`, `cos`, `exp2`, the powers and logarithms of tone stay.
`rgb_to_oklab` is the only caller. The recipe, the schema, the IPC and the UI do not change.

### The seed offset, derived

The exponent field of a float is its base-2 logarithm in steps of 2^23 and the mantissa field is
nearly linear in the logarithm. For `x = 2^e (1 + m)` the bits are `(e + 127) 2^23 + m 2^23`, and
a cube root has bits near `(bits - 127 2^23) / 3 + 127 2^23 = bits / 3 + 2/3 127 2^23`, which is
`0x2A555555` and is the first guess. That line is exact at the powers of 2^3 and too high between
them (`log2(1 + m)` is above `m`), so a smaller constant balances the largest error on both sides.
The ignored test `search_the_seed_offset` scans every offset within 2^22 of the guess (a coarse
scan with a step of 256, then every offset near the best) over every 4099th float of [1, 8) and
keeps the one with the smallest largest relative error of the seed against the root:
`CBRT_SEED_OFFSET = 0x2A5106C2`, largest relative error 3.154 %. (The same search gives
`0x54A23279`, 3.424 %, for the reciprocal seed of candidate D.) The constant is a named `const`;
no library constant and no library code was used.

## Bit-identical

The result of `cbrt` is the same on every platform with IEEE 754 binary32 and binary64 arithmetic,
round to nearest even, no fused multiply-add and no extended precision (Windows x86-64 and macOS
arm64, the two supported targets). It rests on:

- integer operations are exact; `+`, `*`, `/` and the conversions `f32 -> f64 -> f32` are
  correctly rounded by IEEE 754, so they give one answer for one input;
- Rust never contracts `a * b + c` into a fused operation and has no fast-math mode, so the order
  of the operations in the source is the order executed; the expression is written in a fixed order;
- no maths-library function is called.

`rgb_to_oklab` is the same: its matrices are plain `+` and `*` in a fixed order, and
`rgb_to_oklab_gives_fixed_bits` pins the bits of eight colours (black, white, grey, ordinary
colours, one above 1, one with a negative component, a dark one). `fixed_inputs_give_the_expected_bits`
pins the bits of `cbrt` for 26 inputs: subnormals, the smallest normal, 1, 8, 0.18, `f32::MAX`,
negatives, exact cubes, the hardest input above and the first one that a slightly low result gets wrong; each was also checked as the correctly rounded root.

**Why [1, 8) stands for all normals.** Multiplying `a` by 2^(3k) adds `k` to the exponent of the
root. In integer division `(bits + 3k 2^23) / 3 = bits / 3 + k 2^23`, so the seed is multiplied by
exactly 2^k; both Halley steps are homogeneous under this scaling (every product and quotient is
scaled by a power of two, with no overflow or underflow in `f64`); the final rounding is the same
because the result stays normal. So the result for a normal `x` is the result for `y` in [1, 8)
with `x = y 2^(3k)` times 2^k, bit for bit. Subnormals are mapped into normals by the exact factor
2^24 = 2^(3 * 8). The test `scaling_by_eight_scales_the_root_by_two_bit_for_bit` checks this on
a strided set and the exhaustive test covers all of [1, 8).

**Tests in normal CI time** (`cargo test -p gtp-edit`, under a tenth of a second): correct rounding on
every 4099th float of the whole positive finite range (about 520 000 values, every exponent) and on
the subnormals, against the exact integer check; the error bound; exact cubes; odd symmetry;
monotonicity over runs of consecutive floats and over the whole range; zeros, infinities, NaN
(`is_nan` only), the largest and smallest values. Ignored: the exhaustive test, the candidate
errors, the timings, the library statistics and the seed search.

**What is not identical.**

- The payload of a NaN: the input NaN is returned unchanged (a signalling NaN too), and a library
  may return another payload. The tests check `is_nan` only. The mixer skips non-finite pixels
  first, so no render is affected.
- The rest of the Oklab path is not made platform-independent here: `hue_deg` calls `atan2`,
  `oklch_to_rgb` calls `sin` and `cos`, the mixer's lightness gain calls `exp2`, and
  tone calls powers and logarithms. Their last bits can still differ between Windows and macOS, so
  a whole render is not promised to be the same on both. The inversion's logarithm, before this
  path, is a library function too.

## Library difference between Windows and macOS

CI ran the library on macOS (macos-15, arm64; a temporary workflow on a branch since deleted, run
45684767930). The macOS library is correctly rounded on every float of [1, 8) and bit-identical to
ours there: 0 differences, largest error 0.5000 ulp, mean 0.2500 ulp, against 1.8207 ulp, 0.3799 ulp
and 30.28 % not correctly rounded for the Windows library. The ordinary `cbrt` and `oklab` tests,
the fixed bits included, pass on macOS. On Windows the library differs from `cbrt` by at most 2 bits
of the `f32` over [1, 8) (`how_the_library_differs_from_ours`, which writes one `i8` per float to
`<dir>/<os>-<arch>.bin` when `GTP_CBRT_LIBRARY_DIR` names a directory).

The 36 synthetic and direct renders of the measurement below (no real scans on CI) were dumped on
macOS with the library and with ours: 0 of 36 cases changed. **macOS results do not change in any
measured render; the change is on Windows.** Compared with the Windows dumps, 25 747 200 values per
space:

| Pair | Changed sRGB / Adobe RGB / ProPhoto / 8-bit | Over 1 level | Largest change | Cases over 1 level |
|---|---|---|---|---|
| Windows old against macOS old (today) | 1.3057 / 1.2414 / 1.2737 / 0.0053 % | 319 / 341 / 8 / 0 | 7 / 55 / 3 / 1 | 21 of 36 |
| Windows old against Windows new | 1.2543 / 1.1931 / 1.2239 / 0.0051 % | 314 / 321 / 8 / 0 | 7 / 55 / 3 / 1 | 21 of 36 |
| Windows new against macOS new | 0.1774 / 0.1658 / 0.1668 / 0.0007 % | 4 / 34 / 0 / 0 | 3 / 10 / 1 / 1 | 15 of 36 |

So the two systems already differ today by as much as the change does, and with our cube root they
differ about seven times less. They are still not identical: the noise-only direct pictures are the
same on both, and the others differ, presumably through other library functions (the inversion's
logarithm, `atan2`, `sin`, `cos`, `exp2`, the powers of tone); this was not isolated.

## Tolerance and measured changes

The tolerance and its history are in "Context".

The ignored measurement `measure_cbrt_changes_in_finished_values` (`crates/gtp-app/src/pipeline/measure_cbrt.rs`)
rendered 60 cases with the library's cube root and wrote the finished values (16-bit through
`gtp_io::to_output` in sRGB, Adobe RGB and ProPhoto; 8-bit screen bytes through `to_rgba8_srgb`),
then rendered them again with `cbrt` and compared. The cases are synthetic negatives with grain
(three scenes: a noisy gradient, dark blobs, saturated colours with values above 1) rendered with
noise only, the mixer only and both, under a neutral tone, a steep tone (exposure +1, contrast 80,
a steep curve) and a strong mixer (hue, saturation and lightness at +-100 in every band) (24
cases); two colour grids with seeded noise and values from below 0 to above 1, through the stages
directly (12 cases); and three real scans of `samples/private` (Ektar 100, Vision3 250D, Vision3
500T; never copied into the repository) with the automatic recipe plus noise and mixer, normal and
steep, at 1600 pixels and, for both stages, at full size (24 cases). Commands: `GTP_CBRT_MODE=dump`
with the library's code, then `GTP_CBRT_MODE=compare` with the new code, both
`GTP_CBRT_DIR=<dir> cargo test --release -p gtp-app --lib measure_cbrt -- --ignored --nocapture --test-threads=1`.
The comparison fails when a dump is missing or has another length, and when a case is outside the
accepted tolerance.

Largest change over the cases of each kind, in levels:

| Cases | sRGB 16-bit | Adobe RGB 16-bit | ProPhoto 16-bit | 8-bit screen | Share of 8-bit values changed |
|---|---|---|---|---|---|
| Synthetic (24) | 7 | 13 | 1 | 1 | at most 0.0134 % |
| Direct grids (12) | 5 | 55 | 3 | 1 | at most 0.0095 % |
| Real scans (24) | 6 | 50 | 2 | 1 | at most 0.0105 % |

In no case does an 8-bit value change by more than 1 level. The largest share of 16-bit values that
change by more than 1 level is 0.0107 % (sRGB, synthetic dark blobs, steep, both stages), then
0.0066 % in Adobe RGB and 0.0007 % in ProPhoto, so every case is inside the accepted 0.02 %. Per
case, 0.15 to 2.93 % of the sRGB values, 0.11 to 2.74 % of the Adobe RGB values and 0.13 to 3.13 % of
the ProPhoto values change at all, almost all by 1 level. The values that change by more than 1 level
are rare: on the full-size real scans with the steep recipe and both stages, 987 (Ektar 100), 194
(250D) and 1168 (500T) of 67 million sRGB values, 103, 159 and 223 of the Adobe RGB values, 22, 257
and 149 of the ProPhoto values. With the neutral recipe the full-size real renders change by at most
1 level in sRGB and ProPhoto and by 22, 1 and 3 levels in Adobe RGB.

Where they are, over all 60 cases (old values of 65535):

| Space | Changes of 4 levels or more | At old values | Changes of 2 or 3 levels | At old values | Median old value (2 or 3) |
|---|---|---|---|---|---|
| sRGB | 58 | 107 to 6652 | 3077 | 2 to 61574 | 3324 |
| Adobe RGB | 128 | 32 to 2990 | 815 | 48 to 61400 | 1891 |
| ProPhoto | 0 | none | 528 | 5798 to 61906 | 52886 |

So the large changes are in the shadows, where the encoding is steepest (the linear segment of sRGB
has a slope of 12.92; Adobe RGB is a power of 563/256 with an unbounded slope at 0), and most of the
small ones in ProPhoto are at bright values.

**The cause is the `f32` matrices, not only the old cube root.** The review of this change ran a
standalone experiment (not in the repository's tests): a round trip RGB to Oklab (a and b times a
factor `s`) to RGB in `f32` on 12 million random colours per row, compared with the same chain in
`f64`. `LMS_TO_REC2020` and the output matrix cancel, which leaves an absolute linear error of about
1e-7 in a channel near zero; the Adobe RGB power has an unbounded slope there, `(1e-7)^(256/563) *
65535` is about 43 levels. At `s = 1` the rms error against the `f64` chain fell with our cube root
from 0.0135 to 0.0119 levels (sRGB) and from 0.0217 to 0.0169 (Adobe RGB); the values more than 1 level
off fell from 27 to 11 (sRGB) and from 527 to 390 (Adobe RGB); the largest Adobe RGB error fell
from 52.8 to 44.8. Every alternative that changes the last bits (every root one ulp higher, the
sums of the matrices in reverse order) also moves Adobe RGB by 53 to 55 levels and sRGB by 2 against
the library. So the new pictures are closer to the exact result on average but not exact, and any
change to the last bits of this `f32` path breaks the first tolerance, whichever cube root is used.

**An exception to the 8-bit limit.** The 8-bit limit holds in the 60 measured cases, not for every
picture. The tone stage has an older jump at luma 1e-6 (`LUMA_EPSILON` in `crates/gtp-edit/src/tone.rs`):
above it a pixel keeps its colour, below it it becomes neutral grey. With `tone.blacks` = +100, a
pixel after the Oklab stage of about [1.39e-6, 9.4e-7, 1.5e-8] goes in sRGB from [21869, 14493, 0]
with the library's root to [15840, 15840, 15840] with ours (the review emulated the round trip
with the same matrices at `s = 1.2`): 15 840 levels of 16 bits, about 62 levels of 8 bits by scaling.
It needs a luma within about 1e-7 relative of 1e-6, so it is very rare in pictures, and the 60 cases
never set blacks (or whites, dehaze, channel curves, sharpening, split toning, vignette), so they
cannot show it. It is a jump in tone that was there before, not a fault of `cbrt`; changing the last
bits of any earlier stage near that luma would move it. A follow-up issue is proposed for the jump;
it is not fixed here.

The six goldens (`cargo run -p gtp-benchmark -- golden`) use neither noise reduction nor the mixer
(their recipes have both at their defaults, and `rgb_to_oklab` has no other caller), so they are
unchanged: ΔE2000 maximum 0.000 on all six. The benchmark has no render with noise or the mixer.

## Thumbnails

No regeneration is needed. A thumbnail is the pipeline's picture as an 8-bit image; in the measured
cases the largest change of an 8-bit value is 1 level on at most 0.014 % of the values (one value
in 7500), which is not visible and inside the 1 level that the screen and the sRGB export already
differ by (ADR 0015). The exception above (blacks +100 at luma 1e-6) is the one known place where an
8-bit value moves further.
Thumbnails made before the change keep their pixels until they are regenerated for another reason.

## Other candidates from P-31

Not replaced here, for later issues. The only per-function breakdown is the existing ignored test
`mixer_breakdown_on_a_6000_by_4000_image` (random colours, every pixel colourful, CPU time summed
over threads, run on this machine after the change):

| Part of the mixer | CPU ms at 6 threads | CPU ms at 12 threads | Wall ms at 6 threads (CPU / 6) |
|---|---|---|---|
| A `rgb_to_oklab` (now with `cbrt`) | 455 | 737 | 76 |
| B `hypot` and gate | 149 | 249 | 25 |
| C hue, `atan2` | 387 | 515 | 65 |
| D band weights | 213 | 292 | 36 |
| E new hue, chroma and lightness (`sin_cos`, `exp2`) | 1007 | 1250 | 168 |
| F `oklab_to_rgb` and store | 89 | 148 | 15 |

The whole `apply_mixer` took 426 ms wall at 6 threads and 261 ms at 12. So the largest remaining
part is E, `sin_cos` and `exp2`, at most about 170 ms of 426 ms on 6 threads if it were free; then
`atan2` at about 65 ms. A cheaper `atan2`, `sin` and `cos` that are also platform-independent would
make the mixer the same on both platforms; `exp2` of the lightness gain could be a table or a
polynomial. The powers and logarithms of the tone stage (about 285 ms for the stage in the
detail-view breakdown at 12 threads, `time_a_full_size_detail_render_by_stage`) are **not measured
by function**: there is no breakdown of the tone stage, so their possible gain is unmeasured.

## Stage timings

`time_a_full_size_detail_render_by_stage` (6000 x 4000, release, Windows, decode of the scan included
in the whole render), range of six medians, before (library cube root) and after:

| | Noise stage | Mixer | Whole render, decode to RGBA8 |
|---|---|---|---|
| 6 threads, before | 876 to 894 ms | 678 to 691 ms | 2.96 to 2.99 s |
| 6 threads, after | 592 to 624 ms | 416 to 444 ms | 2.30 to 2.50 s |
| 12 threads, before | 784 to 793 ms | 409 to 415 ms | 2.35 to 2.41 s |
| 12 threads, after | 619 to 651 ms | 259 to 270 ms | 1.97 to 2.02 s |

The gain is on Windows. On the shared 3-thread macOS CI machine (noisy) a cube root took 3.44 ns
with the library, 2.28 ns with ours and 2.70 ns with its edge handling; the timings of the noise and
mixer stages there were too noisy to show a clear change, and little or no gain is expected on macOS,
where the library is already fast and correctly rounded.

## Consequences

- **Faster Oklab conversion on Windows.** A cube root takes 2.6 to 3.1 ns instead of 28 ns, and
  `rgb_to_oklab` takes three per pixel (the way back, `oklab_to_rgb`, cubes and has no cube root).
  The stage timings are above; on macOS there is little or no gain.
- **The result of `rgb_to_oklab` is the same on every platform** and is pinned by fixed bits. The mixer
  and the rest of a render still use library functions (see "What is not identical").
- **Pictures change a little on Windows, not on macOS** (see the measured changes): the 8-bit
  pictures by at most 1 level on about a hundredth of a percent of the values, the 16-bit values by
  1 level on up to about 3 % of them and by more (at most 7 in sRGB, 55 in Adobe RGB, 3 in ProPhoto)
  on at most 0.0107 %, in the measured renders, with the one known exception (blacks +100). No saved recipe changes; a file exported before differs from the same export now by
  those figures. Files and thumbnails already written are not touched.
- **Preview, detail view and export agree** as before: one function in `rgb_to_oklab`, no second
  path.
- **No new dependency**, no Tauri command, recipe or schema change; `cbrt` is private to `gtp-edit`.
- **Tests.** No test had to change its expectation: the existing pins of the noise and mixer stages
  and the golden files still hold. Tests added: the ones above in `cbrt.rs`, the fixed bits of
  `rgb_to_oklab` in `oklab.rs`, and the ignored measurements in `cbrt.rs` and
  `crates/gtp-app/src/pipeline/measure_cbrt.rs`.
