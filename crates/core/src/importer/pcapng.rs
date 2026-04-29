//! Minimal pcapng importer (native-endian, EPB only).
//!
//! Walks pcapng blocks (Section Header + Enhanced Packet Block) and
//! writes each EPB packet payload to `raw.bin` as one
//! [`crate::log::index::IndexEntry`] with `Kind::Datagram`,
//! `Dir::In`, `ClockSource::Imported`, `ClockQuality::Imported`.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use async_trait::async_trait;
use uuid::Uuid;

use crate::error_id::{ErrorId, WanloggerError};
use crate::importer::Importer;
use crate::log::index::{Dir, IndexEntry, IndexWriter, Kind};
use crate::log::raw::RawWriter;
use crate::time::{ClockQuality, ClockSource, DualTimestamp};
use crate::Result;

const BLOCK_SHB: u32 = 0x0A0D_0D0A;
const BLOCK_EPB: u32 = 0x0000_0006;
const SHB_MAGIC: u32 = 0x1A2B_3C4D;

/// pcapng importer (native-endian, EPB only).
#[derive(Debug, Default)]
pub struct PcapngImporter;

#[async_trait]
impl Importer for PcapngImporter {
    fn kind(&self) -> &'static str {
        "pcapng"
    }

    async fn import(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst)
    }
}

fn run(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| err("creating dst", e))?;
    let mut rd = BufReader::new(File::open(src).map_err(|e| err("opening src", e))?);
    let mut raw = RawWriter::create(dst).map_err(|e| err("opening raw.bin", e))?;
    let mut idx = IndexWriter::create(dst).map_err(|e| err("opening index.jsonl", e))?;
    let sid = Uuid::new_v4();

    let mut header = [0u8; 8];
    loop {
        match rd.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(err("reading block header", e)),
        }
        let block_type = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let total_len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        if total_len < 12 {
            return Err(simple("pcapng block total length < 12"));
        }
        let body_len = total_len - 12;
        let mut body = vec![0u8; body_len];
        rd.read_exact(&mut body)
            .map_err(|e| err("reading body", e))?;
        let mut tail = [0u8; 4];
        rd.read_exact(&mut tail)
            .map_err(|e| err("reading tail", e))?;

        match block_type {
            BLOCK_SHB => {
                if body_len < 4 {
                    return Err(simple("SHB body too short"));
                }
                let magic = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                if magic != SHB_MAGIC {
                    return Err(simple("pcapng byte-order swap not supported in v0.1"));
                }
            }
            BLOCK_EPB => {
                if body_len < 20 {
                    continue;
                }
                let ts_high = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                let ts_low = u32::from_le_bytes([body[8], body[9], body[10], body[11]]);
                let cap_len = u32::from_le_bytes([body[12], body[13], body[14], body[15]]) as usize;
                let pkt_off = 20;
                if pkt_off + cap_len > body_len {
                    return Err(simple("EPB capture length exceeds block body"));
                }
                let pkt = &body[pkt_off..pkt_off + cap_len];
                let (off, len) = raw.append(pkt).map_err(|e| err("raw append", e))?;
                let ts_us = (u64::from(ts_high) << 32) | u64::from(ts_low);
                let ts_ns = (ts_us as i64).saturating_mul(1_000);
                let ts = imported_ts(ts_ns);
                idx.append(&IndexEntry::from_envelope(
                    &ts,
                    sid,
                    Dir::In,
                    Kind::Datagram,
                    off,
                    len,
                ))
                .map_err(|e| err("index append", e))?;
            }
            _ => {}
        }
    }
    raw.flush().map_err(|e| err("flush raw", e))?;
    idx.flush().map_err(|e| err("flush index", e))?;
    Ok(())
}

fn imported_ts(ts_origin_ns: i64) -> DualTimestamp {
    DualTimestamp {
        ts_origin_ns,
        ts_ingest_ns: crate::time::unix_ns_now(),
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
    WanloggerError::new(
        ErrorId::E1001PipelineGeneric,
        format!("pcapng-import: {ctx}"),
    )
    .with_source(e)
}

fn simple(msg: &str) -> WanloggerError {
    WanloggerError::new(
        ErrorId::E1001PipelineGeneric,
        format!("pcapng-import: {msg}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("wlg-pcapng-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn synth_pcapng(payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        let shb_body_len = 16;
        let shb_total = (12 + shb_body_len) as u32;
        buf.extend_from_slice(&BLOCK_SHB.to_le_bytes());
        buf.extend_from_slice(&shb_total.to_le_bytes());
        buf.extend_from_slice(&SHB_MAGIC.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&u64::MAX.to_le_bytes());
        buf.extend_from_slice(&shb_total.to_le_bytes());
        let pad = (4 - payload.len() % 4) % 4;
        let epb_body_len = 20 + payload.len() + pad;
        let epb_total = (12 + epb_body_len) as u32;
        buf.extend_from_slice(&BLOCK_EPB.to_le_bytes());
        buf.extend_from_slice(&epb_total.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1_000_000u32.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(payload);
        buf.extend(std::iter::repeat(0u8).take(pad));
        buf.extend_from_slice(&epb_total.to_le_bytes());
        buf
    }

    #[tokio::test]
    async fn imports_one_epb() {
        let dir = tempdir();
        let src = dir.join("in.pcapng");
        std::fs::write(&src, synth_pcapng(b"abcd")).unwrap();
        let dst = dir.join("session");
        PcapngImporter.import(&src, &dst).await.unwrap();
        let raw = std::fs::read(dst.join("raw.bin")).unwrap();
        assert_eq!(&raw[..], b"abcd");
        let idx = std::fs::read_to_string(dst.join("index.jsonl")).unwrap();
        assert_eq!(idx.lines().count(), 1);
    }
}
