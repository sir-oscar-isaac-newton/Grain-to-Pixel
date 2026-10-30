# Grain to Pixel: Claude Code guide

Film-negative scan/convert/edit/archive desktop app. Rust core + Tauri + TypeScript UI. Read `docs/structure.md` for the layout.

## Game rules
- Work strictly by `docs/workflow.md` and the GitHub issues defined in `docs/issues.md` (IDs like `P-03`, `L-05`; `docs/issues.md` is huge, read one issue only): propose, wait for approval, implement with tests and docs, open a PR, then ask for the next step. No issue, no work.

## Working rules (token-frugal: Claude Pro limits)
- One crate or one feature per session. Don't read the whole repo.
- Follow `docs/convention.md` (style, naming, testing, commits). Read `rules/` files only when relevant: `rules/colour-pipeline.md`.
- Use prompts in `prompts/` as session starters.
- Record non-trivial choices as ADRs in `decisions/` (copy `0000-template.md`).
- Non-goals: masking, AI generation, advanced retouching. Don't add them.

## Commands
- `cargo check --workspace` / `cargo test --workspace`
- `pnpm -r test --if-present`
