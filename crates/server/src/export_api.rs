//! HTTP session export endpoint.
//!
//! The route only exports session-dirs already known to [`SourceManager`].
//! Clients provide a source `sid`; the server resolves that id to its
//! persisted session-dir and invokes the existing core exporters.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Path as AxumPath, Query, State};
use axum::http::header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use wanlogger_core::exporter::{csv, jsonl, pcapng, text};

use crate::auth::{is_loopback_allowed, BearerVerifier};
use crate::source_manager::SourceManager;

/// State required by the HTTP export route.
#[derive(Debug, Clone)]
pub struct ExportRouteState {
    source_manager: Arc<SourceManager>,
    auth: Arc<BearerVerifier>,
    no_auth: bool,
    defaults: ExportDefaults,
}

/// Defaults used by HTTP export when query parameters are omitted.
#[derive(Debug, Clone, Default)]
pub struct ExportDefaults {
    /// Default timezone for rendered timestamps.
    pub timezone: Option<String>,
    /// Default text encoding for text-like exporters.
    pub encoding: Option<String>,
}

impl ExportRouteState {
    /// Create state for `/api/sessions/{sid}/export`.
    #[must_use]
    pub fn new(
        source_manager: Arc<SourceManager>,
        auth: Arc<BearerVerifier>,
        no_auth: bool,
    ) -> Self {
        Self {
            source_manager,
            auth,
            no_auth,
            defaults: ExportDefaults::default(),
        }
    }

    /// Apply export defaults.
    #[must_use]
    pub fn with_defaults(mut self, defaults: ExportDefaults) -> Self {
        self.defaults = defaults;
        self
    }
}

/// Attach the session export route.
pub fn router(state: ExportRouteState) -> Router {
    Router::new()
        .route("/api/sessions/:sid/export", get(export_handler))
        .with_state(state)
}

#[derive(Debug, Clone, Deserialize)]
struct ExportQuery {
    #[serde(default = "default_format")]
    format: String,
    tz: Option<String>,
    encoding: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Text,
    Csv,
    Jsonl,
    Pcapng,
}

#[derive(Debug, Serialize)]
struct ExportErrorBody {
    error_id: &'static str,
    message: String,
}

#[derive(Debug)]
struct ExportApiError {
    status: StatusCode,
    error_id: &'static str,
    message: String,
}

#[derive(Debug)]
struct ExportArtifact {
    filename: String,
    content_type: &'static str,
    body: Vec<u8>,
}

async fn export_handler(
    State(state): State<ExportRouteState>,
    AxumPath(sid): AxumPath<String>,
    Query(query): Query<ExportQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = authorize_export(&headers, &peer, &state.auth, state.no_auth) {
        return err.into_response();
    }
    match export_session(&state, &sid, query).await {
        Ok(artifact) => artifact.into_response(),
        Err(err) => err.into_response(),
    }
}

async fn export_session(
    state: &ExportRouteState,
    sid_text: &str,
    query: ExportQuery,
) -> Result<ExportArtifact, ExportApiError> {
    let sid = Uuid::parse_str(sid_text).map_err(|e| ExportApiError {
        status: StatusCode::BAD_REQUEST,
        error_id: "E-2001",
        message: format!("invalid sid `{sid_text}`: {e}"),
    })?;
    let format = ExportFormat::parse(&query.format)?;
    let session_dir = state
        .source_manager
        .session_dir_for_sid(sid)
        .ok_or_else(|| ExportApiError {
            status: StatusCode::NOT_FOUND,
            error_id: "E-1001",
            message: format!("source `{sid}` has no persisted session-dir"),
        })?;
    ensure_session_dir(&session_dir, state.source_manager.session_root())?;

    let tmp = TempExportFile::new(temp_export_path(sid, format));
    let dst = tmp.path().to_path_buf();
    let timezone = query.tz.or_else(|| state.defaults.timezone.clone());
    let encoding = query.encoding.or_else(|| state.defaults.encoding.clone());
    let export_result = tokio::task::spawn_blocking(move || match format {
        ExportFormat::Text => text::export_with_timezone_and_encoding(
            &session_dir,
            &dst,
            timezone.as_deref(),
            encoding.as_deref(),
        ),
        ExportFormat::Csv => csv::export_with_timezone_and_encoding(
            &session_dir,
            &dst,
            timezone.as_deref(),
            encoding.as_deref(),
        ),
        ExportFormat::Jsonl => jsonl::export_with_timezone_and_encoding(
            &session_dir,
            &dst,
            timezone.as_deref(),
            encoding.as_deref(),
        ),
        ExportFormat::Pcapng => {
            pcapng::export_with_timezone(&session_dir, &dst, timezone.as_deref())
        }
    })
    .await
    .map_err(|e| ExportApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        error_id: "E-1001",
        message: format!("export task failed: {e}"),
    })?;
    export_result.map_err(|e| ExportApiError {
        status: StatusCode::BAD_REQUEST,
        error_id: e.id.code(),
        message: e.to_string(),
    })?;

    let body = tokio::fs::read(tmp.path())
        .await
        .map_err(|e| ExportApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error_id: "E-1001",
            message: format!("reading export artifact: {e}"),
        })?;
    tmp.cleanup().await;

    Ok(ExportArtifact {
        filename: format!("wanlogger-{sid}.{}", format.extension()),
        content_type: format.content_type(),
        body,
    })
}

fn authorize_export(
    headers: &HeaderMap,
    peer: &SocketAddr,
    auth: &BearerVerifier,
    no_auth: bool,
) -> Result<(), ExportApiError> {
    if no_auth && is_loopback_allowed(peer) {
        return Ok(());
    }
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = token else {
        return Err(ExportApiError::auth("missing bearer token"));
    };
    auth.verify(token)
        .map_err(|_| ExportApiError::auth("bearer token rejected"))
}

fn ensure_session_dir(path: &Path, root: Option<&Path>) -> Result<(), ExportApiError> {
    if !path.exists() {
        return Err(ExportApiError {
            status: StatusCode::NOT_FOUND,
            error_id: "E-1001",
            message: format!("source session-dir does not exist: {}", path.display()),
        });
    }
    let canonical_path = path.canonicalize().map_err(|e| ExportApiError {
        status: StatusCode::NOT_FOUND,
        error_id: "E-1001",
        message: format!(
            "source session-dir is not accessible: {}: {e}",
            path.display()
        ),
    })?;
    if let Some(root) = root {
        let canonical_root = root.canonicalize().map_err(|e| ExportApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error_id: "E-1001",
            message: format!("session root is not accessible: {}: {e}", root.display()),
        })?;
        if !canonical_path.starts_with(&canonical_root) {
            return Err(ExportApiError {
                status: StatusCode::FORBIDDEN,
                error_id: "E-1001",
                message: format!(
                    "source session-dir is outside configured session root: {}",
                    path.display()
                ),
            });
        }
    }
    if !path.join("index.jsonl").is_file() {
        return Err(ExportApiError {
            status: StatusCode::NOT_FOUND,
            error_id: "E-1001",
            message: format!(
                "source session-dir is missing index.jsonl: {}",
                path.display()
            ),
        });
    }
    Ok(())
}

#[derive(Debug)]
struct TempExportFile {
    path: PathBuf,
    cleanup: bool,
}

impl TempExportFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    async fn cleanup(mut self) {
        if self.cleanup {
            let _ = tokio::fs::remove_file(&self.path).await;
            self.cleanup = false;
        }
    }
}

impl Drop for TempExportFile {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn temp_export_path(sid: Uuid, format: ExportFormat) -> PathBuf {
    std::env::temp_dir().join(format!(
        "wanlogger-export-{sid}-{}.{}",
        Uuid::new_v4(),
        format.extension()
    ))
}

fn default_format() -> String {
    "text".to_string()
}

impl ExportFormat {
    fn parse(value: &str) -> Result<Self, ExportApiError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "csv" => Ok(Self::Csv),
            "jsonl" => Ok(Self::Jsonl),
            "pcapng" => Ok(Self::Pcapng),
            other => Err(ExportApiError {
                status: StatusCode::BAD_REQUEST,
                error_id: "E-1001",
                message: format!(
                    "unsupported export format `{other}`; use text, csv, jsonl, or pcapng"
                ),
            }),
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Csv => "csv",
            Self::Jsonl => "jsonl",
            Self::Pcapng => "pcapng",
        }
    }

    const fn content_type(self) -> &'static str {
        match self {
            Self::Text => "text/plain; charset=utf-8",
            Self::Csv => "text/csv; charset=utf-8",
            Self::Jsonl => "application/x-ndjson; charset=utf-8",
            Self::Pcapng => "application/vnd.tcpdump.pcapng",
        }
    }
}

impl ExportApiError {
    fn auth(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error_id: "E-2101",
            message: message.into(),
        }
    }
}

impl IntoResponse for ExportApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ExportErrorBody {
                error_id: self.error_id,
                message: self.message,
            }),
        )
            .into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl IntoResponse for ExportArtifact {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from(self.body));
        *response.status_mut() = StatusCode::OK;
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(self.content_type));
        let disposition = format!("attachment; filename=\"{}\"", self.filename);
        if let Ok(value) = HeaderValue::from_str(&disposition) {
            response.headers_mut().insert(CONTENT_DISPOSITION, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wanlogger_core::decoder::Record;
    use wanlogger_core::exporter::pcapng::PCAP_PACKET_SCHEMA_ID;
    use wanlogger_core::log::frames::{FrameEntry, FramesWriter};
    use wanlogger_core::log::index::{Dir, IndexEntry, IndexWriter, Kind};
    use wanlogger_core::log::raw::RawWriter;
    use wanlogger_core::source::ChannelSpec;
    use wanlogger_core::time::{ClockQuality, ClockSource, DualTimestamp};

    #[test]
    fn auth_allows_loopback_no_auth() {
        // REQ: FR-WIRE-002
        let headers = HeaderMap::new();
        let peer: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        assert!(authorize_export(&headers, &peer, &BearerVerifier::new(), true).is_ok());
    }

    #[test]
    fn auth_rejects_missing_token_when_not_loopback_no_auth() {
        // REQ: FR-WIRE-002
        let headers = HeaderMap::new();
        let peer: SocketAddr = "192.0.2.10:1234".parse().unwrap();
        let err = authorize_export(&headers, &peer, &BearerVerifier::new(), true).unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.error_id, "E-2101");
    }

    #[test]
    fn ensure_session_dir_rejects_paths_outside_session_root() {
        // REQ: FR-EXP-001
        let root = unique_temp_path("wanlogger-export-root");
        let outside = unique_temp_path("wanlogger-export-outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("index.jsonl"), b"{}\n").unwrap();

        let err = ensure_session_dir(&outside, Some(&root)).unwrap_err();

        assert_eq!(err.status, StatusCode::FORBIDDEN);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn ensure_session_dir_reports_missing_session_dir() {
        // REQ: FR-EXP-001
        let missing = unique_temp_path("wanlogger-export-missing");

        let err = ensure_session_dir(&missing, None).unwrap_err();

        assert_eq!(err.status, StatusCode::NOT_FOUND);
        assert!(err.message.contains("does not exist"));
    }

    #[test]
    fn temp_export_file_is_removed_on_error_path_drop() {
        // REQ: FR-EXP-001
        let path = unique_temp_path("wanlogger-export-guard").with_extension("txt");
        std::fs::write(&path, b"partial").unwrap();
        {
            let _guard = TempExportFile::new(path.clone());
        }
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn exports_known_session_dir_with_timezone() {
        // REQ: FR-EXP-001
        let root = std::env::temp_dir().join(format!("wanlogger-export-api-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("in.txt");
        std::fs::write(&input, b"download\n").unwrap();
        let ingest = Arc::new(crate::ingest::Ingest::new());
        let manager = Arc::new(SourceManager::with_session_root(
            ingest,
            root.join("sessions"),
        ));
        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();
        let state = ExportRouteState::new(manager, Arc::new(BearerVerifier::new()), true);
        let artifact = export_session(
            &state,
            &sid.to_string(),
            ExportQuery {
                format: "text".to_string(),
                tz: Some("GMT+9".to_string()),
                encoding: None,
            },
        )
        .await
        .unwrap();
        let body = String::from_utf8(artifact.body).unwrap();
        assert!(body.contains("download"));
        assert!(body.lines().next().unwrap().contains("+09:00"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn exports_known_session_dir_with_default_timezone() {
        // REQ: FR-CLI-012
        let root = std::env::temp_dir().join(format!("wanlogger-export-api-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("in.txt");
        std::fs::write(&input, b"configured\n").unwrap();
        let ingest = Arc::new(crate::ingest::Ingest::new());
        let manager = Arc::new(SourceManager::with_session_root(
            ingest,
            root.join("sessions"),
        ));
        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();
        let state = ExportRouteState::new(manager, Arc::new(BearerVerifier::new()), true)
            .with_defaults(ExportDefaults {
                timezone: Some("GMT+9".to_string()),
                encoding: None,
            });
        let artifact = export_session(
            &state,
            &sid.to_string(),
            ExportQuery {
                format: "text".to_string(),
                tz: None,
                encoding: None,
            },
        )
        .await
        .unwrap();
        let body = String::from_utf8(artifact.body).unwrap();
        assert!(body.contains("configured"));
        assert!(body.lines().next().unwrap().contains("+09:00"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn exports_known_session_dir_as_pcapng() {
        let root = std::env::temp_dir().join(format!("wanlogger-export-api-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("in.txt");
        std::fs::write(&input, b"seed\n").unwrap();
        let ingest = Arc::new(crate::ingest::Ingest::new());
        let manager = Arc::new(SourceManager::with_session_root(
            ingest,
            root.join("sessions"),
        ));
        let sid = manager
            .start_spec(ChannelSpec::File {
                path: input.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid).await.unwrap().unwrap();
        let session_dir = manager.session_dir_for_sid(sid).unwrap();
        write_synthetic_pcap_session(&session_dir, sid);

        let state = ExportRouteState::new(manager, Arc::new(BearerVerifier::new()), true);
        let artifact = export_session(
            &state,
            &sid.to_string(),
            ExportQuery {
                format: "pcapng".to_string(),
                tz: None,
                encoding: Some("shift_jis".to_string()),
            },
        )
        .await
        .unwrap();

        assert!(artifact.filename.ends_with(".pcapng"));
        assert_eq!(artifact.content_type, "application/vnd.tcpdump.pcapng");
        assert!(artifact.body.starts_with(&0x0A0D_0D0Au32.to_le_bytes()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn rejects_unknown_format() {
        let ingest = Arc::new(crate::ingest::Ingest::new());
        let manager = Arc::new(SourceManager::with_session_root(
            ingest,
            std::env::temp_dir().join("wanlogger-export-test"),
        ));
        let state = ExportRouteState::new(manager, Arc::new(BearerVerifier::new()), true);
        let err = export_session(
            &state,
            &Uuid::new_v4().to_string(),
            ExportQuery {
                format: "xlsx".to_string(),
                tz: None,
                encoding: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    fn write_synthetic_pcap_session(dir: &Path, sid: Uuid) {
        for name in ["raw.bin", "index.jsonl", "frames.jsonl", "lines.jsonl"] {
            let _ = std::fs::remove_file(dir.join(name));
        }
        let packet = ethernet_packet();
        let mut raw = RawWriter::create(dir).unwrap();
        let (off, len) = raw.append(&packet).unwrap();
        raw.flush().unwrap();

        let ts = sample_ts();
        let mut entry = IndexEntry::from_envelope(&ts, sid, Dir::In, Kind::Datagram, off, len);
        entry.source = Some("pcap:eth0".to_string());
        entry.schema_id = Some(PCAP_PACKET_SCHEMA_ID.to_string());
        let mut index = IndexWriter::create(dir).unwrap();
        index.append(&entry).unwrap();
        index.flush().unwrap();

        let mut frames = FramesWriter::create(dir).unwrap();
        frames
            .append(&FrameEntry {
                ts: entry.ts_ingest,
                decoder: "pcap".to_string(),
                record: Record {
                    schema_id: Some(PCAP_PACKET_SCHEMA_ID.to_string()),
                    level: None,
                    text: None,
                    fields: serde_json::json!({
                        "raw_off": off,
                        "raw_len": len,
                        "captured_len": len,
                        "original_len": len,
                        "linktype": 1,
                        "interface_id": 0,
                        "interface": "eth0"
                    }),
                    tags: vec!["pcap".to_string()],
                    correlation_id: None,
                },
            })
            .unwrap();
        frames.flush().unwrap();
    }

    fn sample_ts() -> DualTimestamp {
        DualTimestamp {
            ts_origin_ns: 1_700_000_000_123_456_789,
            ts_ingest_ns: 1_700_000_000_223_456_789,
            mono_ns: 42,
            boot_id: Uuid::nil(),
            node_id: Uuid::nil(),
            clock_offset_ms: 0,
            clock_quality: ClockQuality::BestEffort,
            drift_ppm: 0.0,
            clock_source: ClockSource::Imported,
        }
    }

    fn ethernet_packet() -> Vec<u8> {
        vec![
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00,
            0x45, 0x00, 0x00, 0x14,
        ]
    }

    fn unique_temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()))
    }
}
