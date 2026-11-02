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
