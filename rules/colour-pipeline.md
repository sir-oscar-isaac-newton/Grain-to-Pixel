# Colour pipeline invariants

- Process internally in 32-bit float, linear light; keep 16-bit integer only at file I/O boundaries.
- Order: ingest → frame crop → film-base measure/normalize → invert → colour balance → tone → geometry. Dust removal (IR) runs on the scan data before inversion.
- The IR plane is never treated as a colour channel; it is carried alongside RGB.
- Edits are non-destructive: store parameters as a `Recipe` in a sidecar/catalog, never overwrite the source scan.
- Film-base measurement is stored per roll and overridable per frame.
- Embed an ICC profile on every export; state the working space in the Recipe.
