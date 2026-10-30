# Development conventions

Binding rules for everyone (and Claude Code) working in this repo. Companion to [structure.md](structure.md) (layout) and [specification.md](specification.md) (design, budgets, API). If a rule here conflicts with the spec, fix the conflict in a PR; don't ignore either.

**Enforcement tags:** `[CI]` fails the build, `[review]` is checked in PR review. Anything untagged is `[review]`.

## 1. Code style

**Rust** (`crates/`, `apps/desktop/src-tauri`)
- `cargo fmt` default settings; `cargo clippy --workspace -- -D warnings` `[CI]`. Toolchain pinned in `rust-toolchain.toml`; edition 2021.
- `#![forbid(unsafe_code)]` in every `gtp-*` crate. Unsafe lives only in third-party bindings; a new `unsafe` block needs an ADR.
- No `unwrap()`/`expect()`/`panic!`/`todo!()` outside tests and `main` startup. Use `?` with typed errors. `[CI]` via clippy lints (`unwrap_used`, `expect_used`, `panic`, `todo`) set in the workspace `Cargo.toml`.
- Image buffers are `f32` linear, in place where possible. Integer 16-bit only at file I/O boundaries (see `rules/colour-pipeline.md`).
- Public items have a `///` doc line; each crate has a `//!` header stating its one responsibility.
- Prefer small functions and plain data (structs, enums, traits). No macros, generics or async unless they remove real duplication. Blocking work stays off the Tauri main thread (jobs, rayon).

**TypeScript** (`apps/desktop/src`, `packages/`)
- `strict: true`; no `any`, no `@ts-ignore` (use `@ts-expect-error` with a reason). ESLint (with `jsx-a11y`) and Prettier `[CI]`.
- Function components and hooks only; named exports only (no default exports).
- State: Zustand store per feature; no global mutable singletons. Components do not call `invoke` directly: use the typed wrappers from `@gtp/ipc-types`.
- Styling: CSS variables + CSS modules; no inline styles except values computed at runtime.
- Every user-visible string goes through `t('key')` with an entry in `en.json` `[CI]` (lint rule `no-literal-string` on JSX text).

**Markdown/docs:** sentence-case headings, tables for reference data, no marketing language. Docs change in the same PR as the behaviour they describe.

## 2. Naming

| Thing | Convention | Example |
|---|---|---|
| Rust crates | `gtp-<area>` | `gtp-negative` |
| Rust items | std Rust (`snake_case` fns, `UpperCamel` types, `SCREAMING_CASE` consts) | `fn measure_film_base` |
| TS packages | `@gtp/<name>` | `@gtp/ui` |
| TS files | `PascalCase.tsx` for components, `kebab-case.ts` otherwise | `CurveEditor.tsx`, `use-recipe.ts` |
| Tauri commands | `snake_case`, `<noun>_<verb>` as in spec §6 | `roll_create`, `develop_render` |
| Events | `<area>://<name>` | `job://progress` |
| DB tables/columns | singular `snake_case`; ULID `TEXT` ids | `frame_tag.frame_id` |
| Feature flags | `snake_case`, documented in spec §10 | `guided_capture` |
| Cargo features | `snake_case` | `tether` |
| ADRs | `NNNN-kebab-title.md` | `0005-undo-history.md` |
| Branches | `<type>/<short-kebab>` | `feat/film-base-sampler` |
| Names with units | unit suffix | `settle_ms`, `grow_px`, `angle_deg`, `temp_c`, `vbus_mv`, `pulse_10ms` |
| Pixel buffers | state encoding in the name or type | `rgb_linear`, `rgba8_srgb` |

Avoid abbreviations except those in this table's examples and standard ones (`rgb`, `ir`, `icc`, `ev`, `dpi`). Domain words are fixed: **roll, frame, recipe, source file, tag, library** (spec §4); don't invent synonyms.

## 3. Structure

- Dependency direction (structure.md §4) `[CI]` via `cargo deny`/a workspace dependency check script: `gtp-core` ← `gtp-io` ← feature crates (`ingest`, `capture`, `negative`, `edit`, `catalog`). Feature crates never depend on each other; `gtp-app` (the application layer, ADR 0005) is the only crate that combines them, and `src-tauri` is a thin adapter over it. The allowed-edges table is in [architecture.md §3.3](architecture.md) and is what the check script enforces.
- `src-tauri` commands are thin: parse and validate, call a crate, map errors. No pixel math, SQL or protocol code there.
- The UI never touches pixels; it sends a `Recipe` and draws bytes it receives.
- Hardware sits behind traits (`ScannerSource`, `LightSource`, `CameraSource`) with a fake for each. New hardware code without a fake is rejected.
- Where things go: pipeline stages in `gtp-negative`/`gtp-edit`; file formats in `gtp-io`; SQL only in `gtp-catalog`; shared types in `gtp-core`; shared UI in `packages/ui`; docs in `docs/`; decisions in `decisions/`; rules in `rules/`; prompts in `prompts/`; fixtures in `samples/`.
- `packages/ipc-types` is generated from Rust. Never hand-edit; regenerate and commit `[CI]` (CI diffs it).
- A new crate requires: README row in `crates/README.md`, entry in structure.md, the `//!` header, and a mention in the spec if it changes architecture.
- Recipes are versioned JSON; changing the shape means a new `v` and a migration function in `gtp-core` with a test.

## 4. Validation

Validate at the boundary once, then pass typed values (newtypes/enums) inward. Never trust the UI, a file, a serial port or a camera.

| Input | Rule |
|---|---|
| Paths | canonicalize; must resolve inside the library or a directory the user picked through the OS dialog |
| Image files | check dimensions (max 20,000 px per side) and decoded size (max 2 GB) **before** allocating |
| Recipe values | clamp to documented ranges; unknown fields ignored; missing fields default |
| Strings | trim; names at most 200 chars; tags lower-cased, at most 64 chars, no control characters |
| Enums/ids | parse into enums/ULID types; reject unknown values with `InvalidInput` |
| Scan params | frames 1..=40, dpi from the supported list, samples in {1,2,4,8,16} |
| Serial packets | start byte `0xFE`, length checked against buffer, unknown headers dropped |
| Capture result | three files, one per colour, EXIF exposure/ISO/aperture equal (warn), no clipping (warn) |

## 5. Error handling

- Libraries return `Result<T, E>` with `thiserror` enums. `anyhow` only in `src-tauri` and `tools/`.
- The command layer maps every error to `AppError { code, message, details? }` with the codes in spec §6. `message` is user-readable and actionable; `details` holds technical context. No raw OS errors or file paths in `message`.
- Add context where the error crosses a module (`.context("reading ir plane")`). Never swallow an error silently; if ignoring is correct, comment why.
- Cancellation is `Cancelled`, not a failure: no error log, no error dialog.
- Cleanup runs on every exit path: the light is switched off, temp files removed, partial frames not committed (`Drop` guard or explicit cleanup). Library writes are transactional; files are deleted only after the DB commit.
- Panics are bugs. A panic hook logs a crash note (spec §12); expected failures never panic.

## 6. Logging

- `tracing` only. No `println!`/`eprintln!`/`dbg!`/`console.log` in committed code `[CI]` (clippy `print_stdout`, ESLint `no-console`).
- Levels: `error` user-visible failure, `warn` degraded or suspicious (clipped exposure, odd firmware), `info` job start/finish, library open, migration, `debug` detail (off by default).
- Spans around jobs and pipeline stages with `frame_id`, `job_id`, `stage`, `ms`.
- Never log pixel data, serial/camera payloads above `debug`, or full file paths at `info`. No secrets (there should be none).
- Frontend errors go through `log_client_event`; user text never contains stack traces.

## 7. Testing (mandatory)

A PR is not mergeable without these `[review]` unless noted:

- **Every pipeline function** has a unit test, and the stage has a golden-image test against a fixture in `samples/public` (or a synthetic density wedge). Goldens are regenerated only by the explicit command and the diff is reviewed.
- **Every bug fix** adds a test that fails before the fix.
- **Every new command** has a test through the command layer using fakes; every new recipe field has default and migration tests.
- **Every hardware path** (scanner, Scanlight, camera) is tested against its fake; protocol encode/decode has byte-exact tests. Real-hardware behaviour goes in the manual checklists (`docs/research/*`), run before releases that touch ingest or capture.
- **UI:** components with logic get Vitest + Testing Library tests; interactive components also pass `vitest-axe` `[CI]`.
- Tests are deterministic (seeded RNG, no clock or network, no dependence on `samples/private`).
- Changes on a hot path (spec §11) include a `criterion` result or a note that budgets still hold.
- No fixed coverage percentage; the PR description lists what was tested and what was not.

## 8. CI gates

Every PR must pass on Windows and macOS (arm64) `[CI]`, and `main` is protected: PR required, CI green, squash merge.

1. `cargo fmt --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `pnpm lint`, `pnpm typecheck`, `pnpm test`
5. `cargo deny check` (licences, advisories, bans) and `pnpm audit --prod`
6. `ipc-types` regenerated with no diff
7. macOS: build and launch smoke test; Windows: `tauri-driver` end-to-end test (spec §8)

Tag builds (`vX.Y.Z`) additionally run the release pipeline (spec §15). A budget miss from spec §11 blocks a release.

## 9. Commits and pull requests

- **Conventional Commits:** `type(scope): summary`, imperative, at most 72 chars. Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`. Scope is a crate or area: `negative`, `edit`, `capture`, `ingest`, `catalog`, `io`, `core`, `desktop`, `ui`, `ipc`, `docs`, `adr`. Breaking changes: `!` plus a `BREAKING CHANGE:` footer.
- One logical change per commit and per PR; keep PRs small enough to review in one sitting. No unrelated reformatting.
- PR description: what and why, how it was tested (see §7), budget/perf impact, docs/ADR updated. Link the milestone from spec §16.
- Decisions that change architecture, dependencies, storage or hardware approach need an ADR in `decisions/` in the same PR.
- Never force-push `main`. Don't commit generated build output, scans from `samples/private`, or secrets.
- `CHANGELOG.md` is generated from commits; write messages as if they were release notes.

## 10. Security and privacy

- Local only: no network calls except the update check (spec §9). No telemetry or analytics; counters stay in the local `meta` table.
- Tauri capabilities are least-privilege; strict CSP; no remote content, no shell plugin, no WebSerial. Serial, camera and filesystem access happen only in Rust on user-chosen targets.
- Originals are immutable (read-only after write) and hash-verified; destructive actions need confirmation.
- Dependencies: lockfiles committed, new dependency justified in the PR (size, licence, maintenance), `nkscan` pinned exactly; `cargo deny`/audit pass `[CI]`.
- Scanlight rules: `SET_COLOR` save flag is always 0, no `SET_TRIM`, light off on exit/cancel/error, abort above `temp_abort_c`.
- Signing keys and tokens live only in GitHub Actions secrets; never in the repo or logs.
- `samples/private/` is git-ignored; never commit personal scans.

## 11. Performance

- Budgets in spec §11 are requirements, not goals. Measure before optimizing; keep the simple version until a budget is missed.
- No allocation inside per-pixel loops; process by rows with `rayon`; reuse buffers between renders; one `f32` working buffer per export.
- Preview goes over binary IPC (`tauri::ipc::Response`), never JSON/base64; drag renders use the smaller size; stale renders are dropped by sequence number.
- Never block the UI or the Tauri main thread: long work is a job with progress and cancel.
- GPU, caching layers and extra threads need a measured budget miss and an ADR.

## 12. Do / Don't

**Do**
- Keep originals immutable and put every edit in the Recipe.
- Put hardware behind a trait and write the fake first.
- Validate at the boundary, then use typed values.
- Run `cargo fmt`, `clippy`, tests and `pnpm lint` before pushing.
- Update docs, ADRs and the spec table in the same PR as the change.
- Write the failing test before fixing a bug.
- Use unit suffixes in names and the fixed domain vocabulary.
- Log with `tracing` spans; report errors through `AppError`.
- Switch the Scanlight off on every exit path.

**Don't**
- Don't add masking, AI generation or retouching (non-goals, spec §1.2).
- Don't make feature crates depend on each other, or put logic in `src-tauri` commands.
- Don't process pixels in the UI or hand-edit `packages/ipc-types`.
- Don't use `unwrap`, `unsafe`, `println!`, `console.log`, `any`, or default exports.
- Don't hard-code user-visible strings or write dates/numbers without `Intl`.
- Don't log pixel data or full paths at `info`; don't add telemetry.
- Don't save Scanlight presets or write to its non-volatile memory.
- Don't commit scans, build output or secrets.
- Don't add a dependency, flag, or abstraction "for later"; wait for a measured need (KISS).
- Don't merge with failing CI or skip hooks.
