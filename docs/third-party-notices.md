# Third-party notices

Credits that third-party licences ask the documentation of shipped binaries to carry. Licences are checked by `cargo deny check` (`deny.toml`); add an entry here when a new dependency's licence requires a notice. The release packaging (M7) ships this file with the app.

## JPEG export

This software is based in part on the work of the Independent JPEG Group. (The `jpeg-encoder` crate, used for JPEG export, contains code derived from libjpeg.)

## Colour management

Colour conversion on export uses Little CMS (lcms2), Copyright (c) Marti Maria Saguer, under the MIT licence, through the `lcms2` and `lcms2-sys` crates (MIT).
