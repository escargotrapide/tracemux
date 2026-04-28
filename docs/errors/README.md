# Error catalogue

Every public-facing error in `wanlogger` is identified by an
`E-NNNN` code. Codes are allocated in
[`crates/core/src/error_id.rs`](../../crates/core/src/error_id.rs)
and documented one-per-file under this directory.

## Allocation ranges

| Range          | Owner            |
| -------------- | ---------------- |
| `E-1000..E-1099`  | core / pipeline  |
| `E-1100..E-1199`  | source layer     |
| `E-1200..E-1299`  | framer layer     |
| `E-1300..E-1399`  | decoder layer    |
| `E-1400..E-1499`  | logsink / WAL    |
| `E-1500..E-1599`  | importer / exporter |
| `E-2000..E-2099`  | wire / server    |
| `E-2100..E-2199`  | auth / TLS       |
| `E-3000..E-3099`  | CLI              |
| `E-4000..E-4099`  | UI / web         |
| `E-9000..E-9999`  | reserved (test)  |

## Adding a new code

1. Pick the next free number in the appropriate range.
2. Add a variant in `crates/core/src/error_id.rs` (or the relevant
   crate's error registry) with a stable string id.
3. Create `docs/errors/E-NNNN.md` describing cause / impact / remedy.
4. Reference the code from `thiserror` / `anyhow` contexts.
