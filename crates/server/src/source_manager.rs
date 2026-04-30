//! Source lifecycle manager.
//!
//! The manager owns spawned source-runner tasks and keeps lifecycle
//! operations (`start`, `stop`, `resume`, `restart`, `remove`, `wait`)
//! separate from the frozen core traits and wire schema.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use parking_lot::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;
use wanlogger_core::decoder::{passthrough::PassthroughDecoder, Decoder};
use wanlogger_core::framer::{passthrough::PassthroughFramer, Framer};
use wanlogger_core::logsink::{fanout::FanoutLogSink, file::FileLogSink, LogSink};
use wanlogger_core::source::{
    file::FileSource, http_webhook::HttpWebhookSource, mock::MockSource, mqtt::MqttSource,
    pipe::PipeSource, process::ProcessSource, replay::ReplaySource, serial::SerialSource,
    syslog::SyslogSource, tcp::TcpSource, udp::UdpSource, ChannelSpec, Source,
};
use wanlogger_core::time::{system::SystemTimeSource, TimeSource};

use crate::ingest::Ingest;
use crate::runner::{run_source_once_notify, RunnerStats};

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
}

/// Tracks running source tasks by session id.
#[derive(Debug)]
pub struct SourceManager {
    ingest: Arc<Ingest>,
    session_root: Option<PathBuf>,
    sources: Mutex<HashMap<Uuid, SourceEntry>>,
}

#[derive(Debug)]
struct SourceEntry {
    spec: Option<ChannelSpec>,
    session_dir: Option<PathBuf>,
    handle: Option<JoinHandle<anyhow::Result<RunnerStats>>>,
}

struct SourceRegistration {
    spec: Option<ChannelSpec>,
    session_dir: Option<PathBuf>,
}

impl SourceRegistration {
    const fn ephemeral() -> Self {
        Self {
            spec: None,
            session_dir: None,
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
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// Construct with a root directory for persistent session-dirs.
    #[must_use]
    pub fn with_session_root(ingest: Arc<Ingest>, session_root: impl Into<PathBuf>) -> Self {
        Self {
            ingest,
            session_root: Some(session_root.into()),
            sources: Mutex::new(HashMap::new()),
        }
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

    /// Start a source from a persisted/config-compatible channel spec.
    ///
    /// v0.1 uses the minimal server-default pipeline:
    /// passthrough framer, passthrough decoder, optional file-backed
    /// log sink, and a system clock source.
    pub async fn start_spec(&self, spec: ChannelSpec) -> anyhow::Result<Uuid> {
        let sid = Uuid::new_v4();
        self.start_spec_with_sid(sid, spec, None).await
    }

    /// Resume a stopped or completed spec-backed source with the same `sid`.
    ///
    /// Unlike [`Self::restart`], this rejects still-running sources.
    pub async fn resume(&self, sid: Uuid) -> anyhow::Result<Uuid> {
        let (spec, session_dir) = {
            let mut sources = self.sources.lock();
            let entry = sources
                .get_mut(&sid)
                .ok_or_else(|| anyhow::anyhow!("source sid is unknown"))?;
            if entry.handle.as_ref().is_some_and(JoinHandle::is_finished) {
                entry.handle = None;
            }
            if entry.handle.is_some() {
                anyhow::bail!("source sid is already active");
            }
            let spec = entry
                .spec
                .clone()
                .ok_or_else(|| anyhow::anyhow!("source sid is not resumable"))?;
            (spec, entry.session_dir.clone())
        };
        self.start_spec_with_sid(sid, spec, session_dir).await
    }

    /// Restart a spec-backed source with the same `sid`.
    ///
    /// If a task is still running it is aborted first. The session id
    /// and existing session-dir, if any, are reused so subscribers and
    /// on-disk logs stay attached to the same logical source lifetime.
    pub async fn restart(&self, sid: Uuid) -> anyhow::Result<Uuid> {
        let (spec, session_dir) = {
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
            (spec, entry.session_dir.clone())
        };
        self.start_spec_with_sid(sid, spec, session_dir).await
    }

    async fn start_spec_with_sid(
        &self,
        sid: Uuid,
        spec: ChannelSpec,
        session_dir: Option<PathBuf>,
    ) -> anyhow::Result<Uuid> {
        let time = SystemTimeSource::new(Uuid::new_v4());
        let session_dir = self.session_dir_for(&spec, session_dir);
        match spec.clone() {
            ChannelSpec::Serial {
                port,
                baud,
                data_bits,
                parity,
                stop_bits,
                flow,
            } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    SerialSource::new(port, baud, data_bits, parity, stop_bits, flow),
                    time,
                    session_dir,
                )
                .await
            }
            ChannelSpec::Tcp { addr } => {
                self.start_default_pipeline(sid, spec, TcpSource::new(addr), time, session_dir)
                    .await
            }
            ChannelSpec::Udp { bind } => {
                self.start_default_pipeline(sid, spec, UdpSource::new(bind), time, session_dir)
                    .await
            }
            ChannelSpec::File { path, follow } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    FileSource::new(path, follow),
                    time,
                    session_dir,
                )
                .await
            }
            ChannelSpec::Pipe { path } => {
                self.start_default_pipeline(sid, spec, PipeSource::new(path), time, session_dir)
                    .await
            }
            ChannelSpec::Process { argv } => {
                self.start_default_pipeline(sid, spec, ProcessSource::new(argv), time, session_dir)
                    .await
            }
            ChannelSpec::Mock { tag } => {
                self.start_default_pipeline(sid, spec, MockSource::new(tag), time, session_dir)
                    .await
            }
            ChannelSpec::Replay { path } => {
                self.start_default_pipeline(sid, spec, ReplaySource::new(path), time, session_dir)
                    .await
            }
            ChannelSpec::Syslog { bind } => {
                self.start_default_pipeline(sid, spec, SyslogSource::new(bind), time, session_dir)
                    .await
            }
            ChannelSpec::Mqtt { broker, topic } => {
                self.start_default_pipeline(
                    sid,
                    spec,
                    MqttSource::new(broker, topic),
                    time,
                    session_dir,
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
    ) -> anyhow::Result<Uuid>
    where
        S: Source + Send + 'static,
    {
        let logsink = Self::log_sink_for(sid, &spec, session_dir.as_deref())?;
        self.start_source_with_sid(
            sid,
            source,
            PassthroughFramer,
            PassthroughDecoder::new(),
            logsink,
            time,
            SourceRegistration {
                spec: Some(spec),
                session_dir,
            },
        )
        .await
    }

    fn session_dir_for(&self, spec: &ChannelSpec, reuse: Option<PathBuf>) -> Option<PathBuf> {
        reuse.or_else(|| {
            self.session_root
                .as_ref()
                .map(|root| root.join(session_dir_name(spec)))
        })
    }

    fn log_sink_for(
        sid: Uuid,
        spec: &ChannelSpec,
        session_dir: Option<&Path>,
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
            "passthrough",
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
                handle: Some(handle),
            },
        );
        Ok(sid)
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
            }
            entry.spec.is_some() || entry.handle.is_some()
        });
    }
}

/// Default server session root used by `wanlogger serve`.
#[must_use]
pub fn default_session_root() -> PathBuf {
    std::env::var_os("WANLOGGER_SESSION_ROOT")
        .map_or_else(|| PathBuf::from("wanlogger-sessions"), PathBuf::from)
}

fn session_dir_name(spec: &ChannelSpec) -> String {
    format!(
        "wanlogger_{}_{}_{}",
        kind_tag(spec),
        iface_tag(spec),
        wanlogger_core::time::unix_ns_now()
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
        assert!(frames.contains("passthrough"));
        assert!(std::fs::read_to_string(dir.join("meta.toml"))
            .unwrap()
            .contains(&sid.to_string()));

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
