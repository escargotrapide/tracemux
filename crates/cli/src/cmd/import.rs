//! `wanlogger import` -- convert a foreign log artefact into a v0.1
//! session-dir.
//!
//! Dispatches `kind` to the matching implementation in
//! [`wanlogger_core::importer`]. `text` and `csv` are wired through;
//! `teraterm` and `pcapng` still bail with a clear error.

use std::path::Path;

use anyhow::{bail, Result};
use wanlogger_core::importer::{csv::CsvImporter, text::TextImporter, Importer};

/// Stable list of importer kinds known to v0.1.
pub const KINDS: &[&str] = &["teraterm", "pcapng", "csv", "text"];

/// Run the `import` subcommand.
///
/// # Errors
/// Returns an error when `kind` is unknown, the source file is
/// missing, the destination directory already contains a session, or
/// when the underlying [`Importer`] fails. `teraterm` and `pcapng`
/// always bail (not implemented in v0.1).
pub async fn run(kind: &str, src: &Path, dst: &Path) -> Result<()> {
    if !KINDS.contains(&kind) {
        bail!(
            "unknown importer kind `{kind}`; known: {}",
            KINDS.join(", ")
        );
    }
    if !src.exists() {
        bail!("source artefact does not exist: {}", src.display());
    }
    if dst.exists() && dst.is_dir() {
        let already = std::fs::read_dir(dst)
            .map(|mut it| it.next().is_some())
            .unwrap_or(false);
        if already {
            bail!(
                "destination session-dir is non-empty; refusing to overwrite: {}",
                dst.display()
            );
        }
    }

    match kind {
        "text" => TextImporter.import(src, dst).await?,
        "csv" => CsvImporter.import(src, dst).await?,
        "teraterm" | "pcapng" => bail!(
            "importer `{kind}` is not implemented in v0.1; \
             see the add-importer skill"
        ),
        _ => unreachable!("kind already validated"),
    }
    tracing::info!(kind, src = %src.display(), dst = %dst.display(), "import: ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // REQ: FR-IMP-001
    #[tokio::test]
    async fn unknown_kind_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.txt");
        std::fs::write(&src, b"x").unwrap();
        let err = run("nope", &src, &dir.path().join("out"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown importer kind"));
    }

    // REQ: FR-IMP-001
    #[tokio::test]
    async fn missing_source_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = run("text", &dir.path().join("missing"), &dir.path().join("out"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("source artefact does not exist"));
    }

    // REQ: FR-IMP-001
    #[tokio::test]
    async fn deferred_kinds_bail() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.txt");
        std::fs::write(&src, b"x").unwrap();
        for k in ["teraterm", "pcapng"] {
            let err = run(k, &src, &dir.path().join(format!("out-{k}")))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("not implemented"));
        }
    }
}
