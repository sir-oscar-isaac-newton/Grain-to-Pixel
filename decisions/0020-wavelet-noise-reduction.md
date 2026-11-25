# 0020. Wavelet shrinkage replaces the guided filter for the luminance of noise reduction

- Status: Accepted (issue P-29 / GitHub #315)
- Date: 2026-11-24

## Context

ADR 0013 cleaned the lightness of a picture with a guided filter and left wavelet shrinkage for a
later issue. The guided filter smooths flat areas well but treats fine grain and coarse grain
alike, so a strong setting leaves a picture clean and a little plastic. Issue P-29 asks for the
method that keeps grain texture: split the picture into scales of detail and reduce each by its
own amount, so coarse blotches can go while fine grain stays. It asks for an ADR that compares the
two on real scans of at least three film stocks, says what each keeps and removes, gives the cost
on a 24 megapixel frame, and decides whether the wavelet replaces the guided filter or is offered
beside it.

Only the lightness is in question. The colour planes keep the guided filter (ADR 0013): colour
speckle should be smoothed strongly, and the guide of the cleaned lightness keeps colour edges.

## Options

### The wavelet method

The a trous (undecimated) transform of the Oklab lightness `L`, five scales.

- Smoothing `c_0 = L`. `c_(j+1)` is `c_j` filtered along the rows and then the columns with the B3
  spline kernel `[1, 4, 6, 4, 1] / 16` whose taps are `2^j` pixels apart. Borders are mirrored.
- Detail `d_j = c_j - c_(j+1)` for `j = 0` to 4. Scale `j` holds features of about `2^j` to
  `2^(j+1)` pixels of the full scan. The coarse residual is `c_5`.
- Soft threshold per scale: `d'_j = sign(d_j) max(|d_j| - T_j, 0)`.
- `T_j = luminance / 100 * NR_WAVELET_STRENGTH * NR_WAVELET_SHARES[j] * sigma_j`.
- `sigma_j = median(|d_j|) / 0.6745`, measured on the picture being rendered, over 48 patches of
  64 x 64 pixels on a fixed grid of 8 by 6 (`NR_WAVELET_PATCH_PX`, `NR_WAVELET_PATCHES_ACROSS`,
  `NR_WAVELET_PATCHES_DOWN`). The grid depends only on the picture size, never on the number of
  threads.
- `L' = c_5 + sum d'_j`.

Constants, all `pub const` in `gtp-edit`, to be tuned in Q-11:

| Constant | Value | Meaning |
|---|---|---|
| `NR_WAVELET_LEVELS` | 5 | Scales at full size. |
| `NR_WAVELET_SHARES` | 0.3, 0.8, 1, 1, 1 | Share of the noise level each scale removes. The finest scale has a small share, so fine grain stays. |
| `NR_WAVELET_STRENGTH` | 0.9 | Threshold in units of the noise level, at amount 100 and share 1. Chosen by the owner, see "Choice of the strength". |

**Preview and export.** `sigma_j` is measured on the picture being rendered, so a preview gets its
own, lower noise level. The scales are therefore mapped by physical size. At render scale `s`
(1 at export, below 1 for a preview) scale `j` covers `2^j / s` pixels of the full scan. The first
scale of the render takes the share of the full-size scale of that size, `round(-log2 s)` (0 to 4),
and the render uses `5 - round(-log2 s)` scales, at least 1. The coarsest scale stays about the
same size. At `s = 0.5` the four scales use the shares 0.8, 1, 1, 1.

**Cost design.** The picture is cut into tiles of 256 x 128 pixels. Each tile is copied with a
border of 62 pixels (the reach of the five smoothings) taken from the picture mirrored at its
edges, and all five scales are computed inside the tile, in cache. Mirroring once at the border
gives the same result as mirroring at every scale, because the kernel is symmetric. The picture is
read about once and the result written once. A first version made one pass over the whole picture
per scale and took 200 to 260 ms for the luminance at 24 MP, slower than the guided filter,
because those passes are limited by memory. The tiled version is the one measured below. A test
compares it with a plain plane-by-plane version (`noise/reference.rs`) on sizes from 1 x 1 to
300 x 270 and on five scales; the largest difference is under 2e-5.

### The guided filter of P-27 (the baseline)

`L' = guided(p = L, I = L, r = 3, eps = (luminance / 100 * 0.04 * s)^2)` (ADR 0013). In the figures
below it is run at scale 1 on full-size crops. Its constants are kept in the test code
(`noise/measure.rs`) and no longer in the library.

### The measures

The finest scale is the grain texture, and scales 2 to 4 together are the coarse blotches. Two
measures are used, so that one can check the other.

- **The a trous measure.** The lightness is analysed with the five scales above, without
  thresholding. The energy of a scale is the mean of its squared detail over the picture without a
  border of 64 pixels. After a reduction, the **kept** figure of a scale is its energy over the
  energy of the input at the same scale. "Fine kept" is scale 0 and "coarse kept" is scales 2, 3
  and 4 together.
- **The box measure.** It is independent of the wavelet. The fine band is `L - box(L)` with a 3 x 3
  box (features of about 1 to 3 pixels). The coarse band is the 3 x 3 box against a 25 x 25 box
  (features of about 3 to 25 pixels). The energy is taken as above and the kept figures are the
  same ratios. It answers the concern that the a trous measure favours the wavelet, since the
  wavelet thresholds the very scales that this measure counts.

The two agree on the fine figure to about 0.02. On the coarse figure the box measure shows the
guided filter removing more than the a trous measure does (0.71 against 0.89 on Ektar 100 at amount
100), and the wavelet about the same as the a trous measure (0.74 against 0.77 on Ektar 100). So the
a trous measure does not favour the wavelet, it understates the guided filter a little. The wavelet
still takes far more coarse energy than the guided filter on 500T and on the pushed stand-in, and
keeps far more fine grain on every stock, by both measures.

Further figures used:

- **Equal coarse reduction.** The coarse figure that the guided filter reaches at amount 100 is the
  reference. The measure is the finest figure each method keeps when it has removed that much
  coarse energy, and the amount at which the wavelet reaches it.
- **Coarse halves.** The amount at which the coarse energy falls to a half, and the fine figure kept
  there. The measure first planned was this one. It can not be read for the guided filter, which
  never takes the coarse energy under 0.71 up to amount 100 on any stock, by either measure.

Most of the coarse energy of a real frame is picture detail, not grain, and neither method should
remove all of it.

### What was measured

Full-size renders (the picture before the noise stage, linear Rec.2020, the automatic recipe) of
the first frame of three scans from `samples/private` (git-ignored, never copied into the
repository): Ektar 100 (fine grain), Vision3 250D (middle) and Vision3 500T (coarse). Four crops of
1024 x 1024 of each: the flattest region found on a grid of 256 pixels (sky or wall, by the sum of
neighbour differences of the cube root of the luminance) and three fixed ones at 25 %, 50 % and
75 % of the way across. Energies are summed over the four crops. The renders are 3944 x 5681,
3944 x 5673 and 3944 x 5680 pixels. The figures come from ignored tests that stay in the tree:
`measure_noise_crops_of_real_scans` in `crates/gtp-app/src/pipeline/measure_noise.rs` writes the
crops to a directory outside the repository, and the `measure_` tests in
`crates/gtp-edit/src/noise/measure.rs` read them. Run them with `--ignored --nocapture`.

**Pushed.** No pushed scan exists. The stand-in is the 500T crops plus seeded synthetic grain: white
noise plus the same smoothed by the first two, three and four scales of the a trous cascade
(blotches of about 4, 8 and 16 pixels), four layers of equal deviation, scaled so that the grain's
energy in scales 2 to 4 equals the crop's own. The coarse energy therefore doubles. The added
grain also carries a large white component, so the finest scale rises from 212 to 1746 (units of
1e-6). It is a stand-in for a thin or pushed frame, not a pushed scan, and the Q-11 comparison on
real pushed frames should replace it.

### Choice of the strength

The first strength was 0.3. On real scans it removed little: 0.80 to 0.91 of the coarse energy was
left at amount 100, and the slider did not clearly remove blotches.

The owner approved the design and chose the strength on 2026-11-24, after seeing measurements at
0.3, 0.6, 0.9 and 1.2 on the same crops, the pushed stand-in and synthetic patches. The shares
stay 0.3, 0.8, 1, 1, 1. The ignored tests `measure_candidate_strengths_on_real_crops` and
`measure_candidate_strengths_on_synthetic_patches` in `noise/measure.rs` produce the table.

Fine kept and coarse kept at amount 100 (a trous measure, then the box measure in brackets for the
coarse figure):

| Stock | Guided fine / coarse | 0.3 | 0.6 | 0.9 | 1.2 |
|---|---|---|---|---|---|
| Ektar 100 | 0.124 / 0.889 (0.710) | 0.891 / 0.914 (0.902) | 0.798 / 0.836 (0.816) | 0.718 / 0.766 (0.741) | 0.648 / 0.703 (0.675) |
| Vision3 250D | 0.363 / 0.889 (0.725) | 0.929 / 0.862 (0.841) | 0.868 / 0.744 (0.706) | 0.813 / 0.643 (0.592) | 0.763 / 0.558 (0.496) |
| Vision3 500T | 0.453 / 0.920 (0.795) | 0.928 / 0.799 (0.796) | 0.865 / 0.638 (0.634) | 0.809 / 0.510 (0.506) | 0.759 / 0.410 (0.405) |
| Pushed stand-in | 0.532 / 0.930 (0.836) | 0.850 / 0.672 (0.659) | 0.719 / 0.437 (0.419) | 0.604 / 0.282 (0.260) | 0.505 / 0.185 (0.159) |

The figures at 0.9 for amounts 25, 50 and 75 are in the results below.

Amount at which the coarse energy halves (a trous measure; the fine figure kept there):

| Stock | Guided | 0.3 | 0.6 | 0.9 | 1.2 |
|---|---|---|---|---|---|
| Ektar 100 | not reached | not reached | not reached | not reached (about 280) | not reached |
| Vision3 250D | not reached | not reached | not reached | not reached (about 160) | not reached (box: 98.8, 0.788) |
| Vision3 500T | not reached | not reached | not reached | about 103 (0.804) | 77.5 (0.804) |
| Pushed stand-in | not reached | not reached | 84.8 (0.757) | 56.7 (0.757) | 42.6 (0.756) |

White noise, a flat patch of 256 x 192 with only white noise, amount 100: the deviation kept is
0.120 for the guided filter, and 0.910, 0.825, 0.747 and 0.676 at 0.3, 0.6, 0.9 and 1.2.

Step edge, 10 to 90 percent rise width in pixels, input 0.80, amount 100: guided 0.84, and 0.82,
0.85, 0.88 and 0.91 at 0.3, 0.6, 0.9 and 1.2. At amount 50: guided 0.81, and 0.81, 0.82, 0.84 and
0.85.

Flat grain patch (white noise plus blotches, the synthetic one) at amount 100, fine and coarse
kept by the a trous measure: guided 0.003 and 0.677, and 0.847 and 0.633, 0.714 and 0.376, 0.597
and 0.214, 0.496 and 0.120 at 0.3, 0.6, 0.9 and 1.2.

**Why 0.9.** It is the lowest of the candidates that clearly reduces the coarse noise at amount 100
on the coarse stocks while it keeps most of the fine grain. At 0.9 the coarse energy of 500T falls
to a half at about amount 100, so the whole slider range is used, and the pushed stand-in halves at
about 57. Fine grain kept at 100 is 0.72 to 0.81 on the real stocks. At 0.6 the coarse figure on
500T at 100 is 0.64 and the slider does not clearly remove blotches. At 1.2 the fine grain falls
to 0.65 to 0.76, the edge widens to 0.91 pixel, and a half of the coarse energy of 500T is gone by
amount 78, so the top of the slider is lost. On Ektar 100, whose grain is fine, no candidate halves
the coarse energy and 0.9 keeps 0.72 of the fine scale at 100.

The strength is to be tuned in Q-11 on real scans and real pushed frames.

## Results

Strength 0.9, amounts 25, 50, 75 and 100, luminance only, a trous measure. Scale columns are the
fraction of the energy left at that scale. "Fine" is scale 0 alone, "coarse" is scales 2 to 4
together. The guided rows do not depend on the strength.

### Ektar 100 (input energy per scale, 1e-6: 21.8, 13.0, 35.5, 79.0, 121.5)

| Method | Amount | 0 | 1 | 2 | 3 | 4 | Fine kept | Coarse kept |
|---|---|---|---|---|---|---|---|---|
| guided | 25 | 0.475 | 0.693 | 0.915 | 0.968 | 0.992 | 0.475 | 0.972 |
| guided | 50 | 0.273 | 0.532 | 0.821 | 0.926 | 0.980 | 0.273 | 0.938 |
| guided | 75 | 0.177 | 0.435 | 0.748 | 0.891 | 0.970 | 0.177 | 0.910 |
| guided | 100 | 0.124 | 0.371 | 0.693 | 0.864 | 0.962 | 0.124 | 0.889 |
| wavelet | 25 | 0.917 | 0.892 | 0.926 | 0.936 | 0.937 | 0.917 | 0.935 |
| wavelet | 50 | 0.843 | 0.800 | 0.859 | 0.876 | 0.878 | 0.843 | 0.874 |
| wavelet | 75 | 0.777 | 0.721 | 0.798 | 0.820 | 0.823 | 0.777 | 0.818 |
| wavelet | 100 | 0.718 | 0.653 | 0.743 | 0.769 | 0.771 | 0.718 | 0.766 |

Equal coarse reduction (0.889): the guided filter keeps 0.124 of the finest scale, the wavelet 0.860
(it reaches that coarse figure at amount 44). At amount 100 the wavelet leaves 0.766 of the coarse
energy against the guided filter's 0.889. The coarse energy does not halve within the slider.

### Vision3 250D (input energy, 1e-6: 83.2, 70.6, 118.6, 167.8, 218.0)

| Method | Amount | 0 | 1 | 2 | 3 | 4 | Fine kept | Coarse kept |
|---|---|---|---|---|---|---|---|---|
| guided | 25 | 0.782 | 0.884 | 0.951 | 0.983 | 0.996 | 0.782 | 0.981 |
| guided | 50 | 0.613 | 0.738 | 0.868 | 0.954 | 0.990 | 0.613 | 0.949 |
| guided | 75 | 0.473 | 0.602 | 0.787 | 0.923 | 0.982 | 0.473 | 0.917 |
| guided | 100 | 0.363 | 0.493 | 0.718 | 0.896 | 0.975 | 0.363 | 0.889 |
| wavelet | 25 | 0.946 | 0.900 | 0.877 | 0.882 | 0.915 | 0.946 | 0.895 |
| wavelet | 50 | 0.898 | 0.811 | 0.767 | 0.777 | 0.838 | 0.898 | 0.801 |
| wavelet | 75 | 0.854 | 0.733 | 0.670 | 0.684 | 0.769 | 0.854 | 0.717 |
| wavelet | 100 | 0.813 | 0.662 | 0.585 | 0.602 | 0.707 | 0.813 | 0.643 |

Equal coarse reduction (0.889): guided 0.363, wavelet 0.943 (at amount 27). The wavelet takes the
coarse energy to a half at about amount 161, keeping 0.725 of the finest scale.

### Vision3 500T (input energy, 1e-6: 212.0, 113.6, 236.0, 486.1, 706.8)

| Method | Amount | 0 | 1 | 2 | 3 | 4 | Fine kept | Coarse kept |
|---|---|---|---|---|---|---|---|---|
| guided | 25 | 0.861 | 0.909 | 0.966 | 0.986 | 0.995 | 0.861 | 0.987 |
| guided | 50 | 0.705 | 0.788 | 0.905 | 0.960 | 0.987 | 0.705 | 0.964 |
| guided | 75 | 0.569 | 0.677 | 0.843 | 0.934 | 0.979 | 0.569 | 0.941 |
| guided | 100 | 0.453 | 0.579 | 0.788 | 0.910 | 0.971 | 0.453 | 0.920 |
| wavelet | 25 | 0.945 | 0.883 | 0.841 | 0.831 | 0.857 | 0.945 | 0.846 |
| wavelet | 50 | 0.896 | 0.786 | 0.709 | 0.688 | 0.734 | 0.896 | 0.714 |
| wavelet | 75 | 0.851 | 0.703 | 0.597 | 0.568 | 0.630 | 0.851 | 0.603 |
| wavelet | 100 | 0.809 | 0.631 | 0.503 | 0.469 | 0.542 | 0.809 | 0.510 |

Equal coarse reduction (0.920): guided 0.453, wavelet 0.971 (at amount 13). The wavelet takes the
coarse energy to a half at about amount 103, keeping 0.804 of the finest scale.

### Pushed stand-in (500T plus synthetic grain; input energy, 1e-6: 1746.0, 367.0, 681.4, 1032.4, 1147.1)

| Method | Amount | 0 | 1 | 2 | 3 | 4 | Fine kept | Coarse kept |
|---|---|---|---|---|---|---|---|---|
| guided | 25 | 0.948 | 0.967 | 0.984 | 0.994 | 0.998 | 0.948 | 0.994 |
| guided | 50 | 0.821 | 0.884 | 0.943 | 0.980 | 0.994 | 0.821 | 0.977 |
| guided | 75 | 0.672 | 0.781 | 0.889 | 0.960 | 0.988 | 0.672 | 0.954 |
| guided | 100 | 0.532 | 0.678 | 0.833 | 0.938 | 0.981 | 0.532 | 0.930 |
| wavelet | 25 | 0.885 | 0.777 | 0.728 | 0.724 | 0.773 | 0.885 | 0.745 |
| wavelet | 50 | 0.782 | 0.603 | 0.518 | 0.509 | 0.590 | 0.782 | 0.543 |
| wavelet | 75 | 0.689 | 0.465 | 0.361 | 0.350 | 0.449 | 0.689 | 0.392 |
| wavelet | 100 | 0.604 | 0.356 | 0.248 | 0.237 | 0.344 | 0.604 | 0.282 |

Equal coarse reduction (0.930): guided 0.532, wavelet 0.970 (at amount 6). The wavelet takes the
coarse energy to a half at about amount 57, keeping 0.757 of the finest scale.

### Summary of the measure

Finest scale kept where each method has removed the coarse energy the guided filter removes at
amount 100 (a trous measure):

| Stock | Coarse kept (reference) | Guided | Wavelet | Wavelet amount |
|---|---|---|---|---|
| Ektar 100 | 0.889 | 0.124 | 0.860 | 44 |
| Vision3 250D | 0.889 | 0.363 | 0.943 | 27 |
| Vision3 500T | 0.920 | 0.453 | 0.971 | 13 |
| Pushed stand-in | 0.930 | 0.532 | 0.970 | 6 |

At amount 100 itself the wavelet keeps 0.72, 0.81, 0.81 and 0.60 of the finest scale where the
guided filter keeps 0.12, 0.36, 0.45 and 0.53. The coarse figures at amount 100 are 0.77, 0.64, 0.51
and 0.28 for the wavelet against 0.89, 0.89, 0.92 and 0.93 for the guided filter. The box measure
gives the same order: coarse 0.74, 0.59, 0.51 and 0.26 for the wavelet against 0.71, 0.73, 0.80 and
0.84 for the guided filter, and fine 0.74, 0.83, 0.83 and 0.61 against 0.12, 0.36, 0.45 and 0.53.
On Ektar 100 the guided filter removes slightly more coarse energy by the box measure (0.71
against 0.74); on the other three the wavelet removes more.

White noise at amount 100: the wavelet keeps 0.747 of the deviation and the guided filter 0.120.
On a grain patch of white noise plus blotches the wavelet leaves 0.597 of the finest scale and
0.214 of the coarse scales, the guided filter 0.003 and 0.677.

### Step edge

A step in lightness from 0.3 to 0.7 with seeded grain (white noise plus two blurred layers,
deviation 0.012 each), 512 x 256. The 10 to 90 percent rise width of the mean profile over the rows,
in pixels. The unfiltered edge measures 0.80.

| Amount | Guided | Wavelet 0.9 |
|---|---|---|
| 25 | 0.80 | 0.82 |
| 50 | 0.81 | 0.84 |
| 75 | 0.82 | 0.86 |
| 100 | 0.84 | 0.88 |

Both keep the edge sharp. The wavelet widens it by 0.08 pixel at 100.

### Cost

Release build, this development machine (6 cores, 12 threads), a 6000 x 4000 image, amount 50, the
noise level measured on the picture, strength 0.9. The strength does not change the cost. Cold
workspace.

| Step | Threads | Guided (P-27) | Wavelet |
|---|---|---|---|
| Luminance alone | 12 | 182 to 184 ms | 73 to 75 ms |
| Luminance alone | 6 | 164 to 169 ms | 84 to 90 ms |

A run on a busier machine gave slower figures for both: over 250 ms for the guided filter and 75 to 90 ms for the wavelet.

The whole stage (the Oklab conversions in and out, the luminance and the colour filter), both
amounts at 50 (`noise_timing_on_a_6000_by_4000_image`), at 0.9:

| Threads | Guided luminance (before this change) | Wavelet luminance, 0.9 |
|---|---|---|
| 12 | 843, 851, 851 ms | 767 ms |
| 6 | 920, 904, 920 ms | 861 ms |

The stage is dominated by the conversions and the colour filter, so the luminance step is a small
part of it. The new stage is faster than the old on both. With only the luminance at 50 the stage
takes 338 ms on 12 threads and 488 ms on 6, and with only the colour at 50, 740 ms and 819 ms.

**Detail view, first region** (`time_a_full_size_detail_render_by_stage`, 6000 x 4000, every tool
on, median of 5 runs, the decode of the scan and its infrared plane included): 2.34 to 2.38 s on 12
threads and 2.96 to 2.98 s on 6, from 2.40 s and 3.01 s with the guided luminance on the same day.
The noise stage takes 0.76 to 0.79 s and 0.88 to 0.89 s, from 0.83 s and 0.92 s.

**Export budget.** The export of a 21 MP frame is budgeted at 10 s on 4 cores (specification section
11). The whole noise stage on 24 MP takes under 1 s on 6 threads, so it fits. Peak memory was not
measured with a process counter. The luminance step needs one plane of 96 MB at 24 MP beyond the
three Oklab planes, and about 1.3 MB of tile planes per thread; the guided filter needed six
planes at once. The colour filter, which has not changed, sets the peak of the stage.

## Recommendation

**Replace the guided filter with wavelet shrinkage for the luminance, at strength 0.9.** Keep the
guided filter for the colour.

- It keeps the fine grain. At amount 100 the wavelet keeps 0.60 to 0.81 of the finest scale and the
  guided filter 0.12 to 0.53, on every stock, which is the issue's aim.
- It removes blotches. At amount 100 it leaves 0.77, 0.64, 0.51 and 0.28 of the coarse energy
  (Ektar 100, 250D, 500T, pushed stand-in) where the guided filter leaves 0.89 to 0.93. By the box
  measure the guided filter leaves 0.71 to 0.84 and the wavelet still removes more on three of
  the four stocks.
- It costs less: the luminance step takes 73 to 90 ms against 164 to 184 ms at 24 MP, and the whole
  stage is no slower.
- Edges stay as sharp (0.88 pixel rise at 100 against 0.80 unfiltered and 0.84 guided).
- Offering both would need a new recipe field and a control the owner has not asked for.

## Decision

The wavelet method above is the luminance method of the noise reduction stage, with
`NR_WAVELET_STRENGTH` 0.9 and the shares 0.3, 0.8, 1, 1, 1. The owner approved the design and chose
the strength on 2026-11-24. The guided filter stays for `a` and `b` with the cleaned lightness as
guide (the unfiltered lightness when the luminance is 0). The recipe does not change:
`noise.luminance` and `noise.color`, 0 to 100, default 0, version 1. Both amounts at 0 return the
picture untouched, bit for bit. A pixel that is not finite enters as 0 and is restored at the end.
The scale of the render (ADR 0013) now also selects the scales and shares of the wavelet.

`NR_LUMA_RADIUS_PX` and `NR_LUMA_MAX_SIGMA` are removed. `NR_WAVELET_LEVELS`,
`NR_WAVELET_SHARES`, `NR_WAVELET_STRENGTH`, `NR_WAVELET_PATCH_PX`, `NR_WAVELET_PATCHES_ACROSS` and
`NR_WAVELET_PATCHES_DOWN` are added. `NR_CHROMA_RADIUS_PX` and `NR_CHROMA_MAX_SIGMA` stay.

## Consequences

- **The Luminance slider now removes blotches clearly.** At amount 100 it leaves 0.51 of the
  coarse energy of the 500T crops and 0.64 of the 250D crops, and the coarse energy of 500T halves
  at about amount 100. On a pushed frame (a synthetic stand-in) it halves at about 57.
- **It smooths fine grain much less than the guided filter did.** On white pixel noise at 100 the
  wavelet keeps 75 percent of the deviation against 12 percent for the guided filter. On a grain
  patch of white noise plus blotches it keeps 0.60 of the finest scale where the guided filter
  keeps 0.003. A frame with strong white sensor noise is cleaned less than before. That is the
  intended trade: grain is kept. `NR_WAVELET_STRENGTH` and the shares are the knobs.
- **A recipe saved with P-27 loads with the same values, and renders differently.** With
  `noise.luminance` above 0 it now has more fine grain and fewer blotches. The specification says so.
- **Fine-grain stocks change little.** On Ektar 100 the coarse energy left at 100 is 0.77 and no
  amount halves it, since most of its coarse energy is picture detail.
- **The pushed figures are from a stand-in** (500T crops plus synthetic grain), not a pushed scan.
- **A preview still approximates.** The noise level is measured on the preview itself and the
  scales are mapped by physical size, so the preview and the export agree closely on a synthetic
  picture (mean difference 0.0012 per sample at half size and 0.0007 at quarter size, the bound
  of the test is 0.004), but the proxy is made with an area filter that already averages away the finest grain. The panel's hint
  that fine grain shows in the exported file stays true.
- **The noise level is global.** Patches are spread over the whole picture and a median is taken, so
  a frame that is mostly a busy subject has a higher level than its grain alone. A local level
  would be more exact and cost more.
- **The constants are a first guess**, for the Q-11 tuning pass on real scans and real pushed
  frames: `NR_WAVELET_STRENGTH`, `NR_WAVELET_SHARES` and the patch grid, besides the colour
  constants of ADR 0013.
- **Tests.** The luminance pins of the guided filter (the bit-for-bit comparison with the code
  before P-31 for amounts with a luminance, the radii of the luminance filter, and the old
  luminance noise test) are replaced by tests of the wavelet; the colour pins stay.
