# 0018. An untoned black-and-white frame is exported as a one-channel grey file with an sRGB-curve grey profile

- Status: Accepted (the owner approved the plan, this ADR, the new export option and the sRGB-curve grey profile on 2026-11-24, issue P-37 / GitHub #379)
- Date: 2026-11-24

## Context

P-36 (ADR 0017) converts a black-and-white frame from one combined density and writes the same
value to all three channels, but the export still wrote a three-channel RGB file, three times the
size of the picture it holds. Nearly every photo tool reads a grey TIFF or JPEG; a grey file is a
third of the data, and it states plainly that the frame has no colour.

## Decision

1. **Which frames.** Only an untoned black-and-white frame is written as grey: its recipe has
   `invert.combine`, both split-tone strengths are 0 (read after `Recipe::clamped`, as the render
   reads them), and its red, green and blue tone curves cannot tint (all three the identity, or all
   three with the same points; the master curve never tints). A toned black-and-white frame, or one
   with a tinting channel curve, stays RGB, because that adds colour, and a colour
   frame stays RGB, including a colour frame with the mono mix turned on (its pipeline is the colour
   one, and its three channels are only equal up to rounding of the mix).
2. **The option.** `ExportOptions.colour` is `auto` (the default: grey when the rule above says so,
   RGB otherwise) or `rgb` (always RGB, in the chosen space). It is stored in presets and in the last
   export like the other fields. Settings saved before it existed load as `auto`
   (`#[serde(default)]`); the settings format needs no migration.
3. **The file.** TIFF: one channel, 16 bits, BlackIsZero, the same Deflate with horizontal predictor,
   the same text tags and Exif directory as the RGB file. JPEG: 8-bit greyscale (one component), the
   same quality range, size limit and Exif block. Both embed the grey profile.
4. **The grey profile.** A Little CMS grey profile with the D65 white point (as the sRGB profile) and
   the sRGB parametric tone curve (type 4: gamma 2.4, `a = 1/1.055`, `b = 0.055/1.055`,
   `c = 1/12.92`, `d = 0.04045`), described as "Grey (sRGB tone curve)". The conversion is the same
   Little CMS transform as the RGB export, from linear Rec.2020 to this profile with the relative
   colorimetric intent, so the grey value is the luminance of the pixel encoded with the sRGB curve.
   For a frame whose three channels are equal this is the R value of the sRGB RGB export (a test
   holds the two within 0.1% of full scale).
5. **One profile whatever the output space.** A grey file has no primaries to choose, so the output
   space of the options does not apply to it and the profile is the same for sRGB, Adobe RGB and
   ProPhoto exports. The dialog says so.

## Alternatives

- **Gray Gamma 2.2** (the Adobe convention). It differs from the sRGB curve only in the deep
  shadows, where the sRGB curve has its linear toe. Rejected so that a grey export matches the sRGB
  RGB export of the same frame and the previews of the app, which use the sRGB curve (ADR 0015).
- **A grey curve per output space** (gamma 1.8 for ProPhoto, 563/256 for Adobe RGB). Rejected: a
  needless choice for a file with no colour, and a grey file would then change tone with a setting
  that has no meaning for it.
- **Grey for every frame the mono mix makes grey, or for toned frames with a tint profile.**
  Rejected: toned frames carry colour, and a mono mix on colour film is a different pipeline.

## Consequences

- A black-and-white archive is about a third of the size of the RGB one, and opens as grey in other
  software.
- The recipe is unchanged; the export options (IPC and settings) gain `colour`, and the generated
  types in `packages/ipc-types` carry it.
- A user who wants the old behaviour chooses "Always RGB".
- The JPEG is 8-bit grey; the 16-to-8-bit rounding is the same as for RGB.
- No JPEG decoder is in the tree, so the grey JPEG is tested by its structure (one component, the
  profile and Exif) and by the picture handed to the encoder, not by decoding the file.
