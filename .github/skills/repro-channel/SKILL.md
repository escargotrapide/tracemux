---
name: repro-channel
description: Reproduce a flaky / misbehaving channel deterministically
---

# Skill: reproduce a channel

## Steps

1. Capture: `tracemux log --source <spec> --out fixtures/repro/<id>/`
   (or have the user share the `session-dir/`).
2. Replay deterministically:
   `tracemux replay --session <dir> --rate 1.0 --seed 42`. The replay
   engine drives `Source` → `Framer` → `Decoder` → `LogSink` with the
   captured timing.
3. If non-deterministic (timer-driven / async race), bisect with
   `--rate 0` (lockstep) and `--seed N` until it stabilises.
4. Add the fixture under `fixtures/regression/<id>/` with a
   `PROVENANCE.md` and a regression test in
   `crates/core/tests/regression_<id>.rs`.
5. Reference the fixture in the bug fix PR and link the requirement id.

## Pitfalls

- Replays must roundtrip through the wire-protocol exactly when the
  bug is in the server. Use `tracemux replay --via-server`.
