# Add-source template

1. Copy `stub_source.rs` to `crates/core/src/source/<name>.rs`.
2. Rename `StubSource` → your type.
3. Wire `pub mod <name>;` from `crates/core/src/source/mod.rs`.
4. Add a `ChannelSpec` variant in `source/mod.rs`.
5. Add a fixture under `fixtures/source/<name>/` and an integration
   test under `crates/core/tests/source_<name>.rs`.
6. Add `FR-SRC-<name>` to `docs/requirements.md` and reference it from
   the test (`// REQ: FR-SRC-<name>`).
7. `just ai-verify` until green.

See [`.github/skills/add-source/SKILL.md`](../../.github/skills/add-source/SKILL.md).
