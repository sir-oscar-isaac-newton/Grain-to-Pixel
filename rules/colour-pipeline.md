# Colour pipeline invariants

- Process internally in 32-bit float, linear light; keep 16-bit integer only at file I/O boundaries.
- Order: ingest → frame crop → film-base measure/normalize → invert → colour balance → tone → geometry. Dust removal (IR) runs on the scan data before inversion.
- The IR plane is never treated as a colour channel; it is carried alongside RGB.
- Edits are non-destructive: store parameters as a `Recipe` in a sidecar/catalog, never overwrite the source scan.
- Film-base measurement is stored per roll and overridable per frame.
- The film base is measured with medians, never means (dust and hot pixels must not move it): from a user-chosen rebate area (a click or a dragged box), or automatically from the brightest of four thin border strips, with a confidence that is low when no clear rebate exists. The roll base is the per-channel median over its frames. Normalization is `T = I / base` with the base floored at 1e-6, so the result is always finite.
- Inversion is linear in density: `P = max(0, (D - black) / (white - black))` with `D = -log10(T)`. It does not apply the film's contrast curve; that belongs to the tone stage and is calibrated against reference renders. Auto black/white come from density percentiles (0.5 and 99.5) of the cropped frame only, never the rebate.
- Frame detection runs on the base-normalized scan and classifies rebate as luminance within 0.06 of 1.0; it never trusts a brightness step. Below 0.5 confidence it returns the full frame, so a wrong guess is never silent.
- Colour gains always multiply to 1 (brightness is kept); tone controls act on luma and scale the pixel, so they never shift hue; only curves and colour controls change colour. Every edit constant is a named `pub const` in `gtp-edit`, to be tuned in Q-03, never a literal in the maths.
- Embed an ICC profile on every export; state the working space in the Recipe.
