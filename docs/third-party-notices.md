# Third-party notices

Credits that third-party licences ask the documentation of shipped binaries to carry. Licences are checked by `cargo deny check` (`deny.toml`); add an entry here when a new dependency's licence requires a notice. The release packaging (M7) ships this file with the app.

## JPEG export

This software is based in part on the work of the Independent JPEG Group. (The `jpeg-encoder` crate, used for JPEG export, contains code derived from libjpeg.)

## Colour management

Colour conversion on export uses Little CMS (lcms2), Copyright (c) Marti Maria Saguer, under the MIT licence, through the `lcms2` and `lcms2-sys` crates (MIT).

## Camera files (NEF)

NEF files of RGB triplets are decoded with `rawler` 0.8.0, Copyright (c) Daniel Vogelbacher and Pedro Côrte-Real, under the GNU Lesser General Public License, version 2.1 (LGPL-2.1). The app is statically linked with it. Source of the exact version: https://crates.io/crates/rawler/0.8.0 (repository https://github.com/dnglab/dnglab). Why this decoder, and what the project owes for using it: [ADR 0007](../decisions/0007-nef-decoder.md). The full licence text, the credit inside the app and the instructions for rebuilding the app with a modified `rawler` are added by issue Q-08.
