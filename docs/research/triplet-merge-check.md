# Checking a merged triplet (H-05)

Acceptance criterion 3 of H-05 is yours to tick: *a real D600 triplet converts to a plausible
positive; the merge time is recorded against the 10 s budget.* This is what to run and what to
look at. Nothing here is ticked for you.

## Run it

From the repository root, in release mode (the numbers mean nothing otherwise). The three NEFs
are the red-, green- and blue-lit exposure of one frame, in that order:

```powershell
cargo run --release -p gtp-app --example triplet_preview -- `
  "samples/private/<roll>/DSC_4652.NEF" `
  "samples/private/<roll>/DSC_4653.NEF" `
  "samples/private/<roll>/DSC_4654.NEF" `
  target/triplet-preview/frame-01.jpg
```

It prints the merge time, any warnings, the automatic frame and film-base detection, and writes
the positive as a JPEG. Open the JPEG. `target/` is ignored by Git.

The merge time of three real exposures on its own is also tested by
`cargo test -p gtp-ingest --release -- --ignored --nocapture` (three runs are printed).

## Measured while preparing this (release build, this machine)

| frame (files) | merge | analyse | render | warnings |
|---|---|---|---|---|
| 1 (DSC_4652 to 4654) | 0.37 s | 0.15 s | 0.14 s | none |
| 13 (DSC_4689 to 4691) | 0.38 s | 0.15 s | 0.14 s | none |

The merge is decode plus the interpolation of the lit channel, the three files side by side. Storing
the three planes as 16-bit TIFFs (about 3 s each, also side by side) is H-06's part of the 10 s
budget.

## What to look at in the JPEG

- [ ] The colours are plausible for the scene: skies blue, skin skin-coloured, no strong cast in
      the neutral areas. The recipe is only the automatic one (crop, film base, levels), so a mild
      cast is expected; a wrong-way cast (the picture looks like a negative, or one colour is
      clearly the wrong one) would mean a channel swap.
- [ ] The orientation is what you expect. Triplets may come out mirrored: shooting with the emulsion
      side down (good practice, it reduces reflections) mirrors the picture. The merge does not
      flip anything; the recipe has `flip_h` and `flip_v` for it, and the import (H-06) can set it
      for a roll. In this roll the merged frames and the Trichroma Lab exports agree in orientation
      (frame 1: houses on the left, ferry on the right; frame 13: the same chair, child and
      umbrella on the same sides), so no flip is needed here. The text on the chair seat in frame
      13 reads mirrored in both, so it is in the scene.
- [ ] No coloured fringes or ghosting at sharp edges (the three exposures are registered by the
      fixed setup; a shift would show here).
- [ ] The shadows are not full of colour noise or blocks. About 3 % to 8 % of a lit channel is
      exactly 0 (the camera clips at black, see `docs/research/nef-decode.md`); the deepest
      shadows may look flat.
- [ ] Record: merge time ______ s, the frame, the warnings printed.

## Compared with Trichroma Lab

`samples/private/Nikon Scan and Trichroma Lab positives/` holds `Trichroma Lab Export_01.tif` to
`_15.tif`: the 15 triplets of this roll, in order, converted by Trichroma Lab (5252 x 3908 and
similar: cropped to the picture, 16-bit RGB). Frames 1 and 13 side by side with the merged frames
rendered here, which only have the automatic recipe (crop, film base, levels):

- Same picture, same orientation, no channel swapped (the sky of frame 13 is blue and the chair red
  in both; a swap would turn them yellow or green).
- Trichroma Lab's are neutral and balanced; ours have the cast of the automatic recipe: teal haze
  in frame 1, a magenta-leaning chair and a cool cast in frame 13. That is colour balance, which
  the recipe does not do on its own (P-08 and the Develop view do it), not the merge.
- Ours still show the rebate: the frame detection has low confidence on these (0.43 and 0.05) so
  the automatic crop leaves the whole picture. Trichroma Lab crops tightly.
- The 15 pairs are a ready reference set for Q-02 (benchmarks against reference renders): the
  positives are here, the triplets are here, and the same frame numbers match.

To look at one yourself, open `target/triplet-preview/frame-NN.jpg` and the matching
`Trichroma Lab Export_NN.tif` side by side.

- [ ] Your verdict on the comparison: ______________________

## If something is wrong

Say which of the points failed and send the frame's file names; the merge and the demosaic are
tested on constructed mosaics, so a wrong picture points at the real files or the setup first.