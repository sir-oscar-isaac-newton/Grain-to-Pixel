# Grain to Pixel

An all-in-one desktop app (Windows 10/11, macOS) for scanning, converting, editing, and organizing film negatives. Personal project, shared with close friends only. No monetization.

**Workflow:** ingest (Nikon Coolscan LS-50 or tethered RGB captures) → frame detect + film-base measure → negative-to-positive → edit (colour, tone, geometry, IR dust removal) → tagged, searchable archive.

**Success bar:** results comparable to Nikon Scan 4 / Trichroma Lab.

## Repo at a glance

| Path | Purpose |
|---|---|
| `apps/desktop/` | Tauri desktop app (shell + UI) |
| `crates/` | Rust libraries: image pipeline, ingest, catalog |
| `packages/` | TypeScript shared packages (UI kit, IPC types) |
| `samples/` | Test negatives (public via LFS, private ignored) |
| `tools/` | Benchmarks and dev scripts |
| `docs/` | Documentation (start at [docs/structure.md](docs/structure.md)) |
| `decisions/` | Architecture Decision Records |
| `rules/` | Coding and domain rules (also read by Claude Code) |
| `prompts/` | Reusable prompts for AI-assisted work |

## Getting started

Prerequisites: Rust (stable), Node 20+, pnpm 9, Git + Git LFS.

```bash
pnpm install
cargo check --workspace
pnpm dev        # once apps/desktop is scaffolded
```

See [docs/structure.md](docs/structure.md) for layout and conventions.
