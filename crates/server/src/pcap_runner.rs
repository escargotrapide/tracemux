//! Packet-capture runner with metadata-preserving session-dir writes.
//!
//! The frozen `Source::recv()` API can only return a `Frame`, which is not
//! enough to preserve libpcap/Npcap metadata. This runner consumes
//! `PcapSource::recv_packet()` directly and writes `datagram` rows plus
//! `tracemux.pcap.packet.v1` frame metadata.

// REQ: FR-LOG-PCAP
// REQ: FR-MET-PCAP
// REQ: NFR-PERF-PCAP
// REQ: NFR-REL-PCAP

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use rmpv::Value;
use serde_json::{Map, Value as JsonValue};
use tokio::sync::oneshot;
use tracemux_core::decoder::Record;
use tracemux_core::exporter::pcapng::{
    PcapngStreamOptions, PcapngStreamWriter, PCAP_PACKET_SCHEMA_ID,
};
use tracemux_core::log::frames::{FrameEntry, FramesWriter};
use tracemux_core::log::index::{format_rfc3339_ns, Dir, IndexEntry, IndexWriter, Kind};
use tracemux_core::log::raw::RawWriter;
use tracemux_core::packet_summary::summarize_pcap_packet;
use tracemux_core::session::registry::SessionState;
use tracemux_core::source::pcap::{
    PcapConfig, PcapPacket, PcapPublishMode, PcapSaveMode, PcapSource, PcapStats,
};
use tracemux_core::source::Source;
use tracemux_core::time::{unix_ns_now, ClockQuality, ClockSource, DualTimestamp, TimeSource};
use tracemux_core::{ErrorId, TraceMuxError};
use uuid::Uuid;

use crate::ingest::Ingest;
use crate::runner::{encode_data_envelope_with_kind, RunnerStats};
use crate::wire::{encode, Envelope, FrameType};

const PCAP_DECODER_LABEL: &str = "pcap-packet";

pub(crate) async fn run_pcap_once_notify<T>(
    ingest: Arc<Ingest>,
    mut source: PcapSource,
    time: &T,
    session_dir: Option<PathBuf>,
    registered: Option<oneshot::Sender<Uuid>>,
    sid_override: Option<Uuid>,
    host: Option<String>,
    label: Option<String>,
) -> anyhow::Result<RunnerStats>
where
    T: TimeSource,
{
    let config = source.config().clone();
    source.open().await?;
    let meta = source.metadata();
    let mut state = SessionState::new(meta.kind.clone(), meta.iface.clone());
    state.label = label.or_else(|| config.display_name.clone());
    if let Some(sid) = sid_override {
        state.sid = sid;
    }
    let sid = state.sid;
    let source_label = format!("pcap:{}", config.interface);
    let mut writer = if should_write_session(config.save_mode) {
        session_dir
            .as_deref()
            .map(|dir| PcapSessionWriter::create(dir, sid, source_label.clone(), host.clone()))
            .transpose()?
    } else {
        None
    };
    let mut pcapng_writer = direct_pcapng_writer(&config, session_dir.as_deref())?;
    let sid = ingest.register_session(state);
    if let Some(tx) = registered {
        let _ = tx.send(sid);
    }

    let mut stats = RunnerStats {
        sid,
        raw_frames: 0,
        framed: 0,
        decoded_records: 0,
    };
    let mut metrics = PcapMetricsState::default();

    while let Some(packet) = source.recv_packet().await? {
        let ts = packet_timestamp(time, &packet);
        if let Some(writer) = pcapng_writer.as_mut() {
            writer.append_packet(config.iface_label(), &packet)?;
        }
        if let Some(writer) = writer.as_mut() {
            writer.append_packet(&config, sid, &packet, &ts)?;
            stats.decoded_records += 1;
        } else if pcapng_writer.is_some() {
            stats.decoded_records += 1;
        }

        stats.raw_frames += 1;
        if should_publish(config.publish_mode, stats.raw_frames) {
            let wire = encode_data_envelope_with_kind(
                sid,
                0,
                stats.raw_frames,
                &ts,
                &packet.data,
                &source_label,
                "datagram",
            )?;
            let _ = ingest.publish_wire(sid, Bytes::from(wire));
        } else {
            ingest.record_frame(sid, u64::from(packet.captured_len));
        }

        let backend_stats = source.stats().await.unwrap_or_default();
        metrics.observe(&backend_stats, &packet, &ts);
        publish_metrics(
            &ingest,
            sid,
            0,
            stats.raw_frames,
            &ts,
            &source_label,
            &metrics,
        )?;
    }

    if let Some(writer) = writer.as_mut() {
        writer.commit()?;
    }
    if let Some(writer) = pcapng_writer.as_mut() {
        writer.flush()?;
    }
    source.close().await?;
    Ok(stats)
}

fn should_write_session(mode: PcapSaveMode) -> bool {
    matches!(mode, PcapSaveMode::Session | PcapSaveMode::Both)
}

fn direct_pcapng_writer(
    config: &PcapConfig,
    session_dir: Option<&Path>,
) -> tracemux_core::Result<Option<PcapngStreamWriter>> {
    if !matches!(config.save_mode, PcapSaveMode::Pcapng | PcapSaveMode::Both) {
        return Ok(None);
    }
    let path = config
        .pcapng_path
        .clone()
        .or_else(|| session_dir.map(|dir| dir.join("capture.pcapng")))
        .ok_or_else(|| {
            TraceMuxError::new(
                ErrorId::E1001PipelineGeneric,
                "pcapng save requested but no pcapng_path or session-dir is available",
            )
        })?;
    PcapngStreamWriter::create(PcapngStreamOptions::new(path)).map(Some)
}

fn should_publish(mode: PcapPublishMode, seq: u64) -> bool {
    match mode {
        PcapPublishMode::StatsOnly => false,
        PcapPublishMode::Sampled => seq == 1 || seq % 100 == 0,
        PcapPublishMode::Full => true,
    }
}

#[derive(Debug, Default)]
struct PcapMetricsState {
    started_mono_ns: Option<u64>,
    packets_total: u64,
    bytes_total: u64,
    dropped_kernel_total: u64,
    dropped_app_total: u64,
    capture_queue_depth: u64,
    writer_queue_depth: u64,
    last_packet_ts_origin_ns: Option<i64>,
    pps: u64,
    bytes_per_sec: u64,
}

impl PcapMetricsState {
    fn observe(&mut self, backend: &PcapStats, packet: &PcapPacket, ts: &DualTimestamp) {
        let started = *self.started_mono_ns.get_or_insert(ts.mono_ns);
        self.packets_total = backend.packets_total.max(self.packets_total + 1);
        self.bytes_total = backend
            .bytes_total
            .max(self.bytes_total + u64::from(packet.captured_len));
        self.dropped_kernel_total = backend.dropped_kernel_total;
        self.dropped_app_total = backend.dropped_app_total;
        self.capture_queue_depth = backend.capture_queue_depth;
        self.writer_queue_depth = backend.writer_queue_depth;
        self.last_packet_ts_origin_ns = backend
            .last_packet_ts_origin_ns
            .or(Some(packet.ts_origin_ns));
        let elapsed_ns = ts.mono_ns.saturating_sub(started);
        if elapsed_ns > 0 {
            self.pps = rate_per_sec(self.packets_total, elapsed_ns);
            self.bytes_per_sec = rate_per_sec(self.bytes_total, elapsed_ns);
        }
    }
}

fn rate_per_sec(count: u64, elapsed_ns: u64) -> u64 {
    if elapsed_ns == 0 {
        return 0;
    }
    let scaled = u128::from(count) * 1_000_000_000u128 / u128::from(elapsed_ns);
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

fn publish_metrics(
    ingest: &Ingest,
    sid: Uuid,
    ch: u32,
    seq: u64,
    ts: &DualTimestamp,
    source_label: &str,
    metrics: &PcapMetricsState,
) -> anyhow::Result<()> {
    let Some(session) = ingest.registry.get(&sid) else {
        return Ok(());
    };
    let env = Envelope::new(
        FrameType::Metrics,
        seq,
        pcap_metrics_payload(sid, ch, ts, source_label, metrics),
    )
    .with_sid(sid.to_string())
    .with_ch(ch);
    let bytes = encode(&env)?;
    let _ = session.fanout.publish(Bytes::from(bytes));
    Ok(())
}

fn pcap_metrics_payload(
    sid: Uuid,
    ch: u32,
    ts: &DualTimestamp,
    source_label: &str,
    metrics: &PcapMetricsState,
) -> Value {
    let sid_key = Value::String(sid.to_string().into());
    let ch_key = Value::String(format!("{sid}/{ch}").into());
    Value::Map(vec![
        (
            Value::String("ts".into()),
            Value::String(format_rfc3339_ns(ts.ts_ingest_ns).into()),
        ),
        (
            Value::String("bytes_in".into()),
            Value::Map(vec![(sid_key.clone(), Value::from(metrics.bytes_total))]),
        ),
        (
            Value::String("records".into()),
            Value::Map(vec![(ch_key, Value::from(metrics.packets_total))]),
        ),
        (
            Value::String("pcap".into()),
            Value::Map(vec![(
                sid_key,
                Value::Map(vec![
                    (
                        Value::String("source".into()),
                        Value::String(source_label.to_string().into()),
                    ),
                    (
                        Value::String("packets_total".into()),
                        Value::from(metrics.packets_total),
                    ),
                    (
                        Value::String("bytes_total".into()),
                        Value::from(metrics.bytes_total),
                    ),
                    (
                        Value::String("dropped_kernel_total".into()),
                        Value::from(metrics.dropped_kernel_total),
                    ),
                    (
                        Value::String("dropped_app_total".into()),
                        Value::from(metrics.dropped_app_total),
                    ),
                    (
                        Value::String("capture_queue_depth".into()),
                        Value::from(metrics.capture_queue_depth),
                    ),
                    (
                        Value::String("writer_queue_depth".into()),
                        Value::from(metrics.writer_queue_depth),
                    ),
                    (
                        Value::String("last_packet_ts_origin_ns".into()),
                        metrics
                            .last_packet_ts_origin_ns
                            .map_or(Value::Nil, Value::from),
                    ),
                    (Value::String("pps".into()), Value::from(metrics.pps)),
                    (
                        Value::String("bytes_per_sec".into()),
                        Value::from(metrics.bytes_per_sec),
                    ),
                ]),
            )]),
        ),
    ])
}

fn packet_timestamp<T: TimeSource>(time: &T, packet: &PcapPacket) -> DualTimestamp {
    let mut origin = time.stamp_origin();
    origin.ts_origin_ns = packet.ts_origin_ns;
    origin.clock_quality = ClockQuality::Imported;
    origin.clock_source = ClockSource::Imported;
    time.stamp_ingest(origin)
}

struct PcapSessionWriter {
    raw: RawWriter,
    index: IndexWriter,
    frames: FramesWriter,
    source: String,
    host: Option<String>,
}

impl PcapSessionWriter {
    fn create(
        dir: &Path,
        sid: Uuid,
        source: String,
        host: Option<String>,
    ) -> tracemux_core::Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| log_err("creating pcap session-dir", e))?;
        write_meta(dir, sid, &source, host.as_deref())?;
        Ok(Self {
            raw: RawWriter::create(dir).map_err(|e| log_err("opening raw.bin", e))?,
            index: IndexWriter::create(dir).map_err(|e| log_err("opening index.jsonl", e))?,
            frames: FramesWriter::create(dir).map_err(|e| log_err("opening frames.jsonl", e))?,
            source,
            host,
        })
    }

    fn append_packet(
        &mut self,
        config: &PcapConfig,
        sid: Uuid,
        packet: &PcapPacket,
        ts: &DualTimestamp,
    ) -> tracemux_core::Result<()> {
        let (off, len) = self
            .raw
            .append(&packet.data)
            .map_err(|e| log_err("appending pcap raw.bin", e))?;
        let mut entry = IndexEntry::from_envelope(ts, sid, Dir::In, Kind::Datagram, off, len);
        entry.source = Some(self.source.clone());
        entry.host.clone_from(&self.host);
        entry.schema_id = Some(PCAP_PACKET_SCHEMA_ID.to_string());
        self.index
            .append(&entry)
            .map_err(|e| log_err("appending pcap index.jsonl", e))?;

        self.frames
            .append(&FrameEntry {
                ts: entry.ts_ingest,
                decoder: PCAP_DECODER_LABEL.to_string(),
                record: packet_record(config, packet, off, len),
            })
            .map_err(|e| log_err("appending pcap frames.jsonl", e))?;
        Ok(())
    }

    fn commit(&mut self) -> tracemux_core::Result<()> {
        self.raw
            .flush()
            .map_err(|e| log_err("flushing pcap raw.bin", e))?;
        self.index
            .flush()
            .map_err(|e| log_err("flushing pcap index.jsonl", e))?;
        self.frames
            .flush()
            .map_err(|e| log_err("flushing pcap frames.jsonl", e))?;
        Ok(())
    }
}

fn packet_record(config: &PcapConfig, packet: &PcapPacket, raw_off: u64, raw_len: u32) -> Record {
    let mut fields = Map::new();
    fields.insert("seq".to_string(), JsonValue::from(packet.seq));
    fields.insert("raw_off".to_string(), JsonValue::from(raw_off));
    fields.insert("raw_len".to_string(), JsonValue::from(raw_len));
    fields.insert(
        "captured_len".to_string(),
        JsonValue::from(packet.captured_len),
    );
    fields.insert(
        "original_len".to_string(),
        JsonValue::from(packet.original_len),
    );
    fields.insert("linktype".to_string(), JsonValue::from(packet.linktype));
    fields.insert(
        "interface_id".to_string(),
        JsonValue::from(packet.interface_id),
    );
    fields.insert(
        "interface".to_string(),
        JsonValue::String(config.interface.clone()),
    );
    if let Some(filter) = &config.filter {
        fields.insert("filter".to_string(), JsonValue::String(filter.clone()));
    }
    match summarize_pcap_packet(packet.linktype, &packet.data) {
        Ok(summary) => {
            if let Ok(JsonValue::Object(summary_fields)) = serde_json::to_value(summary) {
                fields.extend(summary_fields);
            }
        }
        Err(err) => {
            fields.insert(
                "summary_error".to_string(),
                JsonValue::String(err.to_string()),
            );
        }
    }

    Record {
        schema_id: Some(PCAP_PACKET_SCHEMA_ID.to_string()),
        level: None,
        text: None,
        fields: JsonValue::Object(fields),
        tags: vec!["pcap".to_string()],
        correlation_id: None,
    }
}

fn write_meta(
    dir: &Path,
    sid: Uuid,
    source: &str,
    host: Option<&str>,
) -> tracemux_core::Result<()> {
    let mut body = String::new();
    body.push_str("log_format_version = \"1.0.0\"\n");
    writeln!(body, "sid = \"{sid}\"").expect("writing to String cannot fail");
    writeln!(body, "created = \"{}\"", format_rfc3339_ns(unix_ns_now()))
        .expect("writing to String cannot fail");
    writeln!(body, "source = \"{}\"", toml_escape(source)).expect("writing to String cannot fail");
    if let Some(host) = host {
        writeln!(body, "host = \"{}\"", toml_escape(host)).expect("writing to String cannot fail");
    }
    writeln!(body, "decoder = \"{PCAP_DECODER_LABEL}\"").expect("writing to String cannot fail");
    std::fs::write(dir.join("meta.toml"), body).map_err(|e| log_err("writing pcap meta.toml", e))
}

fn toml_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn log_err(ctx: &'static str, err: std::io::Error) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1001PipelineGeneric, ctx).with_source(err)
}

#[cfg(test)]
mod tests {
    // REQ: FR-LOG-PCAP
    // REQ: FR-MET-PCAP
    // REQ: FR-EXP-PCAPNG

    use tracemux_core::exporter::pcapng;
    use tracemux_core::importer::pcapng::PcapngImporter;
    use tracemux_core::importer::Importer;
    use tracemux_core::packet_summary::LINKTYPE_ETHERNET;
    use tracemux_core::source::pcap::{FakePcapBackend, PcapSaveMode};

    use super::*;

    #[derive(Debug)]
    struct FixedTimeSource {
        id: Uuid,
    }

    impl FixedTimeSource {
        fn new() -> Self {
            Self { id: Uuid::nil() }
        }
    }

    impl TimeSource for FixedTimeSource {
        fn stamp_origin(&self) -> DualTimestamp {
            DualTimestamp {
                ts_origin_ns: 10,
                ts_ingest_ns: 10,
                mono_ns: 10,
                boot_id: self.id,
                node_id: self.id,
                clock_offset_ms: 0,
                clock_quality: ClockQuality::BestEffort,
                drift_ppm: 0.0,
                clock_source: ClockSource::System,
            }
        }

        fn stamp_ingest(&self, mut origin: DualTimestamp) -> DualTimestamp {
            origin.ts_ingest_ns = 20;
            origin.mono_ns = 20;
            origin
        }

        fn boot_id(&self) -> Uuid {
            self.id
        }

        fn node_id(&self) -> Uuid {
            self.id
        }
    }

    #[tokio::test]
    async fn fake_pcap_packets_are_written_as_pcap_session_dir() {
        let root = tempdir();
        let session = root.join("session");
        let packet = PcapPacket::new(
            1,
            1_700_000_000_123_456_789,
            18,
            LINKTYPE_ETHERNET,
            0,
            ethernet_packet(),
        );
        let mut config = PcapConfig::new("fake0");
        config.filter = Some("ether proto 0x88b5".to_string());
        let source = PcapSource::with_backend(config, FakePcapBackend::new([packet.clone()]));
        let ingest = Arc::new(Ingest::new());
        let sid = Uuid::new_v4();
        let mut state = SessionState::new("pcap", "fake0");
        state.sid = sid;
        ingest.register_session(state);
        let session_state = ingest.registry.get(&sid).unwrap();
        let mut metrics_rx = session_state.fanout.subscribe();

        let stats = run_pcap_once_notify(
            ingest.clone(),
            source,
            &FixedTimeSource::new(),
            Some(session.clone()),
            None,
            Some(sid),
            Some("host-a".to_string()),
            None,
        )
        .await
        .unwrap();

        assert_eq!(stats.sid, sid);
        assert_eq!(stats.raw_frames, 1);
        assert_eq!(stats.decoded_records, 1);
        assert_eq!(std::fs::read(session.join("raw.bin")).unwrap(), packet.data);

        let index_body = std::fs::read_to_string(session.join("index.jsonl")).unwrap();
        let index: IndexEntry = serde_json::from_str(index_body.trim()).unwrap();
        assert_eq!(index.kind, Kind::Datagram);
        assert_eq!(index.schema_id.as_deref(), Some(PCAP_PACKET_SCHEMA_ID));
        assert_ne!(index.ts_origin, index.ts_ingest);

        let frames_body = std::fs::read_to_string(session.join("frames.jsonl")).unwrap();
        let frame: FrameEntry = serde_json::from_str(frames_body.trim()).unwrap();
        assert_eq!(
            frame.record.schema_id.as_deref(),
            Some(PCAP_PACKET_SCHEMA_ID)
        );
        assert_eq!(frame.record.fields["raw_off"], index.off);
        assert_eq!(frame.record.fields["raw_len"], index.len);
        assert_eq!(frame.record.fields["captured_len"], packet.captured_len);
        assert_eq!(frame.record.fields["original_len"], packet.original_len);
        assert_eq!(frame.record.fields["protocol"], "ethertype:0x88b5");
        assert_eq!(frame.record.fields["filter"], "ether proto 0x88b5");

        let exported = root.join("out.pcapng");
        pcapng::export(&session, &exported).unwrap();
        assert!(std::fs::metadata(exported).unwrap().len() > 0);

        let ingest_stats = ingest.stats(&sid).unwrap();
        assert_eq!(ingest_stats.frames_in, 1);
        assert_eq!(ingest_stats.bytes_logged, u64::from(packet.captured_len));

        let metrics_frame = metrics_rx.try_recv().unwrap();
        let env = crate::wire::decode(&metrics_frame).unwrap();
        assert_eq!(env.kind, FrameType::Metrics);
        let sid_key = sid.to_string();
        assert_eq!(
            metric_u64(&env.payload, &["pcap", &sid_key, "packets_total"]),
            Some(1)
        );
        assert_eq!(
            metric_u64(&env.payload, &["pcap", &sid_key, "bytes_total"]),
            Some(u64::from(packet.captured_len))
        );
        assert_eq!(
            metric_u64(&env.payload, &["pcap", &sid_key, "capture_queue_depth"]),
            Some(0)
        );
        assert_eq!(metric_u64(&env.payload, &["bytes_in", &sid_key]), Some(18));
    }

    #[tokio::test]
    async fn fake_pcap_packets_can_write_direct_pcapng_artifact() {
        let root = tempdir();
        let session = root.join("session");
        let packet = PcapPacket::new(
            1,
            1_700_000_000_123_456_789,
            18,
            LINKTYPE_ETHERNET,
            0,
            ethernet_packet(),
        );
        let mut config = PcapConfig::new("fake0");
        config.save_mode = PcapSaveMode::Both;
        let source = PcapSource::with_backend(config, FakePcapBackend::new([packet.clone()]));
        let ingest = Arc::new(Ingest::new());
        let sid = Uuid::new_v4();

        let stats = run_pcap_once_notify(
            ingest,
            source,
            &FixedTimeSource::new(),
            Some(session.clone()),
            None,
            Some(sid),
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(stats.raw_frames, 1);
        assert_eq!(stats.decoded_records, 1);
        assert_eq!(std::fs::read(session.join("raw.bin")).unwrap(), packet.data);

        let direct = session.join("capture.pcapng");
        assert!(direct.is_file(), "expected {}", direct.display());
        let imported = root.join("direct-imported");
        PcapngImporter.import(&direct, &imported).await.unwrap();
        assert_eq!(
            std::fs::read(imported.join("raw.bin")).unwrap(),
            packet.data
        );
    }

    fn ethernet_packet() -> Vec<u8> {
        vec![
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x88, 0xb5, 1,
            2, 3, 4,
        ]
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("tracemux-pcap-runner-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn metric_u64(payload: &Value, path: &[&str]) -> Option<u64> {
        let mut current = payload;
        for key in path {
            current = map_get(current, key)?;
        }
        current.as_u64()
    }

    fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
        let Value::Map(entries) = value else {
            return None;
        };
        entries.iter().find_map(|(k, v)| match k {
            Value::String(s) if s.as_str() == Some(key) => Some(v),
            _ => None,
        })
    }
}
