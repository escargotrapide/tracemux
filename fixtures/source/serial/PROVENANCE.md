# Fixture provenance ? serial source

| Field | Value |
| ----- | ----- |
| Kind | serial |
| Source | synthetic / PTY loopback |
| Created | 2025-01-01 |
| Tool | `serialport::TTYPort::pair()` (Unix) |

## Contents

This directory holds golden inputs for the serial source integration tests.

- v0.1: no binary fixtures yet. The integration test in
  `crates/core/tests/source_serial.rs` generates its own data via a virtual
  PTY pair on Unix, or via `TRACEMUX_TEST_SERIAL_PORT` on Windows.
- Future: add captured `raw.bin` / `index.jsonl` pairs here for replay-based
  compat testing, one directory per scenario.

## Re-capture procedure

1. Attach a real or virtual COM port pair.
2. Run `tracemux log serial://COM3?baud=115200 --prefix fixtures/source/serial/v1`.
3. Commit the resulting `session-dir/` snapshot.
4. Add a compat test in `crates/core/tests/` that replays it.
