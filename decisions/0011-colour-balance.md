# 0011. One colour balance per roll of scanner frames, the median of their mid-tone estimates

- Status: Accepted (the author approved the design, issue P-22 / GitHub #204)
- Date: 2026-11-05

## Context

The automatic recipe set no colour balance, so what remains against the reference renders is colour:
the 15 camera triplets drift from frame to frame, and the LS-50 scans sit a steady a\* -17 and b\* -10
to -13 from Nikon Scan. Issue P-22 asked for an automatic step that finds the neutral of a frame or a
roll and writes it into the existing colour axes (red-cyan, green-magenta, blue-yellow), chosen by
measuring estimators on the benchmark (`benchmark report --balance ...`, see
[benchmark.md](../docs/research/benchmark.md)).

Three estimators were built in `gtp-edit` (`balance.rs`). Each looks at the inverted frame, before the
colour stage, and returns the `log2` level of what should be grey, turned into the three axes with the
arithmetic of `neutral_balance`:

1. **Grey world.** The geometric mean of each channel over the mid-tone pixels (luma 0.045 to 0.54 in
   linear light). Assumes the scene averages to grey.
2. **Near neutral.** The grey-world estimate, then only the pixels within 0.5 stop of it, three times
   over. It is the second estimator of the issue (the least saturated pixels), implemented with the
   distance from the current estimate, so saturation is measured around the cast and not around grey.
3. **Roll median.** One correction per roll: the median, per axis, over the roll's frames of their
   grey-world axes (`roll_balance`). Stored like the film base.

A frame cannot be judged when it has fewer than 500 mid-tone pixels, when too few are near neutral
(estimator 2), or when its estimate needs more than 0.6 of the slider range. It then gets `None`.

## Measured

Median ΔE2000 / 95th percentile against the references, `--look neutral` and `--look frontier`.
Triplets are the 15 flat-field corrected frames (`manifest-flat.json`); the LS-50 set is
`manifest-ls50-all.json`.

| Set and look | off | grey_world | near_neutral | roll_median |
|---|---|---|---|---|
| Triplets, Neutral | 16.9 / 44.5 | 21.7 / 47.5 | 21.3 / 50.9 | 18.7 / 44.5 |
| Triplets, Frontier | 8.8 / 23.3 | 15.1 / 33.6 | 15.0 / 34.9 | 13.0 / 25.4 |
| LS-50, Neutral (11 of 12 compared) | 19.0 / 33.9 | 16.3 / 29.3 | 16.6 / 30.7 | 16.0 / 27.0 |
| LS-50, Frontier (9 of 12 compared) | 23.2 / 46.7 | 22.8 / 48.1 | 22.6 / 47.9 | 22.8 / 48.3 |

The LS-50 Frontier row compares 9 frames, not the 11 of the Neutral row (the frames that register
differ), so the two LS-50 rows are not comparable with each other.

Mean colour cast, the size of our mean (a\*, b\*) minus the reference's, on the five triplets held out
by the P-20 split (frames 3, 6, 9, 12, 15), with the other ten after the slash:

| Look | off | grey_world | near_neutral | roll_median |
|---|---|---|---|---|
| Neutral | 7.053 / 5.652 | 14.463 / 13.891 (+105.1 %) | 15.204 / 14.315 (+115.6 %) | 12.982 / 9.277 (+84.1 %) |
| Frontier | 6.381 / 4.582 | 15.907 / 16.225 (+149.3 %) | 17.126 / 15.317 (+168.4 %) | 14.879 / 10.061 (+133.2 %) |

Mean cast per stock on the LS-50 scans with the Neutral look (Ektacolor160 n=2, Ektacolor400 n=3,
Ektar100 n=1, Vision3 250D n=4, Vision3 500T n=1):

| Method | Ektacolor160 | Ektacolor400 | Ektar100 | 250D | 500T |
|---|---|---|---|---|---|
| off | 23.533 | 20.642 | 12.963 | 9.236 | 8.877 |
| grey_world | 2.758 | 11.845 | 8.273 | 10.257 (rises) | 6.597 |
| near_neutral | 3.695 | 10.266 | 8.246 | 10.958 (rises) | 5.882 |
| roll_median | 4.393 | 12.069 | 8.273 | 7.446 | 8.001 |

With the Frontier look (n 1, 3, 1, 2, 2): off 14.375, 16.356, 11.424, 8.880, 9.620; grey_world 3.842,
13.433, 9.795, 8.092, 10.209 (500T rises); near_neutral 3.635, 12.430, 9.614, 7.924, 9.288; roll_median
2.648, 13.048, 9.795, 8.268, 10.211 (500T rises).

Frames that got worse than with no balance by more than 2 ΔE, LS-50 Neutral: grey_world 250D 0004
(9.5 to 13.1), near_neutral 250D 0004 (9.5 to 18.8), roll_median none. With the Frontier look none for
any method. The roll medians include all frames of the roll.

## Decision

1. **The app applies one colour balance per roll, the median of the frames' mid-tone grey-world
   axes, for scanner sources only** (`ImportKind::TiffScan` imports and the scanner job, that is
   `FrameKind::ScanRgbIr`). Camera RGB triplets are not corrected.
2. **Why the roll median.** On the LS-50 scans it is the only method that lowers the mean cast on
   every stock with the Neutral look (grey world and near neutral raise the 250D) and the only one
   that moves no frame more than 2 ΔE the wrong way; it also has the lowest median and 95th percentile
   ΔE there (19.0 / 33.9 to 16.0 / 27.0). One value per roll cannot follow one odd picture.
3. **Why scans only.** Every estimator raised the held-out cast of the triplets (+84 % at best) and
   their median ΔE (16.9 to 18.7 for the best, Neutral). The triplets' failure says that the
   references (Trichroma Lab for the triplets, Nikon Scan for the scans) are not neutral renders of
   the scene, so a mid-tone grey world is not what they aim at; it does not say that colour balance is
   impossible for a camera. Nothing here argues for correcting triplets, so they stay off.
4. **It goes through the existing axes.** The balance is written into `red_cyan`, `green_magenta` and
   `blue_yellow` of the automatic recipe, so Develop shows it and the user can move or undo it. The
   recipe schema does not change.
5. **Storage.** A nullable balance (three axes) on the roll, migration `0008_roll_color_balance.sql`,
   modelled on the film base: the first one stored stays. `update_roll` does not write it, so editing
   a roll's details cannot lose it.
6. **When it is made.**
   - Each new scanner frame estimates its balance when its starting recipe is made.
   - A roll that has a balance gives it to a new frame at once.
   - A roll without one stores, when the job ends, the median over the job's frames that could be
     judged, writes it into the recipes of the job's frames the user has not edited (`edited_at`
     empty; the write does not count as an edit) and has their thumbnails made again. A frame that
     could not be judged does not contribute but still gets the roll's correction. A job that is
     cancelled or fails settles the same way for the frames it stored, so frames and roll agree; the
     thumbnails of those frames are then dropped and rebuilt from the recipe when asked for.
   - A job in which no frame could be judged stores no balance and the result carries the warning
     `BalanceSkipped` (on the job's first frame, because a warning names a frame).
7. **Develop's automatic recipe** for a scanner frame uses the roll's stored balance, so running auto
   again does not drop it; so do the export and thumbnail fallbacks for a frame that has no saved
   recipe, through the shared `with_frame_balance`. Triplet frames and the scanner preview stay off.
8. **The criteria of P-22 are amended** to what was measured: the LS-50 scans, not the triplets, are
   what the correction has to improve (see the issue). Criterion 2 as first written (halve the held-out
   triplet cast) was met by no estimator.

## Consequences

- **A roll of one frame** gets that frame's own estimate as its median (Ektar 100 in the LS-50 set),
  so there is no averaging and nothing protects it from an odd picture.
- **The median is known only at the end of a job.** Frames are first stored with the axes at 0 and
  rewritten when the job ends, and their thumbnails are replaced then. A crash in between leaves the
  frames uncorrected (as before this change) and the roll without a balance; the next job decides.
- **The 500T rises under Frontier** (9.620 to 10.211 with the roll median): its base measured badly in
  Q-09, and the roll has one frame (Neutral) or two (Frontier) to take a median over.
- **Small samples.** 11 frames of 5 stocks for the scans, 15 frames of one film for the triplets, one
  reference tool per set. The ordering of the methods on the scans is a measurement on that sample.
- **The references are not ground truth.** Nikon Scan 4 and Trichroma Lab render with their own
  choices; matching them is the benchmark's definition of success here, not a proof of neutrality.
- **The steady LS-50 offset is not removed** (Ektacolor400 still has a cast of 12.1 with the Neutral
  look). If it remains a concern it is a per-source calibration, its own issue.
- **No way yet to clear or set a roll's balance** except by editing frames; the catalog stores it and
  the first one stays. A control for it would be UI work and is not part of this change.
- **Cost:** one estimate on the existing preview proxy per scanner frame while the roll has no
  balance; nothing once it has one.
