//! Tera Term `.log` importer.
//!
//! Recognises a leading `[YYYY-MM-DD HH:MM:SS(.fff)?] ` prefix and
//! uses it for `ts_origin`. Lines without a recognised prefix are
//! imported as-is with `ts_origin == ts_ingest == now`.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use async_trait::async_trait;
use uuid::Uuid;

use crate::error_id::{ErrorId, TraceMuxError};
use crate::importer::Importer;
use crate::log::index::{Dir, IndexEntry, IndexWriter, Kind};
use crate::log::lines::{LineEntry, LinesWriter};
use crate::log::raw::RawWriter;
use crate::time::{unix_ns_now, ClockQuality, ClockSource, DualTimestamp};
use crate::Result;

/// Tera Term log importer.
#[derive(Debug, Default)]
pub struct TeraTermImporter;

#[async_trait]
impl Importer for TeraTermImporter {
    fn kind(&self) -> &'static str {
        "teraterm"
    }

    async fn import(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst)
    }
}

fn run(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| err("creating dst", e))?;
    let f = File::open(src).map_err(|e| err("opening src", e))?;
    let rd = BufReader::new(f);
    let mut raw = RawWriter::create(dst).map_err(|e| err("opening raw.bin", e))?;
    let mut idx = IndexWriter::create(dst).map_err(|e| err("opening index.jsonl", e))?;
    let mut lines_writer = LinesWriter::create(dst).map_err(|e| err("opening lines.jsonl", e))?;
    let sid = Uuid::new_v4();
    for line in rd.lines() {
        let line = line.map_err(|e| err("reading line", e))?;
        let (ts_ns, body) = parse_prefix(&line).unwrap_or((None, line.as_str()));
        let bytes = body.as_bytes();
        let (off, len) = raw.append(bytes).map_err(|e| err("raw append", e))?;
        let mut ts = imported_ts();
        if let Some(ns) = ts_ns {
            ts.ts_origin_ns = ns;
        }
        let entry = IndexEntry::from_envelope(&ts, sid, Dir::In, Kind::Bytes, off, len);
        idx.append(&entry).map_err(|e| err("index append", e))?;
        let line_entry = LineEntry {
            ts: crate::log::index::format_rfc3339_ns(ts.ts_origin_ns),
            level: None,
            text: body.to_string(),
            correlation_id: None,
            tags: Vec::new(),
        };
        lines_writer
            .append(&line_entry)
            .map_err(|e| err("lines append", e))?;
    }
    raw.flush().map_err(|e| err("flush raw", e))?;
    idx.flush().map_err(|e| err("flush index", e))?;
    lines_writer.flush().map_err(|e| err("flush lines", e))?;
    Ok(())
}

/// Parse a Tera Term timestamp prefix.
fn parse_prefix(line: &str) -> Option<(Option<i64>, &str)> {
    let stripped = line.strip_prefix('[')?;
    let close = stripped.find(']')?;
    let inside = &stripped[..close];
    let rest = stripped[close + 1..].trim_start_matches(' ');
    let ns = parse_ts(inside);
    Some((ns, rest))
}

fn parse_ts(inside: &str) -> Option<i64> {
    use time::format_description::FormatItem;
    use time::macros::format_description;
    use time::PrimitiveDateTime;

    const ISO_MS: &[FormatItem<'static>] =
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");
    const ISO: &[FormatItem<'static>] =
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

    if let Ok(dt) = PrimitiveDateTime::parse(inside, &ISO_MS) {
        return Some(dt.assume_utc().unix_timestamp_nanos() as i64);
    }
    if let Ok(dt) = PrimitiveDateTime::parse(inside, &ISO) {
        return Some(dt.assume_utc().unix_timestamp_nanos() as i64);
    }
    None
}

fn imported_ts() -> DualTimestamp {
    let now = unix_ns_now();
    DualTimestamp {
        ts_origin_ns: now,
        ts_ingest_ns: now,
        mono_ns: 0,
        boot_id: Uuid::nil(),
        node_id: Uuid::nil(),
        clock_offset_ms: 0,
        clock_quality: ClockQuality::Imported,
        drift_ppm: 0.0,
        clock_source: ClockSource::Imported,
    }
}

fn err(ctx: &str, e: std::io::Error) -> TraceMuxError {
    TraceMuxError::new(
        ErrorId::E1001PipelineGeneric,
        format!("teraterm-import: {ctx}"),
    )
    .with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso_ms_prefix() {
        let (ns, body) = parse_prefix("[2024-01-01 00:00:00.123] hello").unwrap();
        assert!(ns.is_some());
        assert_eq!(body, "hello");
    }

    #[test]
    fn parse_unknown_prefix_returns_none_ns() {
        let (ns, body) = parse_prefix("[whatever] body").unwrap();
        assert!(ns.is_none());
        assert_eq!(body, "body");
    }

    #[test]
    fn no_prefix_returns_none() {
        assert!(parse_prefix("plain line").is_none());
    }

    #[tokio::test]
    async fn imports_two_lines() {
        let dir = std::env::temp_dir().join(format!("wlg-tt-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("in.log");
        std::fs::write(&src, "[2024-01-01 00:00:00.000] one\nplain\n").unwrap();
        let dst = dir.join("session");
        TeraTermImporter.import(&src, &dst).await.unwrap();
        let lines = std::fs::read_to_string(dst.join("lines.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 2);
    }
}
