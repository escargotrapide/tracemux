//! Source runner: `Source -> Framer -> Decoder -> LogSink/Fanout`.
//!
//! This module owns the first executable vertical slice of the server
//! ingest path. It intentionally keeps the frozen trait surfaces in
//! `wanlogger-core` unchanged: callers pass concrete trait impls, and
//! the runner wires them together for one source lifetime.

use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use rmpv::Value;
use uuid::Uuid;
use wanlogger_core::decoder::Decoder;
use wanlogger_core::framer::Framer;
use wanlogger_core::logsink::{Direction, LogSink};
use wanlogger_core::source::{Frame, Source};
use wanlogger_core::time::{ClockQuality, ClockSource, DualTimestamp, TimeSource};

use crate::ingest::Ingest;
use crate::wire::{encode, Envelope, FrameType};

/// Summary returned after a source reaches EOF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerStats {
    /// Registered session id.
    pub sid: Uuid,
    /// Raw frames received from the source.
    pub raw_frames: u64,
    /// Framed payloads emitted by the framer.
    pub framed: u64,
    /// Decoded records emitted by the decoder.
    pub decoded_records: u64,
}

/// Run one source until EOF.
///
/// Raw source frames are appended to `logsink` and published to the
/// session fan-out as encoded `data` envelopes. Framed payloads are
/// decoded and decoded records are appended to `logsink`.
///
/// # Errors
/// Returns the first error from the source, framer, decoder, log sink,
/// or wire encoder.
pub async fn run_source_once<S, F, D, L, T>(
    ingest: Arc<Ingest>,
    source: S,
    framer: F,
    decoder: D,
    logsink: L,
    time: &T,
) -> anyhow::Result<RunnerStats>
where
    S: Source,
    F: Framer,
    D: Decoder,
    L: LogSink,
    T: TimeSource,
{
    run_source_once_notify(ingest, source, framer, decoder, logsink, time, None, None).await
}

pub(crate) async fn run_source_once_notify<S, F, D, L, T>(
    ingest: Arc<Ingest>,
    mut source: S,
    mut framer: F,
    mut decoder: D,
    mut logsink: L,
    time: &T,
    registered: Option<tokio::sync::oneshot::Sender<Uuid>>,
    sid_override: Option<Uuid>,
) -> anyhow::Result<RunnerStats>
where
    S: Source,
    F: Framer,
    D: Decoder,
    L: LogSink,
    T: TimeSource,
{
    source.open().await?;
    let meta = source.metadata();
    let mut state =
        wanlogger_core::session::registry::SessionState::new(meta.kind.clone(), meta.iface.clone());
    if let Some(sid) = sid_override {
        state.sid = sid;
    }
    let sid = ingest.register_session(state);
    if let Some(tx) = registered {
        let _ = tx.send(sid);
    }
    let source_label = format!("{}:{}", meta.kind, meta.iface);
    let mut stats = RunnerStats {
        sid,
        raw_frames: 0,
        framed: 0,
        decoded_records: 0,
    };
    let mut buf = BytesMut::new();

    while let Some(frame) = source.recv().await? {
        let raw = frame_payload(frame);
        let ts = time.stamp_ingest(time.stamp_origin());
        logsink.append_raw(&ts, Direction::In, raw.clone()).await?;
        stats.raw_frames += 1;

        let wire = encode_data_envelope(sid, 0, stats.raw_frames, &ts, &raw, &source_label)?;
        let _ = ingest.publish_wire(sid, Bytes::from(wire));

        buf.extend_from_slice(&raw);
        while let Some(framed) = framer.poll_frame(&mut buf)? {
            stats.framed += 1;
            if let Some(record) = decoder.decode(framed)? {
                logsink.append_record(&ts, &record).await?;
                stats.decoded_records += 1;
            }
        }
    }

    logsink.commit().await?;
    logsink.close().await?;
    source.close().await?;
    Ok(stats)
}

fn frame_payload(frame: Frame) -> Bytes {
    match frame {
        Frame::Bytes(bytes) => bytes,
        Frame::Datagram { data, .. }
        | Frame::Ssh { data, .. }
        | Frame::Visa { data, .. }
        | Frame::Other { data, .. } => data,
        _ => Bytes::new(),
    }
}

fn encode_data_envelope(
    sid: Uuid,
    ch: u32,
    seq: u64,
    ts: &DualTimestamp,
    body: &Bytes,
    source_label: &str,
) -> anyhow::Result<Vec<u8>> {
    let payload = Value::Map(vec![
        (
            Value::String("ts_origin".into()),
            Value::from(ts.ts_origin_ns),
        ),
        (
            Value::String("ts_ingest".into()),
            Value::from(ts.ts_ingest_ns),
        ),
        (Value::String("mono_ns".into()), Value::from(ts.mono_ns)),
        (
            Value::String("boot_id".into()),
            Value::String(ts.boot_id.to_string().into()),
        ),
        (
            Value::String("node_id".into()),
            Value::String(ts.node_id.to_string().into()),
        ),
        (
            Value::String("clock_offset_ms".into()),
            Value::from(ts.clock_offset_ms),
        ),
        (
            Value::String("clock_quality".into()),
            Value::String(clock_quality_token(ts.clock_quality).into()),
        ),
        (Value::String("drift_ppm".into()), Value::F32(ts.drift_ppm)),
        (
            Value::String("clock_source".into()),
            Value::String(clock_source_token(ts.clock_source).into()),
        ),
        (
            Value::String("sid".into()),
            Value::String(sid.to_string().into()),
        ),
        (Value::String("ch".into()), Value::from(ch)),
        (Value::String("dir".into()), Value::String("in".into())),
        (Value::String("kind".into()), Value::String("bytes".into())),
        (Value::String("body".into()), Value::Binary(body.to_vec())),
        (
            Value::String("source".into()),
            Value::String(source_label.to_string().into()),
        ),
    ]);
    let env = Envelope::new(FrameType::Data, seq, payload)
        .with_sid(sid.to_string())
        .with_ch(ch);
    Ok(encode(&env)?)
}

const fn clock_quality_token(q: ClockQuality) -> &'static str {
    match q {
        ClockQuality::Synced => "synced",
        ClockQuality::BestEffort => "best-effort",
        ClockQuality::Unknown => "unknown",
        ClockQuality::Imported => "imported",
    }
}

const fn clock_source_token(s: ClockSource) -> &'static str {
    match s {
        ClockSource::System => "system",
        ClockSource::Ntp => "ntp",
        ClockSource::Ptp => "ptp",
        ClockSource::Monotonic => "monotonic",
        ClockSource::Imported => "imported",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wanlogger_core::framer::line::{Eol, LineFramer};
    use wanlogger_core::logsink::fanout::FanoutLogSink;
    use wanlogger_core::source::mock::MockSource;
    use wanlogger_core::time::{ClockQuality, ClockSource};

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
                ts_origin_ns: 1,
                ts_ingest_ns: 1,
                mono_ns: 1,
                boot_id: self.id,
                node_id: self.id,
                clock_offset_ms: 0,
                clock_quality: ClockQuality::BestEffort,
                drift_ppm: 0.0,
                clock_source: ClockSource::System,
            }
        }

        fn stamp_ingest(&self, mut origin: DualTimestamp) -> DualTimestamp {
            origin.ts_ingest_ns = 2;
            origin.mono_ns = 2;
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
    async fn mock_source_flows_to_logsink_and_ingest_stats() {
        let ingest = Arc::new(Ingest::new());
        let source = MockSource::new("fixture");
        source.push_bytes(Bytes::from_static(b"alpha\nbeta\n"));

        let stats = run_source_once(
            ingest.clone(),
            source,
            LineFramer::new(Eol::Lf, 1024),
            wanlogger_core::decoder::passthrough::PassthroughDecoder::new(),
            FanoutLogSink::new(Vec::new()),
            &FixedTimeSource::new(),
        )
        .await
        .unwrap();

        assert_eq!(stats.raw_frames, 1);
        assert_eq!(stats.framed, 2);
        assert_eq!(stats.decoded_records, 2);

        let ingest_stats = ingest.stats(&stats.sid).unwrap();
        assert_eq!(ingest_stats.frames_in, 1);
        assert!(ingest.registry.get(&stats.sid).is_some());
    }
}
