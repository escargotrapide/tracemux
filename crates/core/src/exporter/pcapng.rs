//! pcapng exporter.
//!
//! Converts packet-shaped `session-dir/` datagram rows into a pcapng artifact
//! that can be opened by Wireshark. The exporter keeps the frozen log format
//! intact by joining packet metadata from `frames.jsonl` records with
//! `raw.bin` payloads through `(raw_off, raw_len)`.

// REQ: FR-EXP-PCAPNG

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use pcap_file::pcapng::blocks::enhanced_packet::EnhancedPacketBlock;
use pcap_file::pcapng::blocks::interface_description::{
    InterfaceDescriptionBlock, InterfaceDescriptionOption,
};
use pcap_file::pcapng::PcapNgWriter;
use pcap_file::DataLink;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error_id::{ErrorId, WanloggerError};
use crate::exporter::Exporter;
use crate::log::frames::FrameEntry;
use crate::log::index::{IndexEntry, Kind};
use crate::log::raw::RawReader;
use crate::log::rotate::{should_rotate, RotatePolicy, RotateStats};
use crate::source::pcap::PcapPacket;
use crate::Result;

/// Schema id used by pcap packet metadata records in `frames.jsonl`.
pub const PCAP_PACKET_SCHEMA_ID: &str = "wanlogger.pcap.packet.v1";

const DEFAULT_ETHERNET_LINKTYPE: u32 = 1;
const PCAPNG_NANOSECOND_TSRESOL: u8 = 9;

/// pcapng exporter.
#[derive(Debug, Default)]
pub struct PcapngExporter;

#[async_trait]
impl Exporter for PcapngExporter {
    fn kind(&self) -> &'static str {
        "pcapng"
    }

    async fn export(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst)
    }
}

/// Export a packet-shaped session-dir as pcapng.
pub fn export(src: &Path, dst: &Path) -> Result<()> {
    run(src, dst)
}

/// Export pcapng. The `timezone` option is accepted for CLI/API symmetry but
/// ignored because pcapng stores absolute UTC timestamps.
pub fn export_with_timezone(src: &Path, dst: &Path, _timezone: Option<&str>) -> Result<()> {
    run(src, dst)
}

/// Options for direct pcapng streaming during live packet capture.
#[derive(Debug, Clone)]
pub struct PcapngStreamOptions {
    /// First pcapng artifact path. Rotated artifacts are written beside it.
    pub path: PathBuf,
    /// Rotation thresholds for each pcapng part.
    pub rotate: RotatePolicy,
}

impl PcapngStreamOptions {
    /// Create options with the default rotation policy.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            rotate: RotatePolicy::default(),
        }
    }
}

/// Direct pcapng writer for live packet capture.
///
/// The writer creates a valid pcapng section immediately, writes interface
/// blocks lazily per `(linktype, label)`, and rotates before appending the next
/// packet when the current artifact has crossed the configured threshold.
pub struct PcapngStreamWriter {
    base_path: PathBuf,
    rotate: RotatePolicy,
    next_part_index: u32,
    artifacts: Vec<PathBuf>,
    current: PcapngPartWriter,
}

impl PcapngStreamWriter {
    /// Create a direct pcapng writer.
    pub fn create(options: PcapngStreamOptions) -> Result<Self> {
        let current = PcapngPartWriter::create(&options.path)?;
        Ok(Self {
            artifacts: vec![options.path.clone()],
            base_path: options.path,
            rotate: options.rotate,
            next_part_index: 1,
            current,
        })
    }

    /// Append one packet to the current artifact, rotating first if needed.
    pub fn append_packet(&mut self, interface_label: &str, packet: &PcapPacket) -> Result<()> {
        if self
            .current
            .should_rotate_for(packet.ts_origin_ns, &self.rotate)
        {
            self.rotate()?;
        }
        self.current.append_packet(interface_label, packet)
    }

    /// Flush the current artifact.
    pub fn flush(&mut self) -> Result<()> {
        self.current.flush()
    }

    /// Return artifact paths written so far.
    #[must_use]
    pub fn artifacts(&self) -> &[PathBuf] {
        &self.artifacts
    }

    fn rotate(&mut self) -> Result<()> {
        self.current.flush()?;
        let path = rotated_part_path(&self.base_path, self.next_part_index);
        self.next_part_index = self.next_part_index.saturating_add(1);
        self.current = PcapngPartWriter::create(&path)?;
        self.artifacts.push(path);
        Ok(())
    }
}

struct PcapngPartWriter {
    writer: PcapNgWriter<CountingWriter<BufWriter<File>>>,
    interfaces: HashMap<InterfaceKey, u32>,
    first_ts_origin_ns: Option<i64>,
    packets: u64,
}

impl PcapngPartWriter {
    fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|e| err("creating pcapng parent", e))?;
        }
        let out = File::create(path).map_err(|e| err("creating direct pcapng", e))?;
        let writer = PcapNgWriter::new(CountingWriter::new(BufWriter::new(out)))
            .map_err(|e| simple(format!("pcapng-stream: creating writer: {e}")))?;
        Ok(Self {
            writer,
            interfaces: HashMap::new(),
            first_ts_origin_ns: None,
            packets: 0,
        })
    }

    fn append_packet(&mut self, interface_label: &str, packet: &PcapPacket) -> Result<()> {
        let key = InterfaceKey {
            linktype: packet.linktype,
            label: interface_label.to_string(),
        };
        let interface_id = ensure_interface(&mut self.writer, &mut self.interfaces, &key)?;
        let block = EnhancedPacketBlock {
            interface_id,
            timestamp: timestamp_ns_duration(packet.ts_origin_ns)?,
            original_len: packet.original_len,
            data: Cow::Borrowed(packet.data.as_ref()),
            options: vec![],
        };
        self.writer
            .write_pcapng_block(block)
            .map_err(|e| simple(format!("pcapng-stream: writing packet block: {e}")))?;
        self.first_ts_origin_ns.get_or_insert(packet.ts_origin_ns);
        self.packets = self.packets.saturating_add(1);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer
            .get_mut()
            .flush()
            .map_err(|e| err("flushing direct pcapng", e))
    }

    fn should_rotate_for(&self, next_ts_origin_ns: i64, policy: &RotatePolicy) -> bool {
        if self.packets == 0 {
            return false;
        }
        should_rotate(
            &RotateStats {
                size_bytes: self.writer.get_ref().bytes_written(),
                age: self.age_for(next_ts_origin_ns),
            },
            policy,
        )
    }

    fn age_for(&self, next_ts_origin_ns: i64) -> Duration {
        let Some(first) = self.first_ts_origin_ns else {
            return Duration::ZERO;
        };
        let delta = next_ts_origin_ns.saturating_sub(first);
        Duration::from_nanos(u64::try_from(delta).unwrap_or(0))
    }
}

struct CountingWriter<W> {
    inner: W,
    bytes_written: u64,
}

impl<W> CountingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            bytes_written: 0,
        }
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.bytes_written = self.bytes_written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn run(src: &Path, dst: &Path) -> Result<()> {
    let metadata = read_packet_metadata(src)?;
    let idx = File::open(src.join("index.jsonl")).map_err(|e| err("opening index.jsonl", e))?;
    let mut raw = RawReader::open(src).map_err(|e| err("opening raw.bin", e))?;
    let out = File::create(dst).map_err(|e| err("creating dst", e))?;
    let mut writer = PcapNgWriter::new(BufWriter::new(out))
        .map_err(|e| simple(format!("pcapng-export: creating writer: {e}")))?;
    let mut interfaces = HashMap::<InterfaceKey, u32>::new();
    let mut exported = 0usize;

    for line in BufReader::new(idx).lines() {
        let line = line.map_err(|e| err("reading index line", e))?;
        if line.is_empty() {
            continue;
        }
        let entry: IndexEntry =
            serde_json::from_str(&line).map_err(|e| serde_err("parsing index entry", e))?;
        if entry.kind != Kind::Datagram {
            continue;
        }

        let meta = metadata.get(&(entry.off, entry.len));
        if !is_pcap_datagram(&entry, meta) {
            continue;
        }

        let bytes = raw
            .read_at(entry.off, entry.len)
            .map_err(|e| err("reading raw packet", e))?;
        let linktype = meta
            .and_then(|m| m.linktype)
            .unwrap_or(DEFAULT_ETHERNET_LINKTYPE);
        let interface_label = packet_interface_label(&entry, meta);
        let key = InterfaceKey {
            linktype,
            label: interface_label,
        };
        let interface_id = ensure_interface(&mut writer, &mut interfaces, &key)?;
        let original_len = meta
            .and_then(|m| m.original_len)
            .unwrap_or_else(|| meta.and_then(|m| m.captured_len).unwrap_or(entry.len));
        let timestamp = timestamp_duration(&entry.ts_origin)?;
        let packet = EnhancedPacketBlock {
            interface_id,
            timestamp,
            original_len,
            data: Cow::Borrowed(bytes.as_slice()),
            options: vec![],
        };
        writer
            .write_pcapng_block(packet)
            .map_err(|e| simple(format!("pcapng-export: writing packet block: {e}")))?;
        exported += 1;
    }

    if exported == 0 {
        return Err(simple(
            "pcapng-export: no pcap datagram rows found in session-dir",
        ));
    }

    writer
        .into_inner()
        .flush()
        .map_err(|e| err("flushing dst", e))?;
    Ok(())
}

fn read_packet_metadata(src: &Path) -> Result<HashMap<(u64, u32), PacketMetadata>> {
    let path = src.join("frames.jsonl");
    if !path.is_file() {
        return Ok(HashMap::new());
    }
    let file = File::open(&path).map_err(|e| err("opening frames.jsonl", e))?;
    let mut out = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| err("reading frames line", e))?;
        if line.is_empty() {
            continue;
        }
        let frame: FrameEntry =
            serde_json::from_str(&line).map_err(|e| serde_err("parsing frame entry", e))?;
        if frame.record.schema_id.as_deref() != Some(PCAP_PACKET_SCHEMA_ID) {
            continue;
        }
        if let Some(meta) = PacketMetadata::from_fields(&frame.record.fields) {
            out.insert((meta.raw_off, meta.raw_len), meta);
        }
    }
    Ok(out)
}

fn is_pcap_datagram(entry: &IndexEntry, meta: Option<&PacketMetadata>) -> bool {
    meta.is_some()
        || entry.schema_id.as_deref() == Some(PCAP_PACKET_SCHEMA_ID)
        || entry
            .source
            .as_deref()
            .is_some_and(|source| source.starts_with("pcap:"))
}

fn packet_interface_label(entry: &IndexEntry, meta: Option<&PacketMetadata>) -> String {
    meta.and_then(|m| m.interface.clone())
        .or_else(|| {
            entry
                .source
                .as_deref()
                .and_then(|source| source.strip_prefix("pcap:"))
                .map(str::to_string)
        })
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| "pcap".to_string())
}

fn ensure_interface<W: Write>(
    writer: &mut PcapNgWriter<W>,
    interfaces: &mut HashMap<InterfaceKey, u32>,
    key: &InterfaceKey,
) -> Result<u32> {
    if let Some(id) = interfaces.get(key) {
        return Ok(*id);
    }
    let id = u32::try_from(interfaces.len())
        .map_err(|_| simple("pcapng-export: too many pcapng interfaces"))?;
    let mut block = InterfaceDescriptionBlock::new(DataLink::from(key.linktype), 0);
    block.options.push(InterfaceDescriptionOption::IfTsResol(
        PCAPNG_NANOSECOND_TSRESOL,
    ));
    block
        .options
        .push(InterfaceDescriptionOption::IfName(Cow::Owned(
            key.label.clone(),
        )));
    writer
        .write_pcapng_block(block)
        .map_err(|e| simple(format!("pcapng-export: writing interface block: {e}")))?;
    interfaces.insert(key.clone(), id);
    Ok(id)
}

fn timestamp_duration(ts: &str) -> Result<Duration> {
    let parsed = OffsetDateTime::parse(ts, &Rfc3339).map_err(|e| {
        WanloggerError::new(
            ErrorId::E1001PipelineGeneric,
            "pcapng-export: parsing ts_origin",
        )
        .with_source(e)
    })?;
    let nanos = parsed.unix_timestamp_nanos();
    timestamp_nanos_duration(nanos, || {
        format!("pcapng-export: ts_origin before UNIX epoch is not supported: {ts}")
    })
}

fn timestamp_ns_duration(ts_origin_ns: i64) -> Result<Duration> {
    timestamp_nanos_duration(i128::from(ts_origin_ns), || {
        format!("pcapng-stream: ts_origin before UNIX epoch is not supported: {ts_origin_ns}")
    })
}

fn timestamp_nanos_duration(
    nanos: i128,
    before_epoch_message: impl FnOnce() -> String,
) -> Result<Duration> {
    let nanos = u128::try_from(nanos).map_err(|_| simple(before_epoch_message()))?;
    let secs = nanos / 1_000_000_000;
    let subsec_nanos = (nanos % 1_000_000_000) as u32;
    let secs = u64::try_from(secs).map_err(|_| simple("pcapng: ts_origin out of range"))?;
    Ok(Duration::new(secs, subsec_nanos))
}

fn rotated_part_path(base: &Path, part_index: u32) -> PathBuf {
    let stem = base
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("capture");
    let extension = base
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .unwrap_or("pcapng");
    base.with_file_name(format!("{stem}.{part_index:04}.{extension}"))
}

#[derive(Debug, Clone)]
struct PacketMetadata {
    raw_off: u64,
    raw_len: u32,
    captured_len: Option<u32>,
    original_len: Option<u32>,
    linktype: Option<u32>,
    interface: Option<String>,
}

impl PacketMetadata {
    fn from_fields(fields: &Value) -> Option<Self> {
        let raw_off = u64_field(fields, "raw_off")?;
        let raw_len = u32_field(fields, "raw_len")?;
        Some(Self {
            raw_off,
            raw_len,
            captured_len: u32_field(fields, "captured_len"),
            original_len: u32_field(fields, "original_len"),
            linktype: u32_field(fields, "linktype"),
            interface: string_field(fields, "interface"),
        })
    }
}

#[derive(Debug, Clone, Eq)]
struct InterfaceKey {
    linktype: u32,
    label: String,
}

impl PartialEq for InterfaceKey {
    fn eq(&self, other: &Self) -> bool {
        self.linktype == other.linktype && self.label == other.label
    }
}

impl Hash for InterfaceKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.linktype.hash(state);
        self.label.hash(state);
    }
}

fn u64_field(fields: &Value, key: &str) -> Option<u64> {
    match fields.get(key)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn u32_field(fields: &Value, key: &str) -> Option<u32> {
    u64_field(fields, key).and_then(|value| u32::try_from(value).ok())
}

fn string_field(fields: &Value, key: &str) -> Option<String> {
    fields.get(key)?.as_str().map(str::to_string)
}

fn err(ctx: &str, e: std::io::Error) -> WanloggerError {
    WanloggerError::new(
        ErrorId::E1001PipelineGeneric,
        format!("pcapng-export: {ctx}"),
    )
    .with_source(e)
}

fn serde_err(ctx: &str, e: serde_json::Error) -> WanloggerError {
    WanloggerError::new(
        ErrorId::E1001PipelineGeneric,
        format!("pcapng-export: {ctx}"),
    )
    .with_source(e)
}

fn simple(msg: impl Into<String>) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, msg)
}

#[cfg(test)]
mod tests {
    // REQ: FR-EXP-PCAPNG

    use super::*;
    use crate::decoder::Record;
    use crate::exporter::Exporter;
    use crate::importer::pcapng::PcapngImporter;
    use crate::importer::Importer;
    use crate::log::frames::{FrameEntry, FramesWriter};
    use crate::log::index::{Dir, IndexEntry, IndexWriter, Kind};
    use crate::log::raw::RawWriter;
    use crate::time::{ClockQuality, ClockSource, DualTimestamp};
    use uuid::Uuid;

    const BLOCK_SHB: u32 = 0x0A0D_0D0A;
    const BLOCK_IDB: u32 = 0x0000_0001;
    const BLOCK_EPB: u32 = 0x0000_0006;

    #[tokio::test]
    async fn exports_synthetic_pcap_session() {
        let root = tempdir();
        let session = root.join("session");
        let sid = Uuid::new_v4();
        let packet = write_synthetic_pcap_session(&session, sid, true);
        let dst = root.join("out.pcapng");

        PcapngExporter.export(&session, &dst).await.unwrap();

        let body = std::fs::read(&dst).unwrap();
        let block_types = block_types(&body);
        assert_eq!(block_types.first(), Some(&BLOCK_SHB));
        assert!(block_types.contains(&BLOCK_IDB));
        assert!(block_types.contains(&BLOCK_EPB));

        let imported = root.join("imported");
        PcapngImporter.import(&dst, &imported).await.unwrap();
        assert_eq!(std::fs::read(imported.join("raw.bin")).unwrap(), packet);
    }

    #[tokio::test]
    async fn rejects_non_pcap_datagrams_without_metadata() {
        let root = tempdir();
        let session = root.join("session");
        let sid = Uuid::new_v4();
        write_synthetic_pcap_session(&session, sid, false);
        let dst = root.join("out.pcapng");

        let err = PcapngExporter.export(&session, &dst).await.unwrap_err();

        assert!(err.to_string().contains("no pcap datagram rows"));
    }

    #[tokio::test]
    async fn stream_writer_writes_direct_pcapng() {
        let root = tempdir();
        let dst = root.join("direct.pcapng");
        let packet = pcap_packet(1, 1_700_000_000_123_456_789);
        let mut writer = PcapngStreamWriter::create(PcapngStreamOptions::new(&dst)).unwrap();

        writer.append_packet("eth0", &packet).unwrap();
        writer.flush().unwrap();

        assert_eq!(writer.artifacts(), &[dst.clone()]);
        let body = std::fs::read(&dst).unwrap();
        let block_types = block_types(&body);
        assert_eq!(block_types.first(), Some(&BLOCK_SHB));
        assert!(block_types.contains(&BLOCK_IDB));
        assert!(block_types.contains(&BLOCK_EPB));

        let imported = root.join("direct-imported");
        PcapngImporter.import(&dst, &imported).await.unwrap();
        assert_eq!(
            std::fs::read(imported.join("raw.bin")).unwrap(),
            packet.data
        );
    }

    #[test]
    fn stream_writer_rotates_by_size() {
        let root = tempdir();
        let dst = root.join("rotating.pcapng");
        let mut writer = PcapngStreamWriter::create(PcapngStreamOptions {
            path: dst.clone(),
            rotate: RotatePolicy {
                size_bytes: Some(1),
                duration: None,
            },
        })
        .unwrap();

        writer
            .append_packet("eth0", &pcap_packet(1, 1_700_000_000_000_000_000))
            .unwrap();
        writer
            .append_packet("eth0", &pcap_packet(2, 1_700_000_000_000_000_001))
            .unwrap();
        writer.flush().unwrap();

        assert_eq!(writer.artifacts().len(), 2);
        assert_eq!(writer.artifacts()[0], dst);
        assert!(writer.artifacts()[1].ends_with("rotating.0001.pcapng"));
        for artifact in writer.artifacts() {
            let body = std::fs::read(artifact).unwrap();
            assert!(block_types(&body).contains(&BLOCK_EPB));
        }
    }

    #[test]
    fn stream_writer_rotates_by_duration() {
        let root = tempdir();
        let dst = root.join("duration.pcapng");
        let mut writer = PcapngStreamWriter::create(PcapngStreamOptions {
            path: dst,
            rotate: RotatePolicy {
                size_bytes: None,
                duration: Some(Duration::from_nanos(1)),
            },
        })
        .unwrap();

        writer
            .append_packet("eth0", &pcap_packet(1, 1_700_000_000_000_000_000))
            .unwrap();
        writer
            .append_packet("eth0", &pcap_packet(2, 1_700_000_000_000_000_001))
            .unwrap();
        writer.flush().unwrap();

        assert_eq!(writer.artifacts().len(), 2);
    }

    fn write_synthetic_pcap_session(dir: &Path, sid: Uuid, include_pcap_metadata: bool) -> Vec<u8> {
        std::fs::create_dir_all(dir).unwrap();
        let packet = ethernet_packet();
        let mut raw = RawWriter::create(dir).unwrap();
        let (off, len) = raw.append(&packet).unwrap();
        raw.flush().unwrap();

        let ts = sample_ts();
        let mut entry = IndexEntry::from_envelope(&ts, sid, Dir::In, Kind::Datagram, off, len);
        if include_pcap_metadata {
            entry.source = Some("pcap:eth0".to_string());
            entry.schema_id = Some(PCAP_PACKET_SCHEMA_ID.to_string());
        }
        let mut index = IndexWriter::create(dir).unwrap();
        index.append(&entry).unwrap();
        index.flush().unwrap();

        if include_pcap_metadata {
            let mut frames = FramesWriter::create(dir).unwrap();
            frames
                .append(&FrameEntry {
                    ts: entry.ts_ingest.clone(),
                    decoder: "pcap".to_string(),
                    record: Record {
                        schema_id: Some(PCAP_PACKET_SCHEMA_ID.to_string()),
                        level: None,
                        text: None,
                        fields: serde_json::json!({
                            "seq": 1,
                            "raw_off": off,
                            "raw_len": len,
                            "captured_len": len,
                            "original_len": len,
                            "linktype": DEFAULT_ETHERNET_LINKTYPE,
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

        packet
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

    fn pcap_packet(seq: u64, ts_origin_ns: i64) -> PcapPacket {
        let packet = ethernet_packet();
        PcapPacket::new(
            seq,
            ts_origin_ns,
            packet.len() as u32,
            DEFAULT_ETHERNET_LINKTYPE,
            0,
            packet,
        )
    }

    fn block_types(bytes: &[u8]) -> Vec<u32> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos + 12 <= bytes.len() {
            let block_type =
                u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
            let total_len = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            if total_len < 12 || pos + total_len > bytes.len() {
                break;
            }
            out.push(block_type);
            pos += total_len;
        }
        out
    }

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("wlg-export-pcapng-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
