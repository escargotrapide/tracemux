//! CSV exporter.
//!
//! Emits a header row and `ts_origin,ts_ingest,dir,kind,len,text`
//! records, where `text` is decoded with the session encoding when
//! available, double-quoted and with embedded `"` doubled per RFC 4180.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use async_trait::async_trait;
use time::UtcOffset;

use crate::error_id::{ErrorId, TraceMuxError};
use crate::exporter::encoding::resolve_text_encoding;
use crate::exporter::timestamp::{format_rfc3339_in_timezone, parse_timezone_offset};
use crate::exporter::Exporter;
use crate::log::index::IndexEntry;
use crate::log::raw::RawReader;
use crate::Result;

/// CSV exporter.
#[derive(Debug, Default)]
pub struct CsvExporter;

#[async_trait]
impl Exporter for CsvExporter {
    fn kind(&self) -> &'static str {
        "csv"
    }

    async fn export(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst, None, None)
    }
}

/// Export CSV while formatting timestamps in a fixed timezone.
pub fn export_with_timezone(src: &Path, dst: &Path, timezone: Option<&str>) -> Result<()> {
    export_with_timezone_and_encoding(src, dst, timezone, None)
}

/// Export CSV with an optional fixed timezone and text encoding override.
pub fn export_with_timezone_and_encoding(
    src: &Path,
    dst: &Path,
    timezone: Option<&str>,
    encoding: Option<&str>,
) -> Result<()> {
    let offset = timezone.map(parse_timezone_offset).transpose()?;
    run(src, dst, offset, encoding)
}

fn run(src: &Path, dst: &Path, timezone: Option<UtcOffset>, encoding: Option<&str>) -> Result<()> {
    let idx = File::open(src.join("index.jsonl")).map_err(|e| err("opening index.jsonl", e))?;
    let mut raw = RawReader::open(src).map_err(|e| err("opening raw.bin", e))?;
    let encoding = resolve_text_encoding(src, encoding);
    let out = File::create(dst).map_err(|e| err("creating dst", e))?;
    let mut w = BufWriter::new(out);
    writeln!(w, "ts_origin,ts_ingest,dir,kind,len,text").map_err(|e| err("write header", e))?;

    for line in BufReader::new(idx).lines() {
        let line = line.map_err(|e| err("reading index line", e))?;
        if line.is_empty() {
            continue;
        }
        let entry: IndexEntry =
            serde_json::from_str(&line).map_err(|e| serde_err("parsing index entry", e))?;
        let bytes = raw
            .read_at(entry.off, entry.len)
            .map_err(|e| err("reading raw", e))?;
        let (text, _) = crate::codec::decode(&bytes, &encoding);
        let ts_origin = format_rfc3339_in_timezone(&entry.ts_origin, timezone)?;
        let ts_ingest = format_rfc3339_in_timezone(&entry.ts_ingest, timezone)?;
        writeln!(
            w,
            "{},{},{},{},{},{}",
            ts_origin,
            ts_ingest,
            kind_str(&entry, "dir"),
            kind_str(&entry, "kind"),
            entry.len,
            quote(&text)
        )
        .map_err(|e| err("writing dst", e))?;
    }
    w.flush().map_err(|e| err("flush dst", e))?;
    Ok(())
}

fn kind_str(e: &IndexEntry, which: &str) -> String {
    match which {
        "dir" => match e.dir {
            crate::log::index::Dir::In => "in",
            crate::log::index::Dir::Out => "out",
        }
        .to_string(),
        "kind" => match e.kind {
            crate::log::index::Kind::Bytes => "bytes",
            crate::log::index::Kind::Datagram => "datagram",
            crate::log::index::Kind::Frame => "frame",
            crate::log::index::Kind::Record => "record",
        }
        .to_string(),
        _ => String::new(),
    }
}

fn quote(s: &str) -> String {
    let needs = s.contains([',', '"', '\n', '\r']);
    if !needs {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn err(ctx: &str, e: std::io::Error) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1001PipelineGeneric, format!("csv-export: {ctx}")).with_source(e)
}

fn serde_err(ctx: &str, e: serde_json::Error) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1001PipelineGeneric, format!("csv-export: {ctx}")).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::text::TextImporter;
    use crate::importer::Importer;
    use crate::log::index::{Dir, IndexEntry, IndexWriter, Kind};
    use crate::log::raw::RawWriter;
    use crate::time::{ClockQuality, ClockSource, DualTimestamp};
    use uuid::Uuid;

    #[tokio::test]
    async fn round_trip() {
        let dir = std::env::temp_dir().join(format!("wlg-export-csv-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src_txt = dir.join("in.txt");
        std::fs::write(&src_txt, "hi,there\nplain\n").unwrap();
        let session = dir.join("session");
        TextImporter.import(&src_txt, &session).await.unwrap();
        let dst = dir.join("out.csv");
        CsvExporter.export(&session, &dst).await.unwrap();
        let body = std::fs::read_to_string(&dst).unwrap();
        let mut lines = body.lines();
        assert_eq!(
            lines.next().unwrap(),
            "ts_origin,ts_ingest,dir,kind,len,text"
        );
        let row1 = lines.next().unwrap();
        assert!(row1.ends_with(r#","hi,there""#));
        let row2 = lines.next().unwrap();
        assert!(row2.ends_with(",plain"));
    }

    #[test]
    fn quote_escapes_dquote() {
        assert_eq!(quote(r#"a"b"#), r#""a""b""#);
        assert_eq!(quote("plain"), "plain");
    }

    // REQ: FR-EXP-001
    #[test]
    fn uses_session_meta_encoding() {
        let dir = std::env::temp_dir().join(format!("wlg-export-csv-sjis-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_shift_jis_session(&dir);
        let dst = dir.join("out.csv");

        export_with_timezone(&dir, &dst, None).unwrap();

        let body = std::fs::read_to_string(&dst).unwrap();
        assert!(body.contains("あ"), "export body was {body}");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn write_shift_jis_session(dir: &Path) {
        std::fs::write(
            dir.join("meta.toml"),
            "log_format_version = \"1.0.0\"\ndecoder = \"utf8-text:shift_jis\"\n",
        )
        .unwrap();
        let mut raw = RawWriter::create(dir).unwrap();
        let (off, len) = raw.append(&[0x82, 0xA0]).unwrap();
        raw.flush().unwrap();

        let mut index = IndexWriter::create(dir).unwrap();
        index
            .append(&IndexEntry::from_envelope(
                &sample_ts(),
                Uuid::new_v4(),
                Dir::In,
                Kind::Bytes,
                off,
                len,
            ))
            .unwrap();
        index.flush().unwrap();
    }

    fn sample_ts() -> DualTimestamp {
        DualTimestamp {
            ts_origin_ns: 1_700_000_000_000_000_000,
            ts_ingest_ns: 1_700_000_000_000_000_000,
            mono_ns: 0,
            boot_id: Uuid::nil(),
            node_id: Uuid::nil(),
            clock_offset_ms: 0,
            clock_quality: ClockQuality::Imported,
            drift_ppm: 0.0,
            clock_source: ClockSource::Imported,
        }
    }
}
