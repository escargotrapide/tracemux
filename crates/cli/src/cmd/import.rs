//! `wanlogger import` ? convert a foreign log artefact into a v0.1
//! session-dir.
//!
//! v0.1 dispatches on `kind` to the corresponding implementation in
//! `wanlogger_core::importer`. All built-in importers are stubs; this
//! subcommand surfaces a clear error listing the supported kinds and
//! does not produce a partial session-dir on failure.

use std::path::Path;

use anyhow::{bail, Result};

/// Stable list of importer kinds known to v0.1. Each maps to a
/// module under `wanlogger_core::importer` whose actual logic is
/// stubbed.
pub const KINDS: &[&str] = &["teraterm", "pcapng", "csv", "text"];

/// Run the `import` subcommand.
///
/// # Errors
/// Always returns an error in v0.1 (importers are stubs); the error
/// message lists [`KINDS`] when `kind` is unknown.
pub fn run(kind: &str, src: &Path, dst: &Path) -> Result<()> {
    if !KINDS.contains(&kind) {
        bail!("unknown importer kind `{kind}`; known: {}", KINDS.join(", "));
    }
    if !src.exists() {
        bail!("source artefact does not exist: {}", src.display());
    }
    tracing::warn!(
        kind,
        src = %src.display(),
        dst = %dst.display(),
        "import: importer is a v0.1 stub"
    );
    bail!(
        "importer `{kind}` is not implemented in v0.1; \
         it will be added under `wanlogger_core::importer::{kind}` per the add-importer skill"
    );
}
