//! Remote WSS mirror for `ChannelSpec::Remote`.
//!
//! This module keeps remote aggregation outside the frozen `Source` trait:
//! the edge server remains the source of truth for the physical COM/TCP
//! channel, while the central server subscribes over `tracemux.v1`, stamps
//! central ingest time, persists the mirrored bytes, and proxies write-back.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context as _};
use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use rmpv::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tracemux_core::logsink::{Direction, LogSink};
use tracemux_core::secret::{resolve, KeyringResolver, SecretRef};
use tracemux_core::sink::Sink;
use tracemux_core::time::{ClockQuality, ClockSource, DualTimestamp, TimeSource};
use tracemux_core::{ErrorId, Result as CoreResult, TraceMuxError};
use uuid::Uuid;

use crate::ingest::Ingest;
use crate::runner::{encode_data_envelope, RunnerStats};
use crate::wire::{decode, encode, Envelope, FrameType};

const SUBPROTOCOL: &str = "tracemux.v1";
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

type PendingWrites = HashMap<u64, oneshot::Sender<anyhow::Result<()>>>;

#[derive(Debug, Clone)]
pub(crate) struct RemoteTarget {
    url: String,
    remote_sid: Uuid,
    remote_ch: u32,
    token: Option<String>,
}

impl RemoteTarget {
    pub(crate) fn parse(url: &str) -> anyhow::Result<Self> {
        let sid = query_param(url, "sid").ok_or_else(|| {
            anyhow::anyhow!("remote url must include ?sid=<uuid> for the edge session")
        })?;
        let remote_sid = Uuid::parse_str(&sid).context("remote url sid must be a UUID")?;
        let remote_ch = query_param(url, "ch")
            .as_deref()
            .unwrap_or("0")
            .parse::<u32>()
            .context("remote url ch must be a u32")?;
        let token = remote_token(url)?;
        Ok(Self {
            url: url.to_string(),
            remote_sid,
            remote_ch,
            token,
        })
    }

    pub(crate) fn display_target(&self) -> String {
        format!("{}/{}", sanitized_ws_url(&self.url), self.remote_sid)
    }
}

pub(crate) struct RemoteWriteSink {
    tx: mpsc::Sender<RemoteWriteRequest>,
    pending_target: Option<String>,
    timeout: Duration,
}

impl RemoteWriteSink {
    pub(crate) fn channel() -> (Self, RemoteWriteReceiver) {
        let (tx, rx) = mpsc::channel(128);
        (
            Self {
                tx,
                pending_target: None,
                timeout: WRITE_TIMEOUT,
            },
            rx,
        )
    }
}

#[async_trait]
impl Sink for RemoteWriteSink {
    async fn write(&mut self, data: Bytes) -> CoreResult<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        let request = RemoteWriteRequest {
            body: data,
            target: self.pending_target.take(),
            ack: ack_tx,
        };
        self.tx
            .send(request)
            .await
            .map_err(|_| sink_err("remote mirror task is not running"))?;
        match tokio::time::timeout(self.timeout, ack_rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(err))) => Err(sink_err(format!("remote write failed: {err}"))),
            Ok(Err(_)) => Err(sink_err("remote write acknowledgement channel closed")),
            Err(_) => Err(sink_err("remote write timed out waiting for edge ack")),
        }
    }

    async fn ctl(&mut self, kind: &str, data: Option<Bytes>) -> CoreResult<()> {
        if kind == "udp-next-target" {
            self.pending_target = data
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .filter(|s| !s.is_empty());
        }
        Ok(())
    }

    async fn flush(&mut self) -> CoreResult<()> {
        Ok(())
    }

    async fn close(&mut self) -> CoreResult<()> {
        Ok(())
    }
}

pub(crate) type RemoteWriteReceiver = mpsc::Receiver<RemoteWriteRequest>;

pub(crate) struct RemoteWriteRequest {
    body: Bytes,
    target: Option<String>,
    ack: oneshot::Sender<anyhow::Result<()>>,
}

pub(crate) async fn run<L, T>(
    ingest: Arc<Ingest>,
    local_sid: Uuid,
    target: RemoteTarget,
    mut logsink: L,
    time: T,
    mut write_rx: RemoteWriteReceiver,
) -> anyhow::Result<RunnerStats>
where
    L: LogSink + Send + 'static,
    T: TimeSource + Send + Sync + 'static,
{
    let mut request = target
        .url
        .as_str()
        .into_client_request()
        .context("building remote websocket request")?;
    let protocol = match target.token.as_deref() {
        Some(token) if !token.is_empty() => format!("{SUBPROTOCOL}, bearer.{token}"),
        _ => SUBPROTOCOL.to_string(),
    };
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_str(&protocol).context("invalid remote websocket subprotocol header")?,
    );

    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("connecting remote websocket")?;
    let sub = Envelope::new(FrameType::Sub, 1, Value::Map(vec![]))
        .with_sid(target.remote_sid.to_string())
        .with_ch(target.remote_ch);
    socket
        .send(Message::Binary(
            encode(&sub).context("encoding remote sub frame")?,
        ))
        .await
        .context("sending remote sub frame")?;

    let source_label = format!(
        "remote:{}:{}",
        sanitized_ws_url(&target.url),
        target.remote_ch
    );
    let mut stats = RunnerStats {
        sid: local_sid,
        raw_frames: 0,
        framed: 0,
        decoded_records: 0,
    };
    let mut next_write_seq = 2_u64;
    let mut pending_writes = PendingWrites::new();

    loop {
        tokio::select! {
            maybe_write = write_rx.recv() => {
                let Some(write) = maybe_write else {
                    continue;
                };
                let seq = next_write_seq;
                next_write_seq = next_write_seq.saturating_add(1);
                let env = remote_write_envelope(seq, &target, &write.body, write.target);
                socket
                    .send(Message::Binary(encode(&env).context("encoding remote write frame")?))
                    .await
                    .context("sending remote write frame")?;
                pending_writes.insert(seq, write.ack);
            }
            maybe_msg = socket.next() => {
                let Some(msg) = maybe_msg else {
                    fail_pending(&mut pending_writes, "remote websocket closed");
                    break;
                };
                let msg = msg.context("receiving remote websocket frame")?;
                let Message::Binary(bytes) = msg else {
                    if matches!(msg, Message::Close(_)) {
                        fail_pending(&mut pending_writes, "remote websocket closed");
                        break;
                    }
                    continue;
                };
                let env = decode(&bytes).context("decoding remote websocket frame")?;
                if env.kind == FrameType::Ctl {
                    resolve_write_ack(&env, &mut pending_writes);
                    fail_on_ctl_error(&env)?;
                    continue;
                }
                if !is_target_data(&env, target.remote_sid, target.remote_ch) {
                    continue;
                }
                let Some(body) = map_binary(&env.payload, "body") else {
                    continue;
                };
                stats.raw_frames += 1;
                let remote_ts = timestamp_from_payload(&env.payload, &time);
                let ts = time.stamp_ingest(remote_ts);
                logsink.append_raw(&ts, Direction::In, body.clone()).await?;
                let wire = encode_data_envelope(
                    local_sid,
                    0,
                    stats.raw_frames,
                    &ts,
                    &body,
                    &source_label,
                )?;
                let _ = ingest.publish_wire(local_sid, Bytes::from(wire));
            }
        }
    }

    logsink.commit().await?;
    logsink.close().await?;
    Ok(stats)
}

fn remote_write_envelope(
    seq: u64,
    target: &RemoteTarget,
    body: &[u8],
    write_target: Option<String>,
) -> Envelope {
    let mut payload = vec![(Value::String("body".into()), Value::Binary(body.to_vec()))];
    if let Some(write_target) = write_target {
        payload.push((
            Value::String("target".into()),
            Value::String(write_target.into()),
        ));
    }
    Envelope::new(FrameType::Write, seq, Value::Map(payload))
        .with_sid(target.remote_sid.to_string())
        .with_ch(target.remote_ch)
}

fn resolve_write_ack(env: &Envelope, pending: &mut PendingWrites) {
    let Some(tx) = pending.remove(&env.seq) else {
        return;
    };
    if payload_str(&env.payload, "event") == Some("write_ack") {
        let _ = tx.send(Ok(()));
        return;
    }
    if payload_str(&env.payload, "event") == Some("error") {
        let message = payload_str(&env.payload, "message").unwrap_or("remote write failed");
        let _ = tx.send(Err(anyhow::anyhow!(message.to_string())));
        return;
    }
    pending.insert(env.seq, tx);
}

fn fail_pending(pending: &mut PendingWrites, message: &str) {
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err(anyhow::anyhow!(message.to_string())));
    }
}

fn fail_on_ctl_error(env: &Envelope) -> anyhow::Result<()> {
    if payload_str(&env.payload, "event") != Some("error") {
        return Ok(());
    }
    let message = payload_str(&env.payload, "message").unwrap_or("remote mirror failed");
    bail!(message.to_string())
}

fn is_target_data(env: &Envelope, sid: Uuid, ch: u32) -> bool {
    env.kind == FrameType::Data
        && env.sid.as_deref() == Some(&sid.to_string())
        && env.ch == Some(ch)
}

fn timestamp_from_payload<T>(payload: &Value, time: &T) -> DualTimestamp
where
    T: TimeSource,
{
    let fallback = time.stamp_origin();
    DualTimestamp {
        ts_origin_ns: map_i64(payload, "ts_origin").unwrap_or(fallback.ts_origin_ns),
        ts_ingest_ns: map_i64(payload, "ts_ingest").unwrap_or(fallback.ts_ingest_ns),
        mono_ns: map_u64(payload, "mono_ns").unwrap_or(fallback.mono_ns),
        boot_id: map_uuid(payload, "boot_id").unwrap_or(fallback.boot_id),
        node_id: map_uuid(payload, "node_id").unwrap_or(fallback.node_id),
        clock_offset_ms: map_i64(payload, "clock_offset_ms")
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or(fallback.clock_offset_ms),
        clock_quality: map_str(payload, "clock_quality")
            .and_then(clock_quality)
            .unwrap_or(fallback.clock_quality),
        drift_ppm: map_f32(payload, "drift_ppm").unwrap_or(fallback.drift_ppm),
        clock_source: map_str(payload, "clock_source")
            .and_then(clock_source)
            .unwrap_or(fallback.clock_source),
    }
}

fn remote_token(url: &str) -> anyhow::Result<Option<String>> {
    if let Some(var) = query_param(url, "token_env") {
        let token = std::env::var(&var)
            .with_context(|| format!("remote token environment variable {var} is not set"))?;
        return Ok(Some(token));
    }
    if let Some(secret_ref) = query_param(url, "token_secret") {
        let secret_ref = SecretRef::parse(&secret_ref)
            .ok_or_else(|| anyhow::anyhow!("token_secret must be secret://<name>"))?;
        let secret = resolve(&KeyringResolver::new(), &secret_ref)
            .map_err(|err| anyhow::anyhow!("resolving token_secret: {err}"))?;
        return Ok(Some(secret.0));
    }
    Ok(None)
}

fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next().unwrap_or_default();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(key) == name {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1]);
                let lo = hex_val(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn sanitized_ws_url(url: &str) -> String {
    let without_fragment = url.split('#').next().unwrap_or(url);
    without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment)
        .to_string()
}

fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let Value::Map(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(k, v)| {
        if k.as_str() == Some(key) {
            Some(v)
        } else {
            None
        }
    })
}

fn map_binary(value: &Value, key: &str) -> Option<Bytes> {
    match map_get(value, key)? {
        Value::Binary(bytes) => Some(Bytes::copy_from_slice(bytes)),
        _ => None,
    }
}

fn map_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    map_get(value, key).and_then(Value::as_str)
}

fn payload_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    map_str(value, key)
}

fn map_i64(value: &Value, key: &str) -> Option<i64> {
    map_get(value, key).and_then(Value::as_i64)
}

fn map_u64(value: &Value, key: &str) -> Option<u64> {
    map_get(value, key).and_then(Value::as_u64)
}

fn map_f32(value: &Value, key: &str) -> Option<f32> {
    match map_get(value, key)? {
        Value::F32(v) => Some(*v),
        Value::F64(v) => Some(*v as f32),
        _ => None,
    }
}

fn map_uuid(value: &Value, key: &str) -> Option<Uuid> {
    Uuid::parse_str(map_str(value, key)?).ok()
}

fn clock_quality(value: &str) -> Option<ClockQuality> {
    match value {
        "synced" => Some(ClockQuality::Synced),
        "best-effort" => Some(ClockQuality::BestEffort),
        "unknown" => Some(ClockQuality::Unknown),
        "imported" => Some(ClockQuality::Imported),
        _ => None,
    }
}

fn clock_source(value: &str) -> Option<ClockSource> {
    match value {
        "system" => Some(ClockSource::System),
        "ntp" => Some(ClockSource::Ntp),
        "ptp" => Some(ClockSource::Ptp),
        "monotonic" => Some(ClockSource::Monotonic),
        "imported" => Some(ClockSource::Imported),
        _ => None,
    }
}

fn sink_err(message: impl Into<String>) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1001PipelineGeneric, message)
}

#[cfg(test)]
mod tests {
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
            origin.boot_id = self.id;
            origin.clock_offset_ms = 7;
            origin
        }

        fn boot_id(&self) -> Uuid {
            self.id
        }

        fn node_id(&self) -> Uuid {
            self.id
        }
    }

    #[test]
    fn parse_remote_url_requires_sid_and_reads_ch() {
        // REQ: FR-REMOTE-001
        let sid = Uuid::new_v4();
        let target =
            RemoteTarget::parse(&format!("ws://127.0.0.1:8080/ws?sid={sid}&ch=4")).unwrap();
        assert_eq!(target.remote_sid, sid);
        assert_eq!(target.remote_ch, 4);
        assert!(target.token.is_none());
    }

    #[test]
    fn parse_remote_url_reads_token_from_env() {
        // REQ: FR-REMOTE-001
        let sid = Uuid::new_v4();
        let var = format!("TRACEMUX_TEST_REMOTE_TOKEN_{}", Uuid::new_v4().simple());
        std::env::set_var(&var, "edge-token");
        let target =
            RemoteTarget::parse(&format!("ws://127.0.0.1:8080/ws?sid={sid}&token_env={var}"))
                .unwrap();
        assert_eq!(target.token.as_deref(), Some("edge-token"));
        std::env::remove_var(var);
    }

    #[test]
    fn timestamp_preserves_remote_origin_and_stamps_local_ingest() {
        // REQ: FR-REMOTE-001
        let remote_node = Uuid::new_v4();
        let payload = Value::Map(vec![
            (Value::String("ts_origin".into()), Value::from(100_i64)),
            (Value::String("ts_ingest".into()), Value::from(150_i64)),
            (Value::String("mono_ns".into()), Value::from(1_u64)),
            (
                Value::String("boot_id".into()),
                Value::String(Uuid::new_v4().to_string().into()),
            ),
            (
                Value::String("node_id".into()),
                Value::String(remote_node.to_string().into()),
            ),
            (
                Value::String("clock_quality".into()),
                Value::String("synced".into()),
            ),
            (
                Value::String("clock_source".into()),
                Value::String("ntp".into()),
            ),
            (Value::String("drift_ppm".into()), Value::F32(1.5)),
        ]);

        let time = FixedTimeSource::new();
        let ts = time.stamp_ingest(timestamp_from_payload(&payload, &time));
        assert_eq!(ts.ts_origin_ns, 100);
        assert_eq!(ts.ts_ingest_ns, 20);
        assert_eq!(ts.node_id, remote_node);
        assert_eq!(ts.clock_quality, ClockQuality::Synced);
        assert_eq!(ts.clock_source, ClockSource::Ntp);
        assert_eq!(ts.clock_offset_ms, 7);
    }
}
