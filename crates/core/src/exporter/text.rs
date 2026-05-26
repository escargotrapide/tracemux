//! Plain-text exporter.
//!
//! Reads `src/index.jsonl` + `src/raw.bin` and writes one line per
//! record as `{ts_origin}\t{text}\n` where `text` is decoded with the
//! session encoding when available.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use async_trait::async_trait;
use time::UtcOffset;

use crate::error_id::{ErrorId, WanloggerError};
use crate::exporter::encoding::resolve_text_encoding;
use crate::exporter::timestamp::{format_rfc3339_in_timezone, parse_timezone_offset};
use crate::exporter::Exporter;
use crate::log::index::IndexEntry;
use crate::log::raw::RawReader;
use crate::Result;

/// Plain-text exporter.
#[derive(Debug, Default)]
pub struct TextExporter;

#[async_trait]
impl Exporter for TextExporter {
    fn kind(&self) -> &'static str {
        "text"
    }

    async fn export(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst, None, None)
    }
}

/// Export text while formatting timestamps in a fixed timezone.
pub fn export_with_timezone(src: &Path, dst: &Path, timezone: Option<&str>) -> Result<()> {
    export_with_timezone_and_encoding(src, dst, timezone, None)
}

/// Export text with an optional fixed timezone and text encoding override.
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
        writeln!(w, "{ts_origin}\t{text}").map_err(|e| err("writing dst", e))?;
    }
    w.flush().map_err(|e| err("flush dst", e))?;
    Ok(())
}

fn err(ctx: &str, e: std::io::Error) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, format!("text-export: {ctx}")).with_source(e)
}

fn serde_err(ctx: &str, e: serde_json::Error) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, format!("text-export: {ctx}")).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::text::TextImporter;
    use crate::importer::Importer;
    use uuid::Uuid;

    #[tokio::test]
    async fn round_trip_lines() {
        let dir = std::env::temp_dir().join(format!("wlg-export-text-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src_txt = dir.join("in.txt");
        std::fs::write(&src_txt, b"alpha\nbeta\n").unwrap();
        let session = dir.join("session");
        TextImporter.import(&src_txt, &session).await.unwrap();
        let dst = dir.join("out.txt");
        TextExporter.export(&session, &dst).await.unwrap();
        let body = std::fs::read_to_string(&dst).unwrap();
        assert!(body.contains("alpha"));
        assert!(body.contains("beta"));
        assert_eq!(body.lines().count(), 2);
    }
}
