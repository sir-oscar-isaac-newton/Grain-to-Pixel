# Coding rules

Short form. The full, authoritative rules are in [docs/convention.md](../docs/convention.md).

- Rust: `cargo fmt`, `cargo clippy -- -D warnings`. Crates return `Result`; no `unwrap()` outside tests.
- Feature crates (`ingest`, `negative`, `edit`, `catalog`) depend only on `core`/`io`, never on each other; `gtp-app` is the only crate that combines them.
- TypeScript: strict mode; IPC types come from `@gtp/ipc-types`, never redeclared in the app.
- Every image-processing function gets a test against a small fixture from `samples/public`.
- Scope guard: no masking, AI generation, or advanced retouching (see `docs/product/idea.md`).
- Paths and scripts must work on Windows and macOS.
