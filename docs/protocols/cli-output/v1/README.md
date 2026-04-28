# CLI output schemas v1

> **Frozen v0.1.**

The `wanlogger` CLI emits machine-readable output via `--format json`.
Each subcommand produces a JSON object whose schema lives here.

Schemas are kept under this directory as `<subcommand>.schema.json`
(JSON Schema 2020-12) and are exposed at runtime via:

```
wanlogger json-schema --out docs/protocols/cli-output/v1/
```

CI snapshots (`tests/compat/cli/v1/*.json`) are diffed against the
current output to detect breaking changes.

## Versioning

| Surface | Version | Source of truth |
| ------- | ------- | --------------- |
| Schemas | `1.0.0` | files under this directory |
| Compat  | snapshots| `tests/compat/cli/v1/*` |

## Compatibility rules

- Adding optional fields → minor bump (1.x.y).
- Removing fields, changing types, or renaming → **major bump** (2.0.0)
  via ADR.
