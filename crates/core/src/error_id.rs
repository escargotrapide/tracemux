//! `E-NNNN` error registry. **Frozen v0.1.**
//!
//! Adding a new error code:
//! 1. Pick the next free number in the appropriate range (see
//!    [`docs/errors/README.md`](../../../docs/errors/README.md)).
//! 2. Add a variant to [`ErrorId`].
//! 3. Document under `docs/errors/E-NNNN.md`.
//!
//! Removing or renumbering codes is a breaking change.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable string id for a public-facing error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ErrorId {
    // ---- core / pipeline (1000..=1099) ----
    /// `E-1001` — generic pipeline error.
    E1001PipelineGeneric,
    /// `E-1002` — backpressure deadline exceeded.
    E1002Backpressure,
    /// `E-1003` — framer overflow (frame > limit).
    E1003FramerOverflow,
    // ---- source (1100..=1199) ----
    /// `E-1101` — source open failed.
    E1101SourceOpen,
    /// `E-1102` — source closed unexpectedly.
    E1102SourceClosed,
    /// `E-1103` — packet capture backend unavailable.
    E1103PcapBackendUnavailable,
    /// `E-1104` — packet capture permission denied.
    E1104PcapPermissionDenied,
    /// `E-1105` — packet capture BPF filter invalid.
    E1105PcapInvalidFilter,
    /// `E-1106` — packet capture interface unavailable.
    E1106PcapInterfaceUnavailable,
    // ---- decoder (1300..=1399) ----
    /// `E-1301` — decoder schema mismatch.
    E1301DecoderSchema,
    // ---- logsink / WAL (1400..=1499) ----
    /// `E-1401` — WAL fsync failed.
    E1401WalFsync,
    /// `E-1402` — log rotation failed.
    E1402RotateFail,
    // ---- wire / server (2000..=2099) ----
    /// `E-2001` — wire frame malformed.
    E2001WireMalformed,
    /// `E-2002` — wire `DoS` limit exceeded.
    E2002WireLimit,
    // ---- auth / TLS (2100..=2199) ----
    /// `E-2101` — auth rejected.
    E2101AuthRejected,
    /// `E-2102` — TLS handshake failed.
    E2102TlsHandshake,
    /// `E-2103` — TOFU fingerprint mismatch.
    E2103TofuMismatch,
}

impl ErrorId {
    /// Stable string code (e.g. `"E-1001"`).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::E1001PipelineGeneric => "E-1001",
            Self::E1002Backpressure => "E-1002",
            Self::E1003FramerOverflow => "E-1003",
            Self::E1101SourceOpen => "E-1101",
            Self::E1102SourceClosed => "E-1102",
            Self::E1103PcapBackendUnavailable => "E-1103",
            Self::E1104PcapPermissionDenied => "E-1104",
            Self::E1105PcapInvalidFilter => "E-1105",
            Self::E1106PcapInterfaceUnavailable => "E-1106",
            Self::E1301DecoderSchema => "E-1301",
            Self::E1401WalFsync => "E-1401",
            Self::E1402RotateFail => "E-1402",
            Self::E2001WireMalformed => "E-2001",
            Self::E2002WireLimit => "E-2002",
            Self::E2101AuthRejected => "E-2101",
            Self::E2102TlsHandshake => "E-2102",
            Self::E2103TofuMismatch => "E-2103",
        }
    }
}

/// Crate-wide error type carrying an [`ErrorId`].
#[derive(Debug, Error)]
#[error("{id}: {message}", id = id.code())]
pub struct TraceMuxError {
    /// Error id.
    pub id: ErrorId,
    /// Human-readable detail.
    pub message: String,
    /// Optional source.
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl TraceMuxError {
    /// Build a new error.
    #[must_use]
    pub fn new(id: ErrorId, message: impl Into<String>) -> Self {
        Self {
            id,
            message: message.into(),
            source: None,
        }
    }

    /// Attach a `source`.
    #[must_use]
    pub fn with_source(mut self, src: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(src));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorId, TraceMuxError};

    // REQ: FR-CORE-003 (every error has a stable E-NNNN code)
    #[test]
    fn every_variant_has_unique_code() {
        let all = [
            ErrorId::E1001PipelineGeneric,
            ErrorId::E1002Backpressure,
            ErrorId::E1003FramerOverflow,
            ErrorId::E1101SourceOpen,
            ErrorId::E1102SourceClosed,
            ErrorId::E1103PcapBackendUnavailable,
            ErrorId::E1104PcapPermissionDenied,
            ErrorId::E1105PcapInvalidFilter,
            ErrorId::E1106PcapInterfaceUnavailable,
            ErrorId::E1301DecoderSchema,
            ErrorId::E1401WalFsync,
            ErrorId::E1402RotateFail,
            ErrorId::E2001WireMalformed,
            ErrorId::E2002WireLimit,
            ErrorId::E2101AuthRejected,
            ErrorId::E2102TlsHandshake,
            ErrorId::E2103TofuMismatch,
        ];
        let mut seen = std::collections::HashSet::new();
        for id in all {
            let code = id.code();
            assert!(code.starts_with("E-"), "bad code: {code}");
            assert!(seen.insert(code), "duplicate code: {code}");
        }
    }

    #[test]
    fn display_includes_code_and_message() {
        let e = TraceMuxError::new(ErrorId::E1003FramerOverflow, "frame too large");
        assert_eq!(e.to_string(), "E-1003: frame too large");
    }
}
