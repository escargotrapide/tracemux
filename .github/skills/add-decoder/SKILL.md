---
name: add-decoder
description: Add a Decoder that turns frames into structured records
---

# Skill: add a new Decoder

`Decoder`s convert framed bytes into `Record`s with optional
`schema_id`, level, tags, and `correlation_id`. They are pure
functions of (frame, codec) when possible.

## Steps

1. Read [`crates/core/src/decoder/mod.rs`](../../../crates/core/src/decoder/mod.rs).
   Frozen v0.1.
2. Copy `templates/decoder/` into `crates/core/src/decoder/<name>.rs`.
3. If you emit numeric series, also emit a `TimeseriesPoint` so the
   `TimeseriesSink` can persist Parquet.
4. Register schema(s) in `crates/core/src/log/schemas.rs`. The
   `schema_id` lives in `session-dir/schemas/<id>.json`.
5. Add `FR-DEC-<name>` requirement and golden-output snapshot tests
   under `crates/core/tests/decoder_<name>/`.
6. Fuzz target in `crates/fuzz/fuzz_targets/decoder.rs`.
7. `just ai-verify`.

## Pitfalls

- Decoders must be **schema-on-read tolerant**. If the schema in the
  fixture is older than the current code, parsing must still succeed.
- Use `encoding_rs` for any text decoding (UTF-8 / Shift_JIS / …).
