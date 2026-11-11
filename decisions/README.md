# decisions

Architecture Decision Records (ADRs), numbered, immutable once accepted (supersede instead of editing).

| # | Title | Status |
|---|---|---|
| [0001](0001-monorepo.md) | Single monorepo | Accepted |
| [0002](0002-tech-stack.md) | Tauri + Rust core + TypeScript UI | Accepted |
| [0003](0003-ls-50-via-nkscan.md) | Drive the LS-50 with nkscan | Accepted |
| [0004](0004-guided-rgb-capture.md) | Guided RGB capture: Scanlight serial control and two camera backends | Accepted for macOS; Windows capture open, not required for v0.1.0 |
| [0005](0005-application-layer.md) | Application layer crate (`gtp-app`) | Accepted |
| [0006](0006-e2e-debugging-port.md) | The e2e test reaches WebView2 through a debugging port set in the window config | Accepted |
| [0007](0007-nef-decoder.md) | Decoder for the D600's NEF files (rawler, LGPL-2.1) | Accepted |
| [0008](0008-film-log-look.md) | The Film log look: a log start for editing | Accepted |
| [0009](0009-frontier-look.md) | The Frontier-style look, fitted to Trichroma Lab | Accepted |
| [0010](0010-flat-field.md) | A flat field per roll, applied when a camera frame is read | Accepted |
| [0011](0011-colour-balance.md) | One colour balance per roll of scanner frames, the median of their mid-tone estimates | Accepted |
| [0012](0012-colour-mixer.md) | The colour mixer works in Oklch, with eight smoothly weighted hue bands | Accepted |
| [0013](0013-noise-reduction.md) | Noise reduction is a guided filter in Oklab, between the inversion and the colour stage | Accepted |
| [0014](0014-detail-view.md) | The detail view renders the whole picture at full size and cuts regions from one cached copy | Accepted |

New ADR: copy [0000-template.md](0000-template.md), take the next number, add a row above.
