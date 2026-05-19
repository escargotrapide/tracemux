//! JSON-lines exporter.
//!
//! Emits one JSON object per record combining the
//! [`IndexEntry`](crate::log::index::IndexEntry) fields with a
//! `text` field carrying the lossy-UTF-8 decoding of the raw
//! payload. Suitable for `jq`-based post-processing.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use async_trait::async_trait;
use serde_json::{json, Map, Value};
use time::UtcOffset;

use crate::error_id::{ErrorId, WanloggerError};
use crate::exporter::timestamp::{format_rfc3339_in_timezone, parse_timezone_offset};
use crate::exporter::Exporter;
use crate::log::index::IndexEntry;
use crate::log::raw::RawReader;
use crate::Result;

/// JSON-lines exporter.
#[derive(Debug, Default)]
pub struct JsonlExporter;

#[async_trait]
impl Exporter for JsonlExporter {
    fn kind(&self) -> &'static str {
        "jsonl"
    }

    async fn export(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst, None)
    }
}

/// Export JSONL while formatting timestamp fields in a fixed timezone.
pub fn export_with_timezone(src: &Path, dst: &Path, timezone: Option<&str>) -> Result<()> {
    let offset = timezone.map(parse_timezone_offset).transpose()?;
    run(src, dst, offset)
}

fn run(src: &Path, dst: &Path, timezone: Option<UtcOffset>) -> Result<()> {
    let idx = File::open(src.join("index.jsonl")).map_err(|e| err("opening index.jsonl", e))?;
    let mut raw = RawReader::open(src).map_err(|e| err("opening raw.bin", e))?;
    let out = File::create(dst).map_err(|e| err("creating dst", e))?;
    let mut w = BufWriter::new(out);

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
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut obj: Map<String, Value> = serde_json::to_value(&entry)
            .map_err(|e| serde_err("encoding entry", e))?
            .as_object()
            .cloned()
            .unwrap_or_default();
        obj.insert(
            "ts_origin".to_string(),
            json!(format_rfc3339_in_timezone(&entry.ts_origin, timezone)?),
        );
        obj.insert(
            "ts_ingest".to_string(),
            json!(format_rfc3339_in_timezone(&entry.ts_ingest, timezone)?),
        );
        obj.insert("text".to_string(), json!(text));
        let v = Value::Object(obj);
        writeln!(w, "{v}").map_err(|e| err("writing dst", e))?;
    }
    w.flush().map_err(|e| err("flush dst", e))?;
    Ok(())
}

fn err(ctx: &str, e: std::io::Error) -> WanloggerError {
    WanloggerError::new(
        ErrorId::E1001PipelineGeneric,
        format!("jsonl-export: {ctx}"),
    )
    .with_source(e)
}

fn serde_err(ctx: &str, e: serde_json::Error) -> WanloggerError {
    WanloggerError::new(
        ErrorId::E1001PipelineGeneric,
        format!("jsonl-export: {ctx}"),
    )
    .with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::text::TextImporter;
    use crate::importer::Importer;
    use uuid::Uuid;

    #[tokio::test]
    async fn round_trip() {
        let dir = std::env::temp_dir().join(format!("wlg-export-jsonl-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src_txt = dir.join("in.txt");
        std::fs::write(&src_txt, "alpha\nbeta\n").unwrap();
        let session = dir.join("session");
        TextImporter.import(&src_txt, &session).await.unwrap();
        let dst = dir.join("out.jsonl");
        JsonlExporter.export(&session, &dst).await.unwrap();
        let body = std::fs::read_to_string(&dst).unwrap();
        let lines: Vec<_> = body.lines().collect();
        assert_eq!(lines.len(), 2);
        let v0: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v0["text"], "alpha");
        assert_eq!(v0["kind"], "bytes");
    }
}
