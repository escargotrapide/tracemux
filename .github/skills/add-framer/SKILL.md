---
name: add-framer
description: Add a Framer that turns raw bytes into frames
---

# Skill: add a new Framer

`Framer`s sit between `Source` and `Decoder`. They turn a byte stream
into discrete frames (a line, a length-prefixed packet, a regex match,
…). They are stateful but never decode semantics.

## Steps

1. Read [`crates/core/src/framer/mod.rs`](../../../crates/core/src/framer/mod.rs).
   Frozen v0.1.
2. Copy `templates/framer/` into `crates/core/src/framer/<name>.rs`.
3. Implement `Framer::poll_frame(&mut self, buf: &mut BytesMut) -> Poll<Option<Frame>>`.
4. Property tests with `proptest` for: never lose bytes, never produce
   overlapping frames, idempotent on partial input.
5. Fuzz target in `crates/fuzz/fuzz_targets/framer.rs` calling your impl
   on arbitrary bytes; must not panic.
6. Add `FR-FRM-<name>` requirement.
7. `just ai-verify`.

## Pitfalls

- The byte buffer is shared and may be partial; never block on more
  bytes in `poll_frame` — return `Poll::Pending` style with `Ok(None)`.
- Watch unbounded growth: enforce a max-frame-size (default 1 MiB) and
  emit `E-1003 framer-overflow` on excess.
