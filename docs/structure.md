# Project structure

How Grain to Pixel is organized and why. Decision record: [ADR 0001](../decisions/0001-monorepo.md).

## 1. Repository strategy: one monorepo

**Choice:** a single Git repo containing a Cargo workspace (Rust) and a pnpm workspace (TypeScript).

**Why not a constellation of repos**

- One developer and a few friends as users: no team boundaries or release cadences to separate.
- Features cut across layers. Adding an edit parameter touches `gtp-core`, `gtp-edit`, `gtp-catalog`, and the UI; one repo makes that one commit and one AI session.
- The Claude Pro usage cap favours a single, well-indexed repo with short guide files over juggling several repos' context.
- No versioning, publishing, or submodule overhead.

**Costs we accept:** binary samples (mitigated with Git LFS and a git-ignored `samples/private/`), and a larger CI surface (irrelevant at this size). If a crate becomes reusable on its own (e.g. an LS-50 driver), extract it later with `git subtree split`.

## 2. Folder tree

```
grain-to-pixel/
├─ README.md                 project overview, quick start
├─ CLAUDE.md                 short guide for Claude Code (points into rules/)
├─ Cargo.toml                Rust workspace (members: crates/*)
├─ package.json              JS workspace root + scripts
├─ pnpm-workspace.yaml       apps/*, packages/*
├─ .gitignore  .gitattributes (Git LFS)  .editorconfig
│
├─ apps/
│  └─ desktop/               Tauri 2 app: src-tauri/ (Rust shell) + src/ (React UI)
├─ crates/                   Rust libraries (UI-agnostic)
│  ├─ gtp-core/              domain types: Frame, Roll, Recipe, errors
│  ├─ gtp-io/                16-bit TIFF / RAW / ICC / sidecars
│  ├─ gtp-ingest/            file import, Coolscan LS-50 (RGB+IR) via nkscan
│  ├─ gtp-capture/           Scanlight V4 control + camera tether, R/G/B capture sequence
│  ├─ gtp-negative/          frame detect, film-base measure, inversion
│  ├─ gtp-edit/              colour, tone, geometry, IR dust removal
│  ├─ gtp-catalog/           SQLite archive, tags, search
│  └─ gtp-app/               application layer: use-case services, jobs, pipeline composition (ADR 0005)
├─ packages/                 shared TypeScript
│  ├─ ipc-types/             @gtp/ipc-types: generated Tauri command/event types
│  └─ ui/                    @gtp/ui: reusable components
│
├─ samples/
│  ├─ public/                small shareable negatives (Git LFS) + MANIFEST.md
│  └─ private/               personal scans (git-ignored)
├─ tools/
│  ├─ benchmark/             gtp-benchmark: ΔE2000, golden-image tests (Q-01); later compare vs Nikon Scan 4 / Trichroma Lab
│  └─ scripts/               dev + release helpers (cross-platform)
│
├─ docs/                     documentation
│  ├─ intro.md               architecture overview, run locally, add a module, troubleshooting
│  ├─ structure.md           this file
│  ├─ specification.md       MVP technical spec and roadmap
│  ├─ convention.md          development rules (style, testing, CI, commits)
│  ├─ workflow.md            AI development loop: propose, approve, implement, PR, next
│  ├─ issues.md              backlog definition: 68 issues with AI prompts + JSON import array (generated)
│  ├─ issues_upload_instructions.md   how to bulk-create the GitHub issues with the gh CLI
│  ├─ product/               idea.md, requirements, flows
│  ├─ architecture/          system overview, data model, pipeline
│  └─ research/              LS-50 protocol, film-base maths, competitor notes
├─ decisions/                ADRs (0000-template.md, 0001-…, 0002-…)
├─ rules/                    coding.md, colour-pipeline.md, claude-workflow.md
├─ prompts/                  reusable Claude Code session starters
└─ .claude/                  Claude Code project settings (local overrides ignored)
```

## 3. Where things live

| Need | Location |
|---|---|
| Documentation | `docs/` (decisions are separate) |
| Architecture Decision Records | `decisions/`, numbered `NNNN-title.md`, template `0000-template.md` |
| Rules (conventions, domain invariants) | `rules/` |
| Reusable AI prompts | `prompts/` |
| Shared Rust code | `crates/gtp-core` (types), `crates/gtp-io` (file formats) |
| Shared TypeScript code | `packages/ui`, `packages/ipc-types` |
| Test images | `samples/` |
| Benchmarks / scripts | `tools/` |

## 4. Dependency rules

```
apps/desktop ──► packages/*           (UI side)
      │
      └────────► gtp-app (via Tauri commands in src-tauri)

gtp-app ──► gtp-ingest | gtp-capture | gtp-negative | gtp-edit | gtp-catalog | gtp-io | gtp-core

gtp-core ◄── gtp-io ◄── gtp-ingest | gtp-capture | gtp-negative | gtp-edit | gtp-catalog
```

- Feature crates never depend on each other. `gtp-app` is the only crate that combines them (use-case services, `JobManager`, pipeline composition); `src-tauri` is a thin adapter over `gtp-app`. The exact allowed edges are in [architecture.md §3.3](architecture.md) and enforced by `tools/scripts/check-deps`.
- The UI never processes pixels; it calls Tauri commands.
- `packages/ipc-types` is generated from Rust once wired up; don't hand-edit it then.

## 5. Mapping the workflow to code

| Workflow step | Home |
|---|---|
| Ingest (LS-50 roll scan, file import, tags) | `gtp-ingest`, tags via `gtp-catalog` |
| Guided RGB capture (Scanlight + D600) | `gtp-capture` |
| Frame area + film base | `gtp-negative` |
| Convert to positive | `gtp-negative` |
| Colour, tone, geometry, IR dust removal | `gtp-edit` |
| Searchable archive | `gtp-catalog` |

## 6. Initialization commands

PowerShell on Windows; the same commands work in a macOS shell.

```bash
# prerequisites: Git + Git LFS, Rust (rustup), Node 20+, pnpm (npm i -g pnpm)
git init -b main
git lfs install
git add . && git commit -m "chore: scaffold monorepo"

pnpm install
cargo check --workspace

# scaffold the desktop app in place (see apps/desktop/README.md)
cd apps
pnpm create tauri-app desktop --template react-ts --manager pnpm
```

After scaffolding the app, add `apps/desktop/src-tauri` to `members` in the root `Cargo.toml`.

## 7. Conventions

- Rust crates are prefixed `gtp-`; TS packages use the `@gtp/` scope.
- New decisions go in `decisions/` before large implementation work.
- Keep `CLAUDE.md` short; put detail in `rules/` and `docs/`.
- Personal scans never enter Git (`samples/private/`).

## 8. Known risks reflected in the layout

- **Limited colour-negative samples:** `samples/` has a manifest and a private area; `tools/benchmark` makes quality measurable.
- **LS-50 access on Windows/macOS:** the plan is the open-source `nkscan` driver ([ADR 0003](../decisions/0003-ls-50-via-nkscan.md)), isolated behind a `ScannerSource` trait in `gtp-ingest`. Its LS-50 support is Linux-tested and partly inferred, so the first task is a hardware spike, recorded in `docs/research/nkscan.md`.
- **Claude Pro limits:** small crates, short guides, prompt templates, and ADRs keep each session's context small.
