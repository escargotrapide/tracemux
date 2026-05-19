//! `wanlogger-core` — frozen v0.1 trait surfaces and core implementations.
//!
//! See [`AGENTS.md`](../../../AGENTS.md) and
//! [`docs/adr/0001-foundations.md`](../../../docs/adr/0001-foundations.md)
//! for the architecture.
//!
//! The four-layer pipeline is `Source → Framer → Decoder → LogSink`,
//! with orthogonal services [`sink`], [`importer`], [`exporter`],
//! [`timeseries`], [`time`]. All trait surfaces in this crate are
//! **frozen at v0.1** and may only be amended via an ADR + version
//! bump (see [`docs/protocols/`](../../../docs/protocols/)).

#![warn(missing_docs)]

pub mod classify;
pub mod codec;
pub mod config;
pub mod decoder;
pub mod detect;
pub mod error_id;
pub mod eventbus;
pub mod exporter;
pub mod framer;
pub mod importer;
pub mod log;
pub mod logsink;
pub mod metrics;
pub mod secret;
pub mod session;
pub mod session_name;
pub mod sink;
pub mod source;
pub mod time;
pub mod timeseries;

pub use error_id::{ErrorId, WanloggerError};
pub use time::{ClockQuality, ClockSource, DualTimestamp, TimeSource};

/// Crate-wide `Result` alias.
pub type Result<T, E = WanloggerError> = core::result::Result<T, E>;
