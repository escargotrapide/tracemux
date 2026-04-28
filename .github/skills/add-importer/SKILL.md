---
name: add-importer
description: Add an Importer that ingests historical logs into a session-dir
---

# Skill: add an Importer

`Importer`s convert a foreign log artefact (Tera Term `.log`, pcapng,
Loki/Splunk export, CSV, …) into a wanlogger `session-dir/`.

## Steps

1. Read [`crates/core/src/importer/mod.rs`](../../../crates/core/src/importer/mod.rs).
   Frozen v0.1.
2. Copy `templates/importer/` into `crates/core/src/importer/<name>.rs`.
3. Synthesize `ts_origin` from the source artefact; set
   `ts_ingest = now()` and `clock_quality = "imported"`.
4. Add fixtures under `fixtures/importer/<name>/` with `PROVENANCE.md`.
5. Compat test in `crates/core/tests/importer_<name>.rs`.
6. `FR-IMP-<name>` in requirements.
7. `just ai-verify`.
