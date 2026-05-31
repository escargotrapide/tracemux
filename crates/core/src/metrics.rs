//! Lightweight metrics registry.
//!
//! Provides [`Counter`] and [`Gauge`] handles backed by atomics, plus
//! a tiny [`Registry`] map for naming. v0.1 is intentionally
//! dependency-free; a Prometheus exporter lives behind the
//! `metrics` feature in [`prom`] and is not required at runtime.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

/// Monotonic counter handle.
#[derive(Debug, Clone, Default)]
pub struct Counter(Arc<AtomicU64>);

impl Counter {
    /// Increment by `n`.
    pub fn inc(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    /// Read.
    #[must_use]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Bidirectional gauge handle.
#[derive(Debug, Clone, Default)]
pub struct Gauge(Arc<AtomicI64>);

impl Gauge {
    /// Set absolute value.
    pub fn set(&self, v: i64) {
        self.0.store(v, Ordering::Relaxed);
    }

    /// Add (signed).
    pub fn add(&self, delta: i64) {
        self.0.fetch_add(delta, Ordering::Relaxed);
    }

    /// Read.
    #[must_use]
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Snapshot of one metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snapshot {
    /// Counter snapshot.
    Counter(u64),
    /// Gauge snapshot.
    Gauge(i64),
}

/// Process-wide registry. Names are stable strings.
#[derive(Debug, Default)]
pub struct Registry {
    counters: Mutex<BTreeMap<String, Counter>>,
    gauges: Mutex<BTreeMap<String, Gauge>>,
}

impl Registry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a counter by name.
    pub fn counter(&self, name: &str) -> Counter {
        let mut m = self.counters.lock();
        m.entry(name.to_string()).or_default().clone()
    }

    /// Get or create a gauge by name.
    pub fn gauge(&self, name: &str) -> Gauge {
        let mut m = self.gauges.lock();
        m.entry(name.to_string()).or_default().clone()
    }

    /// Snapshot every metric.
    pub fn snapshot(&self) -> BTreeMap<String, Snapshot> {
        let mut out = BTreeMap::new();
        for (k, v) in self.counters.lock().iter() {
            out.insert(k.clone(), Snapshot::Counter(v.get()));
        }
        for (k, v) in self.gauges.lock().iter() {
            out.insert(k.clone(), Snapshot::Gauge(v.get()));
        }
        out
    }
}

#[cfg(feature = "metrics")]
pub mod prom {
    //! Reserved for `prometheus`-crate registry.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_increments() {
        let r = Registry::new();
        let c = r.counter("frames_in");
        c.inc(3);
        c.inc(2);
        assert_eq!(c.get(), 5);
        let snap = r.snapshot();
        assert_eq!(snap.get("frames_in"), Some(&Snapshot::Counter(5)));
    }

    #[test]
    fn gauge_set_add() {
        let r = Registry::new();
        let g = r.gauge("queue_depth");
        g.set(10);
        g.add(-3);
        assert_eq!(g.get(), 7);
        let snap = r.snapshot();
        assert_eq!(snap.get("queue_depth"), Some(&Snapshot::Gauge(7)));
    }

    #[test]
    fn handles_share_state() {
        let r = Registry::new();
        let a = r.counter("x");
        let b = r.counter("x");
        a.inc(1);
        b.inc(1);
        assert_eq!(a.get(), 2);
    }
}
