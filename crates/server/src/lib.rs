//! `wanlogger-server` — axum + rustls + WSS mux + ingest + AI API.
//!
//! See `docs/protocols/wire-protocol.md`. **Critical paths** include
//! `wire.rs`, `auth.rs`, `tls.rs`, `fingerprint.rs`.

#![warn(missing_docs)]

pub mod ai_api;
pub mod audit;
pub mod auth;
pub mod clientlog;
pub mod coalesce;
pub mod fingerprint;
pub mod hold;
pub mod ingest;
pub mod mux;
pub mod panel_priority;
pub mod range;
pub mod ratelimit;
pub mod routes;
pub mod runner;
pub mod source_manager;
pub mod tls;
pub mod wire;
pub mod ws;

/// Run the server on `bind`.
///
/// v0.1 binds an axum HTTP listener serving the public router from
/// [`routes::build`] (`/healthz`, `/readyz`, `/api/version`,
/// `/api/ai/verify`, and the reserved `/api/sessions/{sid}/range`)
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
    use std::sync::Arc;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(bind)
        .await
        .map_err(|e| anyhow::anyhow!("binding {bind}: {e}"))?;
    let local = listener.local_addr()?;
    tracing::info!(%local, no_auth, "wanlogger-server: listening (HTTP, no TLS yet)");

    // FR-WIRE-002: empty verifier means only loopback `--no-auth`
    // works until tokens are provisioned through the CLI.
    let auth = auth::BearerVerifier::new();
    let conns = Arc::new(ratelimit::ConnCounter::new(ratelimit::MAX_CONNS));
    let ingest = Arc::new(ingest::Ingest::new());
    let source_manager = Arc::new(source_manager::SourceManager::with_session_root(
        ingest,
        session_root,
    ));
    let ws_state = ws::WsState::with_source_manager(auth, no_auth, conns, source_manager);

    let app = routes::build().merge(ws::router(ws_state));
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
