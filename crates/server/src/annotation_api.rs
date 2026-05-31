//! HTTP annotation endpoint backed by server-owned metadata storage.
//!
//! This module deliberately keeps annotations outside frozen WSS `data`
//! frames and outside v0.1 session-dir files. See
//! `docs/adr/0002-server-owned-annotations.md`.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Path as AxumPath, Query, State};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::{is_loopback_allowed, BearerVerifier};

/// Maximum annotation body length, matching the web UI local note limit.
pub const MAX_ANNOTATION_TEXT_LEN: usize = 20_000;
/// Maximum log-type annotation key length, matching the web UI local note limit.
pub const MAX_LOG_TYPE_KEY_LEN: usize = 120;

const METADATA_DIR: &str = ".tracemux";
const ANNOTATIONS_FILE: &str = "annotations-v1.jsonl";

/// Server-owned annotation store.
#[derive(Debug)]
pub struct AnnotationStore {
    path: PathBuf,
    annotations: Mutex<BTreeMap<Uuid, Annotation>>,
}

/// State required by annotation HTTP routes.
#[derive(Debug, Clone)]
pub struct AnnotationRouteState {
    store: Arc<AnnotationStore>,
    auth: Arc<BearerVerifier>,
    no_auth: bool,
}

impl AnnotationRouteState {
    /// Create state for `/api/annotations` routes.
    #[must_use]
    pub fn new(store: Arc<AnnotationStore>, auth: Arc<BearerVerifier>, no_auth: bool) -> Self {
        Self {
            store,
            auth,
            no_auth,
        }
    }
}

/// Annotation target kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationTargetKind {
    /// Annotation attached to a server-known source/session id.
    Session,
    /// Annotation attached to a log type or classification key.
    LogType,
}

/// Stable logical target for an annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationTarget {
    /// Target kind.
    pub kind: AnnotationTargetKind,
    /// Optional source/session id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<Uuid>,
    /// Optional log-type key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// Server-owned annotation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    /// Stable annotation id.
    pub id: Uuid,
    /// Logical annotation target.
    pub target: AnnotationTarget,
    /// User-authored note text.
    pub text: String,
    /// Last update timestamp.
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Optional display label for the actor that last updated this note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    /// Tombstone marker reserved for append-only stores and future sync.
    #[serde(default)]
    pub deleted: bool,
}

/// Attach the annotation routes.
pub fn router(state: AnnotationRouteState) -> Router {
    Router::new()
        .route("/api/annotations", get(list_handler))
        .route(
            "/api/annotations/:id",
            put(upsert_handler).delete(delete_handler),
        )
        .with_state(state)
}

impl AnnotationStore {
    /// Open or create an annotation store under `session_root`.
    pub fn open(session_root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let metadata_dir = session_root.as_ref().join(METADATA_DIR);
        std::fs::create_dir_all(&metadata_dir)?;
        let path = metadata_dir.join(ANNOTATIONS_FILE);
        let annotations = load_annotations(&path).map_err(|e| anyhow::anyhow!(e.message))?;
        Ok(Self {
            path,
            annotations: Mutex::new(annotations),
        })
    }

    fn list(&self, sid: Option<Uuid>) -> Vec<Annotation> {
        self.annotations
            .lock()
            .values()
            .filter(|annotation| !annotation.deleted)
            .filter(|annotation| annotation_visible_for_sid(annotation, sid))
            .cloned()
            .collect()
    }

    fn upsert(
        &self,
        id: Uuid,
        target: AnnotationTarget,
        text: String,
        updated_by: Option<&str>,
        updated_at: OffsetDateTime,
    ) -> Result<Annotation, AnnotationApiError> {
        let target = normalize_target(target)?;
        let text = normalize_text(text)?;
        let updated_by = updated_by.and_then(normalize_actor);
        let annotation = Annotation {
            id,
            target,
            text,
            updated_at,
            updated_by,
            deleted: false,
        };
        let mut annotations = self.annotations.lock();
        annotations.insert(id, annotation.clone());
        self.persist_locked(&annotations)?;
        Ok(annotation)
    }

    fn delete(&self, id: Uuid) -> Result<bool, AnnotationApiError> {
        let mut annotations = self.annotations.lock();
        let removed = annotations.remove(&id).is_some();
        if removed {
            self.persist_locked(&annotations)?;
        }
        Ok(removed)
    }

    fn persist_locked(
        &self,
        annotations: &BTreeMap<Uuid, Annotation>,
    ) -> Result<(), AnnotationApiError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(store_error)?;
        }
        let tmp = self
            .path
            .with_extension(format!("jsonl.tmp-{}", Uuid::new_v4()));
        let mut body = String::new();
        for annotation in annotations
            .values()
            .filter(|annotation| !annotation.deleted)
        {
            let line = serde_json::to_string(annotation).map_err(store_error)?;
            body.push_str(&line);
            body.push('\n');
        }
        std::fs::write(&tmp, body).map_err(store_error)?;
        if self.path.exists() {
            std::fs::remove_file(&self.path).map_err(store_error)?;
        }
        std::fs::rename(&tmp, &self.path).map_err(store_error)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AnnotationListQuery {
    sid: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnnotationUpsertRequest {
    target: AnnotationTarget,
    text: String,
    updated_by: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnnotationErrorBody {
    error_id: &'static str,
    message: String,
}

#[derive(Debug)]
struct AnnotationApiError {
    status: StatusCode,
    error_id: &'static str,
    message: String,
}

async fn list_handler(
    State(state): State<AnnotationRouteState>,
    Query(query): Query<AnnotationListQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = authorize_annotations(&headers, &peer, &state.auth, state.no_auth) {
        return err.into_response();
    }
    let sid = match parse_optional_sid(query.sid.as_deref()) {
        Ok(sid) => sid,
        Err(err) => return err.into_response(),
    };
    Json(state.store.list(sid)).into_response()
}

async fn upsert_handler(
    State(state): State<AnnotationRouteState>,
    AxumPath(id_text): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<AnnotationUpsertRequest>,
) -> Response {
    if let Err(err) = authorize_annotations(&headers, &peer, &state.auth, state.no_auth) {
        return err.into_response();
    }
    let id = match parse_annotation_id(&id_text) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    match state.store.upsert(
        id,
        request.target,
        request.text,
        request.updated_by.as_deref(),
        OffsetDateTime::now_utc(),
    ) {
        Ok(annotation) => Json(annotation).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn delete_handler(
    State(state): State<AnnotationRouteState>,
    AxumPath(id_text): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(err) = authorize_annotations(&headers, &peer, &state.auth, state.no_auth) {
        return err.into_response();
    }
    let id = match parse_annotation_id(&id_text) {
        Ok(id) => id,
        Err(err) => return err.into_response(),
    };
    match state.store.delete(id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => AnnotationApiError::not_found(format!("annotation `{id}` was not found"))
            .into_response(),
        Err(err) => err.into_response(),
    }
}

fn parse_optional_sid(value: Option<&str>) -> Result<Option<Uuid>, AnnotationApiError> {
    value
        .filter(|sid| !sid.trim().is_empty())
        .map(|sid| {
            Uuid::parse_str(sid.trim()).map_err(|e| {
                AnnotationApiError::bad_request(format!("invalid sid `{}`: {e}", sid.trim()))
            })
        })
        .transpose()
}

fn parse_annotation_id(value: &str) -> Result<Uuid, AnnotationApiError> {
    Uuid::parse_str(value.trim()).map_err(|e| {
        AnnotationApiError::bad_request(format!("invalid annotation id `{}`: {e}", value.trim()))
    })
}

fn authorize_annotations(
    headers: &HeaderMap,
    peer: &SocketAddr,
    auth: &BearerVerifier,
    no_auth: bool,
) -> Result<(), AnnotationApiError> {
    if no_auth && is_loopback_allowed(peer) {
        return Ok(());
    }
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = token else {
        return Err(AnnotationApiError::auth("missing bearer token"));
    };
    auth.verify(token)
        .map_err(|_| AnnotationApiError::auth("bearer token rejected"))
}

fn load_annotations(path: &Path) -> Result<BTreeMap<Uuid, Annotation>, AnnotationApiError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(path).map_err(store_error)?;
    let mut out = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let annotation: Annotation = serde_json::from_str(line).map_err(|e| {
            AnnotationApiError::internal(format!(
                "reading annotation line {} from {}: {e}",
                index + 1,
                path.display()
            ))
        })?;
        if annotation.deleted {
            out.remove(&annotation.id);
        } else {
            out.insert(annotation.id, annotation);
        }
    }
    Ok(out)
}

fn annotation_visible_for_sid(annotation: &Annotation, sid: Option<Uuid>) -> bool {
    let Some(sid) = sid else {
        return true;
    };
    match annotation.target.kind {
        AnnotationTargetKind::Session => annotation.target.sid == Some(sid),
        AnnotationTargetKind::LogType => {
            annotation.target.sid.is_none() || annotation.target.sid == Some(sid)
        }
    }
}

fn normalize_target(target: AnnotationTarget) -> Result<AnnotationTarget, AnnotationApiError> {
    match target.kind {
        AnnotationTargetKind::Session => {
            let Some(sid) = target.sid else {
                return Err(AnnotationApiError::bad_request(
                    "session annotations require target.sid",
                ));
            };
            Ok(AnnotationTarget {
                kind: AnnotationTargetKind::Session,
                sid: Some(sid),
                key: None,
            })
        }
        AnnotationTargetKind::LogType => {
            let key = target.key.unwrap_or_default();
            let key = key.trim();
            if key.is_empty() {
                return Err(AnnotationApiError::bad_request(
                    "log_type annotations require target.key",
                ));
            }
            if key.chars().count() > MAX_LOG_TYPE_KEY_LEN {
                return Err(AnnotationApiError::bad_request(format!(
                    "log_type target.key must be at most {MAX_LOG_TYPE_KEY_LEN} characters"
                )));
            }
            Ok(AnnotationTarget {
                kind: AnnotationTargetKind::LogType,
                sid: target.sid,
                key: Some(key.to_string()),
            })
        }
    }
}

fn normalize_text(text: String) -> Result<String, AnnotationApiError> {
    if text.chars().count() > MAX_ANNOTATION_TEXT_LEN {
        return Err(AnnotationApiError::bad_request(format!(
            "annotation text must be at most {MAX_ANNOTATION_TEXT_LEN} characters"
        )));
    }
    Ok(text)
}

fn normalize_actor(actor: &str) -> Option<String> {
    let actor = actor.trim();
    if actor.is_empty() {
        None
    } else {
        Some(actor.chars().take(120).collect())
    }
}

fn store_error(error: impl std::fmt::Display) -> AnnotationApiError {
    AnnotationApiError::internal(format!("annotation store failed: {error}"))
}

impl AnnotationApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error_id: "E-2001",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error_id: "E-1001",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error_id: "E-1001",
            message: message.into(),
        }
    }

    fn auth(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            error_id: "E-2101",
            message: message.into(),
        }
    }
}

impl IntoResponse for AnnotationApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(AnnotationErrorBody {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::extract::ConnectInfo;
    use axum::http::{Method, Request};
    use serde::de::DeserializeOwned;
    use tower::Service;

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("tracemux-annotation-api-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn session_target(sid: Uuid) -> AnnotationTarget {
        AnnotationTarget {
            kind: AnnotationTargetKind::Session,
            sid: Some(sid),
            key: None,
        }
    }

    fn log_type_target(sid: Option<Uuid>, key: &str) -> AnnotationTarget {
        AnnotationTarget {
            kind: AnnotationTargetKind::LogType,
            sid,
            key: Some(key.to_string()),
        }
    }

    #[test]
    fn store_persists_and_reloads_annotations() {
        let root = temp_root();
        let sid = Uuid::new_v4();
        let id = Uuid::new_v4();
        let store = AnnotationStore::open(&root).unwrap();

        let annotation = store
            .upsert(
                id,
                session_target(sid),
                "operator note".to_string(),
                Some(" alice "),
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();

        assert_eq!(annotation.id, id);
        assert_eq!(annotation.target.sid, Some(sid));
        assert_eq!(annotation.updated_by.as_deref(), Some("alice"));

        let reopened = AnnotationStore::open(&root).unwrap();
        assert_eq!(reopened.list(Some(sid)), vec![annotation]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn list_filters_by_session_and_includes_global_log_types() {
        let root = temp_root();
        let sid = Uuid::new_v4();
        let other_sid = Uuid::new_v4();
        let store = AnnotationStore::open(&root).unwrap();
        let global = store
            .upsert(
                Uuid::new_v4(),
                log_type_target(None, " fault "),
                "global".to_string(),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();
        let local = store
            .upsert(
                Uuid::new_v4(),
                log_type_target(Some(sid), "trace"),
                "local".to_string(),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();
        let other = store
            .upsert(
                Uuid::new_v4(),
                session_target(other_sid),
                "other".to_string(),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();

        assert_eq!(global.target.key.as_deref(), Some("fault"));
        assert_eq!(local.target.sid, Some(sid));
        assert_eq!(other.target.sid, Some(other_sid));

        let visible = store.list(Some(sid));
        let mut visible_texts: Vec<_> = visible
            .iter()
            .map(|annotation| annotation.text.as_str())
            .collect();
        visible_texts.sort_unstable();
        assert_eq!(visible_texts, vec!["global", "local"]);

        let other_visible = store.list(Some(other_sid));
        let mut other_visible_texts: Vec<_> = other_visible
            .iter()
            .map(|annotation| annotation.text.as_str())
            .collect();
        other_visible_texts.sort_unstable();
        assert_eq!(other_visible_texts, vec!["global", "other"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_annotation_from_store() {
        let root = temp_root();
        let sid = Uuid::new_v4();
        let id = Uuid::new_v4();
        let store = AnnotationStore::open(&root).unwrap();
        store
            .upsert(
                id,
                session_target(sid),
                "temporary".to_string(),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();

        assert!(store.delete(id).unwrap());
        assert!(!store.delete(id).unwrap());
        assert!(store.list(Some(sid)).is_empty());
        let reopened = AnnotationStore::open(&root).unwrap();
        assert!(reopened.list(Some(sid)).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_invalid_targets_and_too_long_text() {
        let root = temp_root();
        let store = AnnotationStore::open(&root).unwrap();
        let id = Uuid::new_v4();
        let err = store
            .upsert(
                id,
                AnnotationTarget {
                    kind: AnnotationTargetKind::Session,
                    sid: None,
                    key: None,
                },
                String::new(),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);

        let err = store
            .upsert(
                id,
                log_type_target(None, "   "),
                String::new(),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);

        let err = store
            .upsert(
                id,
                session_target(Uuid::new_v4()),
                "x".repeat(MAX_ANNOTATION_TEXT_LEN + 1),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn auth_allows_loopback_no_auth() {
        let headers = HeaderMap::new();
        let peer: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        assert!(authorize_annotations(&headers, &peer, &BearerVerifier::new(), true).is_ok());
    }

    #[test]
    fn auth_rejects_missing_token_when_not_loopback_no_auth() {
        let headers = HeaderMap::new();
        let peer: SocketAddr = "192.0.2.10:1234".parse().unwrap();
        let err = authorize_annotations(&headers, &peer, &BearerVerifier::new(), true).unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.error_id, "E-2101");
    }

    #[tokio::test]
    async fn router_builds() {
        let root = temp_root();
        let store = Arc::new(AnnotationStore::open(&root).unwrap());
        let state = AnnotationRouteState::new(store, Arc::new(BearerVerifier::new()), true);
        let _ = router(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn routes_round_trip_annotations() {
        let root = temp_root();
        let store = Arc::new(AnnotationStore::open(&root).unwrap());
        let state = AnnotationRouteState::new(store, Arc::new(BearerVerifier::new()), true);
        let mut app = router(state);
        let peer: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let sid = Uuid::new_v4();
        let id = Uuid::new_v4();

        let saved = call_json::<Annotation>(
            &mut app,
            peer,
            Method::PUT,
            &format!("/api/annotations/{id}"),
            Some(serde_json::json!({
                "target": { "kind": "session", "sid": sid },
                "text": "route memo",
                "updated_by": "route-test"
            })),
        )
        .await;
        assert_eq!(saved.id, id);
        assert_eq!(saved.target.sid, Some(sid));
        assert_eq!(saved.text, "route memo");

        let listed = call_json::<Vec<Annotation>>(
            &mut app,
            peer,
            Method::GET,
            &format!("/api/annotations?sid={sid}"),
            None,
        )
        .await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);

        let deleted = call_response(
            &mut app,
            peer,
            Method::DELETE,
            &format!("/api/annotations/{id}"),
            None,
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

        let listed = call_json::<Vec<Annotation>>(
            &mut app,
            peer,
            Method::GET,
            &format!("/api/annotations?sid={sid}"),
            None,
        )
        .await;
        assert!(listed.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn routes_reject_missing_auth_when_required() {
        let root = temp_root();
        let store = Arc::new(AnnotationStore::open(&root).unwrap());
        let state = AnnotationRouteState::new(store, Arc::new(BearerVerifier::new()), false);
        let mut app = router(state);
        let peer: SocketAddr = "127.0.0.1:1234".parse().unwrap();

        let response = call_response(&mut app, peer, Method::GET, "/api/annotations", None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(WWW_AUTHENTICATE).unwrap(),
            HeaderValue::from_static("Bearer")
        );
        let body = response_json::<serde_json::Value>(response).await;
        assert_eq!(body["error_id"], "E-2101");
        let _ = std::fs::remove_dir_all(root);
    }

    async fn call_json<T>(
        app: &mut Router,
        peer: SocketAddr,
        method: Method,
        uri: &str,
        json: Option<serde_json::Value>,
    ) -> T
    where
        T: DeserializeOwned,
    {
        let response = call_response(app, peer, method, uri, json).await;
        assert_eq!(response.status(), StatusCode::OK);
        response_json(response).await
    }

    async fn call_response(
        app: &mut Router,
        peer: SocketAddr,
        method: Method,
        uri: &str,
        json: Option<serde_json::Value>,
    ) -> Response {
        let body = json.map_or_else(String::new, |value| value.to_string());
        let mut builder = Request::builder().method(method).uri(uri);
        if !body.is_empty() {
            builder = builder.header("content-type", "application/json");
        }
        let mut request = builder.body(Body::from(body)).unwrap();
        request.extensions_mut().insert(ConnectInfo(peer));
        app.call(request).await.unwrap()
    }

    async fn response_json<T>(response: Response) -> T
    where
        T: DeserializeOwned,
    {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
}
