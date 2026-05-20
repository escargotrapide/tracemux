# ADR-0002: Server-owned annotations outside frozen log records

- **Status:** Proposed
- **Date:** 2026-05-20
- **Deciders:** wanlogger maintainers
- **Related requirements:** FR-UI-017, FR-LOG-001, FR-WIRE-001
- **Affected critical paths:** yes - `docs/adr/**`; future alternatives may affect `docs/protocols/wire-protocol.md`, `docs/protocols/log-format.md`, `crates/core/src/decoder/mod.rs`, and `crates/core/src/log/index.rs`

## Context

The web UI already supports browser-local notes for selected sources/sessions and log-type keys. This satisfies the current `FR-UI-017` constraint that notes are browser-side annotations and must not persist raw log data outside the server-owned session-dir.

Users also need a next step: notes that are shared across browsers and survive browser storage loss. The tempting implementation is to add `memo` or `annotations` fields to wire `data` frames, decoded `Record`, or `index.jsonl` rows. That would make notes portable with the log record, but it would also modify frozen v0.1 surfaces:

- WSS `wanlogger.v1` data payload shape.
- Log-format `index.jsonl` schema.
- `crates/core/src/decoder/mod.rs::Record`.
- `crates/core/src/log/index.rs::IndexEntry`.

Those surfaces require an ADR, a version bump, and compatibility fixtures before they can change. For v0.1, the safer goal is server-shared UI annotations without changing the immutable log stream or raw bytes.

Forces at play:

- **Compatibility:** v0.1 wire/log schemas are frozen.
- **Data ownership:** the server remains the source of truth for log persistence; UI must not persist raw logs.
- **Auditability:** user notes should not rewrite historical log records or raw payload offsets.
- **Portability:** keeping notes outside session-dir avoids log-format churn but means copied session directories do not automatically include notes.
- **Security:** annotation APIs must follow the same auth posture as other session APIs.

## Decision

Do not add annotation fields to `Record`, `IndexEntry`, WSS `data` frames, or v0.1 session-dir files for the first server-shared notes implementation.

Instead, introduce a server-owned **application annotation store** outside the frozen log-format layout. The store is keyed by stable logical targets and exposed through authenticated HTTP APIs. The web UI may sync source/session and log-type notes with this store while keeping browser-local storage as an offline/cache fallback.

The initial annotation target set is intentionally limited:

1. `session` - note attached to a server-known `sid`.
2. `log_type` - note attached to a tag or kind, optionally scoped to a `sid`.

Individual-record annotations are deferred because the current live wire `data` payload does not expose a stable persisted record identifier such as `index.jsonl` offset. A future ADR can define record locators if per-record notes become a hard requirement.

A proposed annotation object shape:

```jsonc
{
  "id": "uuid",
  "target": {
    "kind": "session|log_type",
    "sid": "uuid?",
    "key": "string?"
  },
  "text": "string",
  "updated_at": "RFC3339",
  "updated_by": "string?",
  "deleted": false
}
```

Storage path and format for the first implementation:

- Store under the server session root, outside individual session-dir layouts, for example:
  - `<session-root>/.wanlogger/annotations-v1.jsonl`, or
  - `<session-root>/.wanlogger/annotations/<sid>.json`.
- Treat this as app metadata, not v0.1 log-format content.
- Use atomic write or append-only JSONL with compaction to avoid partial writes.
- Enforce text length limits comparable to current UI localStorage notes.

Proposed HTTP API shape, subject to implementation refinement:

- `GET /api/annotations?sid=<sid>` - list annotations visible for a session.
- `PUT /api/annotations/{id}` - create or replace an annotation.
- `DELETE /api/annotations/{id}` - tombstone or delete an annotation.

These APIs must require authentication when auth is enabled. Loopback `--no-auth` follows existing server policy.

## Consequences

- Positive:
  - Adds shared, server-owned notes without changing frozen wire or log records.
  - Preserves raw log immutability and `raw.bin` / `index.jsonl` offsets.
  - Allows incremental UI migration from browser-local notes to server-synced notes.
  - Keeps backward compatibility with existing replay/import/export consumers.

- Negative:
  - Notes are not portable when a single session-dir is copied without the server annotation store.
  - A backup/export story for annotations must be added separately.
  - Record-level notes remain out of scope until stable record locators are designed.
  - The server needs a small metadata store, conflict policy, and auth checks.

- Compatibility impact:
  - **wire:** none for the initial implementation; no `wanlogger.v2` required.
  - **log:** none for the initial implementation; no log-format version bump required.
  - **cli:** none required initially; optional future CLI commands can call the same HTTP API or export annotation metadata.
  - **app:** web UI can add optional sync behavior and keep localStorage fallback.

## Alternatives considered

1. Add `memo` to WSS `data` payloads and `Record` - rejected for v0.1 because it changes frozen wire and decoder/log-facing record shape.
2. Add `memo` directly to `index.jsonl` rows - rejected for the first step because it changes the frozen log-format schema and makes user edits rewrite or patch historical record metadata.
3. Add `annotations.jsonl` inside each session-dir - rejected for the first step because it still expands the session-dir contract and needs log-format compatibility fixtures. This remains a candidate for a future portable annotation format.
4. Keep only browser-local notes - rejected as the long-term solution because users need shared notes across browsers/devices and recovery from browser storage loss.
5. Store notes in config/TOML - rejected because annotations are session data, not static startup configuration.

## Migration plan

1. Keep existing browser-local `sourceNotes` and `logTypeNotes` behavior unchanged.
2. Implement a new server annotation module and authenticated HTTP routes outside WSS.
3. Add web UI sync:
   - Load server annotations when a source/session is selected.
   - Save edits to the server when available.
   - Continue writing localStorage as a cache/fallback.
   - Surface sync failures as non-fatal toasts.
4. Add tests:
   - Rust unit tests for annotation normalization, upsert, delete/tombstone, and persistence.
   - Server route tests for auth, create/update/delete/list, and malformed targets.
   - Web unit tests for merge precedence between server annotations and local fallback.
   - Web E2E smoke test for notes surviving reload when the server annotation API is mocked.
5. Regenerate RTM only if new requirement IDs or `REQ` references are added.
6. If portable session-dir annotations or individual-record notes are later required, write a follow-up ADR that explicitly bumps log-format and/or wire protocol versions and adds fixtures under `tests/compat/log/` and `tests/compat/wire/`.