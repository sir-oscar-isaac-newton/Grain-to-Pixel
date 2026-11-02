# 0007. Decoder for the D600's NEF files

- Status: Proposed (the author decides; the spike R-05 / GitHub #12 gives the numbers)
- Date: 2026-11-02

## Context

RGB triplets are shot with a Nikon D600 and stored as NEF. The library needs the linear sensor data
of each exposure (specification section 4.2 step 1). The spike (`docs/research/nef-decode.md`)
decoded the author's real files with `rawler` 0.8.0 and with LibRaw 0.20.1: both are correct and
agree to within one count; rawler takes 108 to 142 ms a file, LibRaw 282 to 313 ms. The licences
differ: rawler is LGPL-2.1; LibRaw is LGPL-2.1 or CDDL-1.0 (choose CDDL-1.0); no permissive Rust
decoder handles the D600. `deny.toml` allows permissive licences only.

## Decision

To be filled in when the author chooses, one of:

- **A. rawler.** The author accepts the LGPL-2.1 obligations (notices, source availability, an app
  licence that allows modifying and relinking the combined work for private use).
- **B. LibRaw under CDDL-1.0** through a Rust wrapper (C++ build, `LIBRAW_NODLL` on MSVC).

Recommended: A, for speed, a pure-Rust build and a current codebase.

## Consequences

- Either way the decoder sits behind a trait in `gtp-ingest` (H-05), so the choice can change later.
- A: `deny.toml` gets an explicit exception for the LGPL-2.1 crates, `docs/third-party-notices.md`
  an entry, and the app's licence text the allowance above.
- B: `deny.toml` gets an exception and a hand-written notice for LibRaw (cargo shows only the
  wrapper's MIT or Apache-2.0), CI needs a C++ compiler on both platforms, and LibRaw is the
  2020 version bundled by the wrapper.
- The black level of the files is 0 (already subtracted by the camera), so the decoder step needs no
  black subtraction; the merge starts from the white level 15520.