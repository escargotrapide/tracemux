//! `Exporter` trait — writes a `session-dir/` to a foreign format.
//! **Frozen v0.1.**

use std::path::Path;

use async_trait::async_trait;

use crate::Result;

/// Exporter of `session-dir/` to foreign artefacts.
#[async_trait]
pub trait Exporter: Send + Sync + 'static {
    /// Stable kind string (e.g. `"csv"`, `"text"`, `"jsonl"`).
    fn kind(&self) -> &'static str;

    /// Export `session-dir/` rooted at `src` to `dst`.
    async fn export(&mut self, src: &Path, dst: &Path) -> Result<()>;
}

pub mod csv;
pub mod jsonl;
pub mod text;
