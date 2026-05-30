//! HTTP session export endpoint.
//!
//! The route only exports session-dirs already known to [`SourceManager`].
//! Clients provide a source `sid`; the server resolves that id to its
//! persisted session-dir and invokes the existing core exporters.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{ConnectInfo, Path as AxumPath, Query, State};
use axum::http::header::{
    AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE,
    WWW_AUTHENTICATE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures::Stream;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::io::{AsyncRead, ReadBuf};
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
    tickets: Arc<DownloadTickets>,
    bundle_tickets: Arc<BundleDownloadTickets>,
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
            tickets: Arc::new(DownloadTickets::default()),
            bundle_tickets: Arc::new(BundleDownloadTickets::default()),
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
        .route(
            "/api/sessions/:sid/export-ticket",
            post(export_ticket_handler),
        )
        .route("/api/exports/bundle", get(export_bundle_handler))
        .route(
            "/api/exports/bundle-ticket",
            post(export_bundle_ticket_handler),
        )
        .with_state(state)
}

#[derive(Debug, Clone, Deserialize)]
struct ExportQuery {
    #[serde(default = "default_format")]
    format: String,
    tz: Option<String>,
    encoding: Option<String>,
    ticket: Option<String>,
    filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExportBundleTicketRequest {
    entries: Vec<ExportBundleEntry>,
    #[serde(default = "default_format")]
    format: String,
    tz: Option<String>,
    filename_pattern: Option<String>,
    timestamp_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExportBundleEntry {
    sid: String,
    source_name: Option<String>,
    encoding: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BundleDownloadQuery {
    ticket: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Text,
    Csv,
    Jsonl,
    Pcapng,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DownloadTicketScope {
    sid: Uuid,
    format: ExportFormat,
    timezone: Option<String>,
    encoding: Option<String>,
}

#[derive(Debug, Clone)]
struct DownloadTicket {
    scope: DownloadTicketScope,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct DownloadTickets {
    inner: Mutex<HashMap<String, DownloadTicket>>,
}

#[derive(Debug, Clone)]
struct BundleDownloadTicket {
    request: ExportBundleTicketRequest,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct BundleDownloadTickets {
    inner: Mutex<HashMap<String, BundleDownloadTicket>>,
}

#[derive(Debug, Serialize)]
struct ExportTicketResponse {
    ticket: String,
    expires_in_ms: u64,
    expires_at_ms: u64,
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
    len: u64,
    file: tokio::fs::File,
    cleanup_path: PathBuf,
}

struct BundleExportJob {
    sid: Uuid,
    source_name: Option<String>,
    encoding: Option<String>,
    session_dir: PathBuf,
}

async fn export_handler(
    State(state): State<ExportRouteState>,
    AxumPath(sid): AxumPath<String>,
    Query(query): Query<ExportQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = authorize_export_request(&headers, &peer, &state, &sid, &query) {
        return err.into_response();
    }
    match export_session(&state, &sid, query).await {
        Ok(artifact) => artifact.into_response(),
        Err(err) => err.into_response(),
    }
}

async fn export_ticket_handler(
    State(state): State<ExportRouteState>,
    AxumPath(sid): AxumPath<String>,
    Query(query): Query<ExportQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = authorize_export(&headers, &peer, &state.auth, state.no_auth) {
        return err.into_response();
    }
    let scope = match ticket_scope(&sid, &query, &state.defaults) {
        Ok(scope) => scope,
        Err(err) => return err.into_response(),
    };
    Json(state.tickets.issue(scope)).into_response()
}

async fn export_bundle_ticket_handler(
    State(state): State<ExportRouteState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<ExportBundleTicketRequest>,
) -> Response {
    if let Err(err) = authorize_export(&headers, &peer, &state.auth, state.no_auth) {
        return err.into_response();
    }
    if let Err(err) = validate_bundle_request(&request) {
        return err.into_response();
    }
    Json(state.bundle_tickets.issue(request)).into_response()
}

async fn export_bundle_handler(
    State(state): State<ExportRouteState>,
    Query(query): Query<BundleDownloadQuery>,
) -> Response {
    let request = match state.bundle_tickets.consume(&query.ticket) {
        Ok(request) => request,
        Err(err) => return err.into_response(),
    };
    match export_bundle(&state, request).await {
        Ok(artifact) => artifact.into_response(),
        Err(err) => err.into_response(),
    }
}

fn authorize_export_request(
    headers: &HeaderMap,
    peer: &SocketAddr,
    state: &ExportRouteState,
    sid_text: &str,
    query: &ExportQuery,
) -> Result<(), ExportApiError> {
    match authorize_export(headers, peer, &state.auth, state.no_auth) {
        Ok(()) => Ok(()),
        Err(auth_err) => {
            let Some(ticket) = query.ticket.as_deref() else {
                return Err(auth_err);
            };
            let scope = ticket_scope(sid_text, query, &state.defaults)?;
            state.tickets.consume(ticket, &scope)
        }
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
    let filename = export_filename(sid, format, query.filename.as_deref());
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
    let timezone = clean_optional(query.tz).or_else(|| state.defaults.timezone.clone());
    let encoding = clean_optional(query.encoding).or_else(|| state.defaults.encoding.clone());
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

    let len = tokio::fs::metadata(tmp.path())
        .await
        .map_err(|e| ExportApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error_id: "E-1001",
            message: format!("statting export artifact: {e}"),
        })?
        .len();
    let file = tokio::fs::File::open(tmp.path())
        .await
        .map_err(|e| ExportApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error_id: "E-1001",
            message: format!("opening export artifact: {e}"),
        })?;

    Ok(ExportArtifact {
        filename,
        content_type: format.content_type(),
        len,
        file,
        cleanup_path: tmp.into_path(),
    })
}

fn validate_bundle_request(request: &ExportBundleTicketRequest) -> Result<(), ExportApiError> {
    if request.entries.is_empty() {
        return Err(ExportApiError {
            status: StatusCode::BAD_REQUEST,
            error_id: "E-1001",
            message: "bundle export requires at least one entry".to_string(),
        });
    }
    if request.entries.len() > 0xffff {
        return Err(ExportApiError {
            status: StatusCode::BAD_REQUEST,
            error_id: "E-1001",
            message: "too many entries for ZIP32 bundle export".to_string(),
        });
    }
    ExportFormat::parse(&request.format)?;
    Ok(())
}

async fn export_bundle(
    state: &ExportRouteState,
    request: ExportBundleTicketRequest,
) -> Result<ExportArtifact, ExportApiError> {
    validate_bundle_request(&request)?;
    let format = ExportFormat::parse(&request.format)?;
    let timezone = clean_optional(request.tz).or_else(|| state.defaults.timezone.clone());
    let defaults_encoding = state.defaults.encoding.clone();
    let timestamp_ms = request
        .timestamp_ms
        .unwrap_or_else(|| system_time_ms(SystemTime::now()));
    let mut jobs = Vec::with_capacity(request.entries.len());
    for entry in request.entries {
        let sid = Uuid::parse_str(&entry.sid).map_err(|e| ExportApiError {
            status: StatusCode::BAD_REQUEST,
            error_id: "E-2001",
            message: format!("invalid sid `{}`: {e}", entry.sid),
        })?;
        let session_dir = state
            .source_manager
            .session_dir_for_sid(sid)
            .ok_or_else(|| ExportApiError {
                status: StatusCode::NOT_FOUND,
                error_id: "E-1001",
                message: format!("source `{sid}` has no persisted session-dir"),
            })?;
        ensure_session_dir(&session_dir, state.source_manager.session_root())?;
        jobs.push(BundleExportJob {
            sid,
            source_name: entry.source_name,
            encoding: clean_optional(entry.encoding).or_else(|| defaults_encoding.clone()),
            session_dir,
        });
    }

    let zip_tmp = TempExportFile::new(temp_bundle_path(format));
    let zip_path = zip_tmp.path().to_path_buf();
    let filename = bundle_filename(format, timestamp_ms);
    let filename_pattern = request.filename_pattern;
    let export_result = tokio::task::spawn_blocking(move || {
        create_bundle_zip(
            &zip_path,
            jobs,
            format,
            timezone.as_deref(),
            filename_pattern.as_deref(),
            timestamp_ms,
        )
    })
    .await
    .map_err(|e| ExportApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        error_id: "E-1001",
        message: format!("bundle export task failed: {e}"),
    })?;
    export_result?;

    let len = tokio::fs::metadata(zip_tmp.path())
        .await
        .map_err(|e| ExportApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error_id: "E-1001",
            message: format!("statting bundle artifact: {e}"),
        })?
        .len();
    let file = tokio::fs::File::open(zip_tmp.path())
        .await
        .map_err(|e| ExportApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error_id: "E-1001",
            message: format!("opening bundle artifact: {e}"),
        })?;

    Ok(ExportArtifact {
        filename,
        content_type: "application/zip",
        len,
        file,
        cleanup_path: zip_tmp.into_path(),
    })
}

fn ticket_scope(
    sid_text: &str,
    query: &ExportQuery,
    defaults: &ExportDefaults,
) -> Result<DownloadTicketScope, ExportApiError> {
    let sid = Uuid::parse_str(sid_text).map_err(|e| ExportApiError {
        status: StatusCode::BAD_REQUEST,
        error_id: "E-2001",
        message: format!("invalid sid `{sid_text}`: {e}"),
    })?;
    Ok(DownloadTicketScope {
        sid,
        format: ExportFormat::parse(&query.format)?,
        timezone: clean_optional(query.tz.clone()).or_else(|| defaults.timezone.clone()),
        encoding: clean_optional(query.encoding.clone()).or_else(|| defaults.encoding.clone()),
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

    fn into_path(mut self) -> PathBuf {
        self.cleanup = false;
        self.path.clone()
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

fn temp_bundle_path(format: ExportFormat) -> PathBuf {
    std::env::temp_dir().join(format!(
        "wanlogger-bundle-{}.{}.zip",
        Uuid::new_v4(),
        format.extension()
    ))
}

fn create_bundle_zip(
    zip_path: &Path,
    jobs: Vec<BundleExportJob>,
    format: ExportFormat,
    timezone: Option<&str>,
    filename_pattern: Option<&str>,
    timestamp_ms: u64,
) -> Result<(), ExportApiError> {
    let mut zip = StoredZipWriter::create(zip_path)?;
    let folder = bundle_base_name(format, timestamp_ms);
    let mut used = HashSet::new();
    for job in jobs {
        let export_tmp = TempExportFile::new(temp_export_path(job.sid, format));
        export_one(
            format,
            &job.session_dir,
            export_tmp.path(),
            timezone,
            job.encoding.as_deref(),
        )?;
        let filename = render_bundle_entry_filename(
            filename_pattern,
            job.sid,
            job.source_name.as_deref(),
            format,
            timestamp_ms,
        );
        let entry_name = unique_zip_entry_name(&filename, &mut used)?;
        zip.append_file_from_path(&format!("{folder}/{entry_name}"), export_tmp.path())?;
    }
    zip.finish()
}

fn export_one(
    format: ExportFormat,
    session_dir: &Path,
    dst: &Path,
    timezone: Option<&str>,
    encoding: Option<&str>,
) -> Result<(), ExportApiError> {
    let result = match format {
        ExportFormat::Text => {
            text::export_with_timezone_and_encoding(session_dir, dst, timezone, encoding)
        }
        ExportFormat::Csv => {
            csv::export_with_timezone_and_encoding(session_dir, dst, timezone, encoding)
        }
        ExportFormat::Jsonl => {
            jsonl::export_with_timezone_and_encoding(session_dir, dst, timezone, encoding)
        }
        ExportFormat::Pcapng => pcapng::export_with_timezone(session_dir, dst, timezone),
    };
    result.map_err(|e| ExportApiError {
        status: StatusCode::BAD_REQUEST,
        error_id: e.id.code(),
        message: e.to_string(),
    })
}

fn bundle_base_name(format: ExportFormat, timestamp_ms: u64) -> String {
    sanitize_filename(&format!(
        "wanlogger-all-{}-{}",
        timestamp_token(timestamp_ms),
        format.label()
    ))
}

fn bundle_filename(format: ExportFormat, timestamp_ms: u64) -> String {
    format!("{}.zip", bundle_base_name(format, timestamp_ms))
}

fn export_filename(sid: Uuid, format: ExportFormat, requested: Option<&str>) -> String {
    let ext = format.extension();
    let fallback = format!("wanlogger-{sid}.{ext}");
    let filename = sanitize_filename(
        requested
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&fallback),
    );
    if filename.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        filename
    } else {
        format!("{filename}.{ext}")
    }
}

fn render_bundle_entry_filename(
    pattern: Option<&str>,
    sid: Uuid,
    source_name: Option<&str>,
    format: ExportFormat,
    timestamp_ms: u64,
) -> String {
    let ext = format.extension();
    let source = source_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("source");
    let template = pattern
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("wanlogger-{sid}.{ext}");
    let rendered = template
        .replace("{sid}", &sid.to_string())
        .replace("{source}", source)
        .replace("{timestamp}", &timestamp_token(timestamp_ms))
        .replace("{format}", format.label())
        .replace("{ext}", ext);
    let filename = sanitize_filename(&rendered);
    if filename.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        filename
    } else {
        format!("{filename}.{ext}")
    }
}

fn sanitize_filename(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut previous_space = false;
    for ch in value.chars() {
        let invalid =
            matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || ch.is_control();
        if invalid {
            out.push('-');
            previous_space = false;
        } else if ch.is_whitespace() {
            if !previous_space {
                out.push(' ');
                previous_space = true;
            }
        } else {
            out.push(ch);
            previous_space = false;
        }
    }
    let trimmed = out.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "wanlogger-export".to_string()
    } else {
        trimmed
    }
}

fn unique_zip_entry_name(name: &str, used: &mut HashSet<String>) -> Result<String, ExportApiError> {
    let normalized = name.replace('\\', "/");
    if used.insert(normalized.clone()) {
        return Ok(normalized);
    }
    let dot = normalized.rfind('.');
    let (stem, ext) = dot
        .filter(|index| *index > 0)
        .map_or((normalized.as_str(), ""), |index| {
            (&normalized[..index], &normalized[index..])
        });
    for index in 2..10_000 {
        let candidate = format!("{stem}-{index}{ext}");
        if used.insert(candidate.clone()) {
            return Ok(candidate);
        }
    }
    Err(ExportApiError {
        status: StatusCode::BAD_REQUEST,
        error_id: "E-1001",
        message: "too many duplicate export filenames".to_string(),
    })
}

struct StoredZipWriter {
    out: BufWriter<File>,
    central: Vec<CentralDirectoryEntry>,
    offset: u64,
}

struct CentralDirectoryEntry {
    name: Vec<u8>,
    crc32: u32,
    size: u64,
    local_offset: u64,
}

impl StoredZipWriter {
    fn create(path: &Path) -> Result<Self, ExportApiError> {
        let out = File::create(path).map_err(|e| io_error("creating bundle zip", &e))?;
        Ok(Self {
            out: BufWriter::new(out),
            central: Vec::new(),
            offset: 0,
        })
    }

    fn append_file_from_path(&mut self, name: &str, path: &Path) -> Result<(), ExportApiError> {
        let name = name.replace('\\', "/");
        let name_bytes = name.as_bytes().to_vec();
        let name_len = u16::try_from(name_bytes.len()).map_err(|_| ExportApiError {
            status: StatusCode::BAD_REQUEST,
            error_id: "E-1001",
            message: format!("ZIP entry name is too long: {name}"),
        })?;
        let (crc32, size) = crc32_file(path)?;
        if size > u32::MAX as u64 || self.offset > u32::MAX as u64 {
            return Err(ExportApiError {
                status: StatusCode::BAD_REQUEST,
                error_id: "E-1001",
                message: "ZIP32 bundle export is too large".to_string(),
            });
        }

        let local_offset = self.offset;
        write_u32(&mut self.out, 0x0403_4b50)?;
        write_u16(&mut self.out, 20)?;
        write_u16(&mut self.out, 0x0800)?;
        write_u16(&mut self.out, 0)?;
        write_u16(&mut self.out, 0)?;
        write_u16(&mut self.out, 33)?;
        write_u32(&mut self.out, crc32)?;
        write_u32(&mut self.out, size as u32)?;
        write_u32(&mut self.out, size as u32)?;
        write_u16(&mut self.out, name_len)?;
        write_u16(&mut self.out, 0)?;
        self.out
            .write_all(&name_bytes)
            .map_err(|e| io_error("writing ZIP local header", &e))?;
        self.offset += 30 + u64::from(name_len);

        let mut input =
            BufReader::new(File::open(path).map_err(|e| io_error("opening export", &e))?);
        let copied = std::io::copy(&mut input, &mut self.out)
            .map_err(|e| io_error("writing ZIP entry", &e))?;
        if copied != size {
            return Err(ExportApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                error_id: "E-1001",
                message: format!("ZIP entry changed while writing: {name}"),
            });
        }
        self.offset += copied;
        self.central.push(CentralDirectoryEntry {
            name: name_bytes,
            crc32,
            size,
            local_offset,
        });
        Ok(())
    }

    fn finish(mut self) -> Result<(), ExportApiError> {
        let central_offset = self.offset;
        let mut central_size = 0_u64;
        for entry in &self.central {
            if entry.size > u32::MAX as u64 || entry.local_offset > u32::MAX as u64 {
                return Err(ExportApiError {
                    status: StatusCode::BAD_REQUEST,
                    error_id: "E-1001",
                    message: "ZIP32 bundle export is too large".to_string(),
                });
            }
            let name_len = u16::try_from(entry.name.len()).map_err(|_| ExportApiError {
                status: StatusCode::BAD_REQUEST,
                error_id: "E-1001",
                message: "ZIP entry name is too long".to_string(),
            })?;
            write_u32(&mut self.out, 0x0201_4b50)?;
            write_u16(&mut self.out, 20)?;
            write_u16(&mut self.out, 20)?;
            write_u16(&mut self.out, 0x0800)?;
            write_u16(&mut self.out, 0)?;
            write_u16(&mut self.out, 0)?;
            write_u16(&mut self.out, 33)?;
            write_u32(&mut self.out, entry.crc32)?;
            write_u32(&mut self.out, entry.size as u32)?;
            write_u32(&mut self.out, entry.size as u32)?;
            write_u16(&mut self.out, name_len)?;
            write_u16(&mut self.out, 0)?;
            write_u16(&mut self.out, 0)?;
            write_u16(&mut self.out, 0)?;
            write_u16(&mut self.out, 0)?;
            write_u32(&mut self.out, 0)?;
            write_u32(&mut self.out, entry.local_offset as u32)?;
            self.out
                .write_all(&entry.name)
                .map_err(|e| io_error("writing ZIP central directory", &e))?;
            central_size += 46 + u64::from(name_len);
        }
        if self.central.len() > 0xffff
            || central_size > u32::MAX as u64
            || central_offset > u32::MAX as u64
        {
            return Err(ExportApiError {
                status: StatusCode::BAD_REQUEST,
                error_id: "E-1001",
                message: "ZIP32 bundle export is too large".to_string(),
            });
        }
        write_u32(&mut self.out, 0x0605_4b50)?;
        write_u16(&mut self.out, 0)?;
        write_u16(&mut self.out, 0)?;
        write_u16(&mut self.out, self.central.len() as u16)?;
        write_u16(&mut self.out, self.central.len() as u16)?;
        write_u32(&mut self.out, central_size as u32)?;
        write_u32(&mut self.out, central_offset as u32)?;
        write_u16(&mut self.out, 0)?;
        self.out
            .flush()
            .map_err(|e| io_error("flushing bundle zip", &e))
    }
}

fn crc32_file(path: &Path) -> Result<(u32, u64), ExportApiError> {
    let mut input = BufReader::new(File::open(path).map_err(|e| io_error("opening export", &e))?);
    let mut buf = vec![0_u8; 64 * 1024];
    let mut crc = 0xffff_ffff_u32;
    let mut size = 0_u64;
    loop {
        let read = input
            .read(&mut buf)
            .map_err(|e| io_error("reading export", &e))?;
        if read == 0 {
            break;
        }
        size += read as u64;
        for byte in &buf[..read] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if (crc & 1) != 0 {
                    0xedb8_8320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
        }
    }
    Ok((crc ^ 0xffff_ffff, size))
}

fn write_u16(out: &mut impl Write, value: u16) -> Result<(), ExportApiError> {
    out.write_all(&value.to_le_bytes())
        .map_err(|e| io_error("writing ZIP", &e))
}

fn write_u32(out: &mut impl Write, value: u32) -> Result<(), ExportApiError> {
    out.write_all(&value.to_le_bytes())
        .map_err(|e| io_error("writing ZIP", &e))
}

fn io_error(context: &str, error: &std::io::Error) -> ExportApiError {
    ExportApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        error_id: "E-1001",
        message: format!("{context}: {error}"),
    }
}

fn default_format() -> String {
    "text".to_string()
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
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

    const fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Csv => "csv",
            Self::Jsonl => "jsonl",
            Self::Pcapng => "pcapng",
        }
    }
}

const DOWNLOAD_TICKET_TTL: Duration = Duration::from_secs(60);

impl DownloadTickets {
    fn issue(&self, scope: DownloadTicketScope) -> ExportTicketResponse {
        self.prune_expired(Instant::now());
        let ticket = format!("{}.{}", Uuid::new_v4(), Uuid::new_v4());
        let expires_at = Instant::now() + DOWNLOAD_TICKET_TTL;
        self.inner
            .lock()
            .insert(ticket.clone(), DownloadTicket { scope, expires_at });
        ExportTicketResponse {
            ticket,
            expires_in_ms: DOWNLOAD_TICKET_TTL.as_millis() as u64,
            expires_at_ms: system_time_ms(SystemTime::now() + DOWNLOAD_TICKET_TTL),
        }
    }

    fn consume(&self, ticket: &str, scope: &DownloadTicketScope) -> Result<(), ExportApiError> {
        let now = Instant::now();
        self.prune_expired(now);
        let Some(stored) = self.inner.lock().remove(ticket) else {
            return Err(ExportApiError::auth(
                "download ticket is missing or expired",
            ));
        };
        if stored.expires_at <= now {
            return Err(ExportApiError::auth("download ticket is expired"));
        }
        if stored.scope != *scope {
            return Err(ExportApiError::auth(
                "download ticket does not match export request",
            ));
        }
        Ok(())
    }

    fn prune_expired(&self, now: Instant) {
        self.inner
            .lock()
            .retain(|_, ticket| ticket.expires_at > now);
    }
}

impl BundleDownloadTickets {
    fn issue(&self, request: ExportBundleTicketRequest) -> ExportTicketResponse {
        self.prune_expired(Instant::now());
        let ticket = format!("{}.{}", Uuid::new_v4(), Uuid::new_v4());
        let expires_at = Instant::now() + DOWNLOAD_TICKET_TTL;
        self.inner.lock().insert(
            ticket.clone(),
            BundleDownloadTicket {
                request,
                expires_at,
            },
        );
        ExportTicketResponse {
            ticket,
            expires_in_ms: DOWNLOAD_TICKET_TTL.as_millis() as u64,
            expires_at_ms: system_time_ms(SystemTime::now() + DOWNLOAD_TICKET_TTL),
        }
    }

    fn consume(&self, ticket: &str) -> Result<ExportBundleTicketRequest, ExportApiError> {
        let now = Instant::now();
        self.prune_expired(now);
        let Some(stored) = self.inner.lock().remove(ticket) else {
            return Err(ExportApiError::auth(
                "bundle download ticket is missing or expired",
            ));
        };
        if stored.expires_at <= now {
            return Err(ExportApiError::auth("bundle download ticket is expired"));
        }
        Ok(stored.request)
    }

    fn prune_expired(&self, now: Instant) {
        self.inner
            .lock()
            .retain(|_, ticket| ticket.expires_at > now);
    }
}

fn system_time_ms(value: SystemTime) -> u64 {
    value
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn timestamp_token(timestamp_ms: u64) -> String {
    let seconds = (timestamp_ms / 1000).min(i64::MAX as u64) as i64;
    match OffsetDateTime::from_unix_timestamp(seconds) {
        Ok(value) => value
            .format(&Rfc3339)
            .map_or_else(|_| timestamp_ms.to_string(), |value| value.replace(':', "")),
        Err(_) => timestamp_ms.to_string(),
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
        let stream = CleanupFileStream::new(self.file, self.cleanup_path);
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = StatusCode::OK;
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static(self.content_type));
        if let Ok(value) = HeaderValue::from_str(&self.len.to_string()) {
            response.headers_mut().insert(CONTENT_LENGTH, value);
        }
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        let disposition = format!("attachment; filename=\"{}\"", self.filename);
        if let Ok(value) = HeaderValue::from_str(&disposition) {
            response.headers_mut().insert(CONTENT_DISPOSITION, value);
        }
        response
    }
}

struct CleanupFileStream {
    file: Option<tokio::fs::File>,
    cleanup_path: Option<PathBuf>,
}

impl CleanupFileStream {
    fn new(file: tokio::fs::File, cleanup_path: PathBuf) -> Self {
        Self {
            file: Some(file),
            cleanup_path: Some(cleanup_path),
        }
    }

    fn cleanup(&mut self) {
        self.file = None;
        if let Some(path) = self.cleanup_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Stream for CleanupFileStream {
    type Item = std::io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Some(file) = self.file.as_mut() else {
            return Poll::Ready(None);
        };
        let mut buf = vec![0_u8; 64 * 1024];
        let mut read_buf = ReadBuf::new(&mut buf);
        match Pin::new(file).poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => {
                let filled = read_buf.filled().len();
                if filled == 0 {
                    self.cleanup();
                    Poll::Ready(None)
                } else {
                    buf.truncate(filled);
                    Poll::Ready(Some(Ok(Bytes::from(buf))))
                }
            }
            Poll::Ready(Err(err)) => {
                self.cleanup();
                Poll::Ready(Some(Err(err)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for CleanupFileStream {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
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
    fn download_ticket_allows_matching_export_once() {
        let state = test_state(false);
        let sid = Uuid::new_v4();
        let query = ExportQuery {
            format: "pcapng".to_string(),
            tz: Some("UTC".to_string()),
            encoding: None,
            ticket: None,
            filename: None,
        };
        let issued = state
            .tickets
            .issue(ticket_scope(&sid.to_string(), &query, &state.defaults).unwrap());
        let ticket_query = ExportQuery {
            ticket: Some(issued.ticket),
            ..query
        };
        let headers = HeaderMap::new();
        let peer: SocketAddr = "192.0.2.10:1234".parse().unwrap();

        assert!(
            authorize_export_request(&headers, &peer, &state, &sid.to_string(), &ticket_query,)
                .is_ok()
        );
        let err =
            authorize_export_request(&headers, &peer, &state, &sid.to_string(), &ticket_query)
                .unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn download_ticket_rejects_changed_export_parameters() {
        let state = test_state(false);
        let sid = Uuid::new_v4();
        let query = ExportQuery {
            format: "text".to_string(),
            tz: None,
            encoding: None,
            ticket: None,
            filename: None,
        };
        let issued = state
            .tickets
            .issue(ticket_scope(&sid.to_string(), &query, &state.defaults).unwrap());
        let changed_query = ExportQuery {
            format: "jsonl".to_string(),
            ticket: Some(issued.ticket),
            filename: None,
            ..query
        };
        let headers = HeaderMap::new();
        let peer: SocketAddr = "192.0.2.10:1234".parse().unwrap();

        let err =
            authorize_export_request(&headers, &peer, &state, &sid.to_string(), &changed_query)
                .unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert!(err.message.contains("does not match"));
    }

    #[test]
    fn bundle_download_ticket_is_single_use() {
        let state = test_state(false);
        let request = ExportBundleTicketRequest {
            entries: vec![ExportBundleEntry {
                sid: Uuid::new_v4().to_string(),
                source_name: Some("demo".to_string()),
                encoding: None,
            }],
            format: "text".to_string(),
            tz: None,
            filename_pattern: None,
            timestamp_ms: None,
        };
        let issued = state.bundle_tickets.issue(request.clone());

        assert_eq!(
            state
                .bundle_tickets
                .consume(&issued.ticket)
                .unwrap()
                .entries[0]
                .sid,
            request.entries[0].sid
        );
        let err = state.bundle_tickets.consume(&issued.ticket).unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
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
                ticket: None,
                filename: None,
            },
        )
        .await
        .unwrap();
        let body = String::from_utf8(artifact_bytes(artifact).await).unwrap();
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
                ticket: None,
                filename: None,
            },
        )
        .await
        .unwrap();
        let body = String::from_utf8(artifact_bytes(artifact).await).unwrap();
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
                ticket: None,
                filename: None,
            },
        )
        .await
        .unwrap();

        assert!(Path::new(&artifact.filename)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pcapng")));
        assert_eq!(artifact.content_type, "application/vnd.tcpdump.pcapng");
        let cleanup_path = artifact.cleanup_path.clone();
        assert!(cleanup_path.exists());
        let body = artifact_bytes(artifact).await;
        assert!(body.starts_with(&0x0A0D_0D0Au32.to_le_bytes()));
        assert!(!cleanup_path.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn exports_multiple_sources_as_bundle_zip() {
        let root = std::env::temp_dir().join(format!("wanlogger-export-api-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let input_a = root.join("a.txt");
        let input_b = root.join("b.txt");
        std::fs::write(&input_a, b"alpha\n").unwrap();
        std::fs::write(&input_b, b"beta\n").unwrap();
        let ingest = Arc::new(crate::ingest::Ingest::new());
        let manager = Arc::new(SourceManager::with_session_root(
            ingest,
            root.join("sessions"),
        ));
        let sid_a = manager
            .start_spec(ChannelSpec::File {
                path: input_a.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        let sid_b = manager
            .start_spec(ChannelSpec::File {
                path: input_b.to_string_lossy().to_string(),
                follow: false,
            })
            .await
            .unwrap();
        manager.wait(sid_a).await.unwrap().unwrap();
        manager.wait(sid_b).await.unwrap().unwrap();
        let state = ExportRouteState::new(manager, Arc::new(BearerVerifier::new()), true);

        let artifact = export_bundle(
            &state,
            ExportBundleTicketRequest {
                entries: vec![
                    ExportBundleEntry {
                        sid: sid_a.to_string(),
                        source_name: Some("source-a".to_string()),
                        encoding: None,
                    },
                    ExportBundleEntry {
                        sid: sid_b.to_string(),
                        source_name: Some("source-b".to_string()),
                        encoding: None,
                    },
                ],
                format: "text".to_string(),
                tz: None,
                filename_pattern: Some("{source}-{timestamp}.{ext}".to_string()),
                timestamp_ms: Some(1_780_134_200_000),
            },
        )
        .await
        .unwrap();

        assert!(Path::new(&artifact.filename)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip")));
        assert_eq!(artifact.content_type, "application/zip");
        let body = artifact_bytes(artifact).await;
        assert!(body.starts_with(b"PK\x03\x04"));
        assert!(contains_bytes(&body, b"source-a"));
        assert!(contains_bytes(&body, b"source-b"));
        assert!(contains_bytes(&body, b"alpha"));
        assert!(contains_bytes(&body, b"beta"));
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
                ticket: None,
                filename: None,
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

    async fn artifact_bytes(artifact: ExportArtifact) -> Vec<u8> {
        let response = artifact.into_response();
        let bytes = to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        bytes.to_vec()
    }

    fn contains_bytes(body: &[u8], needle: &[u8]) -> bool {
        body.windows(needle.len()).any(|window| window == needle)
    }

    fn test_state(no_auth: bool) -> ExportRouteState {
        let ingest = Arc::new(crate::ingest::Ingest::new());
        let manager = Arc::new(SourceManager::with_session_root(
            ingest,
            std::env::temp_dir().join("wanlogger-export-ticket-test"),
        ));
        ExportRouteState::new(manager, Arc::new(BearerVerifier::new()), no_auth)
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
