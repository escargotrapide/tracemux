//! `Decoder` trait — turns frames into structured records.
//! **Frozen v0.1.** See `.github/skills/add-decoder/SKILL.md`.

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::Result;

/// Decoded record.
///
/// **Frozen v0.1.** Field names are part of the on-disk schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    /// Optional schema id (referenced from `session-dir/schemas/<id>.json`).
    pub schema_id: Option<String>,
    /// Severity level.
    pub level: Option<Level>,
    /// Optional decoded text body.
    pub text: Option<String>,
    /// Optional structured fields.
    pub fields: serde_json::Value,
    /// Free-form tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Correlation id.
    pub correlation_id: Option<String>,
}

/// Severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// trace
    Trace,
    /// debug
    Debug,
    /// info
    Info,
    /// warn
    Warn,
    /// error
    Error,
    /// fatal
    Fatal,
}

/// Decoder of framed bytes into [`Record`]s.
pub trait Decoder: Send + 'static {
    /// Decode one frame. May return `Ok(None)` if the frame is metadata
    /// (no record produced) or `Err` for a hard schema mismatch.
    fn decode(&mut self, frame: Bytes) -> Result<Option<Record>>;

    /// Stable kind string (e.g. `"json-lines"`).
    fn kind(&self) -> &'static str;
}

pub mod json_lines;
pub mod nmea;
pub mod passthrough;
pub mod utf8_text;
