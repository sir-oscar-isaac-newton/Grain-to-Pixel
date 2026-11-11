# 0012. The colour mixer works in Oklch, with eight smoothly weighted hue bands

- Status: Accepted (the owner approved the design, issue P-25 / GitHub #302)
- Date: 2026-11-11

## Context

Issue P-25 asks for a colour mixer: a change of hue, saturation and lightness for each band of
colours (the reds, the greens, and so on), as photo editors offer. The image is linear Rec.2020
light, so the open questions are in which space a "hue" and a "lightness" are read, how the bands
are laid out so that no hue is a jump, how the effect stays away from greys, and where the stage
sits in the pipeline.

## Decision

1. **Colour model: Oklab, read as Oklch.** Linear Rec.2020 goes to CIE XYZ (D65), to LMS with Oklab's
   first matrix, through a cube root (`cbrt`, defined for negative values, so out-of-gamut colours
   keep their sign) and Oklab's second matrix. Lightness is `L`, chroma `C = hypot(a, b)`, hue
   `h = atan2(b, a)` in degrees from 0 to 360. The Rec.2020 and Oklab matrices are multiplied into
   one constant and its inverse (`gtp-edit/src/oklab.rs`), proved by tests against the published
   matrices. All constants are named.
2. **Eight bands in a fixed order**: red, orange, yellow, green, aqua, blue, purple, magenta (index
   0 to 7 of each list). The centres are Oklch hues, `MIXER_BAND_HUES`: 29, 70, 110, 143, 195, 264,
   296, 328 degrees. Red, yellow, green, aqua, blue and magenta are the hues of the sRGB primaries
   and secondaries (taken to linear Rec.2020 first, rounded to whole degrees); orange is midway
   between red and yellow, purple midway between blue and magenta. A test derives them again.
3. **Weights**: between two neighbouring centres (cyclic) the next band weighs `smoothstep(x)` and
   the lower one `1 - smoothstep(x)`, with `x` the position between the centres. The weights add up
   to 1 at every hue, each band weighs 1 at its centre, and they are continuous with a continuous
   first derivative.
4. **Chroma gate**: every change is multiplied by `smoothstep(clamp(C / 0.04, 0, 1))`, so it fades
   to nothing towards grey and never amplifies the noise or the hue of a neutral.
5. **The three changes**, with `H`, `S`, `Lt` the weighted sums divided by 100 and `g` the gate:
   hue `h + H * 30 degrees * g`; chroma `C * (1 + S * g)`, never below 0 (-100 removes the colour,
   +100 doubles it); lightness a gain of `2^(Lt * 1 stop * g)` in linear light, applied as
   `L` and `C` times the cube root of the gain, which scales the linear RGB by the gain exactly and
   keeps hue and saturation. The result is not clamped.
6. **Recipe**: `color.mixer` with three lists of eight values, -100 to 100, default 0, clamped
   (non-finite values become 0). The version stays 1; a recipe saved before the mixer loads with
   zeros. A list of another length is a malformed recipe and `migrate` refuses it with
   `InvalidInput`, like any other value of the wrong shape.
7. **Stage**: a stage of its own after the colour stage and before the tone stage, skipped in black
   and white. All 24 values at 0 leave the image bit for bit as it was, and so does a pixel with a
   gate of 0 or with no weighted change. The pickers and the automatic balance measure the frame
   before the colour stage, so they do not see it.
8. **Apply settings and presets**: the mixer is a part of its own (`Mixer`, listed after Colour);
   the Colour part keeps the target's own mixer, as it keeps its split toning. A Develop preset has
   it ticked by default, with Colour, Split toning and Tone.

## Rejected

- **HSL or HSV on linear light.** The hue shifts with lightness, and the bands come out uneven:
  the same angle covers very different colours depending on how bright they are, so a slider for
  "the greens" would take parts of the yellows and the aquas along.
- **CIE Lab (or LCh).** Its hue lines bend: blues turn towards purple when their chroma or lightness
  changes, so a pure hue shift or a saturation change is not a change of one thing only. Oklab was
  built to fix this.

## Consequences

- **Cost.** A cube root per channel, an arctangent and a sine and cosine per colourful pixel. A full
  6000 x 4000 render with a few bands set takes about 0.4 s on this development machine (12 logical
  cores) in a release build (random colours, where every pixel is colourful); an image of mostly grey pixels
  is cheaper, because those are left alone before the polar step.
- **Not clamped.** A saturation or lightness increase can push a colour outside the Rec.2020 gamut
  or above 1; the tone stage and the output transform deal with it, as they do for other stages.
- **The bands are a choice.** Eight bands at fixed centres do not follow a particular image; a
  picture with a lot of colour between two centres gets a blend of both bands, by the weights.
- **Hue shifts are bounded.** At most 30 degrees at 100, so one band cannot be moved onto another;
  that is a limit of this design, and a different range is a constant to change.
- **The Oklab white is not exactly neutral** in this chain (it comes from Oklab's published D65
  matrices), so a grey has a chroma of about 1e-4; the gate makes the change there about 1e-5 of
  the effect, and a test bounds it.
