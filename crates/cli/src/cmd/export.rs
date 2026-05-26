//! `wanlogger export` -- render a session-dir into a foreign format.
//!
//! Dispatches `kind` to the matching implementation in
//! [`wanlogger_core::exporter`]. v0.1 text-like kinds plus the packet
//! capture `pcapng` exporter are wired through.

use std::path::Path;

use anyhow::{bail, Result};
use wanlogger_core::exporter::{csv, jsonl, pcapng, text};

/// Stable list of exporter kinds known to v0.1.
pub const KINDS: &[&str] = &["csv", "text", "jsonl", "pcapng"];

/// Run the `export` subcommand.
///
/// # Errors
/// Returns an error when `kind` is unknown, when `src` is not a
/// session-dir, or when the underlying exporter fails.
pub fn run(
    kind: &str,
    src: &Path,
    dst: &Path,
    timezone: Option<&str>,
    encoding: Option<&str>,
) -> Result<()> {
    if !KINDS.contains(&kind) {
        bail!(
            "unknown exporter kind `{kind}`; known: {}",
            KINDS.join(", ")
        );
    }
    if !src.is_dir() {
        bail!("source must be a session-dir: {}", src.display());
    }
    if !src.join("index.jsonl").is_file() {
        bail!(
            "source does not look like a session-dir (missing index.jsonl): {}",
            src.display()
        );
    }

    match kind {
        "text" => text::export_with_timezone_and_encoding(src, dst, timezone, encoding)?,
        "csv" => csv::export_with_timezone_and_encoding(src, dst, timezone, encoding)?,
        "jsonl" => jsonl::export_with_timezone_and_encoding(src, dst, timezone, encoding)?,
        "pcapng" => pcapng::export_with_timezone(src, dst, timezone)?,
        _ => unreachable!("kind already validated"),
    }
    tracing::info!(kind, src = %src.display(), dst = %dst.display(), "export: ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use wanlogger_core::decoder::Record;
    use wanlogger_core::exporter::pcapng::PCAP_PACKET_SCHEMA_ID;
    use wanlogger_core::log::frames::{FrameEntry, FramesWriter};
    use wanlogger_core::log::index::{Dir, IndexEntry, IndexWriter, Kind};
    use wanlogger_core::log::raw::RawWriter;
    use wanlogger_core::time::{ClockQuality, ClockSource, DualTimestamp};

    // REQ: FR-EXP-001
    #[test]
    fn unknown_kind_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = run("nope", dir.path(), &dir.path().join("out"), None, None).unwrap_err();
        assert!(err.to_string().contains("unknown exporter kind"));
    }

    // REQ: FR-EXP-001
    #[test]
    fn rejects_non_session_dir() {
        let dir = tempfile::tempdir().unwrap();
        // empty dir -- no index.jsonl
        let err = run("text", dir.path(), &dir.path().join("out.txt"), None, None).unwrap_err();
        assert!(err.to_string().contains("missing index.jsonl"));
    }

    #[test]
    fn pcapng_kind_is_wired() {
        assert!(KINDS.contains(&"pcapng"));
    }

    #[test]
    fn exports_pcapng_session() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session");
        write_synthetic_pcap_session(&session);
        let dst = dir.path().join("out.pcapng");

        run("pcapng", &session, &dst, Some("GMT+9"), Some("shift_jis")).unwrap();

        let body = std::fs::read(&dst).unwrap();
        assert!(body.starts_with(&0x0A0D_0D0Au32.to_le_bytes()));
    }

    fn write_synthetic_pcap_session(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let packet = ethernet_packet();
        let mut raw = RawWriter::create(dir).unwrap();
        let (off, len) = raw.append(&packet).unwrap();
        raw.flush().unwrap();

        let sid = Uuid::new_v4();
        let ts = sample_ts();
        let mut entry = IndexEntry::from_envelope(&ts, sid, Dir::In, Kind::Datagram, off, len);
        entry.source = Some("pcap:eth0".to_string());
        entry.schema_id = Some(PCAP_PACKET_SCHEMA_ID.to_string());
        let mut index = IndexWriter::create(dir).unwrap();
        index.append(&entry).unwrap();
        index.flush().unwrap();

        let mut frames = FramesWriter::create(dir).unwrap();
        frames
            .append(&FrameEntry {
                ts: entry.ts_ingest,
                decoder: "pcap".to_string(),
                record: Record {
                    schema_id: Some(PCAP_PACKET_SCHEMA_ID.to_string()),
                    level: None,
                    text: None,
                    fields: serde_json::json!({
                        "raw_off": off,
                        "raw_len": len,
                        "captured_len": len,
                        "original_len": len,
                        "linktype": 1,
                        "interface_id": 0,
                        "interface": "eth0"
                    }),
                    tags: vec!["pcap".to_string()],
                    correlation_id: None,
                },
            })
            .unwrap();
        frames.flush().unwrap();
    }

    fn sample_ts() -> DualTimestamp {
        DualTimestamp {
            ts_origin_ns: 1_700_000_000_123_456_789,
            ts_ingest_ns: 1_700_000_000_223_456_789,
            mono_ns: 42,
            boot_id: Uuid::nil(),
            node_id: Uuid::nil(),
            clock_offset_ms: 0,
            clock_quality: ClockQuality::BestEffort,
            drift_ppm: 0.0,
            clock_source: ClockSource::Imported,
        }
    }

    fn ethernet_packet() -> Vec<u8> {
        vec![
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00,
            0x45, 0x00, 0x00, 0x14,
        ]
    }
}
