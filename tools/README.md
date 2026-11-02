# tools

- `benchmark/` (crate `gtp-benchmark`): quality harness. Today: CIEDE2000, image comparison and golden-image regression tests (Q-01). Later: comparison against reference renders from Nikon Scan 4 / Trichroma Lab (the project's success metric).
- `spikes/`: one-off measurements kept for the record, outside the Cargo workspace so their heavy or unusual dependencies never reach a build or CI (`nef-probe`: NEF decoders for R-05).
- `scripts/`: dev helpers (release packaging, fixture conversion). Prefer PowerShell + shell pairs, or Node scripts, so they run on Windows and macOS.

## Golden images

Six fixed cases (synthetic negative plus recipe, listed in `benchmark/src/golden.rs`) are rendered through `gtp_app::render` and compared with the renders stored in `samples/public/golden/`. A case fails when any pixel differs by more than ΔE2000 0.5. They run in `cargo test --workspace`; to run them alone with a readable table:

```bash
cargo run -p gtp-benchmark -- golden
```

Add `--maps target/golden-maps` to write a ΔE map of each differing case (16-bit gray TIFF; white is ΔE 10 or more).

When a pipeline change is intended, rewrite the goldens and commit them with the change; the table it prints (mean, p95 and max ΔE per case) goes into the pull request description:

```bash
cargo run -p gtp-benchmark -- golden --bless
```
