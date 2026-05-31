# Project invariants

These are properties that must hold across the whole codebase. CI
asserts them (lint, tests, fixture compat). Violations are blocking.

1. **No `unsafe`.** `unsafe_code = "deny"` workspace-wide.
2. **Server-of-truth.** Persistence happens only inside `tracemux
   serve`. The UI / Tauri / CLI never write `session-dir/` directly,
   except via the wire protocol's `write` / `ingest` paths.
3. **Two timestamps.** Every persisted record and every wire `data`
   frame carries the dual-timestamp envelope (see ADR-0001 §4 and
   `docs/protocols/timestamp.md`).
4. **Lossless logger.** The logger pipeline never drops records inside
   its configured queue depth. Drops happen, if at all, at the UI
   `broadcast` layer with explicit `lagged(N)` notifications.
5. **No `openssl-sys`.** Only `rustls`. Enforced by `deny.toml`.
6. **No plaintext secrets.** TOML stores `secret://name` references;
   the `keyring` crate resolves them.
7. **Critical paths require human review.** See AGENTS.md §5 and
   `.github/workflows/label-critical.yml`.
8. **Frozen API surfaces.** `wire-protocol`, `log-format`,
   `cli-output` evolve only via ADR + version bump + fixture compat.
9. **All public errors have an `E-NNNN` id** registered in
   `crates/core/src/error_id.rs` and documented in `docs/errors/`.
10. **Tests reference requirements.** Tests carry
    `// REQ: FR-…` / `// REQ: NFR-…` comments; `just rtm` builds the
    matrix.
