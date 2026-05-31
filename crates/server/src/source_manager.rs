//! Source lifecycle manager.
//!
//! The manager owns spawned source-runner tasks and keeps lifecycle
//! operations (`start`, `stop`, `resume`, `restart`, `remove`, `wait`)
//! separate from the frozen core traits and wire schema.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tracemux_core::classify::{ClassifyingDecoder, LogClassifier};
use tracemux_core::decoder::{utf8_text::Utf8TextDecoder, Decoder};
use tracemux_core::detect::content::{
    detect_content, ContentDetectionReport, ContentDetectionSettings, DetectionMode,
    DEFAULT_MAX_SAMPLE_BYTES,
};
use tracemux_core::framer::{passthrough::PassthroughFramer, Framer};
use tracemux_core::logsink::{fanout::FanoutLogSink, file::FileLogSink, LogSink};
use tracemux_core::session_name::{
    render_session_name, SessionNameParts, DEFAULT_SERVER_SESSION_NAME_PATTERN,
};
use tracemux_core::sink::Sink;
use tracemux_core::source::{
    file::FileSource, http_webhook::HttpWebhookSource, mock::MockSource, mqtt::MqttSource,
    pcap::PcapConfig, pcap::PcapSource, pipe::PipeSource, process::ProcessSource,
    replay::ReplaySource, serial::SerialSource, syslog::SyslogSource, tcp::TcpSource,
    udp::UdpSource, ChannelMeta, ChannelSpec, ControlEvt, Frame, Source,
};
use tracemux_core::time::{system::SystemTimeSource, TimeSource};
use tracemux_core::{ErrorId, TraceMuxError};
use uuid::Uuid;

use crate::ingest::Ingest;
use crate::pcap_runner::run_pcap_once_notify;
use crate::remote_mirror::{self, RemoteTarget};
use crate::runner::{run_source_once_notify, RunnerNotifyOptions, RunnerStats};

type SharedSink = Arc<tokio::sync::Mutex<Box<dyn Sink>>>;
const DETECTION_SAMPLE_TIMEOUT: Duration = Duration::from_millis(250);

/// UI-facing source lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    /// Runner task is currently active.
    Running,
    /// Source is known but no runner task is active.
    Stopped,
    /// Session exists outside this manager's task map.
    Unknown,
}

/// Snapshot returned by the WSS source-list control action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshot {
    /// Stable session id.
    pub sid: Uuid,
    /// Source kind tag.
    pub kind: String,
    /// Human-readable display name.
    pub name: String,
    /// Lifecycle status.
    pub status: SourceStatus,
    /// Known channels. v0.1 runner publishes channel 0.
    pub channels: Vec<u32>,
    /// Bytes recorded by ingest stats.
    pub bytes_in: u64,
    /// Session-dir path when this source is being persisted on disk.
    pub session_dir: Option<PathBuf>,
    /// Effective decoder label used by the server pipeline.
    pub decoder: Option<String>,
    /// Text encoding label when the effective decoder is text-based.
    pub encoding: Option<String>,
    /// Content-detection mode used for this source lifetime.
    pub detection_mode: Option<DetectionMode>,
    /// Content-detection report for this source lifetime.
    pub detection: Option<ContentDetectionReport>,
}

/// Optional per-start pipeline settings for a source.
///
/// These override the manager defaults for one logical source lifetime
/// and are preserved across `resume` / `restart` for spec-backed sources.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceStartOptions {
    /// Classifier used by the text decoder for persisted decoded records.
    pub classifier: Option<LogClassifier>,
    /// Text encoding label passed to `Utf8TextDecoder`.
    pub encoding: Option<String>,
    /// Content-detection mode for this source start.
    pub detection_mode: Option<DetectionMode>,
    /// Session-dir name pattern used when a new session-dir is created.
    pub session_name_pattern: Option<String>,
    /// Optional display label for the registered source session.
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SourcePipelineMetadata {
    decoder: Option<String>,
    encoding: Option<String>,
    detection_mode: Option<DetectionMode>,
    detection: Option<ContentDetectionReport>,
}

/// Serial-port parameters used for detected/bulk startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialPortOptions {
    /// Baud rate, for example `115200`.
    pub baud: u32,
    /// Data bits, normally `8`.
    pub data_bits: u8,
    /// Parity mode (`none`, `even`, or `odd`).
    pub parity: String,
    /// Stop bits (`1` or `2`).
    pub stop_bits: u8,
    /// Flow-control mode (`none`, `hardware`, or `software`).
    pub flow: String,
}

impl Default for SerialPortOptions {
    fn default() -> Self {
        Self {
            baud: 115_200,
            data_bits: 8,
            parity: "none".to_string(),
            stop_bits: 1,
            flow: "none".to_string(),
        }
    }
}

/// Result for one serial port in a bulk start request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialStartOutcome {
    /// Serial port name, for example `COM7` or `/dev/ttyUSB0`.
    pub port: String,
    /// Session id when the port was opened successfully.
    pub sid: Option<Uuid>,
    /// Error message when opening failed.
    pub error: Option<String>,
}

/// Build a serial [`ChannelSpec`] from a detected port and shared options.
#[must_use]
pub fn serial_spec_for_port(port: impl Into<String>, options: &SerialPortOptions) -> ChannelSpec {
    ChannelSpec::Serial {
        port: port.into(),
        baud: options.baud,
        data_bits: options.data_bits,
        parity: options.parity.clone(),
        stop_bits: options.stop_bits,
        flow: options.flow.clone(),
    }
}

/// Tracks running source tasks by session id.
#[derive(Debug)]
pub struct SourceManager {
    ingest: Arc<Ingest>,
    session_root: Option<PathBuf>,
    classifier: RwLock<LogClassifier>,
    encoding: RwLock<String>,
    detection_mode: RwLock<DetectionMode>,
    session_name_pattern: RwLock<String>,
    sources: Mutex<HashMap<Uuid, SourceEntry>>,
}

struct SourceEntry {
    spec: Option<ChannelSpec>,
    session_dir: Option<PathBuf>,
    start_options: SourceStartOptions,
    pipeline: SourcePipelineMetadata,
    handle: Option<JoinHandle<anyhow::Result<RunnerStats>>>,
    sink: Option<SharedSink>,
}

impl fmt::Debug for SourceEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceEntry")
            .field("spec", &self.spec)
            .field("session_dir", &self.session_dir)
            .field("start_options", &self.start_options)
            .field("pipeline", &self.pipeline)
            .field(
                "running",
                &self.handle.as_ref().is_some_and(|h| !h.is_finished()),
            )
            .field("sink", &self.sink.is_some())
            .finish()
    }
}

struct SourceRegistration {
    spec: Option<ChannelSpec>,
    session_dir: Option<PathBuf>,
    start_options: SourceStartOptions,
    pipeline: SourcePipelineMetadata,
    sink: Option<Box<dyn Sink>>,
}

impl SourceRegistration {
    fn ephemeral() -> Self {
        Self {
            spec: None,
            session_dir: None,
            start_options: SourceStartOptions::default(),
            pipeline: SourcePipelineMetadata::default(),
            sink: None,
        }
    }
}

struct PrefetchedSource<S> {
    inner: S,
    prefetched: VecDeque<Frame>,
    opened: bool,
}

impl<S> PrefetchedSource<S> {
    fn unopened(inner: S) -> Self {
        Self {
            inner,
            prefetched: VecDeque::new(),
            opened: false,
        }
    }

    fn opened(inner: S, prefetched: VecDeque<Frame>) -> Self {
        Self {
            inner,
            prefetched,
            opened: true,
        }
    }
}

#[async_trait]
impl<S> Source for PrefetchedSource<S>
where
    S: Source,
{
    async fn open(&mut self) -> tracemux_core::Result<()> {
        if !self.opened {
            self.inner.open().await?;
            self.opened = true;
        }
        Ok(())
    }

    async fn recv(&mut self) -> tracemux_core::Result<Option<Frame>> {
        if let Some(frame) = self.prefetched.pop_front() {
            return Ok(Some(frame));
        }
        self.inner.recv().await
    }

    async fn recv_ctl(&mut self) -> tracemux_core::Result<Option<ControlEvt>> {
        self.inner.recv_ctl().await
    }

    fn metadata(&self) -> ChannelMeta {
        self.inner.metadata()
    }

    async fn close(&mut self) -> tracemux_core::Result<()> {
        self.opened = false;
        self.inner.close().await
    }
}

impl SourceManager {
    /// Construct with shared ingest state.
    #[must_use]
    pub fn new(ingest: Arc<Ingest>) -> Self {
        Self {
            ingest,
            session_root: None,
            classifier: RwLock::new(LogClassifier::new()),
            encoding: RwLock::new("utf-8".to_string()),
            detection_mode: RwLock::new(DetectionMode::Configured),
            session_name_pattern: RwLock::new(DEFAULT_SERVER_SESSION_NAME_PATTERN.to_string()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Construct with a root directory for persistent session-dirs.
    #[must_use]
    pub fn with_session_root(ingest: Arc<Ingest>, session_root: impl Into<PathBuf>) -> Self {
        Self {
            ingest,
            session_root: Some(session_root.into()),
            classifier: RwLock::new(LogClassifier::new()),
            encoding: RwLock::new("utf-8".to_string()),
            detection_mode: RwLock::new(DetectionMode::Configured),
            session_name_pattern: RwLock::new(DEFAULT_SERVER_SESSION_NAME_PATTERN.to_string()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Construct with a root directory and server-side classifier.
    #[must_use]
    pub fn with_session_root_and_classifier(
        ingest: Arc<Ingest>,
        session_root: impl Into<PathBuf>,
        classifier: LogClassifier,
    ) -> Self {
        Self {
            ingest,
            session_root: Some(session_root.into()),
            classifier: RwLock::new(classifier),
            encoding: RwLock::new("utf-8".to_string()),
            detection_mode: RwLock::new(DetectionMode::Configured),
            session_name_pattern: RwLock::new(DEFAULT_SERVER_SESSION_NAME_PATTERN.to_string()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Construct with a root directory, classifier, and text encoding.
    #[must_use]
    pub fn with_session_root_classifier_and_encoding(
        ingest: Arc<Ingest>,
        session_root: impl Into<PathBuf>,
        classifier: LogClassifier,
        encoding: impl Into<String>,
    ) -> Self {
        Self {
            ingest,
            session_root: Some(session_root.into()),
            classifier: RwLock::new(classifier),
            encoding: RwLock::new(encoding.into()),
            detection_mode: RwLock::new(DetectionMode::Configured),
            session_name_pattern: RwLock::new(DEFAULT_SERVER_SESSION_NAME_PATTERN.to_string()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Construct with a root directory, classifier, encoding, and name pattern.
    #[must_use]
    pub fn with_session_root_classifier_encoding_and_pattern(
        ingest: Arc<Ingest>,
        session_root: impl Into<PathBuf>,
        classifier: LogClassifier,
        encoding: impl Into<String>,
        session_name_pattern: impl Into<String>,
    ) -> Self {
        Self {
            ingest,
            session_root: Some(session_root.into()),
            classifier: RwLock::new(classifier),
            encoding: RwLock::new(encoding.into()),
            detection_mode: RwLock::new(DetectionMode::Configured),
            session_name_pattern: RwLock::new(session_name_pattern.into()),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Replace the classifier used for subsequently started sources.
    pub fn set_classifier(&self, classifier: LogClassifier) {
        *self.classifier.write() = classifier;
    }

    /// Snapshot the classifier used for newly started sources.
    #[must_use]
    pub fn classifier(&self) -> LogClassifier {
        self.classifier.read().clone()
    }

    /// Replace the text encoding used for subsequently started sources.
    pub fn set_encoding(&self, encoding: impl Into<String>) {
        *self.encoding.write() = encoding.into();
    }

    /// Snapshot the text encoding used for newly started sources.
    #[must_use]
    pub fn encoding(&self) -> String {
        self.encoding.read().clone()
    }

    /// Replace the content-detection mode used for subsequently started sources.
    pub fn set_detection_mode(&self, detection_mode: DetectionMode) {
        *self.detection_mode.write() = detection_mode;
    }

    /// Snapshot the content-detection mode used for newly started sources.
    #[must_use]
    pub fn detection_mode(&self) -> DetectionMode {
        *self.detection_mode.read()
    }

    /// Replace the session-dir name pattern used for subsequently started sources.
    pub fn set_session_name_pattern(&self, pattern: impl Into<String>) {
        *self.session_name_pattern.write() = pattern.into();
    }

    /// Snapshot the session-dir name pattern used for newly started sources.
    #[must_use]
    pub fn session_name_pattern(&self) -> String {
        self.session_name_pattern.read().clone()
    }

    /// Shared ingest state used by this manager.
    #[must_use]
    pub fn ingest(&self) -> Arc<Ingest> {
        self.ingest.clone()
    }

    /// Root directory where session-dirs are created, if persistence is enabled.
    #[must_use]
    pub fn session_root(&self) -> Option<&Path> {
        self.session_root.as_deref()
    }

    /// Resolve the session-dir persisted for a known source id.
    #[must_use]
    pub fn session_dir_for_sid(&self, sid: Uuid) -> Option<PathBuf> {
        self.sources
            .lock()
            .get(&sid)
            .and_then(|entry| entry.session_dir.clone())
    }

    /// Start a source from a persisted/config-compatible channel spec.
    ///
    /// v0.1 uses the minimal server-default pipeline:
    /// passthrough framer, passthrough decoder, optional file-backed
    /// log sink, and a system clock source.
    pub async fn start_spec(&self, spec: ChannelSpec) -> anyhow::Result<Uuid> {
        self.start_spec_with_options(spec, SourceStartOptions::default())
            .await
    }

    /// Start a source from a channel spec with per-start pipeline options.
    pub async fn start_spec_with_options(
        &self,
        spec: ChannelSpec,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let sid = Uuid::new_v4();
        self.start_spec_with_sid(sid, spec, None, start_options)
            .await
    }

    /// Start one serial source per supplied port and return per-port outcomes.
    ///
    /// A failed port does not stop the rest of the batch, which keeps
    /// `--open-all-serial` useful on hosts where one COM port is busy.
    pub async fn start_serial_ports<I, P>(
        &self,
        ports: I,
        serial_options: &SerialPortOptions,
        start_options: SourceStartOptions,
    ) -> Vec<SerialStartOutcome>
    where
        I: IntoIterator<Item = P>,
        P: Into<String>,
    {
        let mut outcomes = Vec::new();
        for port in ports {
            let port = port.into();
            let spec = serial_spec_for_port(port.clone(), serial_options);
            match self
                .start_spec_with_options(spec, start_options.clone())
                .await
            {
                Ok(sid) => outcomes.push(SerialStartOutcome {
                    port,
                    sid: Some(sid),
                    error: None,
                }),
                Err(err) => outcomes.push(SerialStartOutcome {
                    port,
                    sid: None,
                    error: Some(err.to_string()),
                }),
            }
        }
        outcomes
    }

    /// Detect host serial ports and start one source for each candidate.
    pub async fn start_detected_serial_ports(
        &self,
        serial_options: &SerialPortOptions,
        start_options: SourceStartOptions,
    ) -> Vec<SerialStartOutcome> {
        self.start_serial_ports(
            tracemux_core::detect::serial::list(),
            serial_options,
            start_options,
        )
        .await
    }

    /// Resume a stopped or completed spec-backed source with the same `sid`.
    ///
    /// Unlike [`Self::restart`], this rejects still-running sources.
    pub async fn resume(&self, sid: Uuid) -> anyhow::Result<Uuid> {
        let (spec, session_dir, start_options) = {
            let mut sources = self.sources.lock();
            let entry = sources
                .get_mut(&sid)
                .ok_or_else(|| anyhow::anyhow!("source sid is unknown"))?;
            if entry.handle.as_ref().is_some_and(JoinHandle::is_finished) {
                entry.handle = None;
                entry.sink = None;
            }
            if entry.handle.is_some() {
                anyhow::bail!("source sid is already active");
            }
            let spec = entry
                .spec
                .clone()
                .ok_or_else(|| anyhow::anyhow!("source sid is not resumable"))?;
            (spec, entry.session_dir.clone(), entry.start_options.clone())
        };
        self.start_spec_with_sid(sid, spec, session_dir, start_options)
            .await
    }

    /// Restart a spec-backed source with the same `sid`.
    ///
    /// If a task is still running it is aborted first. The session id
    /// and existing session-dir, if any, are reused so subscribers and
    /// on-disk logs stay attached to the same logical source lifetime.
    pub async fn restart(&self, sid: Uuid) -> anyhow::Result<Uuid> {
        self.restart_with_options(sid, SourceStartOptions::default())
            .await
    }

    /// Restart a spec-backed source and merge new per-start options.
    ///
    /// `None` fields keep the source's previous options, while supplied
    /// fields (for example `encoding`) become the new resume/restart
    /// defaults for that source.
    pub async fn restart_with_options(
        &self,
        sid: Uuid,
        overrides: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let (spec, session_dir, start_options) = {
            let mut sources = self.sources.lock();
            let entry = sources
                .get_mut(&sid)
                .ok_or_else(|| anyhow::anyhow!("source sid is unknown"))?;
            let spec = entry
                .spec
                .clone()
                .ok_or_else(|| anyhow::anyhow!("source sid is not restartable"))?;
            if let Some(handle) = entry.handle.take() {
                handle.abort();
            }
            entry.sink = None;
            (
                spec,
                entry.session_dir.clone(),
                merge_start_options(entry.start_options.clone(), overrides),
            )
        };
        self.start_spec_with_sid(sid, spec, session_dir, start_options)
            .await
    }

    async fn start_spec_with_sid(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let time = SystemTimeSource::new(Uuid::new_v4());
        let session_dir = self.session_dir_for(&spec, session_dir, &start_options);
        match spec.clone() {
            ChannelSpec::Serial { .. } => {
                self.start_serial_spec(sid, spec, time, session_dir, start_options)
                    .await
            }
            ChannelSpec::Tcp { .. } => {
                self.start_tcp_spec(sid, spec, time, session_dir, start_options)
                    .await
            }
            ChannelSpec::Udp { .. } => {
                self.start_udp_spec(sid, spec, time, session_dir, start_options)
                    .await
            }
            ChannelSpec::Pcap { .. } => {
                self.start_pcap_spec(sid, spec, time, session_dir, start_options)
                    .await
            }
            ChannelSpec::Process { .. } => {
                self.start_process_spec(sid, spec, time, session_dir, start_options)
                    .await
            }
            ChannelSpec::Remote { .. } => {
                self.start_remote_spec(sid, spec, time, session_dir, start_options)
            }
            _ => {
                self.start_source_only_spec(sid, spec, time, session_dir, start_options)
                    .await
            }
        }
    }

    async fn start_serial_spec(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let ChannelSpec::Serial {
            port,
            baud,
            data_bits,
            parity,
            stop_bits,
            flow,
        } = spec.clone()
        else {
            unreachable!("start_serial_spec called with non-serial spec");
        };
        let (source, sink) =
            SerialSource::open_duplex(port, baud, data_bits, parity, stop_bits, flow)?;
        self.start_default_pipeline(
            sid,
            spec,
            source,
            time,
            session_dir,
            Some(Box::new(sink)),
            start_options,
        )
        .await
    }

    async fn start_tcp_spec(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let ChannelSpec::Tcp { addr } = spec.clone() else {
            unreachable!("start_tcp_spec called with non-tcp spec");
        };
        let (source, sink) = TcpSource::connect_duplex(addr).await?;
        self.start_default_pipeline(
            sid,
            spec,
            source,
            time,
            session_dir,
            Some(Box::new(sink)),
            start_options,
        )
        .await
    }

    async fn start_udp_spec(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let ChannelSpec::Udp { bind } = spec.clone() else {
            unreachable!("start_udp_spec called with non-udp spec");
        };
        let (source, sink) = UdpSource::bind_duplex(bind).await?;
        self.start_default_pipeline(
            sid,
            spec,
            source,
            time,
            session_dir,
            Some(Box::new(sink)),
            start_options,
        )
        .await
    }

    async fn start_process_spec(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let ChannelSpec::Process { argv } = spec.clone() else {
            unreachable!("start_process_spec called with non-process spec");
        };
        let (source, sink) = ProcessSource::spawn_duplex(argv)?;
        self.start_default_pipeline(
            sid,
            spec,
            source,
            time,
            session_dir,
            Some(Box::new(sink)),
            start_options,
        )
        .await
    }

    async fn start_pcap_spec(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let config = PcapConfig::from_channel_spec(&spec)
            .ok_or_else(|| anyhow::anyhow!("start_pcap_spec called with non-pcap spec"))?;
        self.start_pcap_source_with_sid(
            sid,
            spec,
            PcapSource::new(config),
            time,
            session_dir,
            start_options,
        )
        .await
    }

    async fn start_pcap_source_with_sid(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        source: PcapSource,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let ingest = self.ingest.clone();
        let host = local_host_label();
        let task_session_dir = session_dir.clone();
        let label = start_options.label.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            run_pcap_once_notify(
                ingest,
                source,
                &time,
                task_session_dir,
                Some(tx),
                Some(sid),
                host,
                label,
            )
            .await
        });
        let sid = match rx.await {
            Ok(sid) => sid,
            Err(_) => match handle.await {
                Ok(Err(err)) => {
                    return Err(err).context("pcap source exited before session registration");
                }
                Ok(Ok(stats)) => {
                    return Err(anyhow::anyhow!(
                        "pcap source completed before session registration: {}",
                        stats.sid
                    ));
                }
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "pcap source task join failed before session registration: {err}"
                    ));
                }
            },
        };
        self.sources.lock().insert(
            sid,
            SourceEntry {
                spec: Some(spec),
                session_dir,
                start_options,
                pipeline: SourcePipelineMetadata {
                    decoder: Some("pcap-packet".to_string()),
                    encoding: None,
                    detection_mode: Some(DetectionMode::Off),
                    detection: None,
                },
                handle: Some(handle),
                sink: None,
            },
        );
        Ok(sid)
    }

    #[cfg(test)]
    async fn start_pcap_source_for_test(
        &self,
        spec: ChannelSpec,
        source: PcapSource,
    ) -> anyhow::Result<Uuid> {
        let sid = Uuid::new_v4();
        let start_options = SourceStartOptions::default();
        let session_dir = self.session_dir_for(&spec, None, &start_options);
        self.start_pcap_source_with_sid(
            sid,
            spec,
            source,
            SystemTimeSource::new(Uuid::new_v4()),
            session_dir,
            start_options,
        )
        .await
    }

    async fn start_source_only_spec(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        match spec.clone() {
            ChannelSpec::File { path, follow } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    FileSource::new(path, follow),
                    time,
                    session_dir,
                    None,
                    start_options,
                )
                .await
            }
            ChannelSpec::Pipe { path } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    PipeSource::new(path),
                    time,
                    session_dir,
                    None,
                    start_options,
                )
                .await
            }
            ChannelSpec::Mock { tag } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    MockSource::new(tag),
                    time,
                    session_dir,
                    None,
                    start_options,
                )
                .await
            }
            ChannelSpec::Replay { path } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    ReplaySource::new(path),
                    time,
                    session_dir,
                    None,
                    start_options,
                )
                .await
            }
            ChannelSpec::Syslog { bind } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    SyslogSource::new(bind),
                    time,
                    session_dir,
                    None,
                    start_options,
                )
                .await
            }
            ChannelSpec::Mqtt { broker, topic } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    MqttSource::new(broker, topic),
                    time,
                    session_dir,
                    None,
                    start_options,
                )
                .await
            }
            ChannelSpec::HttpWebhook { bind, path } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    HttpWebhookSource::new(bind, path),
                    time,
                    session_dir,
                    None,
                    start_options,
                )
                .await
            }
            other => Err(anyhow::anyhow!(
                "source kind not yet implemented in server: {other:?}"
            )),
        }
    }

    fn start_remote_spec(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid> {
        let ChannelSpec::Remote { url } = spec.clone() else {
            unreachable!("start_remote_spec called with non-remote spec");
        };
        let target = RemoteTarget::parse(&url)?;
        let decoder_label = "remote-mirror";
        let logsink = Self::log_sink_for(sid, &spec, session_dir.as_deref(), decoder_label)?;
        let mut state = tracemux_core::session::registry::SessionState::new(
            kind_tag(&spec),
            target.display_target(),
        );
        state.sid = sid;
        state.label.clone_from(&start_options.label);
        self.ingest.register_session(state);

        let (sink, write_rx) = remote_mirror::RemoteWriteSink::channel();
        let ingest = self.ingest.clone();
        let handle = tokio::spawn(async move {
            remote_mirror::run(ingest, sid, target, logsink, time, write_rx).await
        });
        self.sources.lock().insert(
            sid,
            SourceEntry {
                spec: Some(spec),
                session_dir,
                start_options,
                pipeline: SourcePipelineMetadata {
                    decoder: Some(decoder_label.to_string()),
                    encoding: None,
                    detection_mode: Some(DetectionMode::Off),
                    detection: None,
                },
                handle: Some(handle),
                sink: Some(shared_sink(Box::new(sink))),
            },
        );
        Ok(sid)
    }

    async fn start_default_pipeline<S>(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        source: S,
        time: SystemTimeSource,
        session_dir: Option<PathBuf>,
        sink: Option<Box<dyn Sink>>,
        start_options: SourceStartOptions,
    ) -> anyhow::Result<Uuid>
    where
        S: Source + Send + 'static,
    {
        let configured_encoding = start_options
            .encoding
            .clone()
            .unwrap_or_else(|| self.encoding());
        let classifier = start_options
            .classifier
            .clone()
            .unwrap_or_else(|| self.classifier());
        let detection_mode = self.detection_mode_for(&start_options);
        let (source, detection) = self
            .prefetch_for_detection(source, detection_mode, &configured_encoding, &classifier)
            .await?;
        let encoding = detection.as_ref().map_or_else(
            || configured_encoding.clone(),
            |report| report.effective_encoding.clone(),
        );
        let decoder_label = format!("utf8-text:{encoding}");
        let logsink = Self::log_sink_for(sid, &spec, session_dir.as_deref(), &decoder_label)?;
        let pipeline = SourcePipelineMetadata {
            decoder: Some(decoder_label.clone()),
            encoding: Some(encoding.clone()),
            detection_mode: Some(detection_mode),
            detection,
        };
        self.start_source_with_sid(
            sid,
            source,
            PassthroughFramer,
            ClassifyingDecoder::new(Utf8TextDecoder::new(encoding), classifier),
            logsink,
            time,
            SourceRegistration {
                spec: Some(spec),
                session_dir,
                start_options,
                pipeline,
                sink,
            },
        )
        .await
    }

    async fn prefetch_for_detection<S>(
        &self,
        mut source: S,
        detection_mode: DetectionMode,
        configured_encoding: &str,
        classifier: &LogClassifier,
    ) -> anyhow::Result<(PrefetchedSource<S>, Option<ContentDetectionReport>)>
    where
        S: Source + Send + 'static,
    {
        if !matches!(detection_mode, DetectionMode::Auto | DetectionMode::Suggest) {
            return Ok((PrefetchedSource::unopened(source), None));
        }

        source.open().await?;
        let mut prefetched = VecDeque::new();
        let mut sample = Vec::new();
        while sample.len() < DEFAULT_MAX_SAMPLE_BYTES {
            let next = timeout(DETECTION_SAMPLE_TIMEOUT, source.recv()).await;
            let frame = match next {
                Ok(Ok(Some(frame))) => frame,
                Ok(Ok(None)) | Err(_) => break,
                Ok(Err(err)) => return Err(err.into()),
            };
            if let Some(bytes) = frame_bytes(&frame) {
                let remaining = DEFAULT_MAX_SAMPLE_BYTES.saturating_sub(sample.len());
                sample.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
            }
            prefetched.push_back(frame);
        }

        let settings = ContentDetectionSettings {
            mode: detection_mode,
            configured_encoding: configured_encoding.to_string(),
            classifier: classifier.clone(),
            ..ContentDetectionSettings::default()
        };
        let report = detect_content(&sample, &settings);
        Ok((PrefetchedSource::opened(source, prefetched), Some(report)))
    }

    fn detection_mode_for(&self, start_options: &SourceStartOptions) -> DetectionMode {
        start_options
            .detection_mode
            .unwrap_or_else(|| self.detection_mode())
    }

    fn session_dir_for(
        &self,
        spec: &ChannelSpec,
        reuse: Option<PathBuf>,
        start_options: &SourceStartOptions,
    ) -> Option<PathBuf> {
        reuse.or_else(|| {
            let pattern = start_options
                .session_name_pattern
                .clone()
                .unwrap_or_else(|| self.session_name_pattern());
            self.session_root
                .as_ref()
                .map(|root| root.join(session_dir_name(spec, &pattern)))
        })
    }

    fn log_sink_for(
        sid: Uuid,
        spec: &ChannelSpec,
        session_dir: Option<&Path>,
        decoder_label: &str,
    ) -> anyhow::Result<FanoutLogSink> {
        let Some(dir) = session_dir else {
            return Ok(FanoutLogSink::new(Vec::new()));
        };
        let source = format!("{}:{}", kind_tag(spec), iface_tag(spec));
        let sink = FileLogSink::create_with_labels(
            dir,
            sid,
            Some(source),
            local_host_label(),
            decoder_label,
        )?;
        Ok(FanoutLogSink::new(vec![Box::new(sink)]))
    }

    /// Start one source runner and return its registered session id.
    ///
    /// The returned `sid` is available as soon as the source has opened
    /// and registered its session; the runner continues in the
    /// background until EOF, error, or [`Self::stop`].
    ///
    /// # Errors
    /// Returns an error if the runner exits before registering a
    /// session, or if the registration signal cannot be received.
    pub async fn start_source<S, F, D, L, T>(
        &self,
        source: S,
        framer: F,
        decoder: D,
        logsink: L,
        time: T,
    ) -> anyhow::Result<Uuid>
    where
        S: Source + Send + 'static,
        F: Framer + Send + 'static,
        D: Decoder + Send + 'static,
        L: LogSink + Send + 'static,
        T: TimeSource + Send + Sync + 'static,
    {
        let sid = Uuid::new_v4();
        self.start_source_with_sid(
            sid,
            source,
            framer,
            decoder,
            logsink,
            time,
            SourceRegistration::ephemeral(),
        )
        .await
    }

    async fn start_source_with_sid<S, F, D, L, T>(
        &self,
        sid: Uuid,
        source: S,
        framer: F,
        decoder: D,
        logsink: L,
        time: T,
        registration: SourceRegistration,
    ) -> anyhow::Result<Uuid>
    where
        S: Source + Send + 'static,
        F: Framer + Send + 'static,
        D: Decoder + Send + 'static,
        L: LogSink + Send + 'static,
        T: TimeSource + Send + Sync + 'static,
    {
        let ingest = self.ingest.clone();
        let label = registration.start_options.label.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            run_source_once_notify(
                ingest,
                source,
                framer,
                decoder,
                logsink,
                &time,
                RunnerNotifyOptions {
                    registered: Some(tx),
                    sid_override: Some(sid),
                    label,
                },
            )
            .await
        });
        let sid = match rx.await {
            Ok(sid) => sid,
            Err(_) => match handle.await {
                Ok(Err(err)) => {
                    return Err(err).context("source exited before session registration")
                }
                Ok(Ok(stats)) => {
                    return Err(anyhow::anyhow!(
                        "source completed before session registration: {}",
                        stats.sid
                    ));
                }
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "source task join failed before session registration: {err}"
                    ));
                }
            },
        };
        self.sources.lock().insert(
            sid,
            SourceEntry {
                spec: registration.spec,
                session_dir: registration.session_dir,
                start_options: registration.start_options,
                pipeline: registration.pipeline,
                handle: Some(handle),
                sink: registration.sink.map(shared_sink),
            },
        );
        Ok(sid)
    }

    /// Write bytes back to the sink paired with a running source.
    ///
    /// `target` is currently used by UDP sinks (`host:port`). Other sinks
    /// ignore it. The write path is serialised per session by an async mutex
    /// so frame ordering follows the order in which the server handles write
    /// frames.
    pub async fn write(
        &self,
        sid: Uuid,
        ch: u32,
        body: Bytes,
        target: Option<String>,
    ) -> anyhow::Result<usize> {
        let sink = {
            let mut sources = self.sources.lock();
            let Some(entry) = sources.get_mut(&sid) else {
                return Err(write_error(
                    ErrorId::E2001WireMalformed,
                    "write sid is unknown",
                ));
            };
            if entry.handle.as_ref().is_some_and(JoinHandle::is_finished) {
                entry.handle = None;
                entry.sink = None;
            }
            entry.sink.clone().ok_or_else(|| {
                write_error(
                    ErrorId::E2001WireMalformed,
                    "source does not support write-back or is not running",
                )
            })?
        };
        let bytes = body.len();
        let mut sink = sink.lock().await;
        if let Some(target) = target {
            sink.ctl("udp-next-target", Some(Bytes::from(target)))
                .await?;
        }
        sink.write(body).await?;
        sink.flush().await?;
        tracing::info!(%sid, ch, bytes, "source-manager: write-back complete");
        Ok(bytes)
    }

    /// Wait for a task to finish and remove it from the active task map.
    pub async fn wait(&self, sid: Uuid) -> Option<anyhow::Result<RunnerStats>> {
        let (handle, remove_entry) = {
            let mut sources = self.sources.lock();
            let entry = sources.get_mut(&sid)?;
            let handle = entry.handle.take()?;
            (handle, entry.spec.is_none())
        };
        let result = handle
            .await
            .unwrap_or_else(|err| Err(anyhow::anyhow!("source task join failed: {err}")));
        if let Some(entry) = self.sources.lock().get_mut(&sid) {
            entry.sink = None;
        }
        if remove_entry {
            self.sources.lock().remove(&sid);
        }
        Some(result)
    }

    /// Abort a running source task. The session remains registered so
    /// clients can still query buffered/logged data until removed.
    pub fn stop(&self, sid: Uuid) -> bool {
        let handle = {
            let mut sources = self.sources.lock();
            let Some(entry) = sources.get_mut(&sid) else {
                return false;
            };
            entry.sink = None;
            entry.handle.take()
        };
        let Some(handle) = handle else {
            return false;
        };
        handle.abort();
        true
    }

    /// Stop the task, if still running, and remove its session.
    pub fn remove(&self, sid: Uuid) -> bool {
        let entry = self.sources.lock().remove(&sid);
        let entry_existed = entry.is_some();
        let stopped = entry
            .and_then(|mut e| e.handle.take())
            .is_some_and(|handle| {
                handle.abort();
                true
            });
        let removed = self.ingest.registry.remove(&sid).is_some();
        stopped || removed || entry_existed
    }

    /// Current task ids. Completed tasks remain listed until
    /// [`Self::wait`] or [`Self::reap_finished`] removes them.
    #[must_use]
    pub fn active_ids(&self) -> Vec<Uuid> {
        self.sources
            .lock()
            .iter()
            .filter_map(|(sid, entry)| entry.handle.as_ref().map(|_| *sid))
            .collect()
    }

    /// Snapshot all registered sessions for UI reconnect/list sync.
    #[must_use]
    pub fn list_sources(&self) -> Vec<SourceSnapshot> {
        let sources = self.sources.lock();
        let mut out = Vec::new();
        for sid in self.ingest.registry.ids() {
            let Some(session) = self.ingest.registry.get(&sid) else {
                continue;
            };
            let status = sources.get(&sid).map_or(SourceStatus::Unknown, |entry| {
                if entry.handle.as_ref().is_some_and(|h| !h.is_finished()) {
                    SourceStatus::Running
                } else {
                    SourceStatus::Stopped
                }
            });
            let session_dir = sources
                .get(&sid)
                .and_then(|entry| entry.session_dir.clone());
            let pipeline = sources
                .get(&sid)
                .map_or_else(SourcePipelineMetadata::default, |entry| {
                    entry.pipeline.clone()
                });
            let bytes_in = self
                .ingest
                .stats(&sid)
                .map_or(0, |stats| stats.bytes_logged);
            out.push(SourceSnapshot {
                sid,
                kind: session.kind.clone(),
                name: session
                    .label
                    .clone()
                    .unwrap_or_else(|| session.iface.clone()),
                status,
                channels: vec![0],
                bytes_in,
                session_dir,
                decoder: pipeline.decoder,
                encoding: pipeline.encoding,
                detection_mode: pipeline.detection_mode,
                detection: pipeline.detection,
            });
        }
        out.sort_by_key(|s| s.sid);
        out
    }

    /// Drop completed task handles without awaiting their result.
    pub fn reap_finished(&self) {
        self.sources.lock().retain(|_, entry| {
            if entry.handle.as_ref().is_some_and(JoinHandle::is_finished) {
                entry.handle = None;
                entry.sink = None;
            }
            entry.spec.is_some() || entry.handle.is_some()
        });
    }
}

fn shared_sink(sink: Box<dyn Sink>) -> SharedSink {
    Arc::new(tokio::sync::Mutex::new(sink))
}

fn merge_start_options(
    base: SourceStartOptions,
    overrides: SourceStartOptions,
) -> SourceStartOptions {
    SourceStartOptions {
        classifier: overrides.classifier.or(base.classifier),
        encoding: overrides.encoding.or(base.encoding),
        detection_mode: overrides.detection_mode.or(base.detection_mode),
        session_name_pattern: overrides.session_name_pattern.or(base.session_name_pattern),
        label: overrides.label.or(base.label),
    }
}

fn frame_bytes(frame: &Frame) -> Option<&Bytes> {
    match frame {
        Frame::Bytes(bytes) => Some(bytes),
        Frame::Datagram { data, .. }
        | Frame::Ssh { data, .. }
        | Frame::Visa { data, .. }
        | Frame::Other { data, .. } => Some(data),
        _ => None,
    }
}

fn write_error(id: ErrorId, message: impl Into<String>) -> anyhow::Error {
    TraceMuxError::new(id, message).into()
}

/// Default server session root used by `tracemux serve`.
#[must_use]
pub fn default_session_root() -> PathBuf {
    std::env::var_os("TRACEMUX_SESSION_ROOT")
        .map_or_else(|| PathBuf::from("tracemux-sessions"), PathBuf::from)
}

fn session_dir_name(spec: &ChannelSpec, pattern: &str) -> String {
    let iface = iface_tag(spec);
    let unix_ns = tracemux_core::time::unix_ns_now();
    render_session_name(
        pattern,
        &SessionNameParts {
            prefix: "tracemux",
            kind: kind_tag(spec),
            iface: &iface,
            timestamp: &compact_utc_timestamp(),
            unix_ns,
        },
    )
}

fn compact_utc_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn kind_tag(spec: &ChannelSpec) -> &'static str {
    match spec {
        ChannelSpec::File { .. } => "file",
        ChannelSpec::Tcp { .. } => "tcp",
        ChannelSpec::Udp { .. } => "udp",
        ChannelSpec::Pcap { .. } => "pcap",
        ChannelSpec::Serial { .. } => "serial",
        ChannelSpec::Process { .. } => "process",
        ChannelSpec::Pipe { .. } => "pipe",
        ChannelSpec::Mock { .. } => "mock",
        ChannelSpec::Replay { .. } => "replay",
        ChannelSpec::Syslog { .. } => "syslog",
        ChannelSpec::Mqtt { .. } => "mqtt",
        ChannelSpec::HttpWebhook { .. } => "http-webhook",
        ChannelSpec::Telnet { .. } => "telnet",
        ChannelSpec::Ssh { .. } => "ssh",
        ChannelSpec::Remote { .. } => "remote",
        _ => "other",
    }
}

fn iface_tag(spec: &ChannelSpec) -> String {
    match spec {
        ChannelSpec::Serial { port, .. } => sanitize(port),
        ChannelSpec::Tcp { addr }
        | ChannelSpec::Telnet { addr }
        | ChannelSpec::Ssh { addr, .. } => sanitize(addr),
        ChannelSpec::Udp { bind }
        | ChannelSpec::Syslog { bind }
        | ChannelSpec::HttpWebhook { bind, .. } => sanitize(bind),
        ChannelSpec::Pcap { interface, .. } => sanitize(interface),
        ChannelSpec::File { path, .. } | ChannelSpec::Pipe { path } => {
            let last = std::path::Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file");
            sanitize(last)
        }
        ChannelSpec::Process { argv } => {
            let prog = argv.first().map_or("proc", String::as_str);
            let last = std::path::Path::new(prog)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("proc");
            sanitize(last)
        }
        ChannelSpec::Mqtt { topic, .. } => sanitize(topic),
        ChannelSpec::Mock { tag } => sanitize(tag),
        ChannelSpec::Replay { path } => sanitize(path),
        ChannelSpec::Remote { url } => sanitize(url),
        _ => "iface".to_string(),
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn local_host_label() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use tracemux_core::classify::{ClassificationRule, LogClassifier};
    use tracemux_core::codec::encode_text;
    use tracemux_core::decoder::passthrough::PassthroughDecoder;
    use tracemux_core::framer::line::{Eol, LineFramer};
    use tracemux_core::logsink::fanout::FanoutLogSink;
    use tracemux_core::source::mock::MockSource;
    use tracemux_core::source::ChannelSpec;
    use tracemux_core::time::{ClockQuality, ClockSource, DualTimestamp};

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

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("tracemux-source-manager-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn start_wait_and_remove_mock_source() {
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::new(ingest.clone());
        let source = MockSource::new("manager");
        source.push_bytes(bytes::Bytes::from_static(b"one\ntwo\n"));

        let sid = manager
            .start_source(
                source,
                LineFramer::new(Eol::Lf, 1024),
                PassthroughDecoder::new(),
                FanoutLogSink::new(Vec::new()),
                FixedTimeSource::new(),
            )
            .await
            .unwrap();

        assert!(manager.active_ids().contains(&sid));
        assert!(ingest.registry.get(&sid).is_some());

        let stats = manager.wait(sid).await.unwrap().unwrap();
        assert_eq!(stats.sid, sid);
        assert_eq!(stats.raw_frames, 1);
        assert_eq!(stats.decoded_records, 2);
        assert!(!manager.active_ids().contains(&sid));
        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn start_spec_with_options_sets_session_label() {
        // REQ: FR-CLI-012
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::new(ingest.clone());

        let sid = manager
            .start_spec_with_options(
                ChannelSpec::Mock {
                    tag: "manager".to_string(),
                },
                SourceStartOptions {
                    label: Some("demo source".to_string()),
                    ..SourceStartOptions::default()
                },
            )
            .await
            .unwrap();

        let state = ingest.registry.get(&sid).unwrap();
        assert_eq!(state.label.as_deref(), Some("demo source"));
        let snapshot = manager
            .list_sources()
            .into_iter()
            .find(|source| source.sid == sid)
            .unwrap();
        assert_eq!(snapshot.name, "demo source");

        let _ = manager.wait(sid).await;
        assert!(manager.remove(sid));
    }

    #[tokio::test]
    async fn start_spec_with_session_root_persists_file_source() {
        let root = tempdir();
        let input = root.join("input.log");
        std::fs::write(&input, b"persist\n").unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), &sessions);

        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        let stats = manager.wait(sid).await.unwrap().unwrap();
        assert_eq!(stats.sid, sid);
        assert_eq!(stats.raw_frames, 1);
        assert_eq!(stats.decoded_records, 1);

        let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(session_dirs.len(), 1);
        let dir = &session_dirs[0];
        assert_eq!(std::fs::read(dir.join("raw.bin")).unwrap(), b"persist\n");
        let index = std::fs::read_to_string(dir.join("index.jsonl")).unwrap();
        let index_row: serde_json::Value = serde_json::from_str(index.trim()).unwrap();
        assert_eq!(index_row["sid"], sid.to_string());
        assert_eq!(index_row["kind"], "bytes");
        assert!(index_row["source"].as_str().unwrap().starts_with("file:"));
        let lines = std::fs::read_to_string(dir.join("lines.jsonl")).unwrap();
        assert!(lines.contains("persist"));
        let frames = std::fs::read_to_string(dir.join("frames.jsonl")).unwrap();
        assert!(frames.contains("utf8-text:utf-8"));
        assert!(std::fs::read_to_string(dir.join("meta.toml"))
            .unwrap()
            .contains(&sid.to_string()));

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn start_spec_uses_configured_text_encoding() {
        // REQ: FR-CLI-006
        let root = tempdir();
        let input = root.join("input.log");
        std::fs::write(&input, [0x82, 0xA0]).unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root_classifier_and_encoding(
            ingest.clone(),
            &sessions,
            LogClassifier::new(),
            "shift_jis",
        );

        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(session_dirs.len(), 1);
        let lines = std::fs::read_to_string(session_dirs[0].join("lines.jsonl")).unwrap();
        let line_row: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
        assert_eq!(line_row["text"], "\u{3042}");

        let frames = std::fs::read_to_string(session_dirs[0].join("frames.jsonl")).unwrap();
        assert!(frames.contains("utf8-text:shift_jis"));

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn auto_detection_applies_encoding_and_reports_log_type() {
        // REQ: FR-CLI-011
        let root = tempdir();
        let input = root.join("input.log");
        let (sample, had_errors) = encode_text("E-1001 エラー\n", "shift_jis");
        assert!(!had_errors);
        std::fs::write(&input, sample).unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), &sessions);
        let classifier =
            LogClassifier::from_rules(vec![ClassificationRule::regex(r"E-[0-9]{4}", "error-id")]);

        let sid = manager
            .start_spec_with_options(
                ChannelSpec::File {
                    path: input.to_string_lossy().to_string(),
                    follow: false,
                },
                SourceStartOptions {
                    classifier: Some(classifier),
                    encoding: Some("utf-8".to_string()),
                    detection_mode: Some(DetectionMode::Auto),
                    session_name_pattern: None,
                    label: None,
                },
            )
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        let lines = std::fs::read_to_string(session_dirs[0].join("lines.jsonl")).unwrap();
        assert!(lines.contains("エラー"));

        let snapshot = manager
            .list_sources()
            .into_iter()
            .find(|source| source.sid == sid)
            .unwrap();
        assert_eq!(snapshot.encoding.as_deref(), Some("shift_jis"));
        let detection = snapshot.detection.expect("detection report");
        assert_eq!(detection.mode, DetectionMode::Auto);
        assert_eq!(detection.effective_encoding, "shift_jis");
        assert_eq!(detection.log_type_candidates[0].tag, "error-id");

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn start_spec_with_options_overrides_defaults_and_reuses_on_resume() {
        // REQ: FR-CLI-005
        // REQ: FR-CLI-006
        // REQ: FR-CLI-007
        let root = tempdir();
        let input = root.join("input.log");
        std::fs::write(&input, [b'E', b'R', b'R', b' ', 0x82, 0xA0, b'\n']).unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), &sessions);

        let classifier = LogClassifier::from_rules(vec![ClassificationRule::contains("あ", "jp")]);
        let sid = manager
            .start_spec_with_options(
                ChannelSpec::File {
                    path: input.to_string_lossy().to_string(),
                    follow: false,
                },
                SourceStartOptions {
                    classifier: Some(classifier),
                    encoding: Some("shift_jis".to_string()),
                    detection_mode: None,
                    session_name_pattern: Some("{prefix}-{kind}-{iface}-custom".to_string()),
                    label: None,
                },
            )
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let session = sessions.join("tracemux-file-input.log-custom");
        assert!(session.is_dir(), "expected {}", session.display());
        let lines = std::fs::read_to_string(session.join("lines.jsonl")).unwrap();
        let line_row: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
        assert_eq!(line_row["text"], "ERR あ\n");
        assert_eq!(line_row["tags"], serde_json::json!(["jp"]));
        let frames = std::fs::read_to_string(session.join("frames.jsonl")).unwrap();
        assert!(frames.contains("utf8-text:shift_jis"));

        let snapshot = manager
            .list_sources()
            .into_iter()
            .find(|source| source.sid == sid)
            .unwrap();
        assert_eq!(snapshot.decoder.as_deref(), Some("utf8-text:shift_jis"));
        assert_eq!(snapshot.encoding.as_deref(), Some("shift_jis"));

        let resumed = manager.resume(sid).await.unwrap();
        assert_eq!(resumed, sid);
        manager.wait(sid).await.unwrap().unwrap();
        let resumed_lines = std::fs::read_to_string(session.join("lines.jsonl")).unwrap();
        assert_eq!(resumed_lines.matches("\"tags\":[\"jp\"]").count(), 2);

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn restart_with_options_updates_persisted_encoding() {
        // REQ: FR-WIRE-003
        let root = tempdir();
        let input = root.join("input.log");
        std::fs::write(&input, [0x82, 0xA0]).unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), &sessions);

        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let restarted = manager
            .restart_with_options(
                sid,
                SourceStartOptions {
                    encoding: Some("shift_jis".to_string()),
                    label: None,
                    ..SourceStartOptions::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(restarted, sid);
        manager.wait(sid).await.unwrap().unwrap();

        let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(session_dirs.len(), 1);
        let lines = std::fs::read_to_string(session_dirs[0].join("lines.jsonl")).unwrap();
        assert!(lines.contains("\\uFFFD") || lines.contains("�"));
        assert!(lines.contains("あ"));
        let frames = std::fs::read_to_string(session_dirs[0].join("frames.jsonl")).unwrap();
        assert!(frames.contains("utf8-text:utf-8"));
        assert!(frames.contains("utf8-text:shift_jis"));

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[test]
    fn serial_spec_for_port_uses_bulk_options() {
        // REQ: FR-CLI-008
        let spec = serial_spec_for_port(
            "COM7",
            &SerialPortOptions {
                baud: 9_600,
                data_bits: 7,
                parity: "even".to_string(),
                stop_bits: 2,
                flow: "hardware".to_string(),
            },
        );

        match spec {
            ChannelSpec::Serial {
                port,
                baud,
                data_bits,
                parity,
                stop_bits,
                flow,
            } => {
                assert_eq!(port, "COM7");
                assert_eq!(baud, 9_600);
                assert_eq!(data_bits, 7);
                assert_eq!(parity, "even");
                assert_eq!(stop_bits, 2);
                assert_eq!(flow, "hardware");
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[tokio::test]
    async fn start_spec_uses_configured_session_name_pattern() {
        // REQ: FR-CLI-007
        let root = tempdir();
        let input = root.join("input.log");
        std::fs::write(&input, b"named\n").unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root_classifier_encoding_and_pattern(
            ingest.clone(),
            &sessions,
            LogClassifier::new(),
            "utf-8",
            "{prefix}-{kind}-{iface}",
        );

        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let session = sessions.join("tracemux-file-input.log");
        assert!(session.is_dir(), "expected {}", session.display());
        assert!(session.join("raw.bin").is_file());

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn start_spec_persists_classification_tags() {
        // REQ: FR-CLI-005
        let root = tempdir();
        let input = root.join("input.log");
        std::fs::write(&input, b"ERROR motor stop\n").unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let classifier =
            LogClassifier::from_rules(vec![ClassificationRule::contains("ERROR", "fault")]);
        let manager =
            SourceManager::with_session_root_and_classifier(ingest.clone(), &sessions, classifier);

        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(session_dirs.len(), 1);

        let lines = std::fs::read_to_string(session_dirs[0].join("lines.jsonl")).unwrap();
        let line_row: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
        assert_eq!(line_row["tags"], serde_json::json!(["fault"]));

        let frames = std::fs::read_to_string(session_dirs[0].join("frames.jsonl")).unwrap();
        let frame_row: serde_json::Value = serde_json::from_str(frames.trim()).unwrap();
        assert_eq!(frame_row["record"]["tags"], serde_json::json!(["fault"]));

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn resume_completed_file_source_reuses_sid_and_session_dir() {
        let root = tempdir();
        let input = root.join("input.log");
        std::fs::write(&input, b"again\n").unwrap();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), &sessions);

        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let resumed = manager.resume(sid).await.unwrap();
        assert_eq!(resumed, sid);
        manager.wait(sid).await.unwrap().unwrap();

        let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(session_dirs.len(), 1);
        assert_eq!(
            std::fs::read(session_dirs[0].join("raw.bin")).unwrap(),
            b"again\nagain\n"
        );
        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn list_sources_reports_registry_sessions() {
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::new(ingest.clone());
        let sid = ingest.register_session(tracemux_core::session::registry::SessionState::new(
            "mock", "loopback",
        ));
        ingest.record_frame(sid, 5);

        let list = manager.list_sources();

        assert_eq!(list.len(), 1);
        assert_eq!(list[0].sid, sid);
        assert_eq!(list[0].kind, "mock");
        assert_eq!(list[0].name, "loopback");
        assert_eq!(list[0].status, SourceStatus::Unknown);
        assert_eq!(list[0].channels, vec![0]);
        assert_eq!(list[0].bytes_in, 5);
        assert_eq!(list[0].session_dir, None);
        assert_eq!(list[0].decoder, None);
        assert_eq!(list[0].encoding, None);
    }

    #[tokio::test]
    async fn list_sources_tracks_stop_and_remove_lifecycle() {
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::new(ingest.clone());
        let sid = manager
            .start_spec(ChannelSpec::Udp {
                bind: "127.0.0.1:0".to_string(),
            })
            .await
            .unwrap();

        let running = manager.list_sources();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].sid, sid);
        assert_eq!(running[0].status, SourceStatus::Running);
        assert_eq!(running[0].session_dir, None);
        assert_eq!(running[0].decoder.as_deref(), Some("utf8-text:utf-8"));
        assert_eq!(running[0].encoding.as_deref(), Some("utf-8"));

        assert!(manager.stop(sid));
        let stopped = manager.list_sources();
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].sid, sid);
        assert_eq!(stopped[0].status, SourceStatus::Stopped);
        assert_eq!(stopped[0].decoder.as_deref(), Some("utf8-text:utf-8"));
        assert_eq!(stopped[0].encoding.as_deref(), Some("utf-8"));

        assert!(manager.remove(sid));
        assert!(manager.list_sources().is_empty());
        assert!(ingest.registry.get(&sid).is_none());
    }

    #[tokio::test]
    async fn start_spec_pcap_reports_backend_unavailable_without_registering() {
        // REQ: FR-CLI-PCAP
        // REQ: NFR-PORT-PCAP

        let root = tempdir();
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), root.join("sessions"));

        let err = manager
            .start_spec(ChannelSpec::Pcap {
                interface: "eth0".to_string(),
                display_name: None,
                promiscuous: false,
                snaplen: tracemux_core::source::pcap::DEFAULT_SNAPLEN,
                buffer_bytes: None,
                timeout_ms: tracemux_core::source::pcap::DEFAULT_TIMEOUT_MS,
                immediate: false,
                filter: None,
                save_mode: tracemux_core::source::pcap::PcapSaveMode::Session,
                pcapng_path: None,
                publish_mode: tracemux_core::source::pcap::PcapPublishMode::StatsOnly,
            })
            .await
            .unwrap_err();

        assert!(err
            .chain()
            .any(|cause| cause.to_string().contains("E-1101")));
        assert!(manager.active_ids().is_empty());
        assert!(ingest.registry.ids().is_empty());
    }

    #[tokio::test]
    async fn start_fake_pcap_direct_pcapng_error_does_not_register_session() {
        // REQ: NFR-REL-PCAP

        let root = tempdir();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), &sessions);
        let mut config = tracemux_core::source::pcap::PcapConfig::new("fake0");
        config.save_mode = tracemux_core::source::pcap::PcapSaveMode::Pcapng;
        config.pcapng_path = Some(root.clone());
        let spec = config.clone().into_channel_spec();
        let packet = tracemux_core::source::pcap::PcapPacket::new(
            1,
            1_700_000_000_123_456_789,
            18,
            tracemux_core::packet_summary::LINKTYPE_ETHERNET,
            0,
            ethernet_packet(),
        );
        let source = tracemux_core::source::pcap::PcapSource::with_backend(
            config,
            tracemux_core::source::pcap::FakePcapBackend::new([packet]),
        );

        let err = manager
            .start_pcap_source_for_test(spec, source)
            .await
            .unwrap_err();

        assert!(err
            .chain()
            .any(|cause| cause.to_string().contains("creating direct pcapng")));
        assert!(manager.active_ids().is_empty());
        assert!(ingest.registry.ids().is_empty());
    }

    #[tokio::test]
    async fn start_fake_pcap_source_writes_packet_session_dir() {
        // REQ: FR-LOG-PCAP
        // REQ: FR-EXP-PCAPNG

        let root = tempdir();
        let sessions = root.join("sessions");
        let ingest = Arc::new(Ingest::new());
        let manager = SourceManager::with_session_root(ingest.clone(), &sessions);
        let mut config = tracemux_core::source::pcap::PcapConfig::new("fake0");
        config.filter = Some("ether proto 0x88b5".to_string());
        let spec = config.clone().into_channel_spec();
        let packet = tracemux_core::source::pcap::PcapPacket::new(
            1,
            1_700_000_000_123_456_789,
            18,
            tracemux_core::packet_summary::LINKTYPE_ETHERNET,
            0,
            ethernet_packet(),
        );
        let source = tracemux_core::source::pcap::PcapSource::with_backend(
            config,
            tracemux_core::source::pcap::FakePcapBackend::new([packet.clone()]),
        );

        let sid = manager
            .start_pcap_source_for_test(spec, source)
            .await
            .unwrap();
        let stats = manager.wait(sid).await.unwrap().unwrap();

        assert_eq!(stats.raw_frames, 1);
        assert_eq!(stats.decoded_records, 1);
        let session_dir = manager.session_dir_for_sid(sid).unwrap();
        assert_eq!(
            std::fs::read(session_dir.join("raw.bin")).unwrap(),
            packet.data
        );

        let index_body = std::fs::read_to_string(session_dir.join("index.jsonl")).unwrap();
        let index: tracemux_core::log::index::IndexEntry =
            serde_json::from_str(index_body.trim()).unwrap();
        assert_eq!(index.kind, tracemux_core::log::index::Kind::Datagram);
        assert_eq!(
            index.schema_id.as_deref(),
            Some(tracemux_core::exporter::pcapng::PCAP_PACKET_SCHEMA_ID)
        );
        assert_ne!(index.ts_origin, index.ts_ingest);

        let frames_body = std::fs::read_to_string(session_dir.join("frames.jsonl")).unwrap();
        let frame: tracemux_core::log::frames::FrameEntry =
            serde_json::from_str(frames_body.trim()).unwrap();
        assert_eq!(frame.record.fields["raw_off"], index.off);
        assert_eq!(frame.record.fields["raw_len"], index.len);
        assert_eq!(frame.record.fields["linktype"], packet.linktype);
        assert_eq!(frame.record.fields["interface"], "fake0");
        assert_eq!(frame.record.fields["protocol"], "ethertype:0x88b5");

        let dst = root.join("fake.pcapng");
        tracemux_core::exporter::pcapng::export(&session_dir, &dst).unwrap();
        assert!(std::fs::metadata(dst).unwrap().len() > 0);

        let ingest_stats = ingest.stats(&sid).unwrap();
        assert_eq!(ingest_stats.frames_in, 1);
        assert_eq!(ingest_stats.bytes_logged, u64::from(packet.captured_len));
    }

    fn ethernet_packet() -> Vec<u8> {
        vec![
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x88, 0xb5, 1,
            2, 3, 4,
        ]
    }
}
