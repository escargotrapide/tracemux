//! Wire-protocol implementation (`MessagePack` frame envelopes).
//!
//! **Critical path.** Frozen v0.1.
//! See `docs/protocols/wire-protocol.md`.
//!
//! Every wire frame is a MessagePack map with a fixed envelope:
//!
//! ```text
//! { type: str, sid?: str, ch?: u32, seq: u64, payload: any }
//! ```
//!
//! This module:
//!
//! * defines [`FrameType`] (the closed set of `type` strings),
//! * defines the [`Envelope`] struct and its `serde`-driven
//!   MessagePack codec,
//! * enforces the [`MAX_FRAME_BYTES`] DoS limit on both encode and
//!   decode paths,
//! * surfaces malformed / oversize errors as [`E-2001`] / [`E-2002`].
//!
//! [`E-2001`]: wanlogger_core::ErrorId::E2001WireMalformed
//! [`E-2002`]: wanlogger_core::ErrorId::E2002WireLimit

use rmpv::Value;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use wanlogger_core::{ErrorId, WanloggerError};

/// Maximum size of a single wire frame in bytes (1 MiB).
///
/// See `docs/protocols/wire-protocol.md` § Limits.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Closed set of `type` discriminators used by the v0.1 wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameType {
    /// Client → server: capabilities + app version.
    Hello,
    /// Client → server: bearer reauth (when not in subprotocol).
    Auth,
    /// Client → server: subscribe to `(sid, ch)`.
    Sub,
    /// Client → server: unsubscribe.
    Unsub,
    /// Server → client: record envelope.
    Data,
    /// Bidirectional: control event (connect, EOF, error, …).
    Ctl,
    /// Client → server: write-back to a `Sink`.
    Write,
    /// Server → client: server-side counters.
    Metrics,
    /// Client → server: UI logs forwarded to server logger.
    Clientlog,
    /// Bidirectional: liveness ping.
    Ping,
    /// Bidirectional: pong response.
    Pong,
    /// Bidirectional: dedicated clock sync exchange.
    ClockSync,
    /// Client → server: UI panel visibility / coalescing hint.
    PanelPriority,
    /// Reserved for sub-mux. Not emitted in v0.1.
    Child,
}

impl FrameType {
    /// Stable string token used on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hello => "hello",
            Self::Auth => "auth",
            Self::Sub => "sub",
            Self::Unsub => "unsub",
            Self::Data => "data",
            Self::Ctl => "ctl",
            Self::Write => "write",
            Self::Metrics => "metrics",
            Self::Clientlog => "clientlog",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::ClockSync => "clock_sync",
            Self::PanelPriority => "panel_priority",
            Self::Child => "child",
        }
    }
}

impl std::fmt::Display for FrameType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Wire envelope shared by every frame.
///
/// `payload` is left as an opaque [`rmpv::Value`] so type-specific
/// schemas (see `docs/protocols/wire-protocol.md`) can evolve
/// independently without touching this critical-path module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    /// Frame discriminator.
    #[serde(rename = "type")]
    pub kind: FrameType,
    /// Session id (UUID v4) when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    /// Multiplex channel within the connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ch: Option<u32>,
    /// Monotonic per `(connection, type)` sequence.
    pub seq: u64,
    /// Type-specific payload.
    pub payload: Value,
}

impl Envelope {
    /// Build a new envelope.
    #[must_use]
    pub fn new(kind: FrameType, seq: u64, payload: Value) -> Self {
        Self {
            kind,
            sid: None,
            ch: None,
            seq,
            payload,
        }
    }

    /// Attach a session id.
    #[must_use]
    pub fn with_sid(mut self, sid: impl Into<String>) -> Self {
        self.sid = Some(sid.into());
        self
    }

    /// Attach a multiplex channel.
    #[must_use]
    pub fn with_ch(mut self, ch: u32) -> Self {
        self.ch = Some(ch);
        self
    }
}

/// Errors produced while encoding or decoding a wire frame.
///
/// Every variant maps to a stable [`ErrorId`] and is surfaced through
/// the canonical [`WanloggerError`] via [`From`].
#[derive(Debug, Error)]
pub enum WireError {
    /// The frame is structurally invalid (wrong shape, unknown
    /// `type`, missing required field, etc.).
    #[error("E-2001: wire frame malformed: {0}")]
    Malformed(String),
    /// The frame exceeded [`MAX_FRAME_BYTES`].
    #[error("E-2002: wire frame too large: {size} > {limit}")]
    TooLarge {
        /// Observed size in bytes.
        size: usize,
        /// Configured limit.
        limit: usize,
    },
}

impl WireError {
    /// Stable [`ErrorId`] for this variant.
    #[must_use]
    pub const fn id(&self) -> ErrorId {
        match self {
            Self::Malformed(_) => ErrorId::E2001WireMalformed,
            Self::TooLarge { .. } => ErrorId::E2002WireLimit,
        }
    }
}

impl From<WireError> for WanloggerError {
    fn from(e: WireError) -> Self {
        let id = e.id();
        WanloggerError::new(id, e.to_string())
    }
}

/// Encode an envelope into a MessagePack byte buffer.
///
/// # Errors
/// Returns [`WireError::TooLarge`] if the encoded form exceeds
/// [`MAX_FRAME_BYTES`], or [`WireError::Malformed`] if `serde`
/// serialisation fails (should not happen in practice).
pub fn encode(env: &Envelope) -> Result<Vec<u8>, WireError> {
    // Use named-field encoding (MessagePack map keyed by string) so the
    // wire schema stays human-debuggable and forward-compatible.
    let buf =
        rmp_serde::to_vec_named(env).map_err(|e| WireError::Malformed(format!("encode: {e}")))?;
    if buf.len() > MAX_FRAME_BYTES {
        return Err(WireError::TooLarge {
            size: buf.len(),
            limit: MAX_FRAME_BYTES,
        });
    }
    Ok(buf)
}

/// Decode a MessagePack byte buffer into an envelope.
///
/// # Errors
/// Returns [`WireError::TooLarge`] if `bytes` exceeds
/// [`MAX_FRAME_BYTES`] before decoding, or [`WireError::Malformed`]
/// if the buffer is not a valid MessagePack envelope.
pub fn decode(bytes: &[u8]) -> Result<Envelope, WireError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(WireError::TooLarge {
            size: bytes.len(),
            limit: MAX_FRAME_BYTES,
        });
    }
    rmp_serde::from_slice::<Envelope>(bytes)
        .map_err(|e| WireError::Malformed(format!("decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmpv::Value;

    fn sample(kind: FrameType, seq: u64) -> Envelope {
        Envelope::new(kind, seq, Value::Map(vec![]))
            .with_sid("00000000-0000-4000-8000-000000000001")
            .with_ch(0)
    }

    // REQ: FR-WIRE-001
    #[test]
    fn round_trip_all_frame_types() {
        for (i, k) in [
            FrameType::Hello,
            FrameType::Auth,
            FrameType::Sub,
            FrameType::Unsub,
            FrameType::Data,
            FrameType::Ctl,
            FrameType::Write,
            FrameType::Metrics,
            FrameType::Clientlog,
            FrameType::Ping,
            FrameType::Pong,
            FrameType::ClockSync,
            FrameType::PanelPriority,
            FrameType::Child,
        ]
        .into_iter()
        .enumerate()
        {
            let env = sample(k, i as u64);
            let bytes = encode(&env).expect("encode");
            let back = decode(&bytes).expect("decode");
            assert_eq!(env, back, "round-trip mismatch for {k}");
        }
    }

    #[test]
    fn frame_type_token_is_stable() {
        // Tokens are part of the frozen v0.1 wire schema. If any of
        // these change, bump the subprotocol token and add a compat
        // fixture.
        assert_eq!(FrameType::Hello.as_str(), "hello");
        assert_eq!(FrameType::Data.as_str(), "data");
        assert_eq!(FrameType::ClockSync.as_str(), "clock_sync");
        assert_eq!(FrameType::PanelPriority.as_str(), "panel_priority");
    }

    #[test]
    fn omits_optional_fields_when_absent() {
        let env = Envelope::new(FrameType::Ping, 7, Value::Nil);
        let bytes = encode(&env).expect("encode");
        let back = decode(&bytes).expect("decode");
        assert!(back.sid.is_none());
        assert!(back.ch.is_none());
        assert_eq!(back.seq, 7);
        assert_eq!(back.kind, FrameType::Ping);
    }

    // REQ: FR-WIRE-001 (E-2002 enforcement on decode)
    #[test]
    fn decode_rejects_oversize_input() {
        let big = vec![0u8; MAX_FRAME_BYTES + 1];
        let err = decode(&big).expect_err("must reject");
        assert!(matches!(err, WireError::TooLarge { .. }));
        assert_eq!(err.id(), ErrorId::E2002WireLimit);
    }

    #[test]
    fn decode_rejects_garbage() {
        let err = decode(&[0xff, 0xff, 0xff]).expect_err("must reject");
        assert!(matches!(err, WireError::Malformed(_)));
        assert_eq!(err.id(), ErrorId::E2001WireMalformed);
    }

    #[test]
    fn encode_rejects_oversize_payload() {
        // Build a payload that, once MessagePack-encoded, exceeds the
        // limit. A binary blob slightly larger than the cap is enough.
        let blob = vec![0u8; MAX_FRAME_BYTES + 16];
        let env = Envelope::new(FrameType::Data, 1, Value::Binary(blob));
        let err = encode(&env).expect_err("must reject");
        assert!(matches!(err, WireError::TooLarge { .. }));
    }

    #[test]
    fn wanlogger_error_carries_canonical_id() {
        let we: WanloggerError = WireError::Malformed("x".into()).into();
        assert_eq!(we.id, ErrorId::E2001WireMalformed);
        let we: WanloggerError = WireError::TooLarge { size: 1, limit: 1 }.into();
        assert_eq!(we.id, ErrorId::E2002WireLimit);
    }

    #[test]
    fn unknown_frame_type_is_malformed() {
        // Construct a hand-crafted envelope with an unknown `type`.
        let raw = Value::Map(vec![
            (Value::String("type".into()), Value::String("nope".into())),
            (Value::String("seq".into()), Value::from(0u64)),
            (Value::String("payload".into()), Value::Nil),
        ]);
        let bytes = rmp_serde::to_vec_named(&raw).unwrap();
        let err = decode(&bytes).expect_err("must reject");
        assert!(matches!(err, WireError::Malformed(_)));
    }
}
