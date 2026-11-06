# gphoto-probe (R-03)

Checks that the `gphoto2` Rust crate (3.4.1, LGPL-2.1) can find the D600 and take a picture over USB, as the `gphoto2` command line does. Findings: [docs/research/scanlight-capture.md](../../../docs/research/scanlight-capture.md), section "libgphoto2 on the Mac (R-03)".

Outside the workspace on purpose: the crate needs libgphoto2 installed, and nothing in the app uses it yet. Written from the crate's documentation on a PC without libgphoto2; it compiled on the first build on the Mac and takes a picture (R-03).

```bash
brew install libgphoto2 pkgconf
cd tools/spikes/gphoto-probe
cargo run --release -- list
cargo run --release -- capture test.nef
```

The camera must be free of macOS's camera helper: log other accounts out (or `sudo pkill -9 ptpcamerad` in a loop) and close Photos and Image Capture.
