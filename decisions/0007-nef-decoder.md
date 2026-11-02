# 0007. Decoder for the D600's NEF files

- Status: Accepted (the author chose option A on 2026-11-02, after the spike R-05 / GitHub #12)
- Date: 2026-11-02

## Context

RGB triplets are shot with a Nikon D600 and stored as NEF. The library needs the linear sensor data
of each exposure (specification section 4.2 step 1). The spike (`docs/research/nef-decode.md`)
decoded the author's real files with `rawler` 0.8.0 and with LibRaw 0.20.1: both are correct and
agree to within one count; rawler takes 108 to 142 ms a file, LibRaw 282 to 313 ms. The licences
differ: rawler is LGPL-2.1; LibRaw is LGPL-2.1 or CDDL-1.0 (choose CDDL-1.0); no permissive Rust
decoder handles the D600. `deny.toml` allows permissive licences only.

## Decision

Use **`rawler`** (LGPL-2.1) to decode NEF files, behind a decoder trait in `gtp-ingest` (H-05) so
that it can be replaced later. The author accepts the LGPL-2.1 obligations for a statically linked
program.

## Consequences

What the project owes as long as it distributes the app (a summary of LGPL-2.1 section 6, not legal
advice), to be done with H-05 and before the first release:

- `deny.toml` gets an exception for the crate `rawler` only; the rest of its dependency tree is MIT
  or Apache-2.0.
- `docs/third-party-notices.md` gets `rawler`, its copyright and the LGPL-2.1 text; the app credits it
  where it shows its notices (About or Settings).
- The source of the pinned `rawler` version is available: a link to the crate, and a written offer
  valid for at least three years or the source archive attached to the release.
- Recipients can rebuild the app with a modified `rawler`: the app's source and `Cargo.lock` are
  available, with a short "how to rebuild with a modified rawler" note. While the repository is
  public this holds on its own; if it is made private, a source archive of the app is attached to
  each release (or friends get read access).
- The app's own licence (today `LicenseRef-Private`) must allow modifying the app for the user's own
  use and reverse engineering to debug such changes. The licence text is revised before the first
  release (a separate issue).
- Any patch to `rawler` itself is published under the LGPL-2.1.
- The black level of the files is 0 (already subtracted by the camera), so the decoder step needs no
  black subtraction; the merge starts from the white level 15520.
- LibRaw (CDDL-1.0 through a wrapper) stays the fallback if these obligations ever become a
  problem; the decoder trait keeps that change to one module.