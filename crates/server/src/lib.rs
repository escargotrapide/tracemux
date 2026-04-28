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
pub mod tls;
pub mod wire;
pub mod ws;

/// Run the server on `bind`. v0.1 stub — does not actually bind yet.
#[allow(clippy::unused_async)]
pub async fn run(bind: &str, no_auth: bool) -> anyhow::Result<()> {
    tracing::info!(bind, no_auth, "wanlogger-server: v0.1 stub start");
    Ok(())
}
