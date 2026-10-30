# Colour pipeline invariants

- Process internally in 32-bit float, linear light; keep 16-bit integer only at file I/O boundaries.
- Order: ingest → frame crop → film-base measure/normalize → invert → colour balance → tone → geometry. Dust removal (IR) runs on the scan data before inversion.
- The IR plane is never treated as a colour channel; it is carried alongside RGB.
- Edits are non-destructive: store parameters as a `Recipe` in a sidecar/catalog, never overwrite the source scan.
- Film-base measurement is stored per roll and overridable per frame.
- The film base is measured with medians, never means (dust and hot pixels must not move it): from a user-chosen rebate area (a click or a dragged box), or automatically from the brightest of four thin border strips, with a confidence that is low when no clear rebate exists. The roll base is the per-channel median over its frames. Normalization is `T = I / base` with the base floored at 1e-6, so the result is always finite.
- Embed an ICC profile on every export; state the working space in the Recipe.
