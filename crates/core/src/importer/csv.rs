//! Minimal CSV importer.
//!
//! Recognises an optional header row containing any subset of
//! `ts`, `level`, `text`. If there is no `text` column the entire
//! row is treated as the payload. The CSV dialect is a strict subset
//! of RFC 4180:
//! * comma separator, optional `"…"` quoting (with `""` escape);
//! * `\r\n` or `\n` line endings;
//! * no embedded newlines inside quoted fields.
//!
//! For each row this importer emits one [`crate::log::index::IndexEntry`]
//! (`Kind::Bytes`, `Dir::In`, `ClockSource::Imported`,
//! `ClockQuality::Imported`) plus the raw payload in `raw.bin`. If a
//! `text` column is present, a [`crate::log::lines::LineEntry`] is
//! also written to `lines.jsonl`.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use async_trait::async_trait;
use uuid::Uuid;

use crate::error_id::{ErrorId, WanloggerError};
use crate::importer::Importer;
use crate::log::index::{Dir, IndexEntry, IndexWriter, Kind};
use crate::log::lines::{LineEntry, LinesWriter};
use crate::log::raw::RawWriter;
use crate::time::{unix_ns_now, ClockQuality, ClockSource, DualTimestamp};
use crate::Result;

/// CSV importer.
#[derive(Debug, Default)]
pub struct CsvImporter;

#[async_trait]
impl Importer for CsvImporter {
    fn kind(&self) -> &'static str {
        "csv"
    }

    async fn import(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst)
    }
}

fn run(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| err("creating dst", e))?;
    let f = File::open(src).map_err(|e| err("opening src", e))?;
    let mut rd = BufReader::new(f);
    let mut raw = RawWriter::create(dst).map_err(|e| err("opening raw.bin", e))?;
    let mut idx = IndexWriter::create(dst).map_err(|e| err("opening index.jsonl", e))?;
    let mut lines_writer = LinesWriter::create(dst).map_err(|e| err("opening lines.jsonl", e))?;

    let sid = Uuid::new_v4();
    let mut buf = String::new();
    let n = rd.read_line(&mut buf).map_err(|e| err("reading header", e))?;
    if n == 0 {
        return Ok(());
    }
    let header = parse_row(buf.trim_end_matches(['\r', '\n']));
    let (col_ts, col_level, col_text) = locate_cols(&header);
    let has_header = col_ts.is_some() || col_level.is_some() || col_text.is_some();
    if !has_header {
        emit_row(&header, None, None, None, sid, &mut raw, &mut idx, &mut lines_writer)?;
    }

    loop {
        buf.clear();
        let n = rd.read_line(&mut buf).map_err(|e| err("reading row", e))?;
        if n == 0 {
            break;
        }
        let trimmed = buf.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        let row = parse_row(trimmed);
        emit_row(
            &row, col_ts, col_level, col_text, sid, &mut raw, &mut idx, &mut lines_writer,
        )?;
    }

    raw.flush().map_err(|e| err("flush raw", e))?;
    idx.flush().map_err(|e| err("flush index", e))?;
    lines_writer.flush().map_err(|e| err("flush lines", e))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_row(
    row: &[String],
    col_ts: Option<usize>,
    col_level: Option<usize>,
    col_text: Option<usize>,
    sid: Uuid,
    raw: &mut RawWriter,
    idx: &mut IndexWriter,
    lines_writer: &mut LinesWriter,
) -> Result<()> {
    let payload = if let Some(i) = col_text {
        row.get(i).cloned().unwrap_or_default()
    } else {
        row.join(",")
    };
    let bytes = payload.as_bytes();
    let (off, len) = raw.append(bytes).map_err(|e| err("raw append", e))?;
    let mut ts = imported_ts();
    if let Some(i) = col_ts {
        if let Some(s) = row.get(i) {
            if let Some(ns) = parse_rfc3339_ns(s) {
                ts.ts_origin_ns = ns;
            }
        }
    }
    let mut entry = IndexEntry::from_envelope(&ts, sid, Dir::In, Kind::Bytes, off, len);
    if let Some(i) = col_level {
        if let Some(l) = row.get(i) {
            if !l.is_empty() {
                entry.level = Some(l.clone());
            }
        }
    }
    idx.append(&entry).map_err(|e| err("index append", e))?;
    if col_text.is_some() {
        let line = LineEntry {
            ts: crate::log::index::format_rfc3339_ns(ts.ts_origin_ns),
            level: entry.level.clone(),
            text: payload,
            correlation_id: None,
            tags: Vec::new(),
        };
        lines_writer.append(&line).map_err(|e| err("lines append", e))?;
    }
    Ok(())
}

fn locate_cols(header: &[String]) -> (Option<usize>, Option<usize>, Option<usize>) {
    let mut ts = None;
    let mut level = None;
    let mut text = None;
    for (i, h) in header.iter().enumerate() {
        match h.to_ascii_lowercase().as_str() {
            "ts" | "time" | "timestamp" => ts = Some(i),
            "level" | "severity" => level = Some(i),
            "text" | "message" | "msg" | "body" => text = Some(i),
            _ => {}
        }
    }
    (ts, level, text)
}

/// Minimal RFC4180-ish row parser.
fn parse_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if !in_quoted && cur.is_empty() => in_quoted = true,
            '"' if in_quoted => {
                if matches!(chars.peek(), Some('"')) {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quoted = false;
                }
            }
            ',' if !in_quoted => {
                out.push(std::mem::take(&mut cur));
            }
            other => cur.push(other),
        }
    }
    out.push(cur);
    out
}

fn parse_rfc3339_ns(s: &str) -> Option<i64> {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::parse(s, &Rfc3339)
        .ok()
        .map(|dt| dt.unix_timestamp_nanos() as i64)
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

fn err(ctx: &str, e: std::io::Error) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, format!("csv-import: {ctx}"))
        .with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("wanlogger-import-csv-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn parse_row_basic() {
        let r = parse_row(r#"a,b,"c,d","e""f""#);
        assert_eq!(r, vec!["a", "b", "c,d", r#"e"f"#]);
    }

    #[tokio::test]
    async fn imports_with_header() {
        let dir = tempdir();
        let src = dir.join("in.csv");
        std::fs::write(
            &src,
            "ts,level,text\n2024-01-01T00:00:00Z,info,hello\n2024-01-01T00:00:01Z,warn,\"a,b\"\n",
        )
        .unwrap();
        let dst = dir.join("session");
        CsvImporter.import(&src, &dst).await.unwrap();
        let lines = std::fs::read_to_string(dst.join("lines.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 2);
        let raw = std::fs::read(dst.join("raw.bin")).unwrap();
        assert_eq!(&raw[..], b"helloa,b");
    }

    #[tokio::test]
    async fn imports_without_header() {
        let dir = tempdir();
        let src = dir.join("in.csv");
        std::fs::write(&src, "x,y\nu,v\n").unwrap();
        let dst = dir.join("session");
        CsvImporter.import(&src, &dst).await.unwrap();
        let raw = std::fs::read(dst.join("raw.bin")).unwrap();
        assert_eq!(&raw[..], b"x,yu,v");
    }
}
