# 0001. Single monorepo

- Status: Accepted
- Date: 2026-10-29

## Context

One developer, a handful of friends as users, one shipped product on two OSes. Work is done largely with Claude Code under Claude Pro usage limits, so context per session must stay small and cross-cutting changes (e.g. a new `Recipe` field touching core, edit, catalog, and UI) must be doable in one session.

## Decision

Use one Git repository with a Cargo workspace (`crates/`) and a pnpm workspace (`apps/`, `packages/`). Not a constellation of repos.

## Consequences

- Atomic cross-layer changes; one CI, one version, one issue list.
- No cross-repo versioning, publishing, or submodules to manage.
- Large binaries risk bloating the repo: sample images go through Git LFS, personal scans are git-ignored.
- If a crate (e.g. an LS-50 driver) later proves reusable, it can be extracted with `git subtree split`.
