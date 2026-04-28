---
name: add-source
description: Add a new transport that produces frames into wanlogger
---

# Skill: add a new Source

A `Source` produces a stream of `Frame`s plus control events from some
transport (serial, TCP, file, MQTT, journald, …). It does **not** parse
records; that is the `Framer`'s job, with `Decoder` after that.

## When to use this skill

User asks for a new connector / transport / "log input" / "ingest from X".

## Steps

1. Read [`crates/core/src/source/mod.rs`](../../../crates/core/src/source/mod.rs)
   to refresh the `Source` trait, `Frame` enum and `ControlEvt` enum.
   They are **frozen v0.1**; do not change them — open an ADR if you must.
2. Copy `templates/source/` into `crates/core/src/source/<name>.rs` and
   wire `pub mod <name>;` from `source/mod.rs`.
3. Implement `Source`. For source-only transports (pcap, RTT, CAN), do
   **not** implement `Sink`.
4. Add an enum variant in `ChannelSpec` (`crates/core/src/source/mod.rs`)
   so the config layer can construct it.
5. Add config schema entry in `docs/protocols/cli-output/v1/` and bump
   the cli-output minor version (see `docs/adr/0001-foundations.md`
   §"independent semver").
6. Add a fixture under `fixtures/source/<name>/` with a `PROVENANCE.md`
   note. Add an integration test under
   `crates/core/tests/source_<name>.rs` that drives `MockSource` or the
   real impl against the fixture.
7. Add a requirement id `FR-SRC-<name>` to `docs/requirements.md` and
   reference it from the test (`// REQ: FR-SRC-<name>`).
8. Update the directory map in `AGENTS.md` if directory layout changed.
9. `just ai-verify` → green. Open PR with the standard template.

## Pitfalls

- Don't read past the framer-level boundary. Surface bytes/datagrams,
  not parsed lines.
- Watch backpressure: yield `await` on every send; never `try_send` and
  drop silently.
- Every public error is `E-NNNN`. Add new ids to
  `crates/core/src/error_id.rs` and `docs/errors/`.
