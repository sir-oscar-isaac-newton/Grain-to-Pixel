# Implement a crate feature

Work only in `crates/<crate>`. Read `CLAUDE.md`, `rules/coding.md`, and `rules/colour-pipeline.md` first.

Feature: <one sentence>
Inputs/outputs: <types from gtp-core>
Fixture: `samples/public/<file>`

Steps: write a failing test against the fixture, implement, run `cargo test -p <crate>` and `cargo clippy -p <crate>`. Do not change other crates; if `gtp-core` types must change, stop and say so.
