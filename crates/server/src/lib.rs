//! `wanlogger-server` — axum + rustls + WSS mux + ingest + AI API.
//!
//! See `docs/protocols/wire-protocol.md`. **Critical paths** include
//! `wire.rs`, `auth.rs`, `tls.rs`, `fingerprint.rs`.

#![warn(missing_docs)]

use std::path::PathBuf;

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

/// Sources that should be opened automatically when `wanlogger serve` starts.
#[derive(Debug, Clone, Default)]
pub struct StartupSources {
    /// Serial/COM startup configuration. `None` disables bulk serial startup.
    pub serial: Option<SerialAutostart>,
}

/// Serial/COM startup configuration for `wanlogger serve`.
#[derive(Debug, Clone, Default)]
pub struct SerialAutostart {
    /// Explicit ports to open. `None` means detect all host serial candidates.
    pub ports: Option<Vec<String>>,
    /// Shared serial parameters used for every opened port.
    pub options: source_manager::SerialPortOptions,
}

/// Security settings used by `wanlogger serve`.
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
        wanlogger_core::classify::LogClassifier::new(),
    )
    .await
}

/// Run the server, persisting started sources and applying classification rules.
pub async fn run_with_session_root_and_classifier(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: wanlogger_core::classify::LogClassifier,
) -> anyhow::Result<()> {
    run_with_session_root_classifier_and_encoding(bind, no_auth, session_root, classifier, "utf-8")
        .await
}

/// Run the server with classification rules and a default text encoding.
pub async fn run_with_session_root_classifier_and_encoding(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: wanlogger_core::classify::LogClassifier,
    encoding: impl Into<String>,
) -> anyhow::Result<()> {
    run_with_session_root_classifier_encoding_and_pattern(
        bind,
        no_auth,
        session_root,
        classifier,
        encoding,
        wanlogger_core::session_name::DEFAULT_SERVER_SESSION_NAME_PATTERN,
    )
    .await
}

/// Run the server with classification, text encoding, and session-dir naming.
pub async fn run_with_session_root_classifier_encoding_and_pattern(
    bind: &str,
    no_auth: bool,
    session_root: impl Into<std::path::PathBuf>,
    classifier: wanlogger_core::classify::LogClassifier,
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
    classifier: wanlogger_core::classify::LogClassifier,
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
    classifier: wanlogger_core::classify::LogClassifier,
    encoding: impl Into<String>,
    session_name_pattern: impl Into<String>,
    startup: StartupSources,
    security: ServerSecurity,
) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tokio::net::TcpListener;

    let auth = security.bearer_verifier()?;
    let token_hashes = auth.len();
    let conns = Arc::new(ratelimit::ConnCounter::new(ratelimit::MAX_CONNS));
    let ingest = Arc::new(ingest::Ingest::new());
    let session_root = session_root.into();
    std::fs::create_dir_all(&session_root)?;
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
    start_configured_sources(&source_manager, &startup).await;
    let export_state =
        export_api::ExportRouteState::new(source_manager.clone(), Arc::new(auth.clone()), no_auth);
    let annotation_state = annotation_api::AnnotationRouteState::new(
        annotation_store,
        Arc::new(auth.clone()),
        no_auth,
    );
    let ws_state =
        ws::WsState::with_source_manager(auth, no_auth, conns, source_manager).with_audit(audit);

    let app = routes::build_with_exports_and_annotations(export_state, annotation_state)
        .merge(ws::router(ws_state));

    if let Some(tls_config) = security.tls {
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
            "wanlogger-server: listening (HTTPS/WSS)"
        );
        axum_server::from_tcp_rustls(listener, config)
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
            "wanlogger-server: listening (HTTP/WS)"
        );
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await?;
    }
    Ok(())
}

async fn start_configured_sources(
    source_manager: &source_manager::SourceManager,
    startup: &StartupSources,
) {
    let Some(serial) = &startup.serial else {
        return;
    };
    let ports = serial
        .ports
        .clone()
        .unwrap_or_else(wanlogger_core::detect::serial::list);
    if ports.is_empty() {
        tracing::warn!(
            "wanlogger-server: --open-all-serial requested but no serial ports were detected"
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
            tracing::info!(port = %outcome.port, %sid, "wanlogger-server: serial source started");
        } else {
            failed += 1;
            tracing::warn!(
                port = %outcome.port,
                error = %outcome.error.unwrap_or_else(|| "unknown error".to_string()),
                "wanlogger-server: serial source failed to start"
            );
        }
    }
    tracing::info!(ok, failed, "wanlogger-server: serial bulk startup complete");
}

fn source_manager_default_start_options(
    source_manager: &source_manager::SourceManager,
) -> source_manager::SourceStartOptions {
    source_manager::SourceStartOptions {
        classifier: Some(source_manager.classifier()),
        encoding: Some(source_manager.encoding()),
        session_name_pattern: Some(source_manager.session_name_pattern()),
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
            "wanlogger-security-{}-{}",
            std::process::id(),
            wanlogger_core::time::unix_ns_now()
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
}
