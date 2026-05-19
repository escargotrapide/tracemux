//! Source lifecycle manager.
//!
//! The manager owns spawned source-runner tasks and keeps lifecycle
//! operations (`start`, `stop`, `resume`, `restart`, `remove`, `wait`)
//! separate from the frozen core traits and wire schema.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;
use wanlogger_core::classify::{ClassifyingDecoder, LogClassifier};
use wanlogger_core::decoder::{utf8_text::Utf8TextDecoder, Decoder};
use wanlogger_core::framer::{passthrough::PassthroughFramer, Framer};
use wanlogger_core::logsink::{fanout::FanoutLogSink, file::FileLogSink, LogSink};
use wanlogger_core::session_name::{
    render_session_name, SessionNameParts, DEFAULT_SERVER_SESSION_NAME_PATTERN,
};
use wanlogger_core::sink::Sink;
use wanlogger_core::source::{
    file::FileSource, http_webhook::HttpWebhookSource, mock::MockSource, mqtt::MqttSource,
    pipe::PipeSource, process::ProcessSource, replay::ReplaySource, serial::SerialSource,
    syslog::SyslogSource, tcp::TcpSource, udp::UdpSource, ChannelSpec, Source,
};
use wanlogger_core::time::{system::SystemTimeSource, TimeSource};
use wanlogger_core::{ErrorId, WanloggerError};

use crate::ingest::Ingest;
use crate::runner::{run_source_once_notify, RunnerStats};

type SharedSink = Arc<tokio::sync::Mutex<Box<dyn Sink>>>;

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
    /// Session-dir name pattern used when a new session-dir is created.
    pub session_name_pattern: Option<String>,
}

/// Tracks running source tasks by session id.
#[derive(Debug)]
pub struct SourceManager {
    ingest: Arc<Ingest>,
    session_root: Option<PathBuf>,
    classifier: RwLock<LogClassifier>,
    encoding: RwLock<String>,
    session_name_pattern: RwLock<String>,
    sources: Mutex<HashMap<Uuid, SourceEntry>>,
}

struct SourceEntry {
    spec: Option<ChannelSpec>,
    session_dir: Option<PathBuf>,
    start_options: SourceStartOptions,
    handle: Option<JoinHandle<anyhow::Result<RunnerStats>>>,
    sink: Option<SharedSink>,
}

impl fmt::Debug for SourceEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceEntry")
            .field("spec", &self.spec)
            .field("session_dir", &self.session_dir)
            .field("start_options", &self.start_options)
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
    sink: Option<Box<dyn Sink>>,
}

impl SourceRegistration {
    fn ephemeral() -> Self {
        Self {
            spec: None,
            session_dir: None,
            start_options: SourceStartOptions::default(),
            sink: None,
        }
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
            (spec, entry.session_dir.clone(), entry.start_options.clone())
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
            ChannelSpec::Process { .. } => {
                self.start_process_spec(sid, spec, time, session_dir, start_options)
                    .await
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
        let encoding = start_options
            .encoding
            .clone()
            .unwrap_or_else(|| self.encoding());
        let classifier = start_options
            .classifier
            .clone()
            .unwrap_or_else(|| self.classifier());
        let decoder_label = format!("utf8-text:{encoding}");
        let logsink = Self::log_sink_for(sid, &spec, session_dir.as_deref(), &decoder_label)?;
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
                sink,
            },
        )
        .await
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
        let (tx, rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            run_source_once_notify(
                ingest,
                source,
                framer,
                decoder,
                logsink,
                &time,
                Some(tx),
                Some(sid),
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

fn write_error(id: ErrorId, message: impl Into<String>) -> anyhow::Error {
    WanloggerError::new(id, message).into()
}

/// Default server session root used by `wanlogger serve`.
#[must_use]
pub fn default_session_root() -> PathBuf {
    std::env::var_os("WANLOGGER_SESSION_ROOT")
        .map_or_else(|| PathBuf::from("wanlogger-sessions"), PathBuf::from)
}

fn session_dir_name(spec: &ChannelSpec, pattern: &str) -> String {
    let iface = iface_tag(spec);
    let unix_ns = wanlogger_core::time::unix_ns_now();
    render_session_name(
        pattern,
        &SessionNameParts {
            prefix: "wanlogger",
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
    use wanlogger_core::classify::{ClassificationRule, LogClassifier};
    use wanlogger_core::decoder::passthrough::PassthroughDecoder;
    use wanlogger_core::framer::line::{Eol, LineFramer};
    use wanlogger_core::logsink::fanout::FanoutLogSink;
    use wanlogger_core::source::mock::MockSource;
    use wanlogger_core::source::ChannelSpec;
    use wanlogger_core::time::{ClockQuality, ClockSource, DualTimestamp};

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
        let p = std::env::temp_dir().join(format!("wanlogger-source-manager-{}", Uuid::new_v4()));
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
                    session_name_pattern: Some("{prefix}-{kind}-{iface}-custom".to_string()),
                },
            )
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();

        let session = sessions.join("wanlogger-file-input.log-custom");
        assert!(session.is_dir(), "expected {}", session.display());
        let lines = std::fs::read_to_string(session.join("lines.jsonl")).unwrap();
        let line_row: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
        assert_eq!(line_row["text"], "ERR あ\n");
        assert_eq!(line_row["tags"], serde_json::json!(["jp"]));
        let frames = std::fs::read_to_string(session.join("frames.jsonl")).unwrap();
        assert!(frames.contains("utf8-text:shift_jis"));

        let resumed = manager.resume(sid).await.unwrap();
        assert_eq!(resumed, sid);
        manager.wait(sid).await.unwrap().unwrap();
        let resumed_lines = std::fs::read_to_string(session.join("lines.jsonl")).unwrap();
        assert_eq!(resumed_lines.matches("\"tags\":[\"jp\"]").count(), 2);

        assert!(manager.remove(sid));
        assert!(ingest.registry.get(&sid).is_none());
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

        let session = sessions.join("wanlogger-file-input.log");
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
        let sid = ingest.register_session(wanlogger_core::session::registry::SessionState::new(
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

        assert!(manager.stop(sid));
        let stopped = manager.list_sources();
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].sid, sid);
        assert_eq!(stopped[0].status, SourceStatus::Stopped);

        assert!(manager.remove(sid));
        assert!(manager.list_sources().is_empty());
        assert!(ingest.registry.get(&sid).is_none());
    }
}
