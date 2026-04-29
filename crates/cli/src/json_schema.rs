//! `wanlogger json-schema` — emits JSON schemas for `--format json`
//! output.
//!
//! These schemas are written under
//! `docs/protocols/cli-output/v1/<name>.schema.json`. They form part
//! of the **frozen v0.1 cli-output surface** (see `AGENTS.md` §6) and
//! must change in lockstep with `tests/compat/cli/*` snapshots.
//!
//! The current implementation emits intentionally-minimal placeholder
//! schemas — enough to validate that downstream tooling can locate
//! them — and is expected to be replaced by `schemars`-derived
//! schemas as each `--format json` payload type stabilises.

use std::path::Path;

use anyhow::Context;

/// Schemas emitted by `wanlogger json-schema`.
const SCHEMAS: &[(&str, &str)] = &[
    (
        "ai-verify.schema.json",
        include_str!("schemas/ai-verify.schema.json"),
    ),
    (
        "detect.schema.json",
        include_str!("schemas/detect.schema.json"),
    ),
    (
        "version.schema.json",
        include_str!("schemas/version.schema.json"),
    ),
];

/// Emit every schema into `out`. Existing files are overwritten.
///
/// # Errors
/// Returns an error if the directory cannot be created or any file
/// cannot be written.
pub fn emit(out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;
    for (name, body) in SCHEMAS {
        let target = out.join(name);
        std::fs::write(&target, body).with_context(|| format!("write {}", target.display()))?;
        tracing::info!(path = %target.display(), "json-schema: wrote");
    }
    tracing::info!(out = %out.display(), count = SCHEMAS.len(), "json-schema: done");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_all_schemas_to_a_fresh_dir() {
        let mut dir = std::env::temp_dir();
        let pid = std::process::id();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!("wanlogger-schemas-{pid}-{nonce}"));

        emit(&dir).unwrap();
        for (name, _) in SCHEMAS {
            let p = dir.join(name);
            assert!(p.exists(), "missing {}", p.display());
            let body = std::fs::read_to_string(&p).unwrap();
            // Smoke-check the body is JSON.
            let _: serde_json::Value =
                serde_json::from_str(&body).expect("schema must be valid JSON");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
