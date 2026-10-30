# 0002. Tauri + Rust core + TypeScript UI

- Status: Accepted (adopted by [docs/specification.md](../docs/specification.md))
- Date: 2026-10-29

## Context

Needs Windows 10/11 and macOS, 16-bit image processing at scan resolution (LS-50: 4000 dpi), hardware access (USB scanner, camera tethering), and a rich editing UI. Small private audience means installer size and licensing matter more than ecosystem breadth.

## Decision

- Core pipeline in Rust crates (`crates/`): fast, memory-safe, no runtime to ship.
- Desktop shell: Tauri 2. Smaller installers than Electron; native Rust commands for the core.
- UI: React + TypeScript in `apps/desktop`, shared pieces in `packages/`.
- Catalog: SQLite.

## Consequences

- Two languages, one build story (cargo + pnpm).
- Scanner access: use the open-source `nkscan` driver (a Rust crate and CLI), see [ADR 0003](0003-ls50-via-nkscan.md). `gtp-ingest` still hides it behind an adapter trait.
- IR/ICE dust removal requires the raw 4-channel (RGB+IR) scan, so `gtp-io` must preserve the IR plane.
