# 0017. A black-and-white frame is inverted from one combined density with inverse-variance channel weights, and skips the colour stages

- Status: Accepted (the owner approved the plan, this ADR and the recipe change on 2026-11-24, issue P-36 / GitHub #376)
- Date: 2026-11-24

## Context

A black-and-white negative went through the colour pipeline: three channels, each inverted with its
own black and white point, then made mono in the colour stage. The three channels carry one picture
(the scanner or the Scanlight sees the same silver, or the same dye image, through three colours), so
each has the picture and its own noise. Inverting them separately keeps three noises and three
points that can disagree, and the grey is only as neutral as the mono mix makes it.

Since L-12 a roll has a film type (`color_negative`, `bw_silver`, `bw_chromogenic`). P-36 uses it:
the frames of a black-and-white roll are converted from one combined density, with one pair of
points, which gives less noise and a neutral grey by construction.

## Decision

1. **How the densities are combined.** The density of a pixel is the weighted mean of the three
   channel densities, `D = sum(w_c * D_c) / sum(w_c)`, where `D_c = -log10(T_c)` as in the colour
   inversion. The weights are inverse-variance weights found from the frame itself
   (`gtp_negative::combine_weights`). Model the channel as `D_c = a_c * S + noise_c`: one picture `S`
   seen through three channels, with a gain `a_c` that is proportional to the channel's signal span
   `span_c` (the 99.5th minus the 0.5th percentile of its density) and independent noise of standard
   deviation `sigma_c`. The linear combination that gives the most signal for the noise it carries is
   `w_c` proportional to `a_c / sigma_c^2`, so `w_c` is proportional to `span_c / sigma_c^2`, scaled to
   add up to 1. `sigma_c` is estimated robustly: 1.4826 times the median absolute deviation of the
   density differences between horizontal neighbours, divided by the square root of two (a
   difference of two samples has twice the variance of one), on a regular grid of about 10^6 pixels
   of the cropped frame. Picture detail adds a few large differences, which the median ignores.
   Weights were chosen over "the channel R-07 recommends" because they need no hardware finding and
   they also cover the camera path, where the channels differ in noise by exposure, and a scanner
   with an LED or lamp that is weak in one colour. R-07 may change the default (for example to one
   channel if the spike finds the other two carry nothing); the recipe stores the weights, so a
   different default is a change in `combine_weights` and not in the format, and a user or a future
   control can set any weights.
2. **Fallback.** If any `sigma_c` or `span_c` is not finite and above 0 (a noiseless synthetic frame,
   a flat frame, a frame too small to have neighbours, a frame of non-finite pixels), the weights are
   one third each. The same happens to any stored weights that are not finite and at least 0 with a
   positive sum (`Recipe::clamped`); weights that are valid are scaled to add up to 1.
3. **Where the picture becomes one plane.** At the Invert stage. The combined density is mapped with
   one black point and one white point, `P = max(0, (D - black) / (white - black))`, and `P` is
   written to all three channels, so everything after it sees a neutral grey: no colour edit can tint
   it, and no stage needs a special case for "grey". The image is still three channels (12 bytes a
   pixel) through the pipeline, because the stages, the display conversion and the cache of ADR 0016
   are written for three. A real one-channel image is a change for export: P-37 writes greyscale
   files from it. The recipe keeps its three `black` and three `white` values: the inversion uses
   their means, and the automatic values and the pickers write the same number to all three, so the
   levels sliders of the Develop view work as they do for colour (moving one channel moves the mean,
   and the Develop view keeps the weights when a point moves).
4. **Stages that run and stages that are skipped.** Run: dust removal, crop, film-base normalize,
   inversion, noise reduction (the lightness plane does the work; the colour planes of a grey picture
   are empty), tone and curves, split toning (the way to tone a black-and-white picture), geometry,
   the vignette. Skipped: the colour stage entirely (the axes, temperature, tint, the mono mix,
   saturation, vibrance) and the colour mixer. They would only add a tint to a picture that is grey
   by construction, and the mono mix of a black-and-white recipe has nothing left to mix. The
   `mono` block stays in the recipe for making colour film black and white, and its `enabled` flag is
   turned on in an automatic black-and-white recipe so the Develop view shows the frame as
   black and white. Skipping is done in `run_late_stage`, which sees only the image and the recipe, so
   the kept picture after noise reduction (ADR 0016) needs no change: `invert`, which holds
   `combine`, is already part of its key, and the later stages read `combine` from the recipe they are
   given.
5. **The automatic recipe.** `analyze` for a frame of a black-and-white roll (`analyze_for_film`, the
   film type read from the roll) estimates the weights and the combined points (0.5th and 99.5th
   percentile of the combined density, the same default percentiles as colour), turns `mono` on and
   leaves the colour axes and the saturation at their defaults: no colour balance is estimated, and the
   roll's stored balance is not applied. A colour roll is analyzed exactly as before; its frames and
   output are unchanged bit for bit (the existing golden and snapshot tests pass unchanged, and a
   test compares the two analyses).
6. **The recipe change.** `invert` gains `combine`, an optional list of three weights. It is skipped
   in the JSON when absent, so a colour recipe serialises exactly as before and a recipe saved before
   the field existed loads with it absent (`#[serde(default)]`). The recipe version stays 1: the
   change adds an optional field and needs no migration (convention 3 asks for a new version when
   the shape changes in a way old readers or writers would misread; an older app reading a combined
   recipe ignores the field and inverts per channel with the stored points, which are equal and
   sensible).
7. **Film type and recipe disagree.** The film type of a roll can change after its frames were
   analyzed (the owner sets it on the roll afterwards), and a recipe can be copied from a frame of the
   other kind. Wherever a frame's saved recipe is loaded to be developed, shown as a thumbnail or
   exported, it is fitted to the roll first (`services::film_recipe::fit`):
   - black-and-white roll and `combine` absent: the combined inversion is found from the frame's own
     scan (on a proxy, with the recipe's crop and film base, as the analysis does), and `mono` is
     turned on;
   - colour roll and `combine` present: `combine` is removed, the per-channel points are found again
     the same way, and `mono` is turned off;
   - every other edit (geometry, base, colour, mixer, tone, noise, dust, output) is kept.

   **The per-channel points of the old recipe are not carried over.** They were set against three
   separate densities, each with its own range and offset; the mean of three per-channel points is
   not a point of the combined density, because the combined density is a weighted mean of the
   densities and the weights, the offsets and the spans differ per channel. A black point of 0.10,
   0.12 and 0.15 and a white point of 2.1, 2.3 and 2.5 do not average into the black and white of
   `0.4 D_r + 0.4 D_g + 0.2 D_b`. The reverse holds too: one pair of points for the combined density
   says nothing about the three channels. So the points are found afresh from the frame, which is
   what the automatic recipe does, and a user who had set them by hand sets them again. The fitted
   recipe is saved at once, without stamping the frame as edited (it is not an edit by the user), so
   the next load finds that it fits and does nothing more: there is no save loop and no analysis on
   every load. It is saved only while the stored recipe is still the one that was fitted, so an edit
   the user saves meanwhile is never overwritten (it is fitted on its next load). A read-only library
   is not written to; the fitted recipe is used for that load only. A recipe that cannot be fitted
   (its frame gives nothing to measure) is logged and used as saved, so the frame still opens.
   The Develop view's render takes the recipe it is sent as it is (`render` never guesses), so the
   fitting happens where the recipe is read, in `open`, in thumbnail regeneration and in export.

## Consequences

- A black-and-white frame has one noise: on a synthetic negative with equal noise in the three
  channels the combined plane has less noise relative to its span than any one channel, and with one
  much noisier channel it is no worse than the best one (tests in `gtp-negative`).
- The weights depend on the noise of the frame, so two frames of a roll can have slightly different
  weights; the points are re-estimated with them. This is the same as per-frame automatic levels for
  colour.
- The estimate takes one pass over a grid of about a million pixels, as the automatic levels do, so
  it adds nothing measurable to an analysis.
- The recipe gets a field; the generated types in `packages/ipc-types` carry it, and the Develop view
  keeps `combine` when a point moves and takes it from the automatic recipe when levels are reset.
- Colour edits on a black-and-white frame do nothing (the controls are not hidden by this change; a
  later UI issue may disable them for a combined frame).
- Black-and-white output is still a three-channel file until P-37.
- The weights are validated on synthetic frames only; a check against real black-and-white scans
  from the Scanlight and the LS-50 is still open (R-07 and H-25).
