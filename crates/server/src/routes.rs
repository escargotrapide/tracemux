//! HTTP routes (axum router).
//!
//! v0.1 wires the non-critical, non-WSS surface area:
//!
//! * `GET /healthz`       ? liveness probe (always 200)
//! * `GET /readyz`        ? readiness probe
//! * `GET /api/version`   ? server build version JSON
//! * `GET /api/ai/verify` ? last `target/ai-verify.json` (see [`crate::ai_api`])
//! * `GET /api/sessions/{sid}/range` ? historical raw.bin streaming (see [`crate::range`])
//!
//! WSS (`/ws`) and TLS termination remain in the critical-path
//! modules and are not wired here.

use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

/// Public version metadata returned by `/api/version`.
#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    /// Cargo package version of `wanlogger-server`.
    pub version: &'static str,
    /// Wire-protocol subprotocol token.
    pub subprotocol: &'static str,
    /// Log-format version string.
    pub log_format: &'static str,
}

impl VersionInfo {
    /// Compile-time snapshot.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            subprotocol: "wanlogger.v1",
            log_format: "1.0.0",
        }
    }
}

/// Build the public router.
pub fn build() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/version", get(version))
        .route("/api/ai/verify", get(crate::ai_api::verify))
        .route(
            "/api/sessions/{sid}/range",
            get(crate::range::range_handler),
        )
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ok"
}

async fn version() -> Json<VersionInfo> {
    Json(VersionInfo::current())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_info_is_compile_time() {
        let v = VersionInfo::current();
        assert_eq!(v.subprotocol, "wanlogger.v1");
        assert_eq!(v.log_format, "1.0.0");
        assert!(!v.version.is_empty());
    }

    #[tokio::test]
    async fn router_builds() {
        let _ = build();
    }
}
