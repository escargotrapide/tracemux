//! CSV exporter.
//!
//! Emits a header row and `ts_origin,ts_ingest,dir,kind,len,text`
//! records, where `text` is the lossy-UTF-8 decoding of the raw
//! payload, double-quoted and with embedded `"` doubled per RFC 4180.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use async_trait::async_trait;

use crate::error_id::{ErrorId, WanloggerError};
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
        run(src, dst)
    }
}

fn run(src: &Path, dst: &Path) -> Result<()> {
    let idx = File::open(src.join("index.jsonl")).map_err(|e| err("opening index.jsonl", e))?;
    let mut raw = RawReader::open(src).map_err(|e| err("opening raw.bin", e))?;
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
        let text = String::from_utf8_lossy(&bytes);
        writeln!(
            w,
            "{},{},{},{},{},{}",
            entry.ts_origin,
            entry.ts_ingest,
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

fn err(ctx: &str, e: std::io::Error) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, format!("csv-export: {ctx}")).with_source(e)
}

fn serde_err(ctx: &str, e: serde_json::Error) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, format!("csv-export: {ctx}")).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::text::TextImporter;
    use crate::importer::Importer;
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
}
