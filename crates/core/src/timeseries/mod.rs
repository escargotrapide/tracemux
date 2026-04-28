//! `TimeseriesSink` trait — persists numeric series to columnar storage
//! (parquet). **Frozen v0.1.** Experimental in v0.1.

use async_trait::async_trait;

use crate::{time::DualTimestamp, Result};

/// One numeric sample.
#[derive(Debug, Clone)]
pub struct TimeseriesPoint {
    /// Series name.
    pub series: String,
    /// Value.
    pub value: f64,
    /// Optional unit.
    pub unit: Option<String>,
}

/// Sink that persists numeric series.
#[async_trait]
pub trait TimeseriesSink: Send + Sync + 'static {
    /// Append one point.
    async fn append(&mut self, ts: &DualTimestamp, point: TimeseriesPoint) -> Result<()>;

    /// Flush buffered points to disk.
    async fn flush(&mut self) -> Result<()>;

    /// Close the sink.
    async fn close(&mut self) -> Result<()>;
}

pub mod parquet;
