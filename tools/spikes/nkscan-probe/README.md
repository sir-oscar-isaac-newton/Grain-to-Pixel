# nkscan-probe (R-06)

Finds out, on the author's LS-50, whether a frame can be scanned at a position of our own choosing, and how the roll preview's thumbnail columns map to scanner addresses. The findings and the reasoning are in [docs/research/nkscan.md](../../../docs/research/nkscan.md), section "Frame positions (R-06)".

It is outside the workspace on purpose, like `nef-probe`: the app runs the nkscan command line and does not depend on the nkscan crate, so this must not become part of any build or CI run.

Needs the scanner on, connected through the WinUSB driver on Windows, with the roll loaded.

```bash
cd tools/spikes/nkscan-probe
cargo run --release -- table                     # what nkscan finds, and the thumbnail column addresses
cargo run --release -- shift 2 400               # scan frame 2 as found, and again moved by 400 addresses
cargo run --release -- shift 2 -400              # ... and moved towards the start of the roll
```

`table` scans the thumbnail only. `shift` then scans the one frame twice at 1000 dpi without infrared (the second time with the first one's exposure, so the two pictures can be compared) and prints how far the picture moved along the film. The two scans are written to `probe-out/` as 16-bit PGM files. Paste the printed output into the table in the research note.

The film stays in the scanner; `nkscan eject` takes it out.