//! `Importer` trait — converts a foreign log artefact into a
//! `session-dir/`. **Frozen v0.1.** See `add-importer` skill.

use std::path::Path;

use async_trait::async_trait;

use crate::Result;

/// Importer of foreign log artefacts.
#[async_trait]
pub trait Importer: Send + Sync + 'static {
    /// Stable kind string (e.g. `"teraterm"`, `"pcapng"`, `"csv"`).
    fn kind(&self) -> &'static str;

    /// Import `src` into a fresh `session-dir/` rooted at `dst`.
    async fn import(&mut self, src: &Path, dst: &Path) -> Result<()>;
}

pub mod csv;
pub mod pcapng;
pub mod teraterm;
pub mod text;
