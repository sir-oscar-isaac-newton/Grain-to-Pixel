# Benchmarks against reference renders and the budgets (Q-02)

Two measurements: how far our automatic conversion is from the Trichroma Lab render of the same
frame (`benchmark report`), and how fast the pipeline is against the budgets of
[specification section 11](../specification.md) (`cargo bench -p gtp-app`).

Machine: AMD Ryzen 5 5600X (6 cores, 12 threads), 32 GB, SSD, Windows 10, release build. The
reference machine of the budgets is a 4-core/8 GB laptop, so these are faster than the budgets
assume; read them as a floor, not as proof.

## Quality: our automatic conversion against Trichroma Lab

The pairs are the author's 15 Mamiya 645 / Kodak Gold 200 triplets (Nikon D600, Scanlight V4) and
the 15 `Trichroma Lab Export_NN.tif` files (16-bit sRGB, about 5250 x 3900), matched in capture
order. The Nikon Scan positives of other films have no raw scan beside them and are not pairs.

```text
benchmark manifest --triplets <folder of NEF> --references <folder of exports> --out manifest.json
benchmark report --manifest manifest.json [--maps <folder>]
```

How it works: each triplet is merged and converted the way the app does at import (the 645 frame
window of P-18, and the roll's film base as the import ends up with it, U-17). Our render of the
whole capture is registered with the reference by the normalized cross-correlation of the
brightness over position and scale, so the two are compared over the same part of the picture (at
512 columns). The colour difference is CIEDE2000 in sRGB. Pairs that register below a correlation
of 0.8 are left out and flagged; none was.

Result (automatic recipe, no manual tweaks):

| Frame | Match | Crop gap | Median ΔE | p95 ΔE | Cast L\* | a\* | b\* |
|---|---|---|---|---|---|---|---|
| 01 | 0.915 | 0.9 % | 11.8 | 22.2 | +7.2 | -8.2 | +1.6 |
| 02 | 0.911 | 1.1 % | 13.5 | 45.5 | +21.6 | +0.5 | +1.9 |
| 03 | 0.877 | 0.6 % | 16.7 | 36.8 | +21.6 | -1.8 | -10.9 |
| 04 | 0.916 | 0.6 % | 18.1 | 43.2 | +24.5 | +3.9 | -1.5 |
| 05 | 0.927 | 0.9 % | 10.1 | 34.9 | +13.3 | -1.8 | +0.6 |
| 06 | 0.958 | 1.9 % | 15.8 | 28.6 | +11.8 | -5.5 | +5.0 |
| 07 | 0.964 | 0.9 % | 14.2 | 38.3 | +11.6 | -7.4 | -0.3 |
| 08 | 0.904 | 1.5 % | 27.7 | 49.4 | +28.1 | +0.5 | +8.7 |
| 09 | 0.849 | 1.5 % | 19.1 | 48.2 | +27.0 | -0.8 | +0.7 |
| 10 | 0.917 | 0.8 % | 15.7 | 44.1 | +22.9 | -2.1 | +3.0 |
| 11 | 0.900 | 1.0 % | 14.5 | 41.7 | +20.1 | -0.8 | +1.7 |
| 12 | 0.839 | 1.3 % | 18.2 | 34.7 | +21.9 | -4.3 | +3.7 |
| 13 | 0.905 | 0.6 % | 34.9 | 46.6 | +32.2 | +11.6 | -4.6 |
| 14 | 0.882 | 1.3 % | 31.1 | 43.9 | +28.1 | +9.7 | -6.7 |
| 15 | 0.926 | 15.4 % | 28.3 | 56.7 | +36.1 | -9.1 | -1.4 |

All pixels of the 15 frames: **median ΔE 17.2, p95 44.8**; the median of the frames' medians is
16.7. The target of the specification is a median of at most 6 on automatic output, so it is
**missed by a wide margin** (about three times).

What the numbers say:

- **Tone, not colour, is most of it.** Our render is brighter than the reference on every frame
  (mean L\* +7 to +36). The a\* and b\* casts are mostly a few units; frames 13 and 14 (about +10
  in a\*) and 15 stand out. The reference is darker and more contrasty; ours is washed out. The
  difference maps show the largest errors where the reference is dark (shadows, water, the dark
  parts of objects).
  Better automatic levels (black and white points, a tone curve towards the reference) are the
  first thing to try.
- **The frame detection of P-18 agrees with Trichroma Lab's crop.** The detected crop is within
  0.6 to 1.9 % of the capture of the registered reference crop on 14 of 15 frames. Frame 15 is 15 %
  off: our crop took more of the picture than the reference (the reference is cropped inside it).
- **The registration is trustworthy on these frames** (correlation 0.84 to 0.96), so the pairs are
  the same frames in the order assumed.

The pairs are private, so this is not a CI gate; the harness is for the author to re-run after a
change to the automatic conversion. `--maps` writes ours, the reference and the ΔE (white at 20)
side by side for each frame.

## The Frontier-style look, fitted to Trichroma Lab (P-20)

Trichroma Lab imitates the Fuji Frontier SP-3000, so matching its renders means matching a look,
not the scene. The Frontier look ([ADR 0009](../../decisions/0009-frontier-look.md)) is four
numbers: the slope and pivot of a sigmoid tone curve, a gamma after it, and a saturation, written
into the automatic recipe as the master curve and `color.saturation`. They are found by
`benchmark fit`: every candidate is rendered through the real pipeline on a small copy of each
frame, laid over the part the reference covers, and judged by the mean CIEDE2000 (each pixel capped
at 40) plus the size of our mean chroma minus the reference's, so that the saturation cannot be
used to hide a colour error. The search is Nelder-Mead. Ten frames (1, 2, 4, 5, 7, 8, 10, 11, 13
and 14) are fitted and five (3, 6, 9, 12 and 15) are held out and judged afterwards.

```text
benchmark fit --manifest manifest.json [--holdout 3] [--percentiles 0.5,99.5]...
benchmark report --manifest manifest.json --look frontier
```

**The levels.** The automatic black and white points are the 0.5 and 99.5 percentiles of the
density. Four settings were fitted (median of the frames' medians, held-out frames):

| Levels | Neutral | Frontier, fitted frames | Frontier, held out |
|---|---|---|---|
| 0.1 / 99.9 | 18.7 | 9.2 | 11.9 |
| 0.5 / 99.5 (today's) | 18.3 | 8.9 | 11.5 |
| 1 / 99 | 17.3 | 9.2 | 11.0 |
| 2 / 98 | 17.6 | 9.5 | 11.6 |

They are within a unit of each other, which is about the difference between two runs of the search,
so no setting clearly wins and **the levels stay at 0.5 / 99.5**.

**The look kept** (levels 0.5 / 99.5, saturation range widened to 0.2 to 1.8 after the first runs
put it at the old lower bound): slope 4.918, pivot 0.525, gamma 1.069, saturation 0.465. The low
saturation is not a desaturated look: the master curve works on each channel, so adding contrast
also adds chroma, and the saturation takes that back. The net effect is that our mean chroma
matches the reference's (within 3 on the held-out frames).

Result, per frame, with `benchmark report` (median ΔE2000; **held out** frames in bold):

| Frame | Neutral | Frontier | | Frame | Neutral | Frontier |
|---|---|---|---|---|---|---|
| 01 | 11.8 | 11.0 | | 09 | **19.1** | **11.5** |
| 02 | 13.5 | 10.0 | | 10 | 15.7 | 7.9 |
| 03 | **16.7** | **13.4** | | 11 | 14.5 | 7.1 |
| 04 | 18.1 | 8.7 | | 12 | **18.2** | **10.1** |
| 05 | 10.1 | 8.0 | | 13 | 34.9 | 8.5 |
| 06 | **15.8** | **13.2** | | 14 | 31.1 | 9.3 |
| 07 | 14.2 | 12.4 | | 15 | **28.3** | **19.6** |
| 08 | 27.7 | 9.3 | | | | |

All pixels of the 15 frames: **median ΔE 10.0 (17.2 neutral), p95 22.0 (44.8)**. On the fitted ten
the median of the frames' medians is 9.0; on the five held out it is **13.2** (neutral 18.2). The
registration also improves (0.90 to 0.99 against 0.84 to 0.96), as it should when the two pictures
look more alike.

**The target of 6 is missed.** The held-out median is 13.2, more than twice the target, and the
fitted frames reach 9.0. What is left:

- **Frame 15 stays far off** (19.6): a dusk sky with a strong colour gradient, where a global curve
  cannot follow the lab's rendition (its a\* and b\* are about 7 and 2 off in the mean, and the
  gradient is the error).
- **Frames 3, 6, 9 and 12** (the other held-out ones) are 10 to 13: brightness is close after the
  fit (mean L\* within 12), but their colour casts stay (frame 3's b\* is 11 off).
- **The held-out result moves by about a unit between runs of the search** (11.0 to 13.2 across the
  runs of the fit above), so differences of a unit between settings or between runs mean little.
  With ten fitted frames of one film, four numbers cannot capture what a lab's colour rendition
  does per hue and per density; a richer tone curve would lower the fitted number and the
  held-out one less.
- **The next step the numbers point to is colour**, not tone: an automatic colour balance
  (Q-09 found the same on the LS-50 scans).

## The flat field, and the Frontier look fitted again (P-21)

The P-20 numbers above were fitted on frames with the red edge cast of the lightbox. The bare-light
triplet of the same session (DSC_4602 to 4604, same camera settings as the film frames) became the
roll's flat field ([ADR 0010](../../decisions/0010-flat-field.md)); the manifest names it with
`"flat_field": [red, green, blue]` and the benchmark divides every frame by it with the app's own code.
The report has a new column, **edge cast**: how much redder (red minus green, in 8-bit steps) the
left and right eighth of the picture is than its middle half, ours minus the reference's.

| Neutral look, 15 frames | Without | With the flat field |
|---|---|---|
| Edge cast, left / right (frames 1 to 12) | +6 to +20 / +22 to +47 | -8 to +3 / -5 to +3 |
| Registration match | 0.84 to 0.96 | 0.87 to 0.99 |
| Median ΔE, all pixels (p95) | 17.2 (44.8) | 16.9 (44.5) |
| Film base | 0.148 0.098 0.065 | 0.190 0.105 0.075 |

The edge cast falls by more than half on 12 of the 15 frames (+6 to +47 to within ±8). Three do not
follow: frames 13 and 14 were -17/+14 and -12/+20 and become -28/-16 and -23/-9, and frame 15 goes
from +12/+25 to +10/+10. Their edges were different before (a bluer sky, or a sky gradient the lab
rendered its own way), so the light correction is not the whole of their edge colour.

The neutral look hardly moves in ΔE (16.9 against 17.2) because its error is tone, not the edges.
The Frontier look was fitted again on the corrected frames (levels 0.5 / 99.5, ten frames fitted and
five held out, as before):

| Frontier look | Slope | Pivot | Gamma | Saturation | Median ΔE, fitted ten | Held out five |
|---|---|---|---|---|---|---|
| Before (uncorrected frames) | 4.918 | 0.525 | 1.069 | 0.465 | 9.0 | 13.2 |
| After (flat-field corrected) | 5.719 | 0.495 | 1.284 | 0.562 | 7.7 | **10.8** |

All pixels of the 15 frames: **median ΔE 8.8, p95 23.3** (10.0 and 22.0 before; 17.2 and 44.8 neutral
and uncorrected). The held-out result is still above the target of 6. The refit also asked the other
levels settings: the held-out medians were 21.3 (0.1 / 99.9), 11.0 (0.5 / 99.5), 9.2 (1 / 99) and
10.3 (2 / 98), so 1 / 99 might do a little better, but the runs differ by about a unit and the levels
stay as they are. What is left is colour: frame 3's b\* is 12 off, frame 15's L\* is 23 off.
## Quality: LS-50 scans against Nikon Scan 4 (Q-09)

The author's LS-50 scans (16-bit RGB plus infrared, 3944 x 5952) have Nikon Scan 4's own positives
of the same frames (5959 x 3946, sRGB, embedded as "Nikon sRGB 4"). They lie a quarter turn
against the scans, so the report tries the reference in all eight orientations (four turns, with
and without a mirror) and uses the best; nine of the eleven that registered matched at 90 degrees clockwise and two (the portrait positives of Ektar 100 and Vision3 250D 0004, 3946 x 5959) at 0, none mirrored.

```text
benchmark manifest --scans <folder of scans> --references <folder of positives> --out manifest.json
benchmark report --manifest manifest.json [--maps <folder>]
```

A scan is matched to the positive named "<stock> Positive <number>" for the scan "<stock> <number>",
and the stock is its roll: the film base is measured per roll, as an import would, with the 35 mm
frame detection by rebate (no frame window). A reference must be a 16-bit RGB TIFF; the 16-bit
positives are single strips of 141 MB, which `read_tiff16_single_strip` reads.

**All twelve pairs were run, and eleven are in this table.** The positives of Vision3 250D (all four) and
Ektar 100 were rescanned as 16-bit by the author, because the report refuses an 8-bit reference (it stops
on a reference it cannot read). One pair, Vision3 500T 0002, does not register.

| Frame | Match | Crop gap | Median ΔE | p95 ΔE | Cast L\* | a\* | b\* |
|---|---|---|---|---|---|---|---|
| Ektacolor160 C41 0001 | 0.915 | 3.6 % | 19.9 | 37.8 | +7.5 | -15.6 | -11.7 |
| Ektacolor160 C41 0002 | 0.857 | 1.2 % | 24.0 | 42.9 | -1.4 | -24.2 | -13.2 |
| Ektacolor400 C41 0001 | 0.910 | 2.3 % | 22.9 | 35.4 | +8.7 | -17.3 | -10.1 |
| Ektacolor400 C41 0002 | 0.908 | 0.3 % | 22.6 | 34.0 | +9.4 | -17.0 | -10.1 |
| Ektacolor400 C41 0003 | 0.902 | 2.7 % | 26.2 | 37.0 | +11.0 | -17.6 | -13.4 |
| Ektar100 C41 0001 | 0.929 | 2.2 % | 18.7 | 24.4 | +9.2 | -8.6 | -9.7 |
| Vision3 250D ECN2 0001 | 0.877 | 2.0 % | 17.5 | 23.0 | -11.7 | -7.8 | -5.8 |
| Vision3 250D ECN2 0002 | 0.869 | 0.8 % | 19.1 | 31.0 | +11.7 | -7.6 | -5.6 |
| Vision3 250D ECN2 0003 | 0.870 | 2.3 % | 16.4 | 27.4 | +10.2 | -3.6 | -8.4 |
| Vision3 250D ECN2 0004 | 0.918 | 0.4 % | 9.5 | 21.7 | -3.6 | -7.5 | -4.3 |
| Vision3 500T 0001 | 0.885 | 2.8 % | 23.0 | 30.3 | +18.5 | -8.2 | +3.4 |
| Vision3 500T 0002 | 0.781 | 2.4 % | not compared | | | | |

Per stock, over all pixels of the compared frames: Ektacolor160 median 22.8 (p95 39.8), Ektacolor400
median 23.8 (35.8), Ektar 100 median 18.7 (24.4, one frame), **Vision3 250D median 15.3 (24.7, four frames)**,
Vision3 500T median 23.0 (30.3, one frame). Over all eleven compared frames: **median ΔE 19.0, p95
33.9** (median of the frames' medians 19.9), against 17.2 and 44.8 for the camera triplets of Q-02.
(The "edge cast" column of the report is for the camera triplets and the flat field; it means nothing
for scans, whose crop is not the same as Nikon Scan's.) The first run, on six frames, gave a median
of 23.2 and a p95 of 36.9; the run before the last positive was rescanned, on ten, 19.4 and 34.1.

What the numbers say:

- **Colour, not tone, is the main error here.** The brightness error is small (mean L\* -12 to +19,
  mostly under 12), but ours is greener and bluer than Nikon Scan's on every stock: **a\* about -4 to -9 on
  Ektar 100, the 250Ds and the 500T, and about -16 to -24 on the Ektacolor C41 frames (b\* about -10
  to -13)**. The automatic conversion applies no colour
  balance of its own; Nikon Scan balances the scene. A white balance step in the automatic recipe
  is what these numbers ask for first.
- **The crop agrees with Nikon Scan's frame** to within 0.3 to 3.6 % of the capture (Nikon Scan's
  positives cover nearly the whole scan, rebate included).
- **Vision3 500T 0002 registered below the 0.8 limit** (0.781): it is left out, as the report does
  for any pair it cannot be sure of. A low-contrast or differently cropped frame is the likely
  cause; the pair was not checked by eye.
- **The Vision3 250D is the closest** (frame 0004 at 9.5, the 250D roll at 15.3) and the Ektacolor
  frames the farthest (22.8 and 23.8); the offset is steady within a stock and between frames of it,
  which is what a missing white balance or a missing per-stock calibration looks like, not a scene
  effect (see P-22, which keeps this as a separate question).
- **The Vision3 500T base looks wrong.** It measured `0.92 0.85 0.62`, nothing like the C41 stocks'
  (`0.87 0.50 0.34` and `0.92 0.49 0.30`) or the 250D's (`0.99 0.38 0.23` in a separate check), and
  the frame's base confidence was only 0.69. A base that is too light in green and blue would explain
  the 500T frame's different cast (a\* -8, b\* +3). Pick the base of that roll by hand in Develop
  and re-run before reading anything into its numbers.

## Automatic colour balance (P-22)

Four settings of the report, one per estimator of [decision 0011](../../decisions/0011-colour-balance.md):

```text
benchmark report --manifest <file> --look neutral|frontier --balance off|grey_world|near_neutral|roll_median
```

`off` is the report as before. `grey_world` and `near_neutral` correct each frame by itself;
`roll_median` takes, per roll of the manifest, the median over its frames of their grey-world axes
(`roll_balance`, the function the app uses) and writes it into every frame of the roll. All write
the three colour axes of the automatic recipe. The manifests are the author's private ones:
`samples/private/reference/manifest-flat.json` (the 15 triplets, flat field applied) and
`manifest-ls50-all.json` (the LS-50 scans).

**Compared frames.** The LS-50 report with the Neutral look compares 11 of the 12 pairs; with the
Frontier look it compares 9 of 12, and they are not the same 9 as the 11 (a pair that does not
register under one look can under the other). **The Neutral and Frontier LS-50 rows are not
comparable with each other.** Compare methods only within a row.

Median ΔE2000 / 95th percentile:

| Set and look | off | grey_world | near_neutral | roll_median |
|---|---|---|---|---|
| Triplets, Neutral (15) | 16.9 / 44.5 | 21.7 / 47.5 | 21.3 / 50.9 | 18.7 / 44.5 |
| Triplets, Frontier (15) | 8.8 / 23.3 | 15.1 / 33.6 | 15.0 / 34.9 | 13.0 / 25.4 |
| LS-50, Neutral (11 of 12) | 19.0 / 33.9 | 16.3 / 29.3 | 16.6 / 30.7 | 16.0 / 27.0 |
| LS-50, Frontier (9 of 12) | 23.2 / 46.7 | 22.8 / 48.1 | 22.6 / 47.9 | 22.8 / 48.3 |

Triplets, mean cast size sqrt(a\*^2 + b\*^2) of ours minus the reference's, on the five frames held out
by the P-20 split (3, 6, 9, 12, 15), and on the other ten after the slash (the change of the
held-out cast against `off` in brackets):

| Look | off | grey_world | near_neutral | roll_median |
|---|---|---|---|---|
| Neutral | 7.053 / 5.652 | 14.463 / 13.891 (+105.1 %) | 15.204 / 14.315 (+115.6 %) | 12.982 / 9.277 (+84.1 %) |
| Frontier | 6.381 / 4.582 | 15.907 / 16.225 (+149.3 %) | 17.126 / 15.317 (+168.4 %) | 14.879 / 10.061 (+133.2 %) |

Every estimator raised the cast of the triplets, held out or not, and their ΔE. The triplets stay
uncorrected.

LS-50, mean cast size per stock (n frames in brackets):

| Look and method | Ektacolor160 | Ektacolor400 | Ektar100 | Vision3 250D | Vision3 500T |
|---|---|---|---|---|---|
| Neutral (2, 3, 1, 4, 1): off | 23.533 | 20.642 | 12.963 | 9.236 | 8.877 |
| grey_world | 2.758 | 11.845 | 8.273 | 10.257 (rises) | 6.597 |
| near_neutral | 3.695 | 10.266 | 8.246 | 10.958 (rises) | 5.882 |
| roll_median | 4.393 | 12.069 | 8.273 | 7.446 | 8.001 |
| Frontier (1, 3, 1, 2, 2): off | 14.375 | 16.356 | 11.424 | 8.880 | 9.620 |
| grey_world | 3.842 | 13.433 | 9.795 | 8.092 | 10.209 (rises) |
| near_neutral | 3.635 | 12.430 | 9.614 | 7.924 | 9.288 |
| roll_median | 2.648 | 13.048 | 9.795 | 8.268 | 10.211 (rises) |

Frames that are worse than with `off` by more than 2 ΔE, LS-50 Neutral: `grey_world` Vision3 250D
0004 (9.5 to 13.1), `near_neutral` the same frame (9.5 to 18.8), `roll_median` none. LS-50 Frontier:
none for any method. These roll medians include all frames of the roll.

What the numbers say, and do not say: `roll_median` is the only method that lowers the cast of every
stock with the Neutral look and moves no frame the wrong way by more than 2 ΔE; it is what the app
applies, to scanner frames only. The Frontier 95th percentile of `roll_median` (48.3) is above `off`
(46.7) on 9 frames. The sample is 11 frames of 5 stocks and one reference tool per set, and Nikon Scan
and Trichroma Lab are references, not ground truth: that all estimators fail on the triplets says their
reference is not a neutral render of the scene, not that balance is impossible.

## Speed: the criterion benches

`cargo bench -p gtp-app` (21 MP frames are synthetic negatives of 6016 x 4016, a D600's size; the
dust stage is off):

| Step | Time | Budget (section 11) |
|---|---|---|
| Decode a 21 MP TIFF to f32 | 117 ms | part of "open a frame" |
| Build the proxy from 21 MP | 106 ms | part of "open a frame" |
| Open a frame in Develop (the two above) | about 0.22 s | <= 1.5 s |
| Preview render, 1600 px from a 2048 px proxy | 55 ms | <= 100 ms |
| Automatic analysis on a proxy | 84 ms | <= 500 ms |
| Analysis of a 21 MP frame, proxy included | 147 ms | (500 ms on the proxy) |
| Full-resolution render of a 21 MP frame | 480 ms | the core of the export, <= 10 s |

All are inside their budgets with wide margins on this machine. What is not measured here: the
export's encode and write (TIFF 16 and the colour conversion), dust removal at full resolution,
and the NEF decode; the last is in [nef-decode.md](nef-decode.md) (0.36 to 0.46 s per triplet
merge, 4.8 to 5.0 s per imported frame). A 4-core laptop will be slower than this 6-core desktop, so
before release these should be re-run on the reference machine.
