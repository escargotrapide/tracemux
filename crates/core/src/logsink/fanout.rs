//! Fan-out [`LogSink`] — forwards each call to N inner sinks.
//!
//! On a per-call basis, the first inner-sink error short-circuits
//! the call. Use this when both `file::FileLogSink` and (for
//! example) a future S3 / metrics sink need the same record stream.

use async_trait::async_trait;
use bytes::Bytes;

use super::{Direction, LogSink};
use crate::{decoder::Record, time::DualTimestamp, Result};

/// Fan-out sink.
pub struct FanoutLogSink {
    inner: Vec<Box<dyn LogSink>>,
}

impl FanoutLogSink {
    /// Construct from `inner` sinks.
    #[must_use]
    pub fn new(inner: Vec<Box<dyn LogSink>>) -> Self {
        Self { inner }
    }

    /// Sink count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[async_trait]
impl LogSink for FanoutLogSink {
    async fn append_raw(&mut self, ts: &DualTimestamp, dir: Direction, data: Bytes) -> Result<()> {
        for s in &mut self.inner {
            s.append_raw(ts, dir, data.clone()).await?;
        }
        Ok(())
    }

    async fn append_record(&mut self, ts: &DualTimestamp, record: &Record) -> Result<()> {
        for s in &mut self.inner {
            s.append_record(ts, record).await?;
        }
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        for s in &mut self.inner {
            s.commit().await?;
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        for s in &mut self.inner {
            s.close().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use super::*;

    struct Counting {
        raws: Arc<AtomicU32>,
        commits: Arc<AtomicU32>,
    }

    #[async_trait]
    impl LogSink for Counting {
        async fn append_raw(
            &mut self,
            _ts: &DualTimestamp,
            _dir: Direction,
            _data: Bytes,
        ) -> Result<()> {
            self.raws.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        async fn append_record(
            &mut self,
            _ts: &DualTimestamp,
            _record: &Record,
        ) -> Result<()> {
            Ok(())
        }
        async fn commit(&mut self) -> Result<()> {
            self.commits.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        async fn close(&mut self) -> Result<()> {
            Ok(())
        }
    }

    fn ts() -> DualTimestamp {
        DualTimestamp {
            ts_origin_ns: 0,
            ts_ingest_ns: 0,
            mono_ns: 0,
            boot_id: uuid::Uuid::nil(),
            node_id: uuid::Uuid::nil(),
            clock_offset_ms: 0,
            clock_quality: crate::time::ClockQuality::BestEffort,
            drift_ppm: 0.0,
            clock_source: crate::time::ClockSource::System,
        }
    }

    #[tokio::test]
    async fn forwards_to_all_inner_sinks() {
        let raws = Arc::new(AtomicU32::new(0));
        let commits = Arc::new(AtomicU32::new(0));
        let a = Counting {
            raws: raws.clone(),
            commits: commits.clone(),
        };
        let b = Counting {
            raws: raws.clone(),
            commits: commits.clone(),
        };
        let mut f = FanoutLogSink::new(vec![Box::new(a), Box::new(b)]);
        f.append_raw(&ts(), Direction::In, Bytes::from_static(b"x"))
            .await
            .unwrap();
        f.commit().await.unwrap();
        assert_eq!(raws.load(Ordering::Relaxed), 2);
        assert_eq!(commits.load(Ordering::Relaxed), 2);
    }
}
