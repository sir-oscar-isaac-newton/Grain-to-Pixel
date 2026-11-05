# gphoto-probe (R-03)

Checks that the `gphoto2` Rust crate (3.4.1, LGPL-2.1) can find the D600 and take a picture over USB, as the `gphoto2` command line does. Findings: [docs/research/scanlight-capture.md](../../../docs/research/scanlight-capture.md), section "libgphoto2 on the Mac (R-03)".

Outside the workspace on purpose: the crate needs libgphoto2 installed, and nothing in the app uses it yet. **Not compiled on the author's Windows PC** (no libgphoto2 there); written from the crate's documentation, so the first build on the Mac may need a fix.

```bash
brew install libgphoto2 pkgconf
cd tools/spikes/gphoto-probe
cargo run --release -- list
cargo run --release -- capture test.nef
```

The camera must be free of macOS's camera helper: log other accounts out (or `sudo pkill -9 ptpcamerad` in a loop) and close Photos and Image Capture.
