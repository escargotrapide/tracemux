//! `wanlogger export` ? render a session-dir into a foreign format.
//!
//! v0.1 dispatches on `kind` to the corresponding implementation in
//! `wanlogger_core::exporter`. All built-in exporters are stubs; this
//! subcommand surfaces a clear error listing the supported kinds.

use std::path::Path;

use anyhow::{bail, Result};

/// Stable list of exporter kinds known to v0.1.
pub const KINDS: &[&str] = &["csv", "text", "jsonl"];

/// Run the `export` subcommand.
///
/// # Errors
/// Always returns an error in v0.1 (exporters are stubs); the error
/// message lists [`KINDS`] when `kind` is unknown.
pub fn run(kind: &str, src: &Path, dst: &Path) -> Result<()> {
    if !KINDS.contains(&kind) {
        bail!("unknown exporter kind `{kind}`; known: {}", KINDS.join(", "));
    }
    if !src.is_dir() {
        bail!("source must be a session-dir: {}", src.display());
    }
    tracing::warn!(
        kind,
        src = %src.display(),
        dst = %dst.display(),
        "export: exporter is a v0.1 stub"
    );
    bail!(
        "exporter `{kind}` is not implemented in v0.1; \
         it will be added under `wanlogger_core::exporter::{kind}` per the add-exporter skill"
    );
}
