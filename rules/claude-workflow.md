# Working with Claude Pro limits

- Keep sessions narrow: one crate or one ADR per session; start from a file in `prompts/`.
- Point Claude at specific files; avoid "scan the repo".
- Use a cheaper model for mechanical work (stubs, READMEs, renames); keep the strongest model for pipeline maths and design.
- Put findings in `docs/research/` or `decisions/` so later sessions read a page instead of re-deriving.
- Keep `CLAUDE.md` under ~40 lines; move detail into `rules/`.
