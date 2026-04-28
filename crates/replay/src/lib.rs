//! `wanlogger-replay` — drives a session-dir back through the
//! pipeline. v0.1 stub.

#![warn(missing_docs)]

use std::path::Path;

/// Replay `session_dir` at `rate` (0.0 = lockstep) with deterministic
/// `seed`.
#[allow(clippy::unused_async)] // will become async once impl reads files
pub async fn run(session_dir: &Path, rate: f32, seed: Option<u64>) -> anyhow::Result<()> {
    tracing::info!(?session_dir, rate, ?seed, "replay: v0.1 stub");
    Ok(())
}
