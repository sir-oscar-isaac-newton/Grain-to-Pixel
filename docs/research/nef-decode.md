# NEF decoding for RGB triplets (R-05)

Question: do the D600's NEF files decode to usable linear data, how fast, and with which decoder?
Measured on the author's real triplets in `samples/private/Mamiya 645 Kodak Gold 200 120 Trichrome RGB/`
(45 files, three exposures per frame, lit red, green, blue by the Scanlight; plus the bare-light
triplet in `Barelight RGB Capture/`). Run `tools/spikes/nef-probe` (see its README) to repeat it.

## Result

Both candidates decode the D600 NEFs correctly and agree with each other. The numbers do not decide
between them; the licences do (below). Neither rawler nor LibRaw has anything the other lacks for
this use.

| | `rawler` 0.8.0 | LibRaw 0.20.1 (through `libraw-rs-sys` 0.0.4) |
|---|---|---|
| Licence of the decoder | LGPL-2.1 | LGPL-2.1 or CDDL-1.0 (the user chooses) |
| Licence shown by cargo for the crate | LGPL-2.1 | MIT/Apache-2.0 (the wrapper; LibRaw's own licence is not visible to `cargo deny`) |
| Language, build | Rust, plain `cargo` | C++ compiled by `cc`; fails on MSVC unless `CXXFLAGS=/DLIBRAW_NODLL`; needs a C++ compiler on every build machine |
| Decode of one NEF (release build) | 108 to 142 ms | 282 to 313 ms |
| Raw frame, crop | 6080 x 4028; crop x8 y6, 6016 x 4016 | 6080 x 4028; no crop applied (margins 0) |
| CFA | RGGB | RGGB (`filters` 0xb4b4b4b4) |
| Black level | 0 | 0 (`cblack` all 0) |
| White level | 15520 | `maximum` 15879 |
| Camera white balance | 1.86, 1.0, 1.43 (R, G, B) | `cam_mul` 477, 256, 365 (same ratios) |

The raw samples of the two agree to the last bit or one count: of 24.5 million samples, LibRaw
equals rawler for 25 % to 29 % and is exactly one count higher for the rest; no sample differs by
more than 1 (three files checked). That is rounding in the linearisation curve, far below anything
visible (the white level is 15520).

## Black level and linearity

- Both decoders report a black level of 0, and the files agree: the darkest samples are 0 in every
  CFA plane. In the first frame 2.6 % of the red plane is exactly 0 in the red-lit shot and 6.6 %
  of the blue plane in the blue-lit shot (the planes that are not lit have more: 7 % to 8 % for
  green and blue, 39 % for red under the blue light). So the camera has
  already subtracted its black level and clipped at 0; there is nothing to subtract, and the noise
  below zero is lost. That cannot be undone by any decoder. It only matters in the deepest
  shadows (see the exposure note).
- Both apply the Nikon linearisation curve of the file and give 16-bit integers (14-bit data).
  Two independent implementations returning the same values is the evidence that the data is
  linear; a step wedge shot with the same setup would be the proof. H-05 asks the author to
  confirm that a converted triplet looks right.
- Narrow-band lighting works as expected: in the red-lit shot the red plane has a median of 254,
  the green planes 26 and the blue plane 6 (ten times lower or more). This is also what the colour
  check of H-11 relies on.

## Demosaic

Bilinear, one thread, release build, 6016 x 4016 output as linear f32 (black subtracted, scaled by
the white level):

| | time |
|---|---|
| all three channels | 194 to 205 ms |
| one channel only (the one the exposure was lit in) | 64 ms |

An exposure lit red only contributes its red plane, so only that plane needs interpolating: a
third of the work. The library stores originals as single-channel 16-bit TIFFs (specification
section 5.1), one per exposure.

## Triplet budget (10 s, specification section 11)

Per triplet, measured on this machine (release build):

| step | one file | three files, one after another |
|---|---|---|
| decode (rawler) | 0.11 to 0.14 s | 0.4 s |
| bilinear demosaic of the lit channel | 0.064 s | 0.2 s |
| store the lit plane as a 16-bit TIFF (Deflate with predictor, `gtp-io`) | 2.8 to 3.2 s (14 to 24 MB) | 9.0 s |
| total | | about 9.5 s |

Run one after another the triplet is just inside the budget, and the TIFF encoding is almost all of it
(the first frame of the session; the bare-light frame took 2.2 s a plane).
The three files are independent, so processing them side by side brings the wall time to roughly
the time of one file (about 3.5 s on a machine with three or more free cores; not measured,
H-05 must measure it). Reading the NEFs (3 x 25 MB) and the hash check of the stored files come on
top and are small next to the encoding.

## Exposure observation (for guided capture)

In the red-, green- and blue-lit shots of the first frame the brightest samples reach 2812, 1878 and
1209 of 15520 (18 %, 12 % and 8 % of the range). The exposures could be 1.5 to 3 stops brighter
before anything clips, which would lift the shadows out of the clipped-to-zero region above and
improve their noise. This is a finding for H-08's exposure settings, not a change made here.

## Licences

`deny.toml` allows permissive licences only. The candidates:

- **rawler** (LGPL-2.1) and its ancestor **rawloader** (LGPL-2.1, older, same API shape) are the
  Rust decoders that handle the D600. **quickraw** is LGPL-2.1, **zenraw** and **dng** are
  AGPL-3.0, **imagepipe** is LGPL-3.0, **rawkit** (MIT or Apache-2.0) has no Nikon support that
  this work found. None of the Rust decoders is permissive.
- **LibRaw** is LGPL-2.1 or CDDL-1.0. Under CDDL-1.0 the obligations are file-level: LibRaw's own
  source files stay available and keep their notices; the rest of the application is not affected.
  The Rust wrappers are MIT or Apache-2.0 (`libraw-rs-sys` bundles LibRaw 0.20.1 from 2020, which
  knows the D600; `rsraw`, newer, needs `libclang` for bindgen). Because cargo reports only the
  wrapper's licence, `cargo deny` would not notice LibRaw's: the exception and the notice would have
  to be added by hand.

What LGPL-2.1 asks of a statically linked Rust program (this is a summary, not legal advice): ship
the LGPL notices; make the library's source available; let the user replace the library, which for
a statically linked program means providing the application's source or object files so it can be
relinked; and do not forbid modification for private use or reverse engineering to debug such
changes. The repository is public, so the source is available; the application's own licence
(`LicenseRef-Private`) would need to allow those uses of the combined work.

## Decision

**rawler**, chosen by the author on 2026-11-02 (ADR 0007). It is more than twice as fast as LibRaw,
pure Rust (no C++ toolchain or define on Windows and macOS), current (0.8.0, edition 2024), and its
raw output matches LibRaw's. The LGPL-2.1 obligations and how the project meets them are listed in
the ADR. LibRaw under CDDL-1.0 remains the fallback: H-05 builds the decoder behind a trait, so
changing the choice later touches one module.