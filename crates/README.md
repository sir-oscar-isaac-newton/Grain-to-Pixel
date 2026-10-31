# crates

Rust libraries holding all image-processing and data logic. UI-agnostic and independently testable.

| Crate | Responsibility |
|---|---|
| `gtp-core` | Domain types (Frame, Roll, Recipe), errors. Depended on by all others. |
| `gtp-io` | 16-bit TIFF / RAW read-write, export (16-bit TIFF and JPEG in sRGB, Adobe RGB or ProPhoto with ICC, via Little CMS), sidecar files. |
| `gtp-ingest` | Adapters: file import (TIFF scans, NEF triplets), Coolscan LS-50 (RGB + IR) via `nkscan`. |
| `gtp-capture` | Guided RGB capture: Scanlight V4 serial driver, camera backends (libgphoto2, watched folder), per-frame R/G/B sequence. |
| `gtp-negative` | Frame-area detection, film-base measurement, inversion. |
| `gtp-edit` | Colour balance, tone, geometry, IR dust removal. |
| `gtp-catalog` | SQLite archive, tags, search. |
| `gtp-app` | Application layer: use-case services, `JobManager`, pipeline composition (`render`, `analyze`). Created in P-10 (GTP-029, [ADR 0005](../decisions/0005-application-layer.md)); holds the render pipeline today. |

Dependency direction: `core` ← `io` ← (`ingest`, `capture`, `negative`, `edit`, `catalog`). Feature crates must not depend on each other; `gtp-app` is the only crate that combines them, and `src-tauri` is a thin adapter over it. Allowed edges: [docs/architecture.md §3.3](../docs/architecture.md).
