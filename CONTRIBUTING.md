# Contributing

Welcome! `tracemux` is built to be maintained by both humans and AI
coding agents. Both follow the same rules.

## Start here

1. Read [AGENTS.md](AGENTS.md) (architecture, build, critical paths).
2. For your task, follow the matching skill in
   `.github/skills/<task>/SKILL.md`.
3. Run `just ai-verify` until green.
4. Open a PR using the template.

## Conventions

- Conventional Commits.
- `cargo fmt` + `clippy -D warnings`. 100-column lines.
- Public items in `crates/core` get rustdoc.
- Tests reference requirement ids: `// REQ: FR-…`.
- ADRs for non-trivial design decisions
  (`docs/adr/template.md`).

## Code of conduct

Be kind. Assume good faith. No harassment.
