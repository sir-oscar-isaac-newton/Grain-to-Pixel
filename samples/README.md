# samples

Test negatives for development and benchmarking. Sample colour-negative data is limited (a known project risk), so every sample is documented.

- `public/`: small, shareable files tracked with Git LFS (see `.gitattributes`).
- `private/`: personal scans. Git-ignored; never committed.

Add a row to `public/MANIFEST.md` for each file: film stock, scanner/camera, bit depth, IR channel present, expected result notes.

## Synthetic negatives

Unit tests of the pipeline stages do not need real film. `gtp_core::testing::synthetic::negative(seed, &Params)` builds a negative from a known positive (density wedge, gradient or seeded blobs) and returns the ground truth: film base, black and white densities, frame rectangle, dust mask and the expected positive. The forward model is in the module docs (`crates/gtp-core/src/testing/synthetic.rs`).

- It lives behind the `testing` feature of `gtp-core`. The crate's own tests enable it through a dev-dependency; other crates add `gtp-core = { ..., features = ["testing"] }` under `[dev-dependencies]` only, never under `[dependencies]`.
- Output is deterministic per seed on one machine. Compare with a tolerance and do not store golden files from it.

## Inspecting a scan file

To see how scanner software writes a TIFF (bits, channels, compression, whether the fourth channel of an RGBI scan is marked as alpha or unspecified) without decoding the pixels:

```bash
cargo run -p gtp-io --example tiff_info -- path/to/scan.tif
```

`gtp-io` reads 16-bit gray, RGB and RGB plus a fourth channel (infrared). A fourth channel declared "unspecified" in the file is refused with a clear message, because the TIFF decoder would drop it silently; if one of your files is like that, the layout printed here shows it.
