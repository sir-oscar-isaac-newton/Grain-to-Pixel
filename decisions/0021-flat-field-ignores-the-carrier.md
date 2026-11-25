# 0021. The flat field leaves the film carrier out of its cell means

- Status: Accepted (issue P-35 / GitHub #369)
- Date: 2026-11-24

It supersedes rule 2 of ADR 0010 for how the cells are measured. The grid, the centre and the
floor of that ADR stay.

## Context

ADR 0010 takes the mean of every sample in a cell. The bare-light capture of the author's rig
shows the film carrier at its edges: an opaque border round the opening, with a soft edge about
18 pixels wide. A cell that holds the edge of the opening holds part carrier and part light, so its
mean is too low and the flat field gains that cell too much. The frames then show a bright band
along the inside of the opening, wider than the cell.

On the real capture (Nikon D600, 6016 x 4016, a grid of 96 x 65 cells) cells next to the opening
have between 0.49 and 1.0 of the light the new method gives them (see the figures below). Half of
the samples in the band were more than 3 % off after the correction.

## Options

- **A. Today.** Every sample counts. Simple, and wrong in the band.
- **B. A threshold against the centre, a margin, an open share and a ring fill (chosen).** A sample
  is carrier when it is dark against the centre light. Samples near a carrier sample are left out
  too. A cell with too few open samples is filled from its open neighbours. One pass over the
  samples, no new data kept, and the result is the same grid as before.
- **C. Detect a sharp edge inside a cell.** Needs an edge detector that tells a carrier edge from
  dust or a strong vignette. More code and more ways to be wrong, for no gain in the grid.
- **D. A full-resolution mask stored with the field.** Would also correct the pixels in the soft
  edge, but the stored field would grow from a small grid to a picture, the apply step would
  change, and the catalog would need a migration. The pixels in the carrier are not wanted in the
  frame anyway.

## Decision

Option B, in `build_flat_field` only. `apply_flat_field` does not change.

1. **The centre level** of each channel is the mean over the centre 3 x 3 cells with every sample
   counted, as before. A dark or non-finite centre is refused as before.
2. **Samples** are the same as before: every `SAMPLE_STEP` (3) pixels from the start of each cell.
3. **A sample is covered** when its brightest channel, as a share of that channel's centre level,
   is below `CARRIER_LEVEL` = 0.25. Samples that are not finite are skipped as before.
4. **The margin.** Open samples within `CARRIER_MARGIN_SAMPLES` = 4 samples (about 12 pixels,
   Chebyshev distance on the map of samples) of a covered sample are left out too. It is a
   separable maximum filter (a pass along the rows with prefix counts, then one along the columns),
   in parallel over rows, with the same result on any number of threads. On the full frame it is
   about 2005 x 1339 samples.
5. **A cell** is the per-channel mean of its remaining samples, summed in the old order. A cell with
   less than `MIN_OPEN_SHARE` = 0.25 of its samples open is covered.
6. **The fill** goes ring by ring. In each pass every unfilled cell with at least one filled
   8-neighbour (open, or filled in an earlier pass) takes the per-channel mean of those neighbours.
   The pass reads only what was filled before it, so the order of the cells does not matter. It
   repeats until every cell is filled. Edges and corners of the frame need no special case: the ring
   reaches them from whichever side has light. If no cell is open, or one of the centre 3 x 3
   cells is covered, the capture is refused with a message that says so.
7. **The rest is as before:** divide by the centre level, floor at `FlatField::MIN_LIGHT` = 0.05
   (ADR 0010).

### A capture with no carrier

When no sample is covered nothing is left out, every cell is open, and the result is the same
numbers as the plain method, bit for bit (tolerance 0): the same samples, the same summation order
in `f64`, the same division. A test keeps the old build as a reference in the test code and
compares the grids bit for bit on several inputs, among them a vignetting that falls to 0.3 of the
centre, sizes that do not divide into the grid, and a capture with samples that are not numbers.

### Light truly below the level, without a carrier

A region that is really dark, below a quarter of the centre light, is treated as covered and
filled, as a carrier would be. The real capture does not come near it: its darkest open cell has
0.4979 of the centre light in its dimmest channel and 0.6712 in its brightest, which is the figure
the level is compared with. That cell may hold shadow of the carrier as well as vignetting, so real
vignetting is no darker. The level 0.25 is a factor 2.7 below it.

### The rebuild action

Flat fields stored before this change were built without it, and stay as built until the owner
asks. `flat_field_rebuild(roll)` (button `Rebuild flat field` in the roll's actions) merges the
three kept files again and builds the field with the current method. Sources, exposure, kept
files and date are unchanged, the kept files are not removed (the list stored is the same one),
and the roll's proxies, film base and thumbnails follow as for any flat-field change. It is
refused, with nothing changed, when the roll has no flat field, when the flat field was made from
files that were only read and not kept (an imported one: it says so and the owner sets it again
from the files), when it does not have exactly three kept files, or when a kept file is gone (all
three are looked for before anything is written).

## Measured

From the ignored test `crates/gtp-app/src/services/flat_field/measure.rs` on the real bare-light
capture `Barelight RGB Capture` (three NEFs, `DSC_4602` to `DSC_4604`), release build. The capture
does show the carrier.

```
grid: 96 x 65 cells
covered cells: 650 of 6240
covered samples: 291384 of 2751840 (10.5887 %), left out with the margin: 302709 (11.0002 %)
open cells: light min 0.4979 max 1.0086; darkest brightest-channel light 0.6712 (carrier level 0.25)
band: 260 cells within 2 cells inside the opening edge
  channel 0: old/new mean 0.9216 min 0.4895 max 1.0000
  channel 1: old/new mean 0.9246 min 0.4939 max 1.0000
  channel 2: old/new mean 0.9232 min 0.4929 max 1.0000
  max relative difference old against new: 0.5105
evenness of the bare light divided by its old flat field, in the band, kept samples (mean, 99th percentile, share beyond 3 %, max deviation from the centre): R 0.0859 0.4559 0.4870 0.6684, G 0.0831 0.4312 0.4895 0.6739, B 0.0849 0.4404 0.4963 0.6668
evenness of the bare light divided by its new flat field, in the band, kept samples (mean, 99th percentile, share beyond 3 %, max deviation from the centre): R 0.0063 0.0481 0.0241 0.5687, G 0.0060 0.0701 0.0330 0.5688, B 0.0059 0.0716 0.0340 0.5463
build time, median of 5: old 24.0 ms, new 8.1 ms
```

The evenness is taken over the samples of the band cells that the new method kept, so the carrier
itself is not in it, nor are the samples of the margin. After the new flat field the mean deviation
in the band is under 1 %, against 8 % to 9 % before, and the share of samples beyond 3 % falls from
about 49 % to 2 % to 3 %.

**By distance from the carrier**, green, the mean over all samples at that distance (one sample is
about 3 pixels), as a share of the centre. Nothing is left out here, so it also shows the margin.

| Samples from the carrier | Bare light | Divided by the old field | Margin 4 (chosen) | Margin 8 | Margin 16 |
|---|---|---|---|---|---|
| 1 | 0.310 | 0.731 | 0.409 | 0.406 | 0.401 |
| 3 | 0.548 | 1.144 | 0.721 | 0.715 | 0.706 |
| 5 | 0.722 | 1.357 | 0.948 | 0.940 | 0.929 |
| 6 | 0.750 | 1.346 | 0.984 | 0.977 | 0.965 |
| 7 | 0.756 | 1.301 | 0.993 | 0.985 | 0.973 |
| 8 | 0.759 | 1.255 | 0.996 | 0.989 | 0.977 |
| 12 | 0.764 | 1.138 | 1.001 | 0.995 | 0.984 |
| 20 | 0.772 | 1.056 | 1.002 | 0.998 | 0.992 |
| 40 | 0.787 | 1.000 | 1.000 | 1.000 | 1.000 |

The old field makes the light up to 36 % too bright, out to about 24 samples (72 pixels). The new
one is even within 1 % from 7 samples on, on the mean. Nearer than that the carrier's own soft
shadow is left in the picture: the flat field is not meant to lift it.

**What stays off.** Away from the carrier (over 12 samples) the samples more than 5 % off lie in
two strips, along the top of the picture (x from about 250 to 2250) and in its last rows at the
bottom (x from about 4750 to 5750). The new field leaves them as low as 0.55. They are probably
the carrier's shadow where its edge is just outside the picture: the light falls there without
reaching the level, so nothing marks it as carrier. This was not looked into further.

**Sensitivity**, on the same capture: covered cells, covered share of samples, share left out with
the margin, the largest relative difference old against new in the band, and the darkest open cell.

| Level | Margin | Covered cells | Covered | Left out | Band max difference | Darkest open |
|---|---|---|---|---|---|---|
| 0.15 | 2 | 650 | 10.1527 % | 10.6356 % | 0.4771 | 0.4433 |
| 0.15 | 4 | 650 | 10.1527 % | 10.8566 % | 0.5037 | 0.4893 |
| 0.15 | 8 | 653 | 10.1527 % | 11.2835 % | 0.4122 | 0.4988 |
| 0.25 | 2 | 650 | 10.5887 % | 10.7950 % | 0.4925 | 0.4894 |
| **0.25** | **4** | 650 | 10.5887 % | 11.0002 % | 0.5105 | 0.4979 |
| 0.25 | 8 | 661 | 10.5887 % | 11.4090 % | 0.3745 | 0.5043 |
| 0.35 | 2 | 650 | 10.6765 % | 10.8812 % | 0.5045 | 0.5007 |
| 0.35 | 4 | 651 | 10.6765 % | 11.0849 % | 0.4291 | 0.5073 |
| 0.35 | 8 | 668 | 10.6765 % | 11.4910 % | 0.3362 | 0.5131 |

The covered share hardly moves between 0.15 and 0.35, so the level is not critical. The margin matters a little at the cell level (650 cells at 2 and 4, 661 at 8).
The start values are kept. The level has a wide gap above it. A margin of 4 samples (12 pixels) does
not clear the whole soft edge (about 6 samples), but wider margins are no better: the fill does not
follow the vignette, so with them the corrected light just inside the edge reads 1 % to 3 % low.

**Cost.** The build takes 8 ms against 24 ms: the new method reads each sample once for all three
channels. Nothing is added to the apply step.

## Consequences

- The bright band along the opening goes. The last 6 samples or so (18 pixels) stay darker, as they
  are in the light. The flat field no longer depends on how much of a cell the
  carrier covers.
- A frame is still corrected where the carrier is (the filled values follow the open light), so
  nothing there is divided by the floor.
- The fill is the nearest open light, so in a cell that was covered the gradient of the vignette
  is not followed. In the synthetic tests the error stays under 3 % to 4 % at the edge of the
  opening, and the real capture shows 2 % to 3 % of the kept samples beyond 3 %.
- Old flat fields stay as built until `Rebuild flat field`. An imported one, made from files that
  were only read, cannot be rebuilt: it must be set again from its files.
- The rebuild needs the three kept files in the library. A copy of a flat field (H-26) copies them
  too, so a copy can be rebuilt.

### What is not checked

- A capture with a different carrier (another rig, a carrier that is not dark) was not available:
  only one real capture, from one rig, was measured.
- The result on frames is judged by the figures above and by the synthetic tests, not by eye on
  a corrected scan.
- The ignored measurement is not part of the test run.
