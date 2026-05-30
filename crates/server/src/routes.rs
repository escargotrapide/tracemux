//! HTTP routes (axum router).
//!
//! v0.1 wires the non-critical, non-WSS surface area:
//!
//! * `GET /healthz`       ? liveness probe (always 200)
//! * `GET /readyz`        ? readiness probe
//! * `GET /api/version`   ? server build version JSON
//! * `GET /api/ai/verify` ? last `target/ai-verify.json` (see [`crate::ai_api`])
//! * `GET /api/sessions/{sid}/range` ? historical raw.bin streaming (see [`crate::range`])
//! * `GET /api/sessions/{sid}/export` ? authenticated session export
//! * `/api/annotations` - authenticated server-owned UI annotations
//! * `GET /api/detect`    ? transport kinds and host serial candidates
//!
//! WSS (`/ws`) and TLS termination remain in the critical-path
//! modules and are not wired here.

use std::time::Duration;

use axum::http::header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderValue, Method};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tower_http::cors::{AllowOrigin, CorsLayer};
use wanlogger_core::detect::pcap::PcapInterfaceInfo;

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

/// Transport detection report returned by `/api/detect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectReport {
    /// Transport kinds known to the v0.1 UI/CLI subset.
    pub kinds: &'static [&'static str],
    /// Best-effort serial-port candidates such as `COM7` or `/dev/ttyUSB0`.
    pub serial_candidates: Vec<String>,
    /// Packet-capture interfaces. Empty until the pcap backend is enabled.
    pub pcap_interfaces: Vec<PcapInterfaceInfo>,
}

const DETECT_KINDS: &[&str] = &[
    "file", "tcp", "udp", "serial", "pcap", "process", "pipe", "mock",
];

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
    with_http_api_layers(base_routes())
}

fn base_routes() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/version", get(version))
        .route("/api/detect", get(detect))
        .route("/api/ai/verify", get(crate::ai_api::verify))
        .route("/api/sessions/:sid/range", get(crate::range::range_handler))
}

/// Build the public router plus authenticated session export.
pub fn build_with_exports(export_state: crate::export_api::ExportRouteState) -> Router {
    with_http_api_layers(base_routes().merge(crate::export_api::router(export_state)))
}

/// Build the public router plus authenticated session export and annotations.
pub fn build_with_exports_and_annotations(
    export_state: crate::export_api::ExportRouteState,
    annotation_state: crate::annotation_api::AnnotationRouteState,
) -> Router {
    with_http_api_layers(
        base_routes()
            .merge(crate::export_api::router(export_state))
            .merge(crate::annotation_api::router(annotation_state)),
    )
}

fn with_http_api_layers(router: Router) -> Router {
    router.layer(dev_cors_layer())
}

fn dev_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list([
            HeaderValue::from_static("http://127.0.0.1:5173"),
            HeaderValue::from_static("http://localhost:5173"),
        ]))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE])
        .expose_headers([CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE])
        .max_age(Duration::from_secs(600))
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

/// Build the transport detection report used by both route tests and handler.
#[must_use]
pub fn detect_report() -> DetectReport {
    DetectReport {
        kinds: DETECT_KINDS,
        serial_candidates: wanlogger_core::detect::serial::list(),
        pcap_interfaces: wanlogger_core::detect::pcap::list(),
    }
}

async fn detect() -> Json<DetectReport> {
    Json(detect_report())
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

    #[test]
    fn detect_report_contains_serial_kind() {
        // REQ: FR-UI-016
        let report = detect_report();
        assert!(report.kinds.contains(&"serial"));
    }

    #[test]
    fn detect_report_includes_pcap_schema() {
        let report = detect_report();

        assert!(report.kinds.contains(&"pcap"));
        assert!(report.pcap_interfaces.is_empty());
    }

    #[tokio::test]
    async fn router_builds() {
        let _ = build();
    }
}
