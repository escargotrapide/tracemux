//! Packet capture [`Source`] model, dependency-free fake backend, and optional
//! Npcap/libpcap backend.
//!
//! The real backend is compiled only with the `pcap-capture` feature so normal
//! CI and developer builds do not require Npcap/libpcap headers or privileges.
//! This module keeps the frozen [`Source`] trait compatible by mapping packets
//! to datagram frames, while [`PcapSource::recv_packet`] preserves pcap metadata
//! for the packet-specific server runner.

// REQ: FR-SRC-PCAP
// REQ: NFR-PORT-PCAP
// REQ: NFR-MAINT-PCAP

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
#[cfg(feature = "pcap-capture")]
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use super::{ChannelMeta, ChannelSpec, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, TraceMuxError};

/// Default snap length used by packet capture configs.
pub const DEFAULT_SNAPLEN: u32 = 65_535;
/// Default capture timeout in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u32 = 1_000;

/// Packet capture persistence mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PcapSaveMode {
    /// Persist to tracemux session-dir only.
    Session,
    /// Write direct pcapng only.
    Pcapng,
    /// Persist to session-dir and direct pcapng.
    Both,
}

impl Default for PcapSaveMode {
    fn default() -> Self {
        Self::Session
    }
}

impl fmt::Display for PcapSaveMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Session => "session",
            Self::Pcapng => "pcapng",
            Self::Both => "both",
        };
        f.write_str(value)
    }
}

impl FromStr for PcapSaveMode {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "session" | "session-dir" => Ok(Self::Session),
            "pcapng" => Ok(Self::Pcapng),
            "both" => Ok(Self::Both),
            other => Err(format!("unsupported pcap save mode: {other}")),
        }
    }
}

/// UI fan-out policy for packet capture sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PcapPublishMode {
    /// Publish status/metrics only, never raw packet data.
    StatsOnly,
    /// Publish a bounded/sample packet stream.
    Sampled,
    /// Publish every packet to the UI pipeline.
    Full,
}

impl Default for PcapPublishMode {
    fn default() -> Self {
        Self::StatsOnly
    }
}

impl fmt::Display for PcapPublishMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::StatsOnly => "stats-only",
            Self::Sampled => "sampled",
            Self::Full => "full",
        };
        f.write_str(value)
    }
}

impl FromStr for PcapPublishMode {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "stats" | "stats-only" => Ok(Self::StatsOnly),
            "sample" | "sampled" => Ok(Self::Sampled),
            "full" | "packet-list" => Ok(Self::Full),
            other => Err(format!("unsupported pcap publish mode: {other}")),
        }
    }
}

/// Configuration for opening a packet capture source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcapConfig {
    /// Backend/interface identifier.
    pub interface: String,
    /// Operator-friendly display name.
    pub display_name: Option<String>,
    /// Whether promiscuous mode should be requested.
    pub promiscuous: bool,
    /// Maximum captured bytes per packet.
    pub snaplen: u32,
    /// Capture buffer size in bytes, if supported by the backend.
    pub buffer_bytes: Option<u32>,
    /// Capture read timeout in milliseconds.
    pub timeout_ms: u32,
    /// Whether immediate mode should be requested.
    pub immediate: bool,
    /// Optional BPF filter string.
    pub filter: Option<String>,
    /// Persistence mode requested for the capture.
    pub save_mode: PcapSaveMode,
    /// Optional direct pcapng output path for `pcapng` / `both` save modes.
    pub pcapng_path: Option<PathBuf>,
    /// UI publish policy.
    pub publish_mode: PcapPublishMode,
}

impl PcapConfig {
    /// Construct a config with safe MVP defaults.
    #[must_use]
    pub fn new(interface: impl Into<String>) -> Self {
        Self {
            interface: interface.into(),
            ..Self::default()
        }
    }

    /// Return the operator-facing interface label.
    #[must_use]
    pub fn iface_label(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&self.interface)
    }

    /// Validate fields that can be checked without opening a real backend.
    pub fn validate(&self) -> Result<()> {
        if self.interface.trim().is_empty() {
            return Err(TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                "pcap interface must not be empty",
            ));
        }
        if self.snaplen == 0 {
            return Err(TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                "pcap snaplen must be greater than zero",
            ));
        }
        if matches!(self.buffer_bytes, Some(0)) {
            return Err(TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                "pcap buffer_bytes must be greater than zero when set",
            ));
        }
        Ok(())
    }

    /// Convert a [`ChannelSpec::Pcap`] into a capture config.
    #[must_use]
    pub fn from_channel_spec(spec: &ChannelSpec) -> Option<Self> {
        let ChannelSpec::Pcap {
            interface,
            display_name,
            promiscuous,
            snaplen,
            buffer_bytes,
            timeout_ms,
            immediate,
            filter,
            save_mode,
            pcapng_path,
            publish_mode,
        } = spec
        else {
            return None;
        };
        Some(Self {
            interface: interface.clone(),
            display_name: display_name.clone(),
            promiscuous: *promiscuous,
            snaplen: *snaplen,
            buffer_bytes: *buffer_bytes,
            timeout_ms: *timeout_ms,
            immediate: *immediate,
            filter: filter.clone(),
            save_mode: *save_mode,
            pcapng_path: pcapng_path.as_ref().map(PathBuf::from),
            publish_mode: *publish_mode,
        })
    }

    /// Convert this config into a persisted/config-compatible channel spec.
    #[must_use]
    pub fn into_channel_spec(self) -> ChannelSpec {
        ChannelSpec::Pcap {
            interface: self.interface,
            display_name: self.display_name,
            promiscuous: self.promiscuous,
            snaplen: self.snaplen,
            buffer_bytes: self.buffer_bytes,
            timeout_ms: self.timeout_ms,
            immediate: self.immediate,
            filter: self.filter,
            save_mode: self.save_mode,
            pcapng_path: self.pcapng_path.map(|path| path.display().to_string()),
            publish_mode: self.publish_mode,
        }
    }
}

impl Default for PcapConfig {
    fn default() -> Self {
        Self {
            interface: String::new(),
            display_name: None,
            promiscuous: false,
            snaplen: DEFAULT_SNAPLEN,
            buffer_bytes: None,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            immediate: false,
            filter: None,
            save_mode: PcapSaveMode::default(),
            pcapng_path: None,
            publish_mode: PcapPublishMode::default(),
        }
    }
}

/// One captured packet plus metadata that cannot fit through [`Frame`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PcapPacket {
    /// Monotonic packet sequence number within the capture.
    pub seq: u64,
    /// Packet-origin timestamp in nanoseconds since Unix epoch.
    pub ts_origin_ns: i64,
    /// Number of bytes captured and stored in [`Self::data`].
    pub captured_len: u32,
    /// Original packet length before snaplen truncation.
    pub original_len: u32,
    /// libpcap/Npcap link type.
    pub linktype: u32,
    /// Capture interface identifier within the eventual pcapng output.
    pub interface_id: u32,
    /// Captured packet bytes.
    pub data: Bytes,
}

impl PcapPacket {
    /// Build a packet from byte-like data and derive `captured_len` from it.
    #[must_use]
    pub fn new(
        seq: u64,
        ts_origin_ns: i64,
        original_len: u32,
        linktype: u32,
        interface_id: u32,
        data: impl AsRef<[u8]>,
    ) -> Self {
        Self::from_bytes(
            seq,
            ts_origin_ns,
            original_len,
            linktype,
            interface_id,
            Bytes::copy_from_slice(data.as_ref()),
        )
    }

    /// Build a packet from an existing [`Bytes`] buffer without copying it.
    #[must_use]
    pub fn from_bytes(
        seq: u64,
        ts_origin_ns: i64,
        original_len: u32,
        linktype: u32,
        interface_id: u32,
        data: Bytes,
    ) -> Self {
        let captured_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        Self {
            seq,
            ts_origin_ns,
            captured_len,
            original_len: original_len.max(captured_len),
            linktype,
            interface_id,
            data,
        }
    }
}

/// Packet capture counters surfaced by backends and the pcap runner.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcapStats {
    /// Packets emitted by the backend.
    pub packets_total: u64,
    /// Captured bytes emitted by the backend.
    pub bytes_total: u64,
    /// Packets dropped by the kernel/backend, when known.
    pub dropped_kernel_total: u64,
    /// Packets dropped inside tracemux, when known.
    pub dropped_app_total: u64,
    /// Capture queue depth, when a backend exposes it.
    pub capture_queue_depth: u64,
    /// Persistence queue depth, when the runner or backend exposes it.
    pub writer_queue_depth: u64,
    /// Last packet-origin timestamp seen by the backend.
    pub last_packet_ts_origin_ns: Option<i64>,
}

/// Backend boundary for packet capture implementations.
#[async_trait]
pub trait PcapBackend: fmt::Debug + Send + Sync {
    /// Open the backend using `config`.
    async fn open(&mut self, config: &PcapConfig) -> Result<()>;
    /// Receive the next packet, or `None` on EOF.
    async fn recv_packet(&mut self) -> Result<Option<PcapPacket>>;
    /// Return current backend counters.
    async fn stats(&self) -> Result<PcapStats>;
    /// Close the backend. Must be idempotent.
    async fn close(&mut self) -> Result<()>;
}

/// Packet capture source.
#[derive(Debug)]
pub struct PcapSource {
    config: PcapConfig,
    backend: Box<dyn PcapBackend>,
    opened: bool,
}

impl PcapSource {
    /// Construct with the default backend for this build.
    ///
    /// Builds without `pcap-capture` use an unavailable backend that reports
    /// `E-1103` on open. Builds with `pcap-capture` use Npcap/libpcap.
    #[must_use]
    pub fn new(config: PcapConfig) -> Self {
        #[cfg(feature = "pcap-capture")]
        {
            Self::with_backend(config, NativePcapBackend::default())
        }
        #[cfg(not(feature = "pcap-capture"))]
        {
            Self::with_backend(config, UnavailablePcapBackend)
        }
    }

    /// Construct with an explicit backend, typically [`FakePcapBackend`] in tests.
    #[must_use]
    pub fn with_backend(config: PcapConfig, backend: impl PcapBackend + 'static) -> Self {
        Self {
            config,
            backend: Box::new(backend),
            opened: false,
        }
    }

    /// Return immutable capture config.
    #[must_use]
    pub fn config(&self) -> &PcapConfig {
        &self.config
    }

    /// Whether the source has been opened and not yet closed.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.opened
    }

    /// Receive the next packet while preserving pcap metadata.
    pub async fn recv_packet(&mut self) -> Result<Option<PcapPacket>> {
        if !self.opened {
            return Err(TraceMuxError::new(
                ErrorId::E1102SourceClosed,
                "pcap source not open",
            ));
        }
        self.backend.recv_packet().await
    }

    /// Return current pcap backend statistics.
    pub async fn stats(&self) -> Result<PcapStats> {
        self.backend.stats().await
    }
}

#[async_trait]
impl Source for PcapSource {
    async fn open(&mut self) -> Result<()> {
        if self.opened {
            return Ok(());
        }
        self.config.validate()?;
        self.backend.open(&self.config).await?;
        self.opened = true;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        self.recv_packet().await.map(|packet| {
            packet.map(|packet| Frame::Datagram {
                src: Some(self.config.interface.clone()),
                data: packet.data,
            })
        })
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(None)
    }

    fn metadata(&self) -> ChannelMeta {
        let mut tags = BTreeMap::new();
        tags.insert("interface".into(), self.config.interface.clone());
        tags.insert("promiscuous".into(), self.config.promiscuous.to_string());
        tags.insert("snaplen".into(), self.config.snaplen.to_string());
        tags.insert("timeout_ms".into(), self.config.timeout_ms.to_string());
        tags.insert("immediate".into(), self.config.immediate.to_string());
        tags.insert("save_mode".into(), self.config.save_mode.to_string());
        tags.insert("publish_mode".into(), self.config.publish_mode.to_string());
        if let Some(display_name) = &self.config.display_name {
            tags.insert("display_name".into(), display_name.clone());
        }
        if let Some(buffer_bytes) = self.config.buffer_bytes {
            tags.insert("buffer_bytes".into(), buffer_bytes.to_string());
        }
        if let Some(filter) = &self.config.filter {
            tags.insert("filter".into(), filter.clone());
        }
        if let Some(path) = &self.config.pcapng_path {
            tags.insert("pcapng_path".into(), path.display().to_string());
        }
        ChannelMeta {
            kind: "pcap".into(),
            iface: self.config.iface_label().to_string(),
            tags,
        }
    }

    async fn close(&mut self) -> Result<()> {
        if self.opened {
            self.backend.close().await?;
        }
        self.opened = false;
        Ok(())
    }
}

/// Dependency-free deterministic packet capture backend for tests.
#[derive(Debug, Clone)]
pub struct FakePcapBackend {
    packets: Vec<PcapPacket>,
    next_index: usize,
    stats: PcapStats,
    opened: bool,
}

impl FakePcapBackend {
    /// Construct a fake backend from a finite packet list.
    #[must_use]
    pub fn new(packets: impl IntoIterator<Item = PcapPacket>) -> Self {
        let packets = packets.into_iter().collect::<Vec<_>>();
        let stats = PcapStats {
            capture_queue_depth: packets.len() as u64,
            ..PcapStats::default()
        };
        Self {
            packets,
            next_index: 0,
            stats,
            opened: false,
        }
    }

    /// Set an initial backend/kernel drop counter.
    #[must_use]
    pub fn with_kernel_drops(mut self, dropped: u64) -> Self {
        self.stats.dropped_kernel_total = dropped;
        self
    }

    /// Append a packet to the fake capture stream.
    pub fn push_packet(&mut self, packet: PcapPacket) {
        self.packets.push(packet);
        if self.opened {
            self.stats.capture_queue_depth = self.remaining_packets() as u64;
        } else {
            self.stats.capture_queue_depth = self.packets.len() as u64;
        }
    }

    fn remaining_packets(&self) -> usize {
        self.packets.len().saturating_sub(self.next_index)
    }
}

impl Default for FakePcapBackend {
    fn default() -> Self {
        Self::new([])
    }
}

#[async_trait]
impl PcapBackend for FakePcapBackend {
    async fn open(&mut self, _config: &PcapConfig) -> Result<()> {
        self.next_index = 0;
        self.stats.packets_total = 0;
        self.stats.bytes_total = 0;
        self.stats.capture_queue_depth = self.packets.len() as u64;
        self.stats.writer_queue_depth = 0;
        self.stats.last_packet_ts_origin_ns = None;
        self.opened = true;
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<Option<PcapPacket>> {
        if !self.opened {
            return Err(TraceMuxError::new(
                ErrorId::E1102SourceClosed,
                "fake pcap backend not open",
            ));
        }
        let packet = match self.packets.get(self.next_index).cloned() {
            Some(packet) => packet,
            None => return Ok(None),
        };
        self.next_index += 1;
        self.stats.packets_total += 1;
        self.stats.bytes_total += u64::from(packet.captured_len);
        self.stats.capture_queue_depth = self.remaining_packets() as u64;
        self.stats.last_packet_ts_origin_ns = Some(packet.ts_origin_ns);
        Ok(Some(packet))
    }

    async fn stats(&self) -> Result<PcapStats> {
        Ok(self.stats.clone())
    }

    async fn close(&mut self) -> Result<()> {
        self.opened = false;
        Ok(())
    }
}

#[cfg(feature = "pcap-capture")]
#[derive(Debug)]
/// Npcap/libpcap-backed packet capture implementation.
pub struct NativePcapBackend {
    state: Option<Arc<Mutex<NativePcapState>>>,
    seq: u64,
    stats: Arc<Mutex<PcapStats>>,
}

#[cfg(feature = "pcap-capture")]
impl Default for NativePcapBackend {
    fn default() -> Self {
        Self {
            state: None,
            seq: 0,
            stats: Arc::new(Mutex::new(PcapStats::default())),
        }
    }
}

#[cfg(feature = "pcap-capture")]
struct NativePcapState {
    capture: pcap::Capture<pcap::Active>,
    linktype: u32,
}

#[cfg(feature = "pcap-capture")]
impl fmt::Debug for NativePcapState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativePcapState")
            .field("linktype", &self.linktype)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "pcap-capture")]
#[derive(Debug)]
struct NativePacket {
    ts_origin_ns: i64,
    captured_len: u32,
    original_len: u32,
    linktype: u32,
    data: Bytes,
}

#[cfg(feature = "pcap-capture")]
#[async_trait]
impl PcapBackend for NativePcapBackend {
    async fn open(&mut self, config: &PcapConfig) -> Result<()> {
        let config = config.clone();
        let state = tokio::task::spawn_blocking(move || open_native_capture(&config))
            .await
            .map_err(|err| {
                task_join_err(ErrorId::E1101SourceOpen, "opening pcap capture", err)
            })??;
        self.state = Some(Arc::new(Mutex::new(state)));
        self.seq = 0;
        let mut stats = self
            .stats
            .lock()
            .map_err(|_| mutex_err(ErrorId::E1001PipelineGeneric, "pcap stats lock poisoned"))?;
        *stats = PcapStats::default();
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<Option<PcapPacket>> {
        let state = self.state.clone().ok_or_else(|| {
            TraceMuxError::new(ErrorId::E1102SourceClosed, "native pcap backend not open")
        })?;
        let Some(packet) = recv_native_packet(state).await? else {
            return Ok(None);
        };

        self.seq = self.seq.saturating_add(1);
        let packet = PcapPacket {
            seq: self.seq,
            ts_origin_ns: packet.ts_origin_ns,
            captured_len: packet.captured_len,
            original_len: packet.original_len.max(packet.captured_len),
            linktype: packet.linktype,
            interface_id: 0,
            data: packet.data,
        };
        let mut stats = self
            .stats
            .lock()
            .map_err(|_| mutex_err(ErrorId::E1001PipelineGeneric, "pcap stats lock poisoned"))?;
        stats.packets_total = stats.packets_total.saturating_add(1);
        stats.bytes_total = stats
            .bytes_total
            .saturating_add(u64::from(packet.captured_len));
        stats.last_packet_ts_origin_ns = Some(packet.ts_origin_ns);
        Ok(Some(packet))
    }

    async fn stats(&self) -> Result<PcapStats> {
        let mut snapshot = self
            .stats
            .lock()
            .map_err(|_| mutex_err(ErrorId::E1001PipelineGeneric, "pcap stats lock poisoned"))?
            .clone();
        let Some(state) = self.state.clone() else {
            return Ok(snapshot);
        };
        let stat = tokio::task::spawn_blocking(move || {
            let mut guard = state.lock().map_err(|_| {
                mutex_err(ErrorId::E1001PipelineGeneric, "pcap capture lock poisoned")
            })?;
            guard
                .capture
                .stats()
                .map_err(|err| pcap_err(ErrorId::E1001PipelineGeneric, "reading pcap stats", err))
        })
        .await
        .map_err(|err| task_join_err(ErrorId::E1001PipelineGeneric, "reading pcap stats", err))??;

        snapshot.packets_total = snapshot.packets_total.max(u64::from(stat.received));
        snapshot.dropped_kernel_total = u64::from(stat.dropped) + u64::from(stat.if_dropped);
        let mut stats = self
            .stats
            .lock()
            .map_err(|_| mutex_err(ErrorId::E1001PipelineGeneric, "pcap stats lock poisoned"))?;
        *stats = snapshot.clone();
        Ok(snapshot)
    }

    async fn close(&mut self) -> Result<()> {
        self.state = None;
        Ok(())
    }
}

#[cfg(feature = "pcap-capture")]
fn open_native_capture(config: &PcapConfig) -> Result<NativePcapState> {
    let snaplen = i32_field(config.snaplen, "snaplen")?;
    let timeout = i32_field(config.timeout_ms, "timeout_ms")?;
    let mut inactive = pcap::Capture::from_device(config.interface.as_str())
        .map_err(|err| pcap_err(ErrorId::E1101SourceOpen, "creating pcap capture", err))?
        .promisc(config.promiscuous)
        .snaplen(snaplen)
        .timeout(timeout);
    if let Some(buffer_bytes) = config.buffer_bytes {
        inactive = inactive.buffer_size(i32_field(buffer_bytes, "buffer_bytes")?);
    }
    inactive = configure_immediate_mode(inactive, config.immediate);

    let mut capture = inactive
        .open()
        .map_err(|err| pcap_err(ErrorId::E1101SourceOpen, "activating pcap capture", err))?;
    if let Some(filter) = config
        .filter
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        capture
            .filter(filter, true)
            .map_err(|err| pcap_err(ErrorId::E1101SourceOpen, "applying pcap BPF filter", err))?;
    }
    let linktype = u32::try_from(capture.get_datalink().0).map_err(|_| {
        TraceMuxError::new(ErrorId::E1101SourceOpen, "pcap datalink type was negative")
    })?;
    Ok(NativePcapState { capture, linktype })
}

#[cfg(all(feature = "pcap-capture", windows))]
fn configure_immediate_mode(
    capture: pcap::Capture<pcap::Inactive>,
    immediate: bool,
) -> pcap::Capture<pcap::Inactive> {
    if immediate {
        capture.immediate_mode(true)
    } else {
        capture
    }
}

#[cfg(all(feature = "pcap-capture", not(windows)))]
fn configure_immediate_mode(
    capture: pcap::Capture<pcap::Inactive>,
    _immediate: bool,
) -> pcap::Capture<pcap::Inactive> {
    // The pcap crate exposes `immediate_mode` on Unix only when the detected
    // libpcap version supports it. Keep this feature build portable and use the
    // configured timeout as the fallback batching control.
    capture
}

#[cfg(feature = "pcap-capture")]
async fn recv_native_packet(state: Arc<Mutex<NativePcapState>>) -> Result<Option<NativePacket>> {
    tokio::task::spawn_blocking(move || loop {
        let mut guard = state
            .lock()
            .map_err(|_| mutex_err(ErrorId::E1001PipelineGeneric, "pcap capture lock poisoned"))?;
        let linktype = guard.linktype;
        match guard.capture.next_packet() {
            Ok(packet) => {
                let data = Bytes::copy_from_slice(packet.data);
                return Ok(Some(NativePacket {
                    ts_origin_ns: packet_header_timestamp_ns(packet.header),
                    captured_len: packet.header.caplen,
                    original_len: packet.header.len,
                    linktype,
                    data,
                }));
            }
            Err(pcap::Error::TimeoutExpired) => {}
            Err(pcap::Error::NoMorePackets) => return Ok(None),
            Err(err) => {
                return Err(pcap_err(
                    ErrorId::E1102SourceClosed,
                    "reading pcap packet",
                    err,
                ));
            }
        }
    })
    .await
    .map_err(|err| task_join_err(ErrorId::E1102SourceClosed, "reading pcap packet", err))?
}

#[cfg(feature = "pcap-capture")]
fn packet_header_timestamp_ns(header: &pcap::PacketHeader) -> i64 {
    timestamp_micros_to_unix_ns(i128::from(header.ts.tv_sec), i128::from(header.ts.tv_usec))
}

#[cfg(feature = "pcap-capture")]
fn timestamp_micros_to_unix_ns(seconds: i128, micros: i128) -> i64 {
    let nanos = seconds
        .saturating_mul(1_000_000_000)
        .saturating_add(micros.saturating_mul(1_000));
    nanos.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

#[cfg(feature = "pcap-capture")]
fn i32_field(value: u32, field: &str) -> Result<i32> {
    i32::try_from(value).map_err(|_| {
        TraceMuxError::new(
            ErrorId::E1101SourceOpen,
            format!("pcap {field} exceeds supported range"),
        )
    })
}

#[cfg(feature = "pcap-capture")]
fn pcap_err(id: ErrorId, context: &'static str, err: pcap::Error) -> TraceMuxError {
    let id = if id == ErrorId::E1101SourceOpen {
        classify_pcap_open_error(context, &err.to_string())
    } else {
        id
    };
    let message = format!("{context}: {err}");
    TraceMuxError::new(id, message).with_source(err)
}

#[cfg(any(feature = "pcap-capture", test))]
fn classify_pcap_open_error(context: &str, message: &str) -> ErrorId {
    if context == "applying pcap BPF filter" {
        return ErrorId::E1105PcapInvalidFilter;
    }

    let message = message.to_ascii_lowercase();
    if message.contains("permission")
        || message.contains("access is denied")
        || message.contains("not permitted")
        || message.contains("privilege")
    {
        return ErrorId::E1104PcapPermissionDenied;
    }

    if (context == "creating pcap capture" || context == "activating pcap capture")
        && (message.contains("no such device")
            || message.contains("device doesn't exist")
            || message.contains("device does not exist")
            || message.contains("not found")
            || message.contains("cannot find")
            || message.contains("can't find"))
    {
        return ErrorId::E1106PcapInterfaceUnavailable;
    }

    ErrorId::E1101SourceOpen
}

#[cfg(feature = "pcap-capture")]
fn task_join_err(id: ErrorId, context: &'static str, err: tokio::task::JoinError) -> TraceMuxError {
    let message = format!("{context}: blocking pcap task failed");
    TraceMuxError::new(id, message).with_source(err)
}

#[cfg(feature = "pcap-capture")]
fn mutex_err(id: ErrorId, context: &'static str) -> TraceMuxError {
    TraceMuxError::new(id, context)
}

#[cfg(not(feature = "pcap-capture"))]
#[derive(Debug, Default)]
struct UnavailablePcapBackend;

#[cfg(not(feature = "pcap-capture"))]
#[async_trait]
impl PcapBackend for UnavailablePcapBackend {
    async fn open(&mut self, _config: &PcapConfig) -> Result<()> {
        Err(TraceMuxError::new(
            ErrorId::E1103PcapBackendUnavailable,
            "pcap capture backend is not available in this build; enable the pcap-capture feature",
        ))
    }

    async fn recv_packet(&mut self) -> Result<Option<PcapPacket>> {
        Err(TraceMuxError::new(
            ErrorId::E1102SourceClosed,
            "pcap capture backend is not open",
        ))
    }

    async fn stats(&self) -> Result<PcapStats> {
        Ok(PcapStats::default())
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_summary::LINKTYPE_ETHERNET;

    #[test]
    fn config_defaults_are_safe() {
        let config = PcapConfig::new("eth0");

        assert_eq!(config.interface, "eth0");
        assert_eq!(config.snaplen, DEFAULT_SNAPLEN);
        assert_eq!(config.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(config.save_mode, PcapSaveMode::Session);
        assert_eq!(config.publish_mode, PcapPublishMode::StatsOnly);
        config.validate().unwrap();
    }

    #[test]
    fn packet_constructor_derives_captured_len() {
        let packet = PcapPacket::new(
            7,
            1_000,
            10,
            LINKTYPE_ETHERNET,
            2,
            Bytes::from_static(b"abc"),
        );

        assert_eq!(packet.captured_len, 3);
        assert_eq!(packet.original_len, 10);
    }

    #[test]
    fn channel_spec_round_trip_preserves_config_fields() {
        let mut config = PcapConfig::new("eth0");
        config.display_name = Some("Ethernet 0".into());
        config.promiscuous = true;
        config.snaplen = 1_500;
        config.buffer_bytes = Some(1_048_576);
        config.timeout_ms = 250;
        config.immediate = true;
        config.filter = Some("tcp port 502".into());
        config.save_mode = PcapSaveMode::Both;
        config.pcapng_path = Some(PathBuf::from("capture.pcapng"));
        config.publish_mode = PcapPublishMode::Sampled;

        let spec = config.clone().into_channel_spec();
        let round_trip = PcapConfig::from_channel_spec(&spec).unwrap();

        assert_eq!(round_trip, config);
    }

    #[test]
    fn modes_parse_from_tokens() {
        assert_eq!(
            "session".parse::<PcapSaveMode>().unwrap(),
            PcapSaveMode::Session
        );
        assert_eq!(
            "pcapng".parse::<PcapSaveMode>().unwrap(),
            PcapSaveMode::Pcapng
        );
        assert_eq!(
            "stats-only".parse::<PcapPublishMode>().unwrap(),
            PcapPublishMode::StatsOnly
        );
        assert_eq!(
            "sampled".parse::<PcapPublishMode>().unwrap(),
            PcapPublishMode::Sampled
        );
    }

    #[cfg(not(feature = "pcap-capture"))]
    #[tokio::test]
    async fn placeholder_backend_reports_unavailable() {
        let mut source = PcapSource::new(PcapConfig::new("eth0"));

        let err = source.open().await.unwrap_err();

        assert_eq!(err.id, ErrorId::E1103PcapBackendUnavailable);
    }

    #[test]
    fn pcap_open_errors_are_classified_for_operator_action() {
        assert_eq!(
            classify_pcap_open_error("applying pcap BPF filter", "syntax error"),
            ErrorId::E1105PcapInvalidFilter
        );
        assert_eq!(
            classify_pcap_open_error("activating pcap capture", "permission denied"),
            ErrorId::E1104PcapPermissionDenied
        );
        assert_eq!(
            classify_pcap_open_error("creating pcap capture", "No such device exists"),
            ErrorId::E1106PcapInterfaceUnavailable
        );
        assert_eq!(
            classify_pcap_open_error("activating pcap capture", "backend refused capture"),
            ErrorId::E1101SourceOpen
        );
    }

    #[cfg(feature = "pcap-capture")]
    #[test]
    fn timestamp_conversion_uses_microsecond_precision() {
        assert_eq!(timestamp_micros_to_unix_ns(1, 234_567), 1_234_567_000);
        assert_eq!(timestamp_micros_to_unix_ns(-1, 0), -1_000_000_000);
    }

    #[cfg(feature = "pcap-capture")]
    #[tokio::test]
    #[ignore = "requires TRACEMUX_PCAP_TEST_IFACE and host capture privileges"]
    async fn native_backend_can_open_env_iface() {
        let Ok(iface) = std::env::var("TRACEMUX_PCAP_TEST_IFACE") else {
            return;
        };
        let mut config = PcapConfig::new(iface);
        config.timeout_ms = 100;
        if let Ok(filter) = std::env::var("TRACEMUX_PCAP_TEST_FILTER") {
            config.filter = Some(filter);
        }
        let mut source = PcapSource::new(config);

        source.open().await.unwrap();
        let _ = source.stats().await.unwrap();
        source.close().await.unwrap();
    }
}
