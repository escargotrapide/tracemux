//! `tracemux-server` — axum + rustls + WSS mux + ingest + AI API.
//!
//! See `docs/protocols/wire-protocol.md`. **Critical paths** include
//! `wire.rs`, `auth.rs`, `tls.rs`, `fingerprint.rs`.

#![warn(missing_docs)]

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tracemux_core::source::ChannelSpec;

pub mod ai_api;
pub mod annotation_api;
pub mod audit;
pub mod auth;
pub mod clientlog;
pub mod coalesce;
pub mod export_api;
pub mod fingerprint;
pub mod hold;
pub mod ingest;
pub mod mux;
pub mod panel_priority;
mod pcap_runner;
pub mod range;
pub mod ratelimit;
mod remote_mirror;
pub mod routes;
pub mod runner;
pub mod source_manager;
pub mod tls;
pub mod wire;
pub mod ws;

/// Sources that should be opened automatically when `tracemux serve` starts.
#[derive(Debug, Clone, Default)]
pub struct StartupSources {
    /// Named channel specs to open at server startup.
    pub channels: Vec<StartupChannel>,
    /// Serial/COM startup configuration. `None` disables bulk serial startup.
    pub serial: Option<SerialAutostart>,
}

/// One named channel loaded from server startup configuration.
#[derive(Debug, Clone)]
pub struct StartupChannel {
    /// Stable operator-facing channel name from the config file.
    pub name: String,
    /// Optional human label for diagnostics.
    pub label: Option<String>,
    /// Source specification to open.
    pub spec: ChannelSpec,
    /// Operator-declared default local-echo mode (`auto`/`on`/`off`).
    pub local_echo: Option<String>,
    /// Operator-declared default line ending (`auto`/`cr`/`lf`/`crlf`).
    pub newline: Option<String>,
}

/// Serial/COM startup configuration for `tracemux serve`.
#[derive(Debug, Clone, Default)]
pub struct SerialAutostart {
    /// Explicit ports to open. `None` means detect all host serial candidates.
    pub ports: Option<Vec<String>>,
    /// Shared serial parameters used for every opened port.
    pub options: source_manager::SerialPortOptions,
}

/// Security settings used by `tracemux serve`.
#[derive(Debug, Clone, Default)]
pub struct ServerSecurity {
    /// Pre-hashed bearer tokens in argon2id PHC format.
    pub token_phc: Vec<String>,
    /// Files containing one argon2id PHC string per line. Empty lines
    /// and `#` comments are ignored.
    pub token_phc_files: Vec<PathBuf>,
    /// Optional TLS listener configuration. `None` keeps the HTTP/WS
    /// development listener.
    pub tls: Option<TlsServeConfig>,
}

/// TLS settings for the server listener.
#[derive(Debug, Clone)]
pub struct TlsServeConfig {
    /// Directory containing `server.crt` and `server.key`, or where a
    /// self-signed pair should be generated on first start.
    pub dir: PathBuf,
}

/// Runtime options for `tracemux serve` beyond startup source selection.
#[derive(Debug, Clone, Default)]
pub struct ServerRunOptions {
    /// Default content detection mode for startup and WSS-started sources.
    pub detection_mode: tracemux_core::detect::content::DetectionMode,
    /// Security settings for bearer tokens and TLS.
    pub security: ServerSecurity,
    /// Days to keep session-dirs under the session root. `0` disables pruning.
    pub retention_keep_days: u32,
    /// Defaults for HTTP session export when query parameters are omitted.
    pub export_defaults: export_api::ExportDefaults,
    /// WSS subscription delivery tuning.
    pub ws_delivery: ws::WsDeliveryOptions,
}

impl ServerSecurity {
    fn bearer_verifier(&self) -> anyhow::Result<auth::BearerVerifier> {
        let mut verifier = auth::BearerVerifier::new();
        for phc in &self.token_phc {
            let phc = phc.trim();
            if phc.is_empty() {
                continue;
            }
            verifier.add_phc(phc).map_err(|err| {
                anyhow::anyhow!("invalid bearer token PHC supplied on command line: {err}")
            })?;
        }
        for path in &self.token_phc_files {
            let text = std::fs::read_to_string(path)
                .map_err(|err| anyhow::anyhow!("reading {}: {err}", path.display()))?;
            for (line_no, line) in text.lines().enumerate() {
                let phc = line.trim();
                if phc.is_empty() || phc.starts_with('#') {
                    continue;
                }
                verifier.add_phc(phc).map_err(|err| {
                    anyhow::anyhow!(
                        "invalid bearer token PHC in {}:{}: {err}",
                        path.display(),
                        line_no + 1
                    )
                })?;
            }
        }
        Ok(verifier)
    }
}

/// Run the server on `bind`.
///
/// v0.1 binds an axum HTTP listener serving the public router from
/// [`routes::build`] (`/healthz`, `/readyz`, `/api/version`,
/// `/api/ai/verify`, the reserved `/api/sessions/{sid}/range`, and
/// authenticated `/api/sessions/{sid}/export`)
/// merged with the WSS router from [`ws::router`] (`/ws`).
/// TLS termination remains in [`tls`] and is not wired in by this
/// entry point yet.
pub async fn run(bind: &str, no_auth: bool) -> anyhow::Result<()> {
    run_with_session_root(bind, no_auth, source_manager::default_session_root()).await
}

/// Run the server on `bind`, persisting started sources under `session_root`.
pub async fn run_with_session_root(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
) -> anyhow::Result<()> {
    run_with_session_root_and_classifier(
        bind,
        no_auth,
        session_root,
        tracemux_core::classify::LogClassifier::new(),
    )
    .await
}

/// Run the server, persisting started sources and applying classification rules.
pub async fn run_with_session_root_and_classifier(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: tracemux_core::classify::LogClassifier,
) -> anyhow::Result<()> {
    run_with_session_root_classifier_and_encoding(bind, no_auth, session_root, classifier, "utf-8")
        .await
}

/// Run the server with classification rules and a default text encoding.
pub async fn run_with_session_root_classifier_and_encoding(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: tracemux_core::classify::LogClassifier,
    encoding: impl Into<String>,
) -> anyhow::Result<()> {
    run_with_session_root_classifier_encoding_and_pattern(
        bind,
        no_auth,
        session_root,
        classifier,
        encoding,
        tracemux_core::session_name::DEFAULT_SERVER_SESSION_NAME_PATTERN,
    )
    .await
}

/// Run the server with classification, text encoding, and session-dir naming.
pub async fn run_with_session_root_classifier_encoding_and_pattern(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: tracemux_core::classify::LogClassifier,
    encoding: impl Into<String>,
    session_name_pattern: impl Into<String>,
) -> anyhow::Result<()> {
    run_with_session_root_classifier_encoding_pattern_and_startup(
        bind,
        no_auth,
        session_root,
        classifier,
        encoding,
        session_name_pattern,
        StartupSources::default(),
    )
    .await
}

/// Run the server with startup source configuration.
pub async fn run_with_session_root_classifier_encoding_pattern_and_startup(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: tracemux_core::classify::LogClassifier,
    encoding: impl Into<String>,
    session_name_pattern: impl Into<String>,
    startup: StartupSources,
) -> anyhow::Result<()> {
    run_with_session_root_classifier_encoding_pattern_startup_and_security(
        bind,
        no_auth,
        session_root,
        classifier,
        encoding,
        session_name_pattern,
        startup,
        ServerSecurity::default(),
    )
    .await
}

/// Run the server with startup source and security configuration.
pub async fn run_with_session_root_classifier_encoding_pattern_startup_and_security(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: tracemux_core::classify::LogClassifier,
    encoding: impl Into<String>,
    session_name_pattern: impl Into<String>,
    startup: StartupSources,
    security: ServerSecurity,
) -> anyhow::Result<()> {
    run_with_session_root_classifier_encoding_pattern_startup_and_options(
        bind,
        no_auth,
        session_root,
        classifier,
        encoding,
        session_name_pattern,
        startup,
        ServerRunOptions {
            security,
            ..ServerRunOptions::default()
        },
    )
    .await
}

/// Run the server with startup sources plus runtime options.
pub async fn run_with_session_root_classifier_encoding_pattern_startup_and_options(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: tracemux_core::classify::LogClassifier,
    encoding: impl Into<String>,
    session_name_pattern: impl Into<String>,
    startup: StartupSources,
    options: ServerRunOptions,
) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tokio::net::TcpListener;

    let auth = options.security.bearer_verifier()?;
    let token_hashes = auth.len();
    let conns = Arc::new(ratelimit::ConnCounter::new(ratelimit::MAX_CONNS));
    let ingest = Arc::new(ingest::Ingest::new());
    let session_root = session_root.into();
    std::fs::create_dir_all(&session_root)?;
    prune_old_session_dirs(&session_root, options.retention_keep_days)?;
    let audit = Arc::new(audit::AuditLog::create(&session_root)?);
    let annotation_store = Arc::new(annotation_api::AnnotationStore::open(&session_root)?);
    let source_manager = Arc::new(
        source_manager::SourceManager::with_session_root_classifier_encoding_and_pattern(
            ingest,
            session_root,
            classifier,
            encoding,
            session_name_pattern,
        ),
    );
    source_manager.set_detection_mode(options.detection_mode);
    start_configured_sources(&source_manager, &startup).await;
    let export_state =
        export_api::ExportRouteState::new(source_manager.clone(), Arc::new(auth.clone()), no_auth)
            .with_defaults(options.export_defaults);
    let annotation_state = annotation_api::AnnotationRouteState::new(
        annotation_store,
        Arc::new(auth.clone()),
        no_auth,
    );
    let ws_state = ws::WsState::with_source_manager(auth, no_auth, conns, source_manager)
        .with_audit(audit)
        .with_delivery_options(options.ws_delivery);

    let app = routes::build_with_exports_and_annotations(export_state, annotation_state)
        .merge(ws::router(ws_state));

    if let Some(tls_config) = options.security.tls {
        let listener = std::net::TcpListener::bind(bind)
            .map_err(|e| anyhow::anyhow!("binding {bind}: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| anyhow::anyhow!("setting {bind} nonblocking: {e}"))?;
        let local = listener.local_addr()?;
        let bundle = tls::load_or_generate(&tls_config.dir)?;
        let fingerprint = fingerprint::fingerprint_der(&tls::leaf_cert_der(&bundle)?);
        let rustls = tls::build_server_config(&bundle)?;
        let config = axum_server::tls_rustls::RustlsConfig::from_config(rustls);
        tracing::info!(
            %local,
            no_auth,
            token_hashes,
            tls_dir = %tls_config.dir.display(),
            %fingerprint,
            "tracemux-server: listening (HTTPS/WSS)"
        );
        axum_server::from_tcp_rustls(listener, config)?
            .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
            .await?;
    } else {
        let listener = TcpListener::bind(bind)
            .await
            .map_err(|e| anyhow::anyhow!("binding {bind}: {e}"))?;
        let local = listener.local_addr()?;
        tracing::info!(
            %local,
            no_auth,
            token_hashes,
            "tracemux-server: listening (HTTP/WS)"
        );
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RetentionPruneStats {
    scanned: usize,
    removed: usize,
}

fn prune_old_session_dirs(
    session_root: &Path,
    keep_days: u32,
) -> anyhow::Result<RetentionPruneStats> {
    prune_old_session_dirs_at(session_root, keep_days, SystemTime::now())
}

fn prune_old_session_dirs_at(
    session_root: &Path,
    keep_days: u32,
    now: SystemTime,
) -> anyhow::Result<RetentionPruneStats> {
    if keep_days == 0 {
        return Ok(RetentionPruneStats::default());
    }
    let cutoff = now
        .checked_sub(Duration::from_secs(
            u64::from(keep_days).saturating_mul(86_400),
        ))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut stats = RetentionPruneStats::default();
    for entry in std::fs::read_dir(session_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry.file_name() == std::ffi::OsStr::new(".tracemux") {
            continue;
        }
        let path = entry.path();
        let meta = path.join("meta.toml");
        if !meta.is_file() {
            continue;
        }
        stats.scanned += 1;
        let modified = meta.metadata()?.modified()?;
        if modified < cutoff {
            std::fs::remove_dir_all(&path).map_err(|err| {
                anyhow::anyhow!("removing expired session-dir {}: {err}", path.display())
            })?;
            stats.removed += 1;
        }
    }
    if stats.scanned > 0 || stats.removed > 0 {
        tracing::info!(
            keep_days,
            scanned = stats.scanned,
            removed = stats.removed,
            "tracemux-server: retention prune complete"
        );
    }
    Ok(stats)
}

async fn start_configured_sources(
    source_manager: &source_manager::SourceManager,
    startup: &StartupSources,
) {
    start_configured_channels(source_manager, &startup.channels).await;
    let Some(serial) = &startup.serial else {
        return;
    };
    let ports = serial
        .ports
        .clone()
        .unwrap_or_else(tracemux_core::detect::serial::list);
    if ports.is_empty() {
        tracing::warn!(
            "tracemux-server: --open-all-serial requested but no serial ports were detected"
        );
        return;
    }
    let start_options = source_manager_default_start_options(source_manager);
    let outcomes = source_manager
        .start_serial_ports(ports, &serial.options, start_options)
        .await;
    let mut ok = 0usize;
    let mut failed = 0usize;
    for outcome in outcomes {
        if let Some(sid) = outcome.sid {
            ok += 1;
            tracing::info!(port = %outcome.port, %sid, "tracemux-server: serial source started");
        } else {
            failed += 1;
            tracing::warn!(
                port = %outcome.port,
                error = %outcome.error.unwrap_or_else(|| "unknown error".to_string()),
                "tracemux-server: serial source failed to start"
            );
        }
    }
    tracing::info!(ok, failed, "tracemux-server: serial bulk startup complete");
}

async fn start_configured_channels(
    source_manager: &source_manager::SourceManager,
    channels: &[StartupChannel],
) {
    if channels.is_empty() {
        return;
    }
    let start_options = source_manager_default_start_options(source_manager);
    let mut ok = 0usize;
    let mut failed = 0usize;
    for channel in channels {
        let mut channel_options = start_options.clone();
        channel_options.label.clone_from(&channel.label);
        channel_options.local_echo.clone_from(&channel.local_echo);
        channel_options.newline.clone_from(&channel.newline);
        match source_manager
            .start_spec_with_options(channel.spec.clone(), channel_options)
            .await
        {
            Ok(sid) => {
                ok += 1;
                tracing::info!(
                    channel = %channel.name,
                    label = channel.label.as_deref(),
                    %sid,
                    "tracemux-server: configured source started"
                );
            }
            Err(err) => {
                failed += 1;
                tracing::warn!(
                    channel = %channel.name,
                    label = channel.label.as_deref(),
                    error = %err,
                    "tracemux-server: configured source failed to start"
                );
            }
        }
    }
    tracing::info!(
        ok,
        failed,
        "tracemux-server: configured source startup complete"
    );
}

fn source_manager_default_start_options(
    source_manager: &source_manager::SourceManager,
) -> source_manager::SourceStartOptions {
    source_manager::SourceStartOptions {
        classifier: Some(source_manager.classifier()),
        encoding: Some(source_manager.encoding()),
        detection_mode: Some(source_manager.detection_mode()),
        monitor_window_secs: None,
        session_name_pattern: Some(source_manager.session_name_pattern()),
        label: None,
        local_echo: None,
        newline: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_loads_inline_and_file_phc() {
        // REQ: FR-WIRE-002
        let inline = auth::hash_token("inline").unwrap();
        let file_token = auth::hash_token("file").unwrap();
        let dir = std::env::temp_dir().join(format!(
            "tracemux-security-{}-{}",
            std::process::id(),
            tracemux_core::time::unix_ns_now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tokens.phc");
        std::fs::write(&file, format!("# comment\n\n{file_token}\n")).unwrap();

        let security = ServerSecurity {
            token_phc: vec![inline],
            token_phc_files: vec![file],
            tls: None,
        };
        let verifier = security.bearer_verifier().unwrap();
        assert_eq!(verifier.len(), 2);
        assert!(verifier.verify("inline").is_ok());
        assert!(verifier.verify("file").is_ok());
        assert!(verifier.verify("nope").is_err());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn retention_prune_removes_only_expired_session_dirs() {
        // REQ: FR-CLI-012
        let root = tempdir("tracemux-retention");
        let old_session = root.join("old-session");
        let not_session = root.join("not-session");
        let metadata = root.join(".tracemux");
        std::fs::create_dir_all(&old_session).unwrap();
        std::fs::create_dir_all(&not_session).unwrap();
        std::fs::create_dir_all(&metadata).unwrap();
        std::fs::write(old_session.join("meta.toml"), b"sid = 'old'\n").unwrap();
        std::fs::write(not_session.join("note.txt"), b"keep\n").unwrap();
        std::fs::write(metadata.join("meta.toml"), b"not a session\n").unwrap();

        let future = SystemTime::now()
            .checked_add(Duration::from_secs(2 * 86_400))
            .unwrap();
        let stats = prune_old_session_dirs_at(&root, 1, future).unwrap();

        assert_eq!(stats.scanned, 1);
        assert_eq!(stats.removed, 1);
        assert!(!old_session.exists());
        assert!(not_session.exists());
        assert!(metadata.exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn retention_prune_zero_days_is_disabled() {
        // REQ: FR-CLI-012
        let root = tempdir("tracemux-retention-disabled");
        let session = root.join("session");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("meta.toml"), b"sid = 'keep'\n").unwrap();

        let future = SystemTime::now()
            .checked_add(Duration::from_secs(365 * 86_400))
            .unwrap();
        let stats = prune_old_session_dirs_at(&root, 0, future).unwrap();

        assert_eq!(stats, RetentionPruneStats::default());
        assert!(session.exists());
        std::fs::remove_dir_all(root).ok();
    }

    fn tempdir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            tracemux_core::time::unix_ns_now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
