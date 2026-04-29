//! `wanlogger-replay` — drives a session-dir back through the
//! pipeline. v0.1 ships a basic walker that emits each indexed
//! record at `1.0/rate` of the originally-recorded inter-arrival
//! time. `rate == 0.0` means "as fast as possible" (lockstep). A
//! deterministic `seed` is reserved for shuffled / fault-injection
//! replay modes added later.

#![warn(missing_docs)]

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use wanlogger_core::log::index::IndexEntry;

/// Replay summary returned to callers (and surfaced via tracing).
#[derive(Debug, Default, Clone)]
pub struct ReplayStats {
    /// Records walked.
    pub records: u64,
    /// Total wall-clock duration.
    pub elapsed: Duration,
}

/// Replay `session_dir` at `rate` (`0.0` = lockstep) with a
/// deterministic `seed`.
///
/// # Errors
/// Returns an error if `index.jsonl` cannot be read or parsed.
pub async fn run(
    session_dir: &Path,
    rate: f32,
    seed: Option<u64>,
) -> anyhow::Result<ReplayStats> {
    tracing::info!(?session_dir, rate, ?seed, "replay: starting");
    let started = std::time::Instant::now();
    let idx_path = session_dir.join("index.jsonl");
    let f = File::open(&idx_path)
        .with_context(|| format!("replay open {}", idx_path.display()))?;
    let mut prev_ts: Option<u64> = None;
    let mut count = 0u64;
    for line in BufReader::new(f).lines() {
        let line = line.context("replay read")?;
        if line.is_empty() {
            continue;
        }
        let entry: IndexEntry =
            serde_json::from_str(&line).context("replay parse")?;
        if rate > 0.0 {
            if let Some(prev) = prev_ts {
                let delta_ns = entry.mono_ns.saturating_sub(prev);
                #[allow(clippy::cast_precision_loss)]
                let scaled = (delta_ns as f64 / f64::from(rate)) as u64;
                if scaled > 0 {
                    tokio::time::sleep(Duration::from_nanos(scaled)).await;
                }
            }
            prev_ts = Some(entry.mono_ns);
        }
        tracing::debug!(
            sid = %entry.sid,
            off = entry.off,
            len = entry.len,
            "replay: record"
        );
        count += 1;
    }
    let stats = ReplayStats {
        records: count,
        elapsed: started.elapsed(),
    };
    tracing::info!(
        records = stats.records,
        elapsed_ms = stats.elapsed.as_millis() as u64,
        "replay: done"
    );
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replays_an_empty_session() {
        let dir =
            std::env::temp_dir().join(format!("wlg-replay-lib-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.jsonl"), b"").unwrap();
        let s = run(&dir, 0.0, None).await.unwrap();
        assert_eq!(s.records, 0);
    }

    #[tokio::test]
    async fn replays_three_records_lockstep() {
        use wanlogger_core::log::index::{Dir, IndexEntry, IndexWriter, Kind};
        use wanlogger_core::time::{ClockQuality, ClockSource, DualTimestamp};
        let dir =
            std::env::temp_dir().join(format!("wlg-replay-3-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut iw = IndexWriter::create(&dir).unwrap();
        let sid = uuid::Uuid::nil();
        for i in 0..3u64 {
            let ts = DualTimestamp {
                ts_origin_ns: i as i64,
                ts_ingest_ns: i as i64,
                mono_ns: i,
                boot_id: uuid::Uuid::nil(),
                node_id: uuid::Uuid::nil(),
                clock_offset_ms: 0,
                clock_quality: ClockQuality::BestEffort,
                drift_ppm: 0.0,
                clock_source: ClockSource::System,
            };
            let e = IndexEntry::from_envelope(&ts, sid, Dir::In, Kind::Bytes, i, 1);
            iw.append(&e).unwrap();
        }
        iw.flush().unwrap();
        let s = run(&dir, 0.0, None).await.unwrap();
        assert_eq!(s.records, 3);
    }
}
