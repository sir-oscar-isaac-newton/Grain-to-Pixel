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

Prerequisites: Rust (the version pinned in `rust-toolchain.toml`, installed automatically by rustup), Node 20+ (`.nvmrc`), pnpm 9 (`npm i -g pnpm@9`), Git + Git LFS, and `cargo-deny` (`cargo install cargo-deny --locked`).

```bash
pnpm install
cargo check --workspace
pnpm dev        # once apps/desktop is scaffolded
```

Local checks (the same gates CI will run; see [docs/convention.md](docs/convention.md) section 8):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
pnpm lint && pnpm typecheck && pnpm test:tools
pnpm check:deps          # crate dependency rules (docs/architecture.md section 3.3)
```

See [docs/structure.md](docs/structure.md) for layout and conventions.
