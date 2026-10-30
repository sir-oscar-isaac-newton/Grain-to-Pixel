# Architecture and staged implementation plan

Grain to Pixel: a personal Windows/macOS app that scans, converts, edits and organizes film negatives. This document selects the tech stack, defines a clean modular architecture and the full project composition, and gives a **copy-pastable prompt for every implementation step**.

Related: [idea](product/idea.md) · [structure.md](structure.md) · [specification.md](specification.md) · [convention.md](convention.md) · [workflow.md](workflow.md) · [issues.md](issues.md).

> Path note: the idea one-pager is at `docs/product/idea.md` (a copy also sits at the repo root as `idea.md`). Prompts use `docs/product/idea.md`.

**How to use the prompts (§8):** paste one prompt per session, in order. Each tells the assistant to follow [workflow.md](workflow.md): post a proposal and **stop for approval**, then implement with tests and docs, open a PR, and ask for the next step. Each prompt maps to issues in [issues.md](issues.md).

> **Issue IDs changed.** The step prompts below were written with the earlier `GTP-0NN` IDs. The backlog was audited and re-issued with area-tagged IDs (`T-`, `R-`, `P-`, `L-`, `H-`, `D-`, `U-`, `Q-`); the mapping is in [issues.md](issues.md) ("Mapping from the previous IDs"). Wherever a prompt says "update docs/issues.md" it now means: update the GitHub issue (status label, ticked criteria, comments) and regenerate `docs/issues.md` only when the plan itself changes. Each GitHub issue also carries its own, self-contained prompt, which is the preferred one to paste.

---

## 1. Stack selection

### 1.1 What the idea demands
Windows 10/11 and macOS from one codebase; heavy 16-bit image maths on 20+ MP scans; USB hardware (scanner, serial light, camera); a rich editing UI; free to build and ship; a single developer working through Claude Pro limits; friends as users (small installers, no licences).

### 1.2 Selected stack

| Concern | Choice | Why it fits |
|---|---|---|
| Shell | **Tauri 2** | one codebase for Win/mac; installers of tens of MB; Rust backend owns hardware and files |
| Core language | **Rust (stable)** | speed for pixel work without a runtime; memory safety on untrusted files; nkscan is a Rust crate; `rayon` for CPU parallelism |
| UI | **React 18 + TypeScript (strict) + Vite** | mature ecosystem for editor-style UIs; strong typing end to end |
| UI state / primitives | **Zustand**, **Radix UI**, CSS modules, **@tanstack/react-virtual** | small, no framework sprawl; accessible controls; virtualized thumbnail grid |
| IPC contract | **tauri-specta** → `packages/ipc-types` | Rust is the single source of truth for command, event and error types |
| Catalog | **SQLite** (`rusqlite` bundled, FTS5, `rusqlite_migration`) | one file per library, full-text search, no server |
| Images | `tiff`, `lcms2`, `jpeg-encoder`, `fast_image_resize`, `rawler` (NEF) | pure CPU f32 pipeline; GPU only if budgets fail |
| Hardware | `nkscan` (LS-50), `serialport` (Scanlight), `gphoto2` binding (macOS tether), `notify` (watched folder) | all behind traits with fakes |
| Observability | `tracing` family | structured, rolling files, local only |
| Tests | cargo test, `proptest`, `criterion`, Vitest + Testing Library + `vitest-axe`, `tauri-driver` | matches spec §8 |
| Tooling | pnpm, GitHub Actions, `cargo-deny` | spec §8, §9 |

Versions: use the latest stable release at implementation time and pin through lockfiles; record exceptions in an ADR.

### 1.3 Alternatives considered

| Option | Rejected because |
|---|---|
| Electron + Node | 100+ MB installers; pixel work needs native addons anyway; hardware access via Node bindings is weaker |
| Qt/C++ | highest development cost; slow iteration with an AI assistant; licensing questions |
| Swift (mac) + C# (Windows) | two codebases for a one-person project |
| Python + Qt | packaging and performance risks for 21 MP float images; harder cross-platform installers |
| Pure web app | no USB serial, scanner or filesystem control |
| GPU-first (wgpu/WebGL) | extra complexity before any budget is missed; revisit per spec §11 |

---

## 2. Architectural principles

1. **Clean layers, dependencies point inward.** Domain has no I/O; application orchestrates; infrastructure adapters implement ports; the Tauri shell is a thin adapter.
2. **Ports and adapters for every device and store.** `ScannerSource`, `LightSource`, `CameraSource`, and the catalog are interfaces with real and fake implementations. All logic is testable without hardware or a window.
3. **Rust owns state and pixels; the UI renders.** The UI sends a `Recipe` and receives bytes (spec §3).
4. **Immutable originals, parametric edits.** Everything editable lives in the `Recipe`.
5. **Jobs for anything long.** Scan, import, export, capture, verify: progress events, cancel, commit per frame.
6. **KISS.** No GPU, plugin system, server, ORM or extra abstraction without a measured need.

---

## 3. Layers and modules

### 3.1 Layer map

```
┌──────────────────────────── Presentation ────────────────────────────┐
│ apps/desktop/src            React UI (features/*, shared/*)           │
│ packages/ui                 reusable components                       │
│ packages/ipc-types          GENERATED command/event/error types      │
├────────────────────────── Interface adapter ─────────────────────────┤
│ apps/desktop/src-tauri      Tauri commands, event sink, app state     │
├──────────────────────────── Application ────────────────────────────┤
│ crates/gtp-app              use-case services, JobManager, pipeline   │
├───────────────────── Domain + feature libraries ─────────────────────┤
│ gtp-core      entities, value objects, Recipe, errors, ImageF32       │
│ gtp-negative  film base, inversion, frame detection (pure functions)  │
│ gtp-edit      colour, tone, geometry, IR dust (pure functions)        │
├────────────────────────── Infrastructure ────────────────────────────┤
│ gtp-io        TIFF, NEF decode, ICC, export encoders, hashing         │
│ gtp-catalog   SQLite repositories, migrations, FTS                    │
│ gtp-ingest    ScannerSource port + Nkscan/Fixture adapters, triplets  │
│ gtp-capture   LightSource/CameraSource ports + Scanlight/gphoto/folder│
└──────────────────────────────────────────────────────────────────────┘
```

### 3.2 Crate responsibilities

| Crate | Responsibility | Key public API |
|---|---|---|
| `gtp-core` | types shared everywhere; no I/O | `ImageF32`, `RollId/FrameId/TagId`, `Roll`, `Frame`, `SourceFile`, `Recipe` (v1, clamp, migrate), `AppError`, `testing::synthetic` (feature `testing`) |
| `gtp-io` | file formats and colour output | `read_tiff16`, `write_tiff16`, `Limits`, `sha256_file`, `OutputSpace`, `encode_tiff16`, `encode_jpeg`, `decode_nef` |
| `gtp-negative` | negative to positive maths | `sample_film_base`, `auto_film_base`, `roll_base`, `normalize`, `invert`, `auto_levels`, `detect_frame` |
| `gtp-edit` | creative and corrective stages | `apply_color`, `apply_tone`, `Curve`, `apply_geometry`, `remove_dust` |
| `gtp-catalog` | persistence | `Catalog::open`, repositories for roll/frame/source/recipe/tag/search/meta, `migrate`, `backup` |
| `gtp-ingest` | getting images in | `ScannerSource`, `NkscanSource`, `FixtureSource`, `group_triplets`, `merge_triplet` |
| `gtp-capture` | guided R/G/B capture | `LightSource`, `ScanlightV4`, `CameraSource`, `GphotoCamera`, `WatchedFolder`, `CaptureSequence`, fakes |
| `gtp-app` | application layer | `AppContext`, `JobManager`, services (`Library`, `Import`, `Develop`, `Export`, `Scan`, `Capture`, `TagsSearch`, `Settings`, `Diagnostics`), `pipeline::{render, analyze}` |
| `src-tauri` | adapter | one module per command group, `TauriJobSink`, error mapping, logging init |

### 3.3 Allowed dependency edges (enforced by `tools/scripts/check-deps`)

| Crate | May depend on |
|---|---|
| `gtp-core` | external crates only |
| `gtp-io`, `gtp-negative`, `gtp-edit`, `gtp-catalog`, `gtp-capture` | `gtp-core` |
| `gtp-ingest` | `gtp-core`, `gtp-io` |
| `gtp-app` | all `gtp-*` crates (the only crate allowed to combine feature crates) |
| `src-tauri` | `gtp-app`, `gtp-core` |
| `tools/benchmark` | `gtp-app`, `gtp-core` |

Feature crates never depend on each other. This resolves the composition gap noted in issue GTP-029 (see §9).

### 3.4 Key interfaces

```rust
// gtp-core
pub struct ImageF32 { pub width: u32, pub height: u32, pub channels: u8, pub data: Vec<f32> } // linear, row-major
pub enum FrameKind { ScanRgbIr, RgbTriplet }
pub struct Recipe { pub v: u32, pub geometry: Geometry, pub base: Base, pub invert: Invert,
                    pub color: Color, pub tone: Tone, pub dust: Dust, pub output: Output }  // spec §4.1
pub enum ErrorCode { NotFound, InvalidInput, Io, Db, UnsupportedFormat, ScannerUnavailable,
                     ScanFailed, Cancelled, LibraryTooNew, Internal }
pub struct AppError { pub code: ErrorCode, pub message: String, pub details: Option<String> }

// gtp-ingest (port)
pub trait ScannerSource {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>>;
    fn scan(&self, p: &ScanParams, on_frame: &mut dyn FnMut(u32, RawScan) -> Result<()>,
            progress: &dyn Fn(f32), cancel: &CancelToken) -> Result<()>;
}

// gtp-capture (ports)
pub trait LightSource { fn connect(&mut self, port: Option<&str>) -> Result<LightInfo>;
    fn set_levels(&mut self, rgb: [u8; 3]) -> Result<()>;           // SET_COLOR, save flag always 0
    fn shutter_pulse(&mut self, tens_of_ms: u8) -> Result<()>;
    fn status(&self) -> LightStatus; }
pub trait CameraSource { fn arm(&mut self) -> Result<()>;
    fn trigger(&mut self, light: &mut dyn LightSource) -> Result<()>;
    fn wait_for_image(&mut self, timeout: Duration) -> Result<CapturedFile>; }

// gtp-app
pub struct PipelineInput { pub rgb: Arc<ImageF32>, pub ir: Option<Arc<ImageF32>>, pub film_base: Option<[f32; 3]> }
pub enum RenderTarget { Preview { max_edge: u32 }, Full }
pub fn render(input: &PipelineInput, recipe: &Recipe, target: RenderTarget) -> Result<ImageF32>;
pub fn analyze(input: &PipelineInput) -> Result<Analysis>;         // crop, base, histogram, auto recipe
pub trait JobSink: Send + Sync { fn progress(&self, id: JobId, stage: &str, fraction: f32);
    fn done(&self, id: JobId, result: JobResult); fn error(&self, id: JobId, err: AppError); }
pub struct JobManager; // spawn(kind, FnOnce(JobCtx) -> Result<JobResult>) -> JobId; cancel(JobId)
pub struct JobCtx;     // progress(stage, fraction); is_cancelled()
```

### 3.5 Data flows

**Develop (interactive):** UI slider → `develop_render(frame_id, recipe, 1600, seq)` → `DevelopService` loads the proxy from the 4-entry LRU (built on `develop_open`) → `pipeline::render` (stages in fixed order) → RGBA8 bytes via `tauri::ipc::Response` → canvas. Stale results dropped by `seq`.

**Scan roll:** `scan_start(params, roll_id)` → `ScanService` creates a job → `ScannerSource::scan(..., on_frame)` → per frame: write TIFFs into `originals/`, hash, thumbnail, catalog insert in one transaction, emit `library://changed` → cancel keeps finished frames; resume starts at the next frame number.

**Capture triplet:** `capture_triplet(roll_id)` → `CaptureService` job → for R, G, B: `set_levels` → settle → `arm` → `trigger` → `wait_for_image` → light off in all paths → validate EXIF and clipping → `merge_triplet` (NEF decode, demosaic, channel pick) → store as `RgbTriplet` frame.

**Export:** `export_start(ids, options)` → job → per frame: load original, `pipeline::render(Full)`, output transform with ICC (`gtp-io`), write file, log in `export_log`.

---

## 4. Full project composition

```
grain-to-pixel/
├─ Cargo.toml                    workspace: crates/*, apps/desktop/src-tauri, tools/benchmark
├─ rust-toolchain.toml  rustfmt.toml  deny.toml  .nvmrc  .editorconfig
├─ package.json  pnpm-workspace.yaml  eslint.config.js  .prettierrc  tsconfig.base.json
├─ .gitignore  .gitattributes (LFS: tif, tiff, dng, nef, icc)
├─ .github/  workflows/{ci.yml, release.yml}  pull_request_template.md  dependabot.yml
├─ CLAUDE.md  README.md  idea.md
│
├─ apps/desktop/
│  ├─ package.json  vite.config.ts  tsconfig.json  index.html
│  ├─ src/
│  │  ├─ main.tsx  App.tsx
│  │  ├─ app/            store.ts (view state)  error-boundary.tsx
│  │  ├─ features/
│  │  │  ├─ library/     LibraryView.tsx  RollSidebar.tsx  FrameGrid.tsx  SearchBar.tsx  use-library.ts
│  │  │  ├─ develop/     DevelopView.tsx  Canvas.tsx  Filmstrip.tsx  panels/{BaseInvert,Color,Tone,Geometry,Dust}Panel.tsx  use-develop.ts
│  │  │  ├─ import/      ImportView.tsx  ScannerSection.tsx  FilesSection.tsx  CaptureSection.tsx
│  │  │  └─ settings/    SettingsView.tsx  FlagsList.tsx
│  │  └─ shared/         ipc.ts (typed wrappers)  i18n/{index.ts,en.json}  styles/{tokens.css,base.css}  hooks/
│  └─ src-tauri/
│     ├─ Cargo.toml  tauri.conf.json  build.rs  capabilities/default.json  icons/
│     └─ src/  main.rs  lib.rs  state.rs  error.rs  events.rs  logging.rs  sink.rs
│              commands/{mod,library,roll,frame,ingest,capture,develop,tags,search,export,system}.rs
│
├─ crates/
│  ├─ gtp-core/        src/{lib,ids,error,image}.rs  model/{roll,frame,source,tag}.rs
│  │                    recipe/{mod,v1,clamp,migrate}.rs  testing/{mod,synthetic}.rs
│  ├─ gtp-io/          src/{lib,limits,tiff,hash,icc}.rs  export/{tiff16,jpeg}.rs  raw/{nef,demosaic}.rs
│  ├─ gtp-negative/    src/{lib,film_base,normalize,invert,frame_detect}.rs
│  ├─ gtp-edit/        src/{lib,color,tone,curve,geometry,dust}.rs
│  ├─ gtp-catalog/     src/{lib,db,backup,fts}.rs  repo/{roll,frame,source,recipe,tag,search,meta}.rs
│  │                    migrations/{0001_init,0002_search,0003_export_log}.sql
│  ├─ gtp-ingest/      src/{lib,scanner,nkscan,fixture,files,triplet}.rs
│  ├─ gtp-capture/     src/{lib,light,camera,sequence,profile,fakes,gphoto,watched}.rs  scanlight/{mod,protocol,port}.rs
│  └─ gtp-app/         src/{lib,context,paths,library_fs}.rs  jobs/{mod,manager,cancel}.rs
│                       pipeline/{mod,stages,proxy,analyze}.rs
│                       services/{library,roll_frame,import,develop,export,scan,capture,tags_search,settings,diagnostics}.rs
│     (each crate also has tests/ for integration tests)
│
├─ packages/
│  ├─ ipc-types/       GENERATED (src/bindings.ts) + src/index.ts
│  └─ ui/              src/{Slider,CurveEditor,Histogram,TagChips,ProgressList,StatusBadge}.tsx  src/i18n/
├─ tools/
│  ├─ benchmark/       Rust bin: ΔE2000, golden compare/bless, reference report, criterion benches
│  └─ scripts/         check-deps/  check-ipc/  spikes/ (throwaway)
├─ samples/{public/{golden,scanlight}/, private/{reference,...}/}   MANIFEST.md
├─ docs/               structure, specification, architecture, convention, workflow, issues, product/, research/
├─ decisions/          0000..0005 ADRs
├─ rules/  prompts/    (prompts/ may hold the §8 prompts as files)
└─ .claude/
```

### 4.1 Persistence: migrations (`gtp-catalog/migrations`)

| File | Content |
|---|---|
| `0001_init.sql` | tables `roll`, `frame`, `source_file`, `recipe`, `tag` (NOCASE unique), `frame_tag`, `meta`; indexes `idx_frame_roll`, `idx_frame_tag_tag`; foreign keys ON DELETE CASCADE (spec §5.2) |
| `0002_search.sql` | `CREATE VIRTUAL TABLE search USING fts5(frame_id UNINDEXED, text)` |
| `0003_export_log.sql` | table `export_log` |

Forward-only; the library backs up to `library.db.bak-v<N>` before migrating; pragmas WAL, `foreign_keys=ON`, `synchronous=NORMAL`.

### 4.2 Interface catalogue (Tauri commands = the app's "routes")

| Group | Commands (request → response) | Service | Step |
|---|---|---|---|
| system | `library_info() → LibraryInfo`, `settings_get() → Settings`, `settings_set(patch)`, `diagnostics_export(dest)`, `log_client_event(level, msg)` | Settings, Diagnostics | 0.2, 0.4, 2.2, 3.3, 7.2 |
| library | `library_create(path, name)`, `library_open(path) → LibraryInfo`, `library_verify() → JobId` | Library | 2.2, 2.3 |
| roll | `roll_create(RollInput) → Roll`, `roll_update(RollId, RollPatch)`, `roll_list() → Roll[]`, `roll_delete(RollId)`, `roll_set_film_base(RollId, [f32;3])` | RollFrame | 2.2 |
| frame | `frame_list(FrameQuery) → Page<FrameSummary>`, `frame_get(FrameId)`, `frame_delete(FrameId[])`, `frame_set_note(FrameId, String)` | RollFrame | 2.2 |
| ingest | `import_files(paths, RollId, ImportKind) → JobId`, `triplet_group(paths) → TripletCandidate[]`, `scanner_list() → DeviceInfo[]`, `scan_start(ScanParams, RollId) → JobId` | Import, Scan | 2.3, 4.1, 5.1 |
| capture | `light_list()`, `light_connect(port?) → LightInfo`, `light_set([u8;3])`, `light_off()`, `camera_list()`, `capture_profile_get/set`, `capture_test(Channel) → TestShot`, `capture_triplet(RollId) → JobId` | Capture | 6.3 |
| develop | `develop_open(FrameId) → DevelopInfo`, `develop_analyze(FrameId) → Analysis`, `develop_render(FrameId, Recipe, max_edge, seq) → bytes`, `recipe_save(FrameId, Recipe)`, `recipe_apply_to(FrameId[], source, RecipeField[])` | Develop | 3.1, 3.3 |
| tags | `tag_list()`, `tag_apply(FrameId[], add[], remove[])`, `tag_rename`, `tag_delete` | TagsSearch | 2.4 |
| search | `search(SearchQuery, page) → Page<FrameSummary>` | TagsSearch | 2.4 |
| export | `export_start(FrameId[], ExportOptions) → JobId` | Export | 3.3 |
| jobs | `job_cancel(JobId)` | JobManager | 2.1 |

Events: `job://progress|done|error`, `library://changed`, `capture://step`, `light://status`. Error shape: `AppError { code, message, details? }` (spec §6).

### 4.3 Cross-cutting

- **Errors:** `thiserror` per crate, mapped once to `AppError` in `src-tauri/error.rs`; cleanup guards (light off, partial frames) on all exits.
- **Logging:** `tracing` with spans per job and stage; JSON rolling file; no pixel data (spec §12).
- **Config:** `settings.json` (app) + `meta` table (library); flags in spec §10.
- **Security:** least-privilege capabilities, strict CSP, validated paths and limits, local only (spec §9; convention §10).
- **Testing pyramid:** unit per crate (synthetic fixtures, proptest) → golden whole-pipeline → fakes for services/jobs → component tests → e2e (spec §8).
- **CI:** gates in workflow §8; hardware checks stay manual checklists.

---

## 5. Staged plan overview

| Stage | Milestone | Steps | Issues | Deliverable |
|---|---|---|---|---|
| 0 Foundations | M0 | 0.1 to 0.5 | GTP-001 to 012 | runnable app, CI, typed IPC, logging, spike answers |
| 1 Pipeline | M1 | 1.1 to 1.6 | GTP-020 to 032 | converted images from files, golden + benchmark |
| 2 Library | M2 | 2.1 to 2.5 | GTP-040 to 047 | catalog, import, tags, search, Library view |
| 3 Develop | M3 | 3.1 to 3.3 | GTP-050 to 057 | full file-based workflow |
| 4 LS-50 | M4 | 4.1, 4.2 | GTP-060 to 064 | roll scan with IR |
| 5 Triplets | M5 | 5.1 | GTP-070 to 072 | NEF triplet import |
| 6 Capture | M6 | 6.1 to 6.3 | GTP-080 to 085 | Scanlight + camera guided capture |
| 7 Release | M7 | 7.1 to 7.3 | GTP-090 to 096 | v0.1.0 |

Critical path: 0 → 1 → 2 → 3, then 4, 5 and 6 (6 needs 5). Spikes 0.5 can run any time after 0.1 and decide the M4 and M6 designs.

---

## 6. Standard prompt contract

Every prompt below carries the same CONTEXT and PROCESS blocks so it can be pasted alone. All prompts require: tests per convention §7, docs updated, `docs/issues.md` updated, and the gates in workflow §8 run.

---

## 7. Notes for prompt authors

- Paste prompts unchanged (only `{SPIKE}` in step 0.5 is filled in); they reference exact files. If reality differs (a crate API changed), the assistant must say so in its proposal.
- Prompts list **acceptance** in addition to the issue's own criteria; both must be met.
- Hardware steps end with a checklist for the human; the assistant never ticks `[hw]` items.

---

## 8. Implementation prompts

### Stage 0: Foundations

#### Step 0.1: Git, tooling, dependency rules (GTP-001, GTP-002)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: personal Windows/macOS app to scan, convert, edit and organize film negatives; non-goals: masking, AI generation).
- Structure: docs/structure.md (monorepo: Cargo workspace crates/ with gtp-* crates, pnpm workspace apps/ + packages/).
- Also read: docs/architecture.md §3.3 and §4, docs/convention.md §1-3 and §8, docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-001 and GTP-002 and STOP for my approval before writing code.

TASK: put the scaffold under Git and make the convention rules enforceable locally.
PREREQUISITES (human): Git, Git LFS, pnpm installed. Verify with `git --version`, `git lfs version`, `pnpm -v`; if missing, stop and tell me.
FILES:
- git: `git init -b main`, `git lfs install`, first commit. Add `*.nef` to .gitattributes LFS patterns.
- rust-toolchain.toml (pinned stable), rustfmt.toml, deny.toml (licence allowlist), .nvmrc (20).
- Cargo.toml: [workspace.lints.rust] unsafe_code = "forbid"; [workspace.lints.clippy] unwrap_used, expect_used, panic, todo, print_stdout, print_stderr, dbg_macro = "deny". Every crates/*/Cargo.toml gets `[lints] workspace = true`.
- eslint.config.js (typescript-eslint, jsx-a11y, no-console, no literal strings in JSX), .prettierrc, tsconfig.base.json (strict). package.json scripts: lint, typecheck, test (pnpm -r --if-present), format.
- tools/scripts/check-deps/: small Rust binary using `cargo metadata`; allowed edges are in docs/architecture.md §3.3. CLI: `check-deps [--manifest-path <Cargo.toml>]`, exit 1 and print every forbidden edge.
- tools/scripts/check-deps/tests/fixtures/bad-workspace/: a mini workspace where gtp-edit depends on gtp-negative.
INTERFACES/ROUTES/MIGRATIONS: none.
TESTS: check-deps passes on this workspace and fails on the bad fixture (integration test).
ACCEPTANCE:
- `git status` clean after the commit; `git check-ignore samples/private/x.tif` and `target/` report ignored; `git lfs track` lists TIFF/DNG/NEF/ICC.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check`, `pnpm lint`, `pnpm typecheck` all pass.
- A deliberate `unwrap()` in a crate fails clippy (show the failure, then remove it).
OUT OF SCOPE: CI workflow, GitHub remote.
DONE WHEN: all acceptance items pass, docs/issues.md updated (GTP-001/002), then ask me for the next step.
```

#### Step 0.2: Desktop skeleton and typed IPC (GTP-003, GTP-004)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: scan, convert, edit, organize film negatives on Windows/macOS).
- Structure: docs/structure.md (apps/desktop = Tauri app: src-tauri Rust shell + src React UI; packages/ipc-types generated; crates/gtp-core).
- Also read: docs/architecture.md §3.1, §3.4, §4 (apps/desktop tree), §4.2; docs/specification.md §2, §3, §6, §9; docs/convention.md; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-003 and GTP-004 and STOP for my approval before writing code.

TASK: a window that launches on Windows and macOS, with Rust as the single source of truth for IPC types.
FILES:
- apps/desktop/{package.json, vite.config.ts, tsconfig.json, index.html}, src/{main.tsx, App.tsx}, src/app/store.ts (Zustand: view = 'library' | 'develop' | 'import'), src/features/{library,develop,import}/*View.tsx (empty placeholders using t()), src/shared/{ipc.ts, i18n/index.ts, i18n/en.json, styles/tokens.css}.
- apps/desktop/src-tauri/{Cargo.toml, tauri.conf.json, build.rs, capabilities/default.json}, src/{main.rs, lib.rs, state.rs, error.rs, events.rs}, src/commands/{mod.rs, system.rs}. Add to the Cargo workspace members.
- crates/gtp-core/src/error.rs: ErrorCode and AppError (see docs/architecture.md §3.4).
- packages/ipc-types/{package.json, src/index.ts, src/bindings.ts (GENERATED)}; root script `pnpm gen:ipc`; tools/scripts/check-ipc/ (fails when generated bindings are stale).
TYPES/ROUTES:
- `library_info() -> LibraryInfo { app_version: String }` in commands/system.rs, registered through tauri-specta. error.rs maps gtp-core errors to AppError.
- CSP default-src 'self'; no shell plugin; capabilities minimal.
MIGRATIONS: none.
TESTS: Rust unit tests for AppError JSON shape (one per ErrorCode); Vitest test rendering App and switching views; check-ipc test on a deliberately stale file.
ACCEPTANCE:
- `pnpm dev` opens the window on Windows; I will confirm macOS ([mac]).
- `pnpm gen:ipc` twice gives no diff; the UI calls library_info only via the typed wrapper in shared/ipc.ts.
- src-tauri passes the workspace lints; check-deps still passes.
OUT OF SCOPE: real features, CI.
DONE WHEN: acceptance items pass, docs (apps/desktop/README.md, packages/README.md) and docs/issues.md (GTP-003/004) updated, then ask me for the next step.
```

#### Step 0.3: CI and end-to-end smoke (GTP-005, GTP-006)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel, personal Windows/macOS film-negative app).
- Structure: docs/structure.md (monorepo; .github/ at the repo root; tools/scripts/ holds check-deps and check-ipc).
- Also read: docs/specification.md §8, §15; docs/convention.md §8; docs/workflow.md §5 (PR template) and §8; docs/architecture.md §4.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-005 and GTP-006 and STOP for my approval before writing code.
PREREQUISITE (human): GitHub repository exists and the remote is added.

TASK: gates 1-8 from workflow §8 run on every PR on Windows and macOS.
FILES:
- .github/workflows/ci.yml: matrix windows-latest and macos-14; steps: cargo fmt --check, clippy -D warnings, cargo test --workspace, pnpm install --frozen-lockfile, lint, typecheck, test, cargo deny check, pnpm audit --prod, tools/scripts/check-deps, tools/scripts/check-ipc; caching (Swatinem/rust-cache, pnpm store).
- .github/workflows/pr-title.yml: Conventional Commit title check.
- .github/pull_request_template.md: exactly the template in docs/workflow.md §5 Step 6.
- .github/dependabot.yml: cargo, npm, github-actions weekly.
- smoke tests: apps/desktop/tests/smoke/mac-launch.sh (build, start the app, wait for a "ready" log line, exit non-zero on crash); apps/desktop/tests/e2e/{wdio.conf.ts, window.e2e.ts} using tauri-driver on Windows: window title and library_info result.
- docs: a "Branch protection" section in docs/convention.md §8 listing the exact settings for me to apply.
INTERFACES/ROUTES/MIGRATIONS: none.
TESTS: a throwaway PR with a clean change (all green) and one with a lint error (red).
ACCEPTANCE:
- All gates run on both OS; the macOS smoke job and Windows e2e job are separate required checks.
- Failing lint turns CI red; clean PR is green.
- [human] branch protection applied per the documented settings.
OUT OF SCOPE: release workflow, packaging.
DONE WHEN: acceptance passes, docs/workflow.md §8 and docs/convention.md §8 no longer say "until CI exists", docs/issues.md updated (GTP-005/006), then ask me for the next step.
```

#### Step 0.4: Logging and error foundation (GTP-007)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel; local personal app shared with friends, so diagnostics must work offline).
- Structure: docs/structure.md (src-tauri adapter in apps/desktop/src-tauri; UI in apps/desktop/src).
- Also read: docs/specification.md §12; docs/convention.md §5, §6; docs/architecture.md §4.3; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-007 and STOP for my approval before writing code.

TASK: observability baseline.
FILES:
- apps/desktop/src-tauri/src/logging.rs: tracing-subscriber JSON layer writing a rolling file (daily, keep 7 days, 50 MB cap) in the OS log dir; human-readable stderr in dev; `init(log_dir, dev: bool)`; helper `job_span(job_id, kind)` and `stage_span(stage)`.
- src-tauri/src/lib.rs: panic hook writing `crash-<timestamp>.txt` and a `last_crash` marker; on next start emit `app://crash-report` event.
- src-tauri/src/commands/system.rs: `log_client_event(level: LogLevel, msg: String)`.
- apps/desktop/src/app/error-boundary.tsx and window.onerror/unhandledrejection hooks calling log_client_event; a recoverable error screen with no stack trace in the UI text.
TYPES: `enum LogLevel { Error, Warn, Info, Debug }`.
ROUTES: log_client_event as above. MIGRATIONS: none.
TESTS: rotation with a tiny cap in a temp dir; a subprocess test that panics and checks the crash note and marker; Vitest test for the error boundary; test that log_client_event writes level and message.
ACCEPTANCE: no println!/console.log remain (lints pass); logs never contain pixel data; the next launch reports a previous crash.
OUT OF SCOPE: diagnostics zip (later), analytics.
DONE WHEN: acceptance passes, spec §12 adjusted only if details changed, docs/issues.md updated (GTP-007), then ask me for the next step.
```

#### Step 0.5: Hardware and data spikes (GTP-008 to GTP-012)
Run each spike as its own session. Replace `{SPIKE}` with one block below.
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: scan, convert, edit, organize film negatives; inputs are the modded Nikon LS-50 and R/G/B exposure triplets from a D600 under a Scanlight V4).
- Structure: docs/structure.md (research notes in docs/research/, ADRs in decisions/, throwaway code in tools/scripts/spikes/ or the scratchpad, never in crates/).
- Also read: docs/specification.md §6, §6.1, §16; docs/architecture.md §3.4; docs/workflow.md §8 (spikes and [hw] rules).
PROCESS: follow docs/workflow.md. Post a proposal for the spike and STOP for my approval. The assistant prepares exact commands and a checklist; I run them on the hardware and report results; you record them. You never tick [hw] or [data] items.

SPIKE: {SPIKE}
OUTPUT: update the named research note and ADR, list evidence (commands, outputs, numbers), give a recommendation, and propose follow-up issues. No production code merged.
DONE WHEN: the spike's questions are answered with my reported evidence, note + ADR changes merged, docs/issues.md updated, then ask me for the next step.
```
- **GTP-008, nkscan + modded LS-50:** build nkscan on Windows and macOS; list the device (expect SA-30 roll adapter); single-frame `--ir` scan; advance across 3+ frames; test the Windows driver stack with VueScan's driver installed. Update `docs/research/nkscan.md` and ADR 0003 (Accepted, amended or Rejected; contribute upstream vs rely on TIFF import).
- **GTP-009, Scanlight serial:** open the port on both OS; `GET_FW_VERSION` (rev B, firmware ID 2 expected); each colour alone with `SET_COLOR` (save flag 0); read LED_TEMP and VBUS during an R,G,B sequence; `SHUTTER_PULSE` and `SET_FOCUS` with the D600 remote cable. Save recorded packets in `samples/public/scanlight/` as test vectors. Update `docs/research/scanlight-capture.md` and ADR 0004.
- **GTP-010, libgphoto2 + D600 on macOS:** install or build libgphoto2; capture and download a NEF; resolve macOS USB claiming; compile a Rust binding and list the camera; assess bundling in the .app (dylibs, camlibs/iolibs paths, ad-hoc signing) and give a go/no-go for the M6 cut line. Update the same note and ADR 0004.
- **GTP-011, digiCamControl on Windows:** confirm D600 detection, that Scanlight-pulse shots land in the session folder without digiCamControl triggering them, and record file-write timing and partial-file behaviour.
- **GTP-012, NEF decode with rawler:** from the NEF triplet I provide in `samples/private/`, decode with rawler (black level, CFA pattern, linear values), bilinear demosaic, take the matching channel; measure time per file against the 10 s triplet budget; decide rawler vs LibRaw. Create `docs/research/nef-decode.md`.

### Stage 1: Pipeline on files

#### Step 1.1: Core domain types, Recipe, synthetic fixtures (GTP-020, GTP-021)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: convert scanned film negatives to positives and edit colour, tone, geometry; non-goals: masking, AI generation).
- Structure: docs/structure.md (crates/gtp-core = shared types; no I/O).
- Also read: docs/specification.md §4, §4.1, §4.2 (forward model), §8; docs/architecture.md §3.4; docs/convention.md §3, §4, §7; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-020 and GTP-021 and STOP for my approval before writing code.

TASK: the domain model and ground-truth test data.
FILES (crates/gtp-core):
- src/lib.rs (//! header), ids.rs (RollId, FrameId, TagId as ULID newtypes with Display/FromStr), image.rs (ImageF32 per architecture §3.4 with width/height/channels checks and row accessors), error.rs (if not present).
- src/model/{roll,frame,source,tag}.rs: Roll, Frame, FrameKind {ScanRgbIr, RgbTriplet}, SourceFile, SourceRole {Rgb, Ir, R, G, B}, Tag (fields from spec §4).
- src/recipe/{mod,v1,clamp,migrate}.rs: Recipe v1 exactly as spec §4.1 (geometry, base, invert, color, tone with curve points, dust, output), serde defaults, `Recipe::clamp()` for every documented range, `migrate(json: &str) -> Result<Recipe>` (accepts v1, rejects future versions with UnsupportedFormat).
- src/testing/{mod,synthetic}.rs behind feature `testing`: `synthetic::negative(seed, params) -> Synthetic { image: ImageF32, ir: Option<ImageF32>, truth: Truth }` with Truth = film base, black/white points, frame rect, dust mask, expected positive. Documented forward model (positive -> density -> transmittance * base). Deterministic by seed.
ROUTES/MIGRATIONS: none.
TESTS: recipe JSON round trip; unknown fields ignored; missing fields default; clamp idempotent (proptest) and covers all ranges; migrate rejects v2; ID parse failures; synthetic determinism (same seed identical); ground truth consistent with the forward model; feature `testing` absent from the default build.
ACCEPTANCE: issues GTP-020/021 criteria; `cargo test -p gtp-core --features testing` passes; no I/O in the crate (check deps).
OUT OF SCOPE: TIFF I/O, pipeline stages.
DONE WHEN: acceptance passes, docs/specification.md §4.1 and samples/README.md updated if needed, docs/issues.md updated, then ask me for the next step.
```

#### Step 1.2: TIFF I/O and colour output (GTP-022, GTP-023)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: scans in, high-quality positives out).
- Structure: docs/structure.md (crates/gtp-io = file formats; depends only on gtp-core).
- Also read: docs/specification.md §5.1, §9 (untrusted files), §4.2 step 9; docs/architecture.md §3.2; docs/convention.md §4; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-022 and GTP-023 and STOP for my approval before writing code.

TASK: safe 16-bit TIFF I/O, hashing, and colour-managed export encoders.
FILES (crates/gtp-io):
- src/lib.rs, limits.rs (Limits { max_side: 20_000, max_decoded_bytes: 2 GiB }, check before allocating), tiff.rs, hash.rs, icc.rs, export/{tiff16,jpeg}.rs.
TYPES/INTERFACES:
- `read_tiff16(path, &Limits) -> Result<Tiff16>`; `Tiff16 { width, height, channels, data: Vec<u16> }`; `to_f32_linear(&Tiff16) -> ImageF32` (scale only; no gamma).
- `write_tiff16(path, &Tiff16, Compression::DeflatePredictor)`; `mark_read_only(path)`; `sha256_file(path) -> Result<String>`.
- `enum OutputSpace { Srgb, AdobeRgb, ProPhoto }`; `fn to_output(img: &ImageF32, space: OutputSpace) -> Result<Tiff16Output>` (lcms2: linear Rec.2020 -> space); `encode_tiff16(img, space, path)` and `encode_jpeg(img, space, quality, path)` embedding the ICC profile.
ROUTES/MIGRATIONS: none.
TESTS: bit-exact round trip (RGB and gray); crafted header with huge dimensions rejected without large allocation; corrupt/truncated files return typed errors; Deflate+predictor output smaller than raw on synthetic data; reference patches convert within ΔE2000 <= 1; ICC profile parsed back from outputs; JPEG size monotonic in quality. Never use unwrap outside tests.
ACCEPTANCE: GTP-022/023 criteria; builds on Windows; I will confirm macOS ([mac]).
OUT OF SCOPE: NEF decode (step 5.1), catalog.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 1.3: Negative stages (GTP-024, GTP-025, GTP-026)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: determine the frame area and measure film base, then convert to a positive; target quality comparable to Nikon Scan 4 / Trichroma Lab).
- Structure: docs/structure.md (crates/gtp-negative = pure functions over ImageF32; depends only on gtp-core).
- Also read: docs/specification.md §4.2 steps 3-5 and "Film base", "Frame detection"; rules/colour-pipeline.md; docs/convention.md §7; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-024, GTP-025, GTP-026 and STOP for my approval before writing code. (Three PRs, one per issue, unless I approve one.)

TASK: negative-to-positive maths.
FILES (crates/gtp-negative/src): lib.rs, film_base.rs, normalize.rs, invert.rs, frame_detect.rs.
INTERFACES:
- `sample_film_base(img: &ImageF32, region: Rect) -> [f32; 3]`; `auto_film_base(img) -> BaseEstimate { rgb, confidence }`; `roll_base(bases: &[[f32; 3]]) -> [f32; 3]` (robust, ignores outliers).
- `normalize(img: &mut ImageF32, base: [f32; 3])` (T = I / base, epsilon-guarded).
- `invert(img: &mut ImageF32, params: &Invert)`; `auto_levels(img: &ImageF32, lo_pct: f32, hi_pct: f32) -> Invert` (defaults 0.5 / 99.5); density D = -log10(T).
- `detect_frame(img: &ImageF32) -> FrameDetection { rect: NormRect, confidence: f32 }` (full frame when confidence is low).
ROUTES/MIGRATIONS: none.
TESTS (use gtp-core `testing` fixtures): auto base within 1% of truth; normalize maps base to 1.0 and never produces NaN/inf; roll_base ignores an outlier frame; synthetic round trip within a documented per-channel tolerance; inversion monotonic (proptest); auto levels fills Invert and explicit values override; detect_frame within 0.5% of each side, low confidence on blank/noise, works with rebate on two or four sides.
ACCEPTANCE: GTP-024/025/026 criteria; functions are pure and in place where noted; no unwrap.
OUT OF SCOPE: colour/tone, pipeline composition.
DONE WHEN: acceptance passes, rules/colour-pipeline.md refined if the maths changed, docs/issues.md updated, then ask me for the next step.
```

#### Step 1.4: Edit stages (GTP-027, GTP-028)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: edit colour balance, tone and geometry; no masking or advanced retouching).
- Structure: docs/structure.md (crates/gtp-edit = pure stage functions; depends only on gtp-core).
- Also read: docs/specification.md §4.1, §4.2 steps 6-8, §11; rules/colour-pipeline.md; docs/convention.md §7, §11; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-027 and GTP-028 and STOP for my approval before writing code.

TASK: colour, tone and geometry stages.
FILES (crates/gtp-edit/src): lib.rs, color.rs, tone.rs, curve.rs, geometry.rs.
INTERFACES:
- `apply_color(img: &mut ImageF32, c: &Color)` (temperature/tint as per-channel gains with the mapping documented in module docs; saturation in linear Rec.2020, luma-preserving).
- `apply_tone(img: &mut ImageF32, t: &Tone)` (exposure EV, contrast about mid-grey, master + R/G/B curves).
- `Curve::new(points: &[(f32, f32)]) -> Result<Curve>` monotone spline with a 4096-entry LUT; `Curve::apply(f32) -> f32`.
- `apply_geometry(img: &ImageF32, g: &Geometry) -> Result<ImageF32>` (normalized crop, rot90, flips, straighten angle, single resample; `output_size(w, h, g)`).
ROUTES/MIGRATIONS: none.
TESTS: default recipe is identity within 1e-6; +1 EV doubles linear values; contrast 0 identity; curves monotone for monotone inputs and invalid points rejected/clamped; in-place processing of a 21 MP buffer allocates no extra full-size buffer (documented check); rot90 x4 identity, flips involutions, angle 0 identity, crop clamped, degenerate crop -> InvalidInput, straighten resamples once (compare with a combined transform); criterion bench for apply_tone on 21 MP.
ACCEPTANCE: GTP-027/028 criteria; stage timings noted against spec §11.
OUT OF SCOPE: IR dust (step 4.2), composition.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 1.5: Application layer: pipeline, proxy, analyze (GTP-029, ADR 0005)
```text
CONTEXT
- Idea: docs/product/idea.md (Grain to Pixel: ingest, frame/base detection, convert, edit, organize).
- Structure: docs/structure.md (crates/ with gtp-* feature crates; src-tauri composes). This step adds crates/gtp-app as the application layer; see docs/architecture.md §3 and §9 for the proposed structure change.
- Also read: docs/specification.md §3, §4.2, §6 (develop_*), §11; docs/convention.md §3; docs/workflow.md.
PROCESS: follow docs/workflow.md. FIRST write ADR 0005 (gtp-app application layer; the only crate allowed to combine feature crates) and the structure/convention/architecture edits it implies, and post them as the proposal. STOP for my approval before any code.

TASK: pipeline composition without Tauri.
FILES:
- decisions/0005-application-layer.md; update decisions/README.md, docs/structure.md (tree + dependency rules), crates/README.md, docs/convention.md §3, tools/scripts/check-deps allowed-edges table, docs/issues.md (GTP-029 title/scope).
- crates/gtp-app/{Cargo.toml, src/{lib.rs, context.rs}, src/pipeline/{mod.rs, stages.rs, proxy.rs, analyze.rs}}.
INTERFACES: per docs/architecture.md §3.4: PipelineInput, RenderTarget, `render`, `analyze`, `Analysis { crop, base, histogram, auto_recipe }`, `build_proxy(&ImageF32, max_edge) -> ImageF32` (fast_image_resize; 2048 and 1600 px long edge). Stage order fixed per spec §4.2: (IR dust hook, no-op for now) -> crop -> normalize -> invert -> color -> tone -> geometry. Output of Preview is linear f32; sRGB RGBA8 conversion is a separate `to_rgba8_srgb` function.
ROUTES/MIGRATIONS: none.
TESTS: end-to-end synthetic negative -> render equals analytical expectation within tolerance; stage-order test (spy on stage calls); proxy sizes; analyze returns crop/base/histogram and an auto recipe that renders without error; criterion bench: Preview 1600 px render time recorded against the 100 ms budget.
ACCEPTANCE: GTP-029 criteria; check-deps passes with the new edge table; docs consistent.
OUT OF SCOPE: IR dust, services, Tauri.
DONE WHEN: acceptance passes, ADR accepted by me, docs/issues.md updated, then ask me for the next step.
```

#### Step 1.6: Golden harness, benchmarks, calibration (GTP-030, GTP-031, GTP-032)
```text
CONTEXT
- Idea: docs/product/idea.md (success metric: results comparable to Nikon Scan 4 or Trichroma Lab).
- Structure: docs/structure.md (tools/benchmark = Rust tool; samples/{public,private}/reference; docs/research/).
- Also read: docs/specification.md §1 (quality metric), §8, §11; docs/architecture.md §4; docs/convention.md §7; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-030 (then 031, 032 as separate proposals) and STOP for my approval before writing code.

TASK: make quality and speed measurable.
FILES:
- tools/benchmark/{Cargo.toml, src/{main.rs, delta_e.rs, diff.rs, golden.rs, reference.rs, report.rs}, benches/{pipeline.rs}}; add to workspace members. Depends on gtp-app and gtp-core only.
- samples/public/golden/*.tif (LFS) generated from synthetic fixtures; samples/MANIFEST schema in samples/README.md; docs/research/benchmark.md.
INTERFACES:
- `delta_e2000(lab1, lab2) -> f32`; `ImageDiff { mean, p95, max }`; CLI: `benchmark golden [--bless]`, `benchmark reference --set <dir>`, `benchmark report`.
- Reference manifest `samples/*/reference/<set>/manifest.toml`: [[frame]] id, source (tiff path), kind, render (reference render path).
- criterion benches: decode, 1600 px render, analyze, full-resolution render.
ROUTES/MIGRATIONS: none.
TESTS: ΔE2000 matches published Sharma test pairs; changed pipeline output fails the golden test and `--bless` updates it with a reviewable diff; `cargo test --workspace` runs goldens with no network or private data.
ACCEPTANCE: GTP-030/031/032 criteria. [data] I supply >= 10 reference pairs; you compute median/p95 ΔE2000 and propose calibrated thresholds; I approve the numbers before spec §1 and §11 change.
OUT OF SCOPE: tuning the pipeline to hit the thresholds (propose a follow-up issue instead).
DONE WHEN: acceptance passes, M1 exit confirmed by me, docs/issues.md updated, then ask me for the next step.
```

### Stage 2: Library

#### Step 2.1: Jobs and catalog (GTP-040, GTP-041)
```text
CONTEXT
- Idea: docs/product/idea.md (organize converted negatives in a searchable archive via tags).
- Structure: docs/structure.md (crates/gtp-catalog = SQLite; crates/gtp-app = application layer, jobs/).
- Also read: docs/specification.md §3 (jobs), §5.2, §5.3; docs/architecture.md §3.4, §4.1; docs/convention.md §3-7; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-040 and GTP-041 and STOP for my approval before writing code.

TASK: job infrastructure and the catalog with migrations.
FILES:
- crates/gtp-app/src/jobs/{mod.rs, manager.rs, cancel.rs}: `JobId(Ulid)`, `JobKind`, `JobCtx { progress(stage, fraction), is_cancelled() }`, `JobManager::spawn(kind, f) -> JobId`, `cancel(JobId)`, trait `JobSink { progress, done, error }`.
- crates/gtp-catalog/{Cargo.toml, src/{lib.rs, db.rs, backup.rs, fts.rs}, src/repo/{roll,frame,source,recipe,tag,search,meta}.rs, migrations/{0001_init.sql, 0002_search.sql, 0003_export_log.sql}}.
- src-tauri: commands/system.rs gets `job_cancel(JobId)`; src/sink.rs `TauriJobSink` emitting job://progress|done|error; regenerate ipc-types.
MIGRATIONS: 0001 = roll, frame, source_file, recipe, tag (COLLATE NOCASE unique), frame_tag, meta + indexes (spec §5.2, ON DELETE CASCADE); 0002 = fts5 `search(frame_id UNINDEXED, text)`; 0003 = export_log. `Catalog::open(path)` applies migrations with rusqlite_migration; before migrating, copy to `library.db.bak-v<N>`; pragmas WAL, foreign_keys=ON, synchronous=NORMAL; `integrity_check()` available; opening a newer schema returns LibraryTooNew.
TESTS: jobs: progress order, cancel stops cooperatively, error and done delivered once, no panic on cancel after finish (use a fake JobSink); catalog (in-memory and temp-file): migrate from empty, idempotent reopen, backup file created, cascade deletes, unique tag NOCASE, newer schema -> LibraryTooNew, FTS insert/delete in the same transaction as the parent change.
ACCEPTANCE: GTP-040/041 criteria; every migration applies on both OS.
OUT OF SCOPE: library folder layout (next step), UI.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 2.2: Library lifecycle, rolls, frames (GTP-042, GTP-043)
```text
CONTEXT
- Idea: docs/product/idea.md (organize film negatives by roll and frame in an archive).
- Structure: docs/structure.md (crates/gtp-app services; src-tauri commands thin; packages/ipc-types generated).
- Also read: docs/specification.md §5.1, §5.3, §5.4, §6; docs/architecture.md §3.5, §4.2; docs/convention.md §4, §5; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-042 and GTP-043 and STOP for my approval before writing code.

TASK: managed library folder plus roll and frame commands.
FILES:
- crates/gtp-app/src/{library_fs.rs, paths.rs}: layout `<Library>/{library.db, originals/<roll>/<frame>/, thumbs/, exports/}`; path safety (canonicalize inside library); free-space query.
- crates/gtp-app/src/services/{library.rs, roll_frame.rs, settings.rs}; context.rs holds the open Library.
- src-tauri/src/commands/{library.rs, roll.rs, frame.rs, system.rs (settings_get/set)}.
- apps/desktop settings.json handling: last library path, window size, flags (defaults in code per spec §10).
ROUTES (typed via specta): library_create(path, name) -> LibraryInfo; library_open(path) -> LibraryInfo; library_info() -> LibraryInfo; roll_create(RollInput) -> Roll; roll_update(RollId, RollPatch) -> Roll; roll_list() -> Roll[]; roll_delete(RollId); roll_set_film_base(RollId, [f32;3]) -> Roll; frame_list(FrameQuery{roll_id?, page, page_size}) -> Page<FrameSummary>; frame_get(FrameId) -> Frame; frame_delete(FrameId[]); frame_set_note(FrameId, String); settings_get() -> Settings; settings_set(SettingsPatch).
MIGRATIONS: none new (uses 0001 to 0003).
VALIDATION: names <= 200 chars, trimmed; ids parsed; paths canonicalized; delete removes files only after the DB commit.
TESTS: services against a temp library with a real catalog; layout created and re-opened; newer-version library opens read-only; delete removes files after commit and leaves files if the transaction fails (inject failure); command-layer tests with error mapping; settings round trip.
ACCEPTANCE: GTP-042/043 criteria; ipc-types regenerated without diff.
OUT OF SCOPE: import, tags, UI.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 2.3: TIFF import and library verify (GTP-044, GTP-047)
```text
CONTEXT
- Idea: docs/product/idea.md (ingest scans into the archive).
- Structure: docs/structure.md (gtp-ingest, gtp-io, gtp-app services/import.rs, src-tauri commands/ingest.rs).
- Also read: docs/specification.md §5.1, §5.3, §6, §11; docs/architecture.md §3.5; docs/convention.md §4, §5; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-044 and GTP-047 and STOP for my approval before writing code.

TASK: import existing TIFF scans as frames; verify the library.
FILES:
- crates/gtp-ingest/src/files.rs: `scan_tiff_candidates(paths) -> Vec<TiffCandidate>` (RGB, optional IR pair by filename convention `*_ir.tif`), limit checks via gtp-io.
- crates/gtp-app/src/services/import.rs: job that, per frame: copy to originals (Deflate+predictor TIFF), sha256 verify, mark read-only, 512 px JPEG thumbnail, one catalog transaction (frame, source_file rows, default recipe, search text), emit library://changed; cancel keeps finished frames; partial frame never committed.
- crates/gtp-app/src/services/library.rs: `verify()` job re-hashing originals and listing missing/corrupt.
- src-tauri/src/commands/ingest.rs: import_files(paths: String[], roll_id, kind: ImportKind) -> JobId; commands/library.rs: library_verify() -> JobId.
TYPES: `enum ImportKind { TiffScan, RgbTriplet }` (RgbTriplet reserved for step 5.1), `struct ImportResult { imported: u32, skipped: Vec<SkipReason> }`.
MIGRATIONS: none.
TESTS: import 5 synthetic TIFFs end to end (files, rows, thumbnail, hash); cancel mid-import keeps finished frames only; corrupt TIFF skipped with typed reason; oversized header rejected; verify detects a tampered and a deleted original; per-frame import time recorded vs the 8 s budget.
ACCEPTANCE: GTP-044/047 criteria; originals read-only; no frame without its files.
OUT OF SCOPE: scanner, triplets, tags.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 2.4: Tags and search (GTP-045)
```text
CONTEXT
- Idea: docs/product/idea.md (add tags for organization; searchable archive via tags).
- Structure: docs/structure.md (gtp-catalog repo/tag.rs + search.rs; gtp-app services/tags_search.rs).
- Also read: docs/specification.md §5.2, §6 (SearchQuery), §11 (search budget); docs/architecture.md §4.2; docs/convention.md §4; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-045 and STOP for my approval before writing code.

TASK: tag management and full-text/tag search.
FILES: crates/gtp-catalog/src/repo/{tag.rs, search.rs}, src/fts.rs (maintain `search.text` from roll name, film stock, notes, frame note, tags within the same transaction); crates/gtp-app/src/services/tags_search.rs; src-tauri/src/commands/{tags.rs, search.rs}.
ROUTES: tag_list() -> Tag[]; tag_apply(frame_ids, add: String[], remove: String[]); tag_rename(TagId, String); tag_delete(TagId); search(SearchQuery{text?, tags_all[], roll_id?, date_from?, date_to?, film_stock?}, page) -> Page<FrameSummary>.
VALIDATION: tags trimmed, lower-cased, <= 64 chars, no control characters, dedup; FTS query escaped (no injection of FTS operators unless intended and documented).
MIGRATIONS: none new.
TESTS: apply/remove/rename/delete keep the FTS row correct; AND semantics for tags_all; text search with special characters; benchmark: search over 50,000 synthetic frames <= 100 ms; concurrency: reads during writes (WAL).
ACCEPTANCE: GTP-045 criteria; budget met or numbers reported.
OUT OF SCOPE: UI.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 2.5: Library view (GTP-046)
```text
CONTEXT
- Idea: docs/product/idea.md (organize in a searchable archive; personal app, keyboard-friendly).
- Structure: docs/structure.md (apps/desktop/src/features/library; packages/ui for shared components; packages/ipc-types generated).
- Also read: docs/specification.md §7, §11 (grid 60 fps), §13 (a11y/i18n); docs/architecture.md §4; docs/convention.md §1, §2; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-046 and STOP for my approval before writing code.

TASK: the Library view.
FILES: apps/desktop/src/features/library/{LibraryView.tsx, RollSidebar.tsx, FrameGrid.tsx (virtualized with @tanstack/react-virtual), SearchBar.tsx, use-library.ts, library.module.css}; packages/ui/src/{TagChips.tsx, StatusBadge.tsx, ProgressList.tsx}; strings in shared/i18n/en.json; first-run dialog to create/open a library showing free space and location.
BEHAVIOUR: roll sidebar (create/rename/delete with confirmation), thumbnail grid with lazy loading, multi-select (click, shift, ctrl/cmd), search bar with tag chips, batch tag add/remove, `[` `]` and arrow-key navigation, listens to library://changed and job://* events.
TYPES/ROUTES: uses only the typed wrappers over the commands from steps 2.2 to 2.4.
MIGRATIONS: none.
TESTS: Vitest + Testing Library for sidebar, grid selection, search bar, tag chips; `vitest-axe` has no violations; virtualization renders only visible rows (assert row count); keyboard navigation test; no hard-coded strings (lint).
ACCEPTANCE: GTP-046 criteria; 5,000-frame fixture scrolls smoothly (report measured fps); all text via t().
OUT OF SCOPE: Develop view.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

### Stage 3: Develop

#### Step 3.1: Develop backend (GTP-050)
```text
CONTEXT
- Idea: docs/product/idea.md (edit image color balance, tone, geometry on converted positives).
- Structure: docs/structure.md (gtp-app services/develop.rs and pipeline/; src-tauri commands/develop.rs).
- Also read: docs/specification.md §3 (preview transport), §6, §11; docs/architecture.md §3.4, §3.5; docs/convention.md §11; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal for GTP-050 and STOP for my approval before writing code.

TASK: backend for interactive editing.
FILES: crates/gtp-app/src/services/develop.rs (LRU of 4 proxies keyed by FrameId, latest-request-wins render lock per frame); src-tauri/src/commands/develop.rs.
ROUTES:
- develop_open(FrameId) -> DevelopInfo { width, height, has_ir, recipe, roll_base: Option<[f32;3]> } (loads original, builds the 2048 px proxy, caches it)
- develop_analyze(FrameId) -> Analysis
- develop_render(FrameId, Recipe, max_edge: u32, seq: u64) -> binary: 16-byte header (seq u64, width u32, height u32) + RGBA8 sRGB pixels via tauri::ipc::Response; stale or superseded requests return Cancelled
- recipe_save(FrameId, Recipe) is added in step 3.3.
VALIDATION: recipe clamped server-side; max_edge in [256, 4096].
MIGRATIONS: none.
TESTS: open/render on synthetic frames; header layout; LRU eviction at 5 frames; superseded render returns Cancelled; clamped recipe; 1600 px render time recorded (budget <= 100 ms); memory stays within the proxy cache.
ACCEPTANCE: GTP-050 criteria; ipc-types regenerated; no pixel data in logs.
OUT OF SCOPE: UI, saving recipes, export.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 3.2: Develop UI (GTP-051 to GTP-054)
```text
CONTEXT
- Idea: docs/product/idea.md (edit color balance, tone, geometry; results comparable to Nikon Scan 4 / Trichroma Lab).
- Structure: docs/structure.md (apps/desktop/src/features/develop; packages/ui for Slider, CurveEditor, Histogram).
- Also read: docs/specification.md §4.1, §4.2, §7, §13; rules/colour-pipeline.md; docs/architecture.md §4; docs/convention.md §1; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval before writing code. Deliver as three PRs: (a) GTP-051 canvas/histogram/before-after, (b) GTP-052 + GTP-053 panels, (c) GTP-054 crop UI.

TASK: the Develop view.
FILES:
- features/develop/{DevelopView.tsx, Canvas.tsx, Filmstrip.tsx, use-develop.ts (recipe state, debounced render requests with seq, 1600 px while dragging, 2048 px on release), panels/{BaseInvertPanel,ColorPanel,TonePanel,GeometryPanel,DustPanel}.tsx, develop.module.css}.
- packages/ui/src/{Slider.tsx (Radix), CurveEditor.tsx (keyboard-operable points), Histogram.tsx}.
BEHAVIOUR: draw RGBA8 from develop_render onto a canvas with pan/zoom; before/after toggle (`\`); film-base picker (click on rebate -> develop recipe base manual) and roll default; temperature/tint/saturation; exposure/contrast/curves; crop overlay with auto-detected rectangle and low-confidence hint; rotate/flip/straighten; histogram of the current preview. `[`/`]` move through the filmstrip.
ROUTES used: develop_open, develop_analyze, develop_render, roll_set_film_base. MIGRATIONS: none.
TESTS: Vitest for use-develop (debounce, stale result dropped by seq, drag vs release sizes using a mocked ipc); component tests for panels; CurveEditor keyboard test; vitest-axe clean; strings via t().
ACCEPTANCE: GTP-051 to GTP-054 criteria; manual check by me of the full conversion flow on a synthetic and a real frame ([data]).
OUT OF SCOPE: saving, export, dust panel behaviour (placeholder until step 4.2).
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 3.3: Save recipes, export, settings (GTP-055, GTP-056, GTP-057)
```text
CONTEXT
- Idea: docs/product/idea.md (convert and edit, then output results; archive stays organized).
- Structure: docs/structure.md (gtp-app services/export.rs, develop.rs; gtp-io export encoders; features/settings in the UI).
- Also read: docs/specification.md §4.1, §5.4, §6 (export_start), §10, §11; docs/architecture.md §3.5; docs/convention.md §4, §5; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval before writing code. Three PRs, one per issue.

TASK: persist edits, export files, settings screen.
FILES:
- recipe persistence: services/develop.rs `save_recipe` (debounced from the UI by 500 ms; writes recipe row and `edited_at`); recipe_apply_to(frame_ids, source, fields).
- crates/gtp-app/src/services/export.rs: job per frame: load original, pipeline::render(Full), output transform with ICC, naming template (`{roll}_{index:03}.{ext}`), write, insert export_log; cancel-safe (temp file then rename).
- src-tauri/src/commands/{develop.rs, export.rs}; UI: features/develop copy/apply-to-roll menu, export dialog; features/settings/{SettingsView.tsx, FlagsList.tsx}.
ROUTES: recipe_save(FrameId, Recipe); recipe_apply_to(FrameId[], source: FrameId, fields: RecipeField[]); export_start(FrameId[], ExportOptions{format: Tiff16|Jpeg, space: OutputSpace, quality, dest_dir, template}) -> JobId; settings UI over settings_get/set (flags, library path, free space).
VALIDATION: dest_dir chosen via OS dialog and canonicalized; template sanitized (no path separators); overwrite requires confirmation.
MIGRATIONS: none (export_log exists in 0003).
TESTS: save/reload round trip; apply_to copies only selected fields; export of a synthetic frame to TIFF16 and JPEG with the right ICC; temp-file cleanup on cancel; one 21 MP full export timed against the 10 s budget; settings flags default on/off per spec §10; vitest-axe on export dialog and settings.
ACCEPTANCE: GTP-055/056/057 criteria; M3 exit (full file-based workflow end to end) confirmed by me.
OUT OF SCOPE: scanner and capture.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

### Stage 4: LS-50

#### Step 4.1: Scanner port, nkscan adapter, roll-scan job (GTP-060, GTP-061, GTP-062)
```text
CONTEXT
- Idea: docs/product/idea.md (ingest via Nikon Coolscan LS-50; whole-roll scanning up to 40 frames with the modded scanner).
- Structure: docs/structure.md (crates/gtp-ingest = scanner port + adapters; gtp-app services/scan.rs; src-tauri commands/ingest.rs).
- Also read: docs/specification.md §6 (ScannerSource), §5.1, §11; decisions/0003-ls-50-via-nkscan.md; docs/research/nkscan.md (spike GTP-008 results); docs/architecture.md §3.4, §3.5; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval. The proposal must state how the GTP-008 outcome shapes the design (crate vs CLI, SA-30 support, fallback). If GTP-008 is not Done, stop and tell me. Three PRs, one per issue.

TASK: scan whole rolls into the library.
FILES:
- crates/gtp-ingest/src/{scanner.rs (trait + ScanParams + RawScan + CancelToken), nkscan.rs (NkscanSource), fixture.rs (FixtureSource replaying TIFFs from samples)}.
- crates/gtp-app/src/services/scan.rs: roll-scan job; commit each frame immediately (same path as import: originals, hash, thumbnail, catalog transaction); resume from next frame; failure after k frames keeps k frames.
- src-tauri/src/commands/ingest.rs: scanner_list() -> DeviceInfo[]; scan_start(ScanParams, RollId) -> JobId.
TYPES: `ScanParams { device_id, frames: RangeInclusive<u32> (1..=40), dpi: u32 (default 4000), samples: u8 (1|2|4|8|16), ir: bool, autofocus: bool }`; `RawScan { rgb16, ir16: Option<_>, dpi, meta }`.
VALIDATION: frames 1..=40; dpi from the supported list; samples in {1,2,4,8,16}.
MIGRATIONS: none.
TESTS: FixtureSource drives the whole job in tests; per-frame commit; cancel and resume; error on frame k keeps frames 1..k-1; ScanParams validation; NkscanSource compiles on both OS (hardware behaviour = [hw]).
ACCEPTANCE: GTP-060/061/062 criteria. [hw] I run a 3-frame roll scan with IR on each OS and report; you record results in docs/research/nkscan.md.
OUT OF SCOPE: UI, IR dust removal.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 4.2: IR dust removal and scanner UI (GTP-063, GTP-064)
```text
CONTEXT
- Idea: docs/product/idea.md (IR/ICE dust removal for LS-50 scans; edit and organize).
- Structure: docs/structure.md (crates/gtp-edit dust.rs; gtp-app pipeline stage hook; features/import/ScannerSection.tsx).
- Also read: docs/specification.md §4.1 (dust), §4.2 step 2; rules/colour-pipeline.md; docs/architecture.md §3.4; docs/convention.md §7; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval before writing code. Two PRs, one per issue.

TASK: dust removal and the scan UI.
FILES:
- crates/gtp-edit/src/dust.rs: `remove_dust(rgb: &mut ImageF32, ir: &ImageF32, d: &Dust) -> DustReport { masked_px }` (normalize IR, threshold, dilate by grow_px, iterative neighbour-weighted fill from unmasked pixels).
- crates/gtp-app/src/pipeline/stages.rs: enable the IR stage before crop when `ir` is present and `dust.enabled`; flag `dust_removal` (spec §10) can disable it.
- features/import/{ImportView.tsx, ScannerSection.tsx}: device list, scan settings (frames range, dpi, samples, IR), progress list per frame, cancel, resume from next frame; features/develop/panels/DustPanel.tsx behaviour (enable, threshold, grow).
TESTS: synthetic IR dust spots are removed (masked pixels filled within tolerance, non-dust pixels unchanged); threshold/grow monotonic; no IR plane -> stage skipped; dust off by flag; UI tests for ScannerSection with a mocked ipc (progress, cancel, resume); vitest-axe.
ACCEPTANCE: GTP-063/064 criteria; [hw] visible dust removal on a real IR scan confirmed by me.
OUT OF SCOPE: capture, triplets.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

### Stage 5: Triplets

#### Step 5.1: NEF triplet import (GTP-070, GTP-071, GTP-072)
```text
CONTEXT
- Idea: docs/product/idea.md (ingest separate RGB images via camera, with tags; convert to positive).
- Structure: docs/structure.md (gtp-io raw/, gtp-ingest triplet.rs, gtp-app services/import.rs, features/import/FilesSection.tsx).
- Also read: docs/specification.md §4.2 step 1, §6, §11; docs/research/nef-decode.md (spike GTP-012); docs/architecture.md §3.5; docs/convention.md §4; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval. If GTP-012 is not Done, stop and tell me. Three PRs.

TASK: import R/G/B exposure triplets from NEF files.
FILES:
- crates/gtp-io/src/raw/{nef.rs, demosaic.rs}: `decode_nef(path, &Limits) -> RawLinear { w, h, cfa, black, data }` (linear, no WB, no tone curve; decoder per GTP-012), `demosaic_bilinear(&RawLinear) -> ImageF32`.
- crates/gtp-ingest/src/triplet.rs: `group_triplets(files: &[ExifFile]) -> Vec<TripletCandidate>` (sort by capture time, groups of three in R,G,B order, user-reorderable), `merge_triplet(r: &Path, g: &Path, b: &Path) -> Result<ImageF32>` (R from red-lit, G from green-lit, B from blue-lit), `check_exif(&[ExifInfo;3]) -> Vec<Warning>` (exposure/ISO/aperture equal) and `check_clipping`.
- crates/gtp-app/src/services/import.rs: ImportKind::RgbTriplet path: store three single-channel 16-bit TIFFs (r.tif, g.tif, b.tif), SourceFile roles R/G/B, FrameKind::RgbTriplet, merged proxy on open.
- src-tauri: triplet_group(paths: String[]) -> TripletCandidate[]; import_files with kind RgbTriplet. UI: FilesSection.tsx with grouping preview, reorder, confirm.
MIGRATIONS: none.
TESTS: synthetic Bayer data -> demosaic -> channel pick exact on constructed inputs; grouping by time with a missing/extra shot flagged; reorder; EXIF mismatch and clipping warnings; import end to end with synthetic NEF-like fixtures (decoder abstracted behind a trait for tests); merge time on a real triplet vs the 10 s budget ([data]).
ACCEPTANCE: GTP-070/071/072 criteria; [data] a real D600 triplet converts to a plausible positive, confirmed by me.
OUT OF SCOPE: live capture.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

### Stage 6: Guided capture

#### Step 6.1: Scanlight driver and capture orchestrator (GTP-080, GTP-081)
```text
CONTEXT
- Idea: docs/product/idea.md (ingest separate RGB images via camera tethering; here driven by the app with a Scanlight V4 and a Nikon D600).
- Structure: docs/structure.md (crates/gtp-capture = LightSource/CameraSource ports + adapters; gtp-app services/capture.rs).
- Also read: docs/specification.md §6.1 (protocol table, sequence, safety rules); decisions/0004-guided-rgb-capture.md; docs/research/scanlight-capture.md (spike GTP-009 results and test vectors in samples/public/scanlight/); docs/architecture.md §3.4, §3.5; docs/convention.md §5, §10; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval. If GTP-009 is not Done, stop and tell me. Two PRs.

TASK: Scanlight serial driver and the per-frame R/G/B sequence, fully testable with fakes.
FILES (crates/gtp-capture/src):
- light.rs (trait LightSource, LightInfo, LightStatus), scanlight/{mod.rs, protocol.rs (packet encode/decode: 0xFE, header, length, data; SET_COLOR, GET_FW_VERSION, SHUTTER_PULSE, SET_FOCUS; LED_TEMP, VBUS, FW_VERSION parse), port.rs (serialport open, probe ports with GET_FW_VERSION, background reader updating status)}, camera.rs (CameraSource, CapturedFile), sequence.rs (CaptureSequence::run), profile.rs (CaptureProfile with defaults: settle_ms 200, pulse_10ms 10, temp_warn_c 60, temp_abort_c 75, focus_signal false), fakes.rs (FakeLight recording commands, FakeCamera scripted).
- crates/gtp-app/src/services/capture.rs: `capture_triplet` job wrapper.
INTERFACES: per docs/architecture.md §3.4. `SET_COLOR` always sends save flag 0; there is no SET_TRIM; sequence: for R,G,B -> set_levels(only that channel) -> sleep settle -> arm -> trigger -> wait_for_image(15 s) -> emit step; always light off (Drop guard) on success, cancel, error, timeout, panic.
VALIDATION: firmware ID 1 or 2 accepted (warn otherwise); packet length checked against buffer; unknown headers dropped; levels clamped 0..=255.
MIGRATIONS/ROUTES: none yet (commands in step 6.3).
TESTS: byte-exact packet tests from the recorded vectors; decoder fuzz-style tests on truncated and garbage input (never panic); FakeLight/FakeCamera: order R,G,B, light off after cancel/error/timeout, temperature abort, EXIF mismatch and clipping warnings, save flag never 1.
ACCEPTANCE: GTP-080/081 criteria; check-deps passes (gtp-capture depends on gtp-core only).
OUT OF SCOPE: real camera backends, UI.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 6.2: Camera backends (GTP-082, GTP-083)
```text
CONTEXT
- Idea: docs/product/idea.md (ingest RGB images via camera tethering).
- Structure: docs/structure.md (crates/gtp-capture gphoto.rs behind Cargo feature `tether`, watched.rs; macOS is primary, Windows uses digiCamControl + watched folder).
- Also read: docs/specification.md §6.1 (camera backends), §9; decisions/0004-guided-rgb-capture.md; docs/research/scanlight-capture.md (spikes GTP-010, GTP-011); docs/architecture.md §3.4; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval. If GTP-010 or GTP-011 is not Done, stop and tell me. Build macOS first (GTP-082), then Windows (GTP-083); two PRs. If the GTP-010 go/no-go was "no-go", implement only GTP-083 and say so.

TASK: real CameraSource implementations.
FILES (crates/gtp-capture/src):
- gphoto.rs (feature `tether`, macOS): `GphotoCamera` over the binding chosen in GTP-010: arm = ensure connected; trigger = PTP capture (ignores the light arg); wait_for_image = download the NEF to a temp file and return CapturedFile; clear error when macOS holds the USB device (message with the documented workaround).
- watched.rs: `WatchedFolder { dir }` using `notify`: arm = snapshot dir; trigger = `light.shutter_pulse(pulse_10ms)` (and `SET_FOCUS` when the profile enables it); wait_for_image = first new `.nef` whose size is stable for 500 ms; ignore partial/temp files; timeout error.
- src-tauri/Cargo.toml: feature `tether` enabled for macOS targets only; macOS bundle config for the libgphoto2 libraries and env paths per GTP-010.
MIGRATIONS/ROUTES: none (commands in step 6.3).
VALIDATION: watched dir canonicalized and user-selected; files validated like any import.
TESTS: WatchedFolder against a temp dir with simulated slow writes, temp-then-rename, partial files, timeout; GphotoCamera compiles with and without the feature; trait conformance tests shared with FakeCamera.
ACCEPTANCE: GTP-082/083 criteria. [hw] on macOS: a full R,G,B triplet captured by the app with libgphoto2; on Windows: a triplet via Scanlight pulse + digiCamControl; I report results and you record them.
OUT OF SCOPE: UI.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 6.3: Capture profile, commands, Capture view (GTP-084, GTP-085)
```text
CONTEXT
- Idea: docs/product/idea.md (ingest via camera tethering with tags for organization; app control of the whole capture).
- Structure: docs/structure.md (src-tauri commands/capture.rs; features/import/CaptureSection.tsx; settings.json for the capture profile).
- Also read: docs/specification.md §6.1 (profile, test shot, safety), §7 (Capture view), §11 (capture budgets), §13; docs/architecture.md §4.2; docs/convention.md §1, §10; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval before writing code. Two PRs.

TASK: expose capture to the UI.
FILES:
- src-tauri/src/commands/capture.rs; events `capture://step {channel, state: lit|fired|received}` and `light://status {temp_c, vbus_mv}` (via TauriJobSink and the light reader).
- features/import/CaptureSection.tsx, features/import/capture/{StatusBar.tsx, LevelSliders.tsx, TestShot.tsx, NextFrameButton.tsx}; capture profile persisted in settings.json.
ROUTES: light_list() -> PortInfo[]; light_connect(port?: String) -> LightInfo; light_set(levels: [u8;3]); light_off(); camera_list() -> CameraInfo[]; capture_profile_get() -> CaptureProfile; capture_profile_set(patch); capture_test(channel: Channel) -> TestShot { histogram, clipped_pct }; capture_triplet(roll_id) -> JobId (result carries the new FrameId).
BEHAVIOUR: connection badges for light and camera; per-colour level sliders; test shot with histogram; large "Next frame" button (Space) running capture_triplet then showing the merged result and a roll frame counter; light switched off when the view closes or the app exits; warning banners for temperature, EXIF mismatch, clipping; film advance is manual.
VALIDATION: levels 0..=255; port chosen from light_list or probed; temp limits ordered warn < abort.
MIGRATIONS: none.
TESTS: command-layer tests with FakeLight/FakeCamera; light off on view unmount (UI test) and on app exit (Rust test); component tests with mocked ipc (status, test shot, next frame, error banners); vitest-axe; strings via t().
ACCEPTANCE: GTP-084/085 criteria; capture-per-frame time (excluding exposure) <= 15 s on my rig ([hw], I report); colour switch latency <= 50 ms in logs; M6 exit confirmed by me.
OUT OF SCOPE: live view, camera exposure control, auto film advance.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

### Stage 7: Release

#### Step 7.1: Packaging, updater, release pipeline (GTP-090, GTP-091)
```text
CONTEXT
- Idea: docs/product/idea.md (personal app shared with close friends; Windows 10/11 and macOS; no monetization).
- Structure: docs/structure.md (.github/workflows/release.yml; apps/desktop/src-tauri/tauri.conf.json).
- Also read: docs/specification.md §9 (updates, unsigned builds), §15; docs/convention.md §10; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval before writing code or touching CI. Two PRs.
PREREQUISITE (human): create the Tauri updater signing key pair; put the private key in GitHub Actions secrets; give you only the public key.

TASK: installers and updates.
FILES: tauri.conf.json bundle settings (NSIS for Windows x64; dmg for macOS arm64 with `signingIdentity: "-"` ad-hoc), updater endpoint and public key; .github/workflows/release.yml (on tag v*.*.*: lint/test, tauri build matrix windows-latest + macos-14, draft GitHub Release with installers and signed latest.json); docs/install.md (SmartScreen and Gatekeeper steps).
TESTS: workflow dry run on a pre-release tag; installers install and launch ([human] on both OS); updater finds a newer prerelease and verifies the signature; a tampered manifest is rejected.
ACCEPTANCE: GTP-090/091 criteria; release is a DRAFT until I publish; installer size <= 40 MB (spec §11); updater prompts and never installs silently; spec §15 open item (updater behaviour on unsigned macOS builds) answered in docs.
OUT OF SCOPE: code signing certificates.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 7.2: Diagnostics, accessibility, user docs (GTP-092, GTP-093, GTP-094)
```text
CONTEXT
- Idea: docs/product/idea.md (personal app for friends: easy support, usable by everyone).
- Structure: docs/structure.md (gtp-app services/diagnostics.rs; docs/ for user docs; features/settings).
- Also read: docs/specification.md §12, §13, §14; docs/convention.md §6, §10; docs/workflow.md.
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval. Three PRs, one per issue.

TASK: supportability, accessibility, documentation.
FILES:
- crates/gtp-app/src/services/diagnostics.rs: `diagnostics_export(dest)` zips logs, app/OS versions, DB schema version, flags, local counters, last job errors; no images; counters in `meta`: first_import_at, first_convert_at, first_export_at, frames_imported, frames_exported, scans_failed. Settings screen shows counters and the Export diagnostics button.
- accessibility fixes from an audit: keyboard paths, focus rings, aria-live for job status, contrast >= 4.5:1, reduced motion; add vitest-axe coverage for every view; manual NVDA and VoiceOver checklist in docs/research/a11y.md.
- docs/user-guide.md: install, library backup (copy the folder), scanner setup (driver notes from GTP-008), capture setup (Scanlight, camera, macOS vs Windows), known limitations.
TESTS: zip contents (no pixel data, no full paths at info level); counters increment once at the right events; axe clean on all views; docs links valid.
ACCEPTANCE: GTP-092/093/094 criteria; [human] NVDA and VoiceOver run reported by me.
OUT OF SCOPE: remote analytics.
DONE WHEN: acceptance passes, docs/issues.md updated, then ask me for the next step.
```

#### Step 7.3: Verification and v0.1.0 (GTP-095, GTP-096)
```text
CONTEXT
- Idea: docs/product/idea.md (success: activation, and results comparable to Nikon Scan 4 / Trichroma Lab).
- Structure: docs/structure.md (tools/benchmark; docs/research/benchmark.md; CHANGELOG generated from commits).
- Also read: docs/specification.md §1, §11, §15, §16 (M7); docs/convention.md §8; docs/workflow.md §11 (DoD).
PROCESS: follow docs/workflow.md. Post a proposal and STOP for my approval. I own the final publish and tag.

TASK: prove the MVP meets its budgets and quality bar, then release.
STEPS:
- Run tools/benchmark on the release candidate on my reference machine: every spec §11 budget and the calibrated ΔE2000 thresholds; write docs/research/benchmark.md with tables; list any miss and propose issues (a miss blocks release).
- Run the release checklist: all CI gates green on main, `cargo deny`/audit clean, installers installed on clean Windows and macOS, a full workflow (capture or scan -> convert -> edit -> tag -> search -> export) completed, migrations from an empty library, diagnostics export checked.
- Prepare the v0.1.0 notes from commits; prepare the tag command for me; do NOT tag, publish, or push without my explicit instruction.
FILES: docs/research/benchmark.md, docs/release-checklist.md, CHANGELOG generation config.
TESTS/ACCEPTANCE: GTP-095/096 criteria; [human] clean-machine installs and the end-to-end run reported by me; milestone M7 exit criteria confirmed by me.
DONE WHEN: I publish v0.1.0; docs/issues.md shows all milestones Done; then summarize the MVP and propose the post-MVP backlog (spec §16).
```

---

## 9. Changes to existing docs (applied 2026-10-29)

This plan adds one structural element, the `gtp-app` application layer. The doc edits below were applied together with a draft [ADR 0005](../decisions/0005-application-layer.md) (Proposed). The crate itself is created in step 1.5 (GTP-029); `tools/benchmark` is added to the Cargo workspace members in step 1.6 (adding it earlier would break `cargo check`); the `check-deps` edge table is implemented in step 0.1.

| Change | Where | Reason |
|---|---|---|
| New crate `crates/gtp-app` (application layer: services, JobManager, pipeline composition) | structure.md tree and §4, crates/README.md, spec §3 | feature crates cannot depend on each other, yet the pipeline needs `negative` + `edit`; keeps `src-tauri` thin and everything testable without Tauri |
| Allowed-edges table in `tools/scripts/check-deps` (§3.3) | GTP-002, convention §3 | enforce the layering |
| GTP-029 retitled "Application layer: pipeline, proxy, analyze (ADR 0005)" and GTP-040 job manager lives in `gtp-app` | issues.md | consistent with the above |
| `tools/benchmark` added to workspace members | Cargo.toml, structure.md | step 1.6 |
| Prompts may be saved under `prompts/` | prompts/README.md | optional reuse |

## 10. Risks and decision points

| Decision point | Where | Options |
|---|---|---|
| Does nkscan drive the modded LS-50 (SA-30, multi-frame)? | step 0.5 → 4.1 | contribute upstream, or rely on TIFF import (M4 shrinks to `FixtureSource` and import) |
| libgphoto2 on the Mac | step 0.5 → 6.2 | `GphotoCamera`, or `WatchedFolder` on both OS (M6 cut line) |
| `rawler` reads D600 NEF | step 0.5 → 5.1 | rawler, or LibRaw binding |
| CPU preview budget | step 3.1 | smaller preview, then GPU (new ADR) |
| Quality metric | step 1.6 | calibrated thresholds approved by me |
| Unsigned installers | step 7.1 | accept warnings, or buy certificates later |
