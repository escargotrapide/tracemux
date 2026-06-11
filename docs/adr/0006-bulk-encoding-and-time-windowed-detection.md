# ADR-0006: Bulk encoding override and time-windowed encoding detection

- **Status:** Proposed
- **Date:** 2026-06-10
- **Deciders:** tracemux maintainers
- **Related requirements:** FR-CLI-006 (serve text encoding), FR-CLI-011
  (content detection mode), FR-UI-014 (display settings / encoding)
- **Affected critical paths:** yes — `docs/adr/**`; if the time-windowed
  mode is accepted, also `docs/protocols/wire-protocol.md` and
  `docs/requirements.md` (FR-CLI-011). The chosen v0.1 approach
  deliberately avoids the frozen `Decoder` trait and `crates/server/src/wire.rs`;
  code changes land in `crates/core/src/detect/content.rs`,
  `crates/server/src/source_manager.rs`, and `crates/cli/` (non-critical).

## Context

Two related ergonomics requests came in for encoding handling:

1. **Bulk encoding override** — set the text encoding of *all* sources at
   once (e.g. "make every source Shift_JIS"), instead of editing each
   source individually.
2. **Time-windowed encoding estimation** — monitor a source over a period
   (not just the first bytes at startup) and estimate its encoding. Sources
   whose encoding cannot be estimated with confidence must be left
   **unchanged** (keep their current encoding).

### What already exists

Encoding handling is already structured around the four-layer pipeline
(`Source -> Framer -> Decoder -> LogSink/UI`):

- **Per-source encoding** is carried in `SourceStartOptions.encoding`
  (`crates/server/src/source_manager.rs`) and applied by
  `Utf8TextDecoder` via `crate::codec::decode(bytes, label)` using
  `encoding_rs` (`crates/core/src/codec.rs`,
  `crates/core/src/decoder/utf8_text.rs`).
- **Runtime change** is possible today via the `restart` ctl action, which
  merges partial `SourceStartOptions` (unspecified fields are preserved).
  `restart` re-runs the source.
- **Content detection already exists** (`crates/core/src/detect/content.rs`):
  `DetectionMode` has `Configured | Auto | Suggest | Off`, scoring is done by
  `detect_encodings(sample)`, and results are reported in
  `ContentDetectionReport`. Today detection samples a **bounded prefix once
  at source startup** (`DEFAULT_MAX_SAMPLE_BYTES = 64 KiB`) and only adopts a
  candidate when `confidence >= DEFAULT_MIN_ENCODING_CONFIDENCE (80)` and the
  gap to the runner-up is `>= DEFAULT_MIN_ENCODING_DELTA (8)`. If thresholds
  are not met, the configured encoding is kept. This already satisfies the
  "leave it unchanged if not confidently estimated" requirement.
- Supported encodings (frozen v0.1): UTF-8, Shift_JIS, CP932, EUC-JP,
  ISO-2022-JP.

### Gaps

- There is no single action to apply one encoding to all running sources.
- Detection is **one-shot at startup over a byte prefix**, not **continuous
  over a time window**. Slow or bursty sources may not emit enough
  distinguishing bytes in the startup sample, and the encoding cannot be
  re-estimated later without a full source `restart`.

### Forces

- **Compatibility:** the `tracemux.v1` wire protocol and the core trait
  surfaces are frozen v0.1. Any new wire field/mode is a CRITICAL-path change
  requiring this ADR + human review + a `tests/compat/wire/*` fixture +
  app-version bump.
- **Minimalism:** prefer the smallest change; reuse the existing detection
  engine and `restart` merge semantics where possible.
- **Lossless logger pipeline:** the logger path is bounded blocking
  (lossless); detection must not drop bytes, matching the existing
  "samples bounded raw bytes ... without dropping them" rule (FR-CLI-011).

## Decision

Split the work into two independently shippable parts.

### Part A — Bulk encoding override (no wire change)

Implement bulk override as a **UI-driven fan-out over the existing `restart`
ctl action**:

1. The web UI gains a "set encoding for all sources" action (Settings →
   Source Start, and/or the Sources panel header). It sends one
   `restart { sid, encoding }` per eligible running source.
2. **Eligibility:** only sources whose pipeline uses a text decoder are
   targeted. Binary / source-only kinds (pcap, RTT, CAN) have no text
   encoding and are skipped.
3. **Partial failure** is surfaced as an aggregate notification
   (`N succeeded / M skipped / K failed`).
4. No change to `tracemux.v1`, `wire.rs`, the log format, or any frozen
   trait. The default-encoding preference in
   `web/src/state/sourceStartOptions.ts` continues to govern *new* sources;
   the bulk action explicitly opts existing sources in (consistent with
   FR-UI-014's "defaults apply only to newly started sources unless the user
   explicitly restarts").

A server-side bulk ctl action (e.g. `set_encoding_all`) is **deferred**: it
would touch `wire-protocol.md` and `wire.rs` for marginal benefit over the
client fan-out.

### Part B — Time-windowed encoding detection (prefetch-window approach)

Server-side content detection already **prefetches and buffers** the first
frames of a source before the live pipeline starts, runs `detect_content()`,
then **replays the buffered frames** through the pipeline
(`SourceManager::prefetch_for_detection`). Today that buffering loop
accumulates up to `DEFAULT_MAX_SAMPLE_BYTES` and breaks early on a short
per-frame idle timeout (`DETECTION_SAMPLE_TIMEOUT = 250 ms`), which starves
slow/bursty sources of distinguishing bytes.

The v0.1 design extends **that existing prefetch step** with a bounded time
window instead of introducing a mid-stream decoder hot-swap:

1. **New detection mode value** `monitor` added to `DetectionMode`
   (`crates/core/src/detect/content.rs`) and to the CLI
   `--detect-mode configured|auto|suggest|off|monitor` (FR-CLI-011). For
   encoding/log-type selection, `monitor` behaves exactly like `auto`
   (apply a high-confidence detected encoding, else keep configured); it only
   changes the *sampling horizon*. The serde representation is kebab-case,
   consistent with the existing variants, and serialises over the wire as the
   string `"monitor"` via the existing `DetectionMode::as_str()` path in
   `crates/server/src/ws.rs` (no `wire.rs` change).
2. **Window bounds** are governed by `MonitorWindow { max_ms, max_bytes }`
   with conservative defaults (e.g. `max_ms = 10_000`, `max_bytes` reusing
   `DEFAULT_MAX_SAMPLE_BYTES`). For `monitor`, the prefetch loop keeps waiting
   until a **total window deadline** (`max_ms`) or the byte cap is reached —
   rather than breaking on the first 250 ms idle gap — so slow sources still
   accumulate a usable sample. Frames are buffered losslessly (the existing
   `VecDeque` replay path), never dropped.
3. **Estimation reuses** `detect_encodings()` and the existing confidence /
   delta thresholds. When the window closes:
   - If a candidate clears the thresholds, the pipeline is started with that
     encoding and the **buffered window frames are replayed through it**.
   - Otherwise the configured encoding is **kept unchanged** — directly
     satisfying the request that un-estimable sources stay as they are.
4. **No forward-only limitation.** Because detection completes *before* the
   pipeline starts and the buffered window is then replayed, every record —
   including those captured during the window — is decoded with the final
   encoding. The only cost is up to `max_ms` of startup latency before the
   first records reach the UI/log for that source.
5. **No frozen-trait / no hot-swap.** This reuses the existing
   prefetch+replay mechanism, so the `Decoder` trait (frozen, critical) is
   untouched and no in-flight decoder swap is needed.
6. **Reporting reuses** `ContentDetectionReport` (mode, sample_bytes,
   configured/effective/sampled encoding, candidates). The only additive
   wire surface is the new `monitor` enum value and, if needed, a
   `window_ms`/`window_bytes` echo field in the report — additive and
   ignorable by `tracemux.v1` clients per the existing
   "unknown fields are ignored" rule.

The live mid-stream hot-swap (decode forward, swap encoding without a startup
delay) is recorded as a deferred alternative below; it would require a change
to the frozen `Decoder` trait and is out of scope for v0.1.

## Consequences

- Positive:
  - Part A ships immediately with **zero wire/log/trait change** and no
    human-review-required path beyond this ADR.
  - Part B reuses the existing prefetch+replay step, scoring engine,
    thresholds, and report struct; the "leave unchanged when unsure"
    guarantee falls out of existing logic, and there is **no forward-only
    limitation** (window frames are replayed and decoded correctly).
  - Part B leaves the frozen `Decoder` trait and `wire.rs` untouched; code
    changes are confined to non-critical files (`detect/content.rs`,
    `source_manager.rs`, `cli/`).
  - Slow/bursty sources get a realistic chance at correct auto-detection.
- Negative:
  - Part B still touches the CRITICAL **docs** `wire-protocol.md` and
    `docs/requirements.md` (documenting the additive `monitor` value), so it
    needs human review, an app-version bump, and a wire compatibility fixture.
  - `monitor` adds up to `max_ms` of **startup latency** for a source before
    its first records appear (the cost of buffering the detection window).
  - Part A issues N `restart` messages; ordering/lock contention in
    `SourceManager` must be handled, and a restart briefly re-runs each
    source.
- Compatibility impact:
  - **wire:** Part A — none. Part B — additive `monitor` enum value (+ optional
    report echo fields); `tracemux.v1` subprotocol unchanged, app capability
    version bumped; new `tests/compat/wire/*` fixture proving an old decoder
    ignores the additions and a new decoder reads them.
  - **log:** none (decoded text already stored as UTF-8; no schema change).
  - **cli:** Part B adds the `monitor` choice to `--detect-mode`
    (additive enum value); JSON output schemas unchanged in shape.
  - **app:** UI gains the bulk action (Part A) and a monitor-mode selector
    plus a "monitoring…" / detected-encoding indicator (Part B).

## Alternatives considered

1. **Server-side bulk ctl action for Part A** (`set_encoding_all`) — rejected
   for v0.1: changes the frozen wire protocol for little gain over a client
   fan-out that reuses the existing `restart` merge semantics.
2. **Per-frame / per-channel encoding switching for Part B** — rejected:
   encoding is a per-source decoder property in v0.1; per-frame switching
   would explode the decoder and report model and has no requirement backing.
3. **Live mid-stream decoder hot-swap** (decode forward, swap the active
   `Utf8TextDecoder` label after the window without a startup delay) —
   deferred: the runner owns the decoder generically, so an in-flight swap
   needs a new method on the frozen, CRITICAL `Decoder` trait (ADR + version
   bump). It also introduces a forward-only limitation (records captured
   before the swap stay decoded under the old encoding). The prefetch-window
   approach avoids both for a bounded startup-latency cost; the hot-swap is a
   candidate follow-up if startup latency proves unacceptable.
4. **Retro-decode the persisted session** — rejected for v0.1: the log format
   stores decoded UTF-8 text, so re-decoding would require re-deriving decoded
   output from `raw.bin` and rewriting persisted output (a log-format-touching
   operation). The prefetch-window approach makes this unnecessary for the
   window itself, since those frames are decoded correctly on first replay.
5. **Continuous (never-ending) re-estimation** — rejected: unbounded sampling
   risks oscillating encodings and wastes CPU; a single bounded window with a
   one-shot decision is predictable and matches the "estimate once, else leave
   unchanged" intent.

## Migration plan

- **Part A** is non-breaking and needs no version bump or fixtures.
- **Part B** changes a frozen surface:
  1. Bump the app (capability) version; keep the `tracemux.v1` subprotocol id.
  2. Add the `monitor` value to `DetectionMode` (`detect/content.rs`, the CLI
     `--detect-mode` help/error, and the wire `detection_mode` value list in
     `docs/protocols/wire-protocol.md`). The frozen `Decoder` trait and
     `crates/server/src/wire.rs` are **not** modified.
  3. Add a `tests/compat/wire/*` fixture: an old decoder ignores the new mode
     / optional report fields; a new decoder round-trips them.
  4. Update FR-CLI-011 in `docs/requirements.md` to document `monitor`, and
     regenerate `docs/rtm.md`.
  5. Run `just ai-verify` until green.

---

## Appendix (informative): implementation detail

This appendix is non-normative. It records the concrete mechanics behind the
Decision so the eventual PR is not designed ad hoc. Line numbers are
indicative and may drift.

### A. Threshold and window defaults — rationale and tuning

Existing constants (`crates/core/src/detect/content.rs`):

| Constant | Value | Meaning |
| --- | --- | --- |
| `DEFAULT_MAX_SAMPLE_BYTES` | `64 * 1024` (65536) | bytes sampled for detection |
| `DEFAULT_MIN_ENCODING_CONFIDENCE` | `80` | min score to adopt a candidate |
| `DEFAULT_MIN_ENCODING_DELTA` | `8` | min gap to the best *different-family* runner-up |

`effective_encoding()` only adopts a detected encoding in `Auto` mode, and
only when `best.confidence >= min_encoding_confidence` **and**
`best.confidence - second >= min_encoding_delta`, where `second` is the top
candidate from a *different* encoding family (`encoding_family()`), so near-
identical relatives (e.g. `shift_jis` vs `cp932`) do not block each other.

Rationale for the thresholds: `80` keeps auto-switching conservative (the
configured value wins on weak evidence), and an `8`-point cross-family gap
avoids flapping between genuinely ambiguous encodings.

Proposed `monitor`-mode defaults (new):

| Constant | Proposed | Rationale |
| --- | --- | --- |
| `DEFAULT_MONITOR_WINDOW_MS` | `10_000` | 10 s gives bursty/slow sources time to emit distinguishing bytes without a long UX wait |
| `DEFAULT_MONITOR_MAX_BYTES` | reuse `DEFAULT_MAX_SAMPLE_BYTES` (64 KiB) | bound memory; whichever bound hits first ends the window |

`monitor` reuses the **same** confidence/delta thresholds as `Auto` so the
"leave unchanged when unsure" guarantee is identical; only the *sampling
horizon* differs (time-windowed accumulation vs. startup prefix). All four
values should be overridable via config so operators can widen the window for
very slow links.

### B. Prefetch time-window — where the change lands

Current prefetch (`crates/server/src/source_manager.rs`,
`crates/server/src/runner.rs`):

- `prefetch_for_detection()` already `open()`s the source, buffers frames into
  a `VecDeque`, accumulates raw bytes up to `DEFAULT_MAX_SAMPLE_BYTES`, runs
  `detect_content()`, then hands a `PrefetchedSource` (buffer + source) to the
  pipeline, which **replays** the buffered frames through the decoder. Today
  the loop breaks early on `DETECTION_SAMPLE_TIMEOUT = 250 ms` of idle.
- The decoder (`ClassifyingDecoder<Utf8TextDecoder>`) is moved into the runner
  task and owned exclusively there; it is built **after** detection in
  `start_default_pipeline()`, so picking the encoding before the pipeline
  starts requires no trait change and no shared mutable state.

For `monitor`, the only change is the prefetch loop's termination policy:

1. `prefetch_for_detection()` is extended to handle
   `DetectionMode::Monitor` (alongside `Auto`/`Suggest`). For `monitor` it
   loops until a **total deadline** `DEFAULT_MONITOR_WINDOW_MS` (tracked from
   the first received frame) or the `DEFAULT_MONITOR_MAX_BYTES` cap, instead
   of breaking on the first 250 ms idle gap. Per-`recv` it still uses a short
   timeout so a totally idle source does not block past the deadline.
2. After the window, `detect_content()` runs once exactly as for `auto`; the
   pipeline is then started with `report.effective_encoding`, and the buffered
   frames are replayed through the resulting decoder — so window frames are
   decoded with the final encoding.
3. `SourcePipelineMetadata` (`encoding`/`detection_mode`/`detection`) is
   populated exactly as today; no new control channel, no `restart`, no
   `Decoder` trait method.

Concurrency note for Part A (bulk): the client fan-out issues N independent
`restart` calls; each already serialises on the `sources` mutex inside
`restart_with_options`, so no new server-side coordination is required.

### C. Records captured during the window

Because detection completes before the pipeline starts and the buffered window
is replayed, **all** window records are decoded with the final encoding — there
is no forward-only gap and nothing to retro-decode. The trade-off is bounded
startup latency (≤ `DEFAULT_MONITOR_WINDOW_MS`) before the source's first
records appear. The deferred live hot-swap alternative would remove that
latency at the cost of a frozen-trait change and a forward-only limitation
(records before the swap stay decoded under the old encoding); see
“Alternatives considered”.

### D. UI / UX detail

Part A (bulk apply), reusing existing plumbing:

- The Sources panel already sends per-source restart with encoding via
  `onRestartWithServerEncoding(sid)` (`web/src/panels/sources/SourcesPanel.tsx`)
  using `sendCtl(sid, "restart", …, { encoding })`.
- Add a header action "apply encoding to all sources" that maps over eligible
  running sources and calls the same `sendCtl(... "restart" ...)`, then shows
  one aggregate toast: `N applied / M skipped (binary) / K failed`.
- i18n keys (mirror EN/JA), under the `sources.bulk_encoding.*` namespace, as
  shipped in Part A:
  - `sources.bulk_encoding.title` / `sources.bulk_encoding.apply` — labels
  - `sources.bulk_encoding.confirm` — destructive-action confirm
    (restart re-runs sources)
  - `sources.bulk_encoding.none` — nothing eligible
  - `sources.bulk_encoding.result` — result toast with `{applied}`,
    `{skipped}`, `{failed}`, `{encoding}` counts

Part B (monitor mode) UI:

- Settings → Source Start gains `monitor` in the detect-mode selector
  (`settings.source_start.encoding` / detect-mode group).
- Sources detail shows a "monitoring…" indicator while the prefetch window is
  open and, once started, the resolved `encoding` plus the existing
  `ContentDetectionReport` candidate/confidence rendering. No mid-stream
  change notice is needed because the encoding is fixed before the first
  record is shown.

### E. Error / diagnostic IDs (E-NNNN)

Existing registry (`crates/core/src/error_id.rs`) ranges: core `1000–1099`,
source `1100–1199`, **decoder `1300–1399`**, logsink `1400–1499`, wire/server
`2000–2099`, auth/TLS `2100–2199`. Highest currently used: `E-2103`. New codes
go in the decoder/detection band `1300–1399` (next free below `E-1301`
decoder-schema):

| Proposed | Kind | Meaning |
| --- | --- | --- |
| `E-1310` | diagnostic (not fatal) | `monitor`/`auto` window produced no confident candidate; source kept its configured encoding |
| `E-1311` | error | requested encoding label is not in the supported set; override refused (instead of silently falling back to UTF-8 in `codec::decode`) |

`E-1310` is informative (surfaced in the detection report / a soft toast, not a
hard failure). `E-1311` makes the currently-silent "unknown label → UTF-8"
fallback in `codec::encoding_for_label()` observable at the override boundary.
Each needs an `ErrorId` variant, a `code()` mapping, and a `docs/errors/E-NNNN.md`
page (file-per-code convention).

### F. Test plan

Unit (`crates/core`):

- `detect/content.rs`: `monitor` mode selects the same candidate as `auto`
  for a fixed corpus; below-threshold corpora yield the configured encoding
  (the "leave unchanged" invariant); `DetectionMode::parse("monitor")` and
  `as_str()` round-trip.

Server (`crates/server`):

- `source_manager`: a fake source that emits Shift_JIS bytes slowly (gaps
  longer than 250 ms but within the window) under `detect_mode = monitor`
  ends with `SourcePipelineMetadata.encoding == "shift_jis"`, and the buffered
  window records decode as Shift_JIS on replay; an ambiguous feed keeps the
  configured encoding.
- Part A: bulk restart over a mix of text + pcap sources applies encoding to
  text sources and skips pcap, with the expected aggregate counts.

Compat (`tests/compat/wire/v1/`, harness `crates/server/tests/wire_compat.rs`,
`check()` + `TRACEMUX_WIRE_BLESS=1`):

- Add `sources_with_monitor_detection.msgpack` proving the additive `monitor`
  `detection_mode` value (and any optional `window_ms`/`window_bytes` report
  echo) round-trips, and that an old-shape decoder ignores the new fields. Do
  **not** bless on CI.

Web (`web/tests`): bulk-apply toast counts, monitor-mode selector
persistence, and the auto-switch notification.
