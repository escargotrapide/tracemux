//! `wanlogger export` -- render a session-dir into a foreign format.
//!
//! Dispatches `kind` to the matching implementation in
//! [`wanlogger_core::exporter`]. All three v0.1 kinds (`csv`, `text`,
//! `jsonl`) are wired through.

use std::path::Path;

use anyhow::{bail, Result};
use wanlogger_core::exporter::{csv, jsonl, text};

/// Stable list of exporter kinds known to v0.1.
pub const KINDS: &[&str] = &["csv", "text", "jsonl"];

/// Run the `export` subcommand.
///
/// # Errors
/// Returns an error when `kind` is unknown, when `src` is not a
/// session-dir, or when the underlying exporter fails.
pub fn run(kind: &str, src: &Path, dst: &Path, timezone: Option<&str>) -> Result<()> {
    if !KINDS.contains(&kind) {
        bail!(
            "unknown exporter kind `{kind}`; known: {}",
            KINDS.join(", ")
        );
    }
    if !src.is_dir() {
        bail!("source must be a session-dir: {}", src.display());
    }
    if !src.join("index.jsonl").is_file() {
        bail!(
            "source does not look like a session-dir (missing index.jsonl): {}",
            src.display()
        );
    }

    match kind {
        "text" => text::export_with_timezone(src, dst, timezone)?,
        "csv" => csv::export_with_timezone(src, dst, timezone)?,
        "jsonl" => jsonl::export_with_timezone(src, dst, timezone)?,
        _ => unreachable!("kind already validated"),
    }
    tracing::info!(kind, src = %src.display(), dst = %dst.display(), "export: ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // REQ: FR-EXP-001
    #[test]
    fn unknown_kind_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = run("nope", dir.path(), &dir.path().join("out"), None).unwrap_err();
        assert!(err.to_string().contains("unknown exporter kind"));
    }

    // REQ: FR-EXP-001
    #[test]
    fn rejects_non_session_dir() {
        let dir = tempfile::tempdir().unwrap();
        // empty dir -- no index.jsonl
        let err = run("text", dir.path(), &dir.path().join("out.txt"), None).unwrap_err();
        assert!(err.to_string().contains("missing index.jsonl"));
    }
}
