# 0005. Application layer crate (`gtp-app`)

- Status: Proposed (accept when GTP-029 starts; the human approves)
- Date: 2026-10-29

## Context

Feature crates (`gtp-negative`, `gtp-edit`, `gtp-ingest`, `gtp-capture`, `gtp-catalog`) must not depend on each other (convention §3). The render pipeline needs `gtp-negative` and `gtp-edit` together, and import, scan, capture and export each combine several crates. The spec said `src-tauri` composes the crates, but it also forbids business logic in `src-tauri`. That left no home for composition.

## Decision

Add `crates/gtp-app` as the application layer.
- It is the only crate allowed to depend on all `gtp-*` crates.
- It holds the use-case services (library, roll/frame, import, develop, export, scan, capture, tags/search, settings, diagnostics), the `JobManager` and `JobSink` trait, and `pipeline::{render, analyze}`.
- It does not depend on Tauri. `src-tauri` implements `JobSink` and exposes each service method as a command, mapping errors to `AppError`.
- The allowed-edges table is in `docs/architecture.md` §3.3 and is enforced by `tools/scripts/check-deps`.

## Consequences

- Services, jobs and the pipeline are testable without Tauri or a window.
- `src-tauri` stays thin; moving the UI shell later would not touch the logic.
- One more crate to maintain; no other crate gains a dependency.
- Docs updated with this ADR: structure.md, crates/README.md, convention.md §3, specification.md §3, issues.md (GTP-029, GTP-040).
- The crate itself is created in GTP-029; until then it appears in the docs as planned.
