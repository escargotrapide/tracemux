//! WebSocket handler (subprotocol `wanlogger.v1`). **Critical path.**
//!
//! Frozen v0.1. See `docs/protocols/wire-protocol.md`.
//!
//! Wires:
//!
//! * Subprotocol negotiation -- the server only advertises
//!   `wanlogger.v1`. Bearer tokens (`bearer.<token>`) live in the
//!   same `Sec-WebSocket-Protocol` header and are stripped before
//!   negotiation; see [`crate::auth`].
//! * Connection cap via [`crate::ratelimit::ConnCounter`].
//! * Per-frame size cap via [`crate::wire::MAX_FRAME_BYTES`].
//! * Frame loop: decode [`crate::wire::Envelope`], reply to `ping`
//!   with `pong`, dispatch `sub` / `unsub` to session fan-out
//!   streams owned by [`crate::ingest`], and dispatch `ctl`
//!   lifecycle actions to [`crate::source_manager`].

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use rmpv::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;
use wanlogger_core::classify::{ClassificationRule, LogClassifier};
use wanlogger_core::detect::content::{ContentDetectionReport, DetectionMode};
use wanlogger_core::source::pcap::{
    PcapPublishMode, PcapSaveMode, DEFAULT_SNAPLEN, DEFAULT_TIMEOUT_MS,
};
use wanlogger_core::{source::ChannelSpec, ErrorId, WanloggerError};

use crate::audit::{AuditEvent, AuditKind, AuditLog, AuditResult};
use crate::auth::{extract_bearer, is_loopback_allowed, BearerVerifier};
use crate::ingest::Ingest;
use crate::ratelimit::ConnCounter;
use crate::source_manager::{SourceManager, SourceStartOptions, SourceStatus};
use crate::wire::{decode, encode, Envelope, FrameType, MAX_FRAME_BYTES};

/// Subprotocol token advertised by the server.
pub const SUBPROTOCOL: &str = "wanlogger.v1";

/// Shared state injected into the `/ws` handler.
#[derive(Debug, Clone)]
pub struct WsState {
    /// Bearer-token verifier. Empty → only loopback + `no_auth` works.
    pub auth: Arc<BearerVerifier>,
    /// Whether the operator passed `--no-auth`. Honoured only on
    /// loopback (FR-WIRE-002).
    pub no_auth: bool,
    /// Connection cap.
    pub conns: Arc<ConnCounter>,
    /// Ingest/session state used for WSS subscriptions.
    pub ingest: Arc<Ingest>,
    /// Source lifecycle manager used by `ctl` lifecycle actions.
    pub source_manager: Arc<SourceManager>,
    /// Optional append-only audit log for write-back requests.
    pub audit: Option<Arc<AuditLog>>,
}

impl WsState {
    /// Build a state for the given verifier and policy.
    #[must_use]
    pub fn new(auth: BearerVerifier, no_auth: bool, conns: Arc<ConnCounter>) -> Self {
        Self::with_ingest(auth, no_auth, conns, Arc::new(Ingest::new()))
    }

    /// Build a state with shared ingest/session state.
    #[must_use]
    pub fn with_ingest(
        auth: BearerVerifier,
        no_auth: bool,
        conns: Arc<ConnCounter>,
        ingest: Arc<Ingest>,
    ) -> Self {
        let source_manager = Arc::new(SourceManager::new(ingest));
        Self::with_source_manager(auth, no_auth, conns, source_manager)
    }

    /// Build a state with a shared source lifecycle manager.
    #[must_use]
    pub fn with_source_manager(
        auth: BearerVerifier,
        no_auth: bool,
        conns: Arc<ConnCounter>,
        source_manager: Arc<SourceManager>,
    ) -> Self {
        let ingest = source_manager.ingest();
        Self {
            auth: Arc::new(auth),
            no_auth,
            conns,
            ingest,
            source_manager,
            audit: None,
        }
    }

    /// Attach an audit log to this WSS state.
    #[must_use]
    pub fn with_audit(mut self, audit: Arc<AuditLog>) -> Self {
        self.audit = Some(audit);
        self
    }
}

/// Attach the `/ws` route to a router.
pub fn router(state: WsState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// Outcome of the auth step. Exposed for testing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    /// Authenticated by bearer token.
    Bearer,
    /// Loopback peer with `--no-auth` set.
    LoopbackNoAuth,
    /// Rejected -- send 401.
    Rejected(&'static str),
}

/// Decide whether the request is authorised.
///
/// Pure function so it can be unit-tested without spinning up a
/// listener.
#[must_use]
pub fn check_auth(
    header_value: Option<&str>,
    peer: &SocketAddr,
    auth: &BearerVerifier,
    no_auth: bool,
) -> AuthOutcome {
    if let Some(h) = header_value {
        if let Some(tok) = extract_bearer(h) {
            return if auth.verify(tok).is_ok() {
                AuthOutcome::Bearer
            } else {
                AuthOutcome::Rejected("bearer rejected")
            };
        }
    }
    if no_auth && is_loopback_allowed(peer) {
        return AuthOutcome::LoopbackNoAuth;
    }
    AuthOutcome::Rejected("no bearer and not loopback-no-auth")
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<WsState>,
    headers: HeaderMap,
) -> Response {
    let header_value = headers
        .get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok());

    match check_auth(header_value, &peer, &state.auth, state.no_auth) {
        AuthOutcome::Bearer | AuthOutcome::LoopbackNoAuth => {}
        AuthOutcome::Rejected(reason) => {
            tracing::warn!(%peer, reason, "ws: auth rejected");
            return (StatusCode::UNAUTHORIZED, "auth rejected").into_response();
        }
    }

    let Some(guard) = state.conns.acquire_owned() else {
        tracing::warn!(%peer, "ws: connection cap reached");
        return (StatusCode::SERVICE_UNAVAILABLE, "connection cap reached").into_response();
    };

    ws.protocols([SUBPROTOCOL])
        .max_frame_size(MAX_FRAME_BYTES)
        .max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| async move {
            let g = guard;
            handle_socket_with_source_manager_and_audit(
                socket,
                peer,
                state.ingest,
                state.source_manager,
                state.audit,
            )
            .await;
            drop(g);
        })
}

/// Per-connection frame loop. Public so integration tests in
/// `tests/` can spin it up directly.
pub async fn handle_socket(socket: WebSocket, peer: SocketAddr) {
    handle_socket_with_ingest(socket, peer, Arc::new(Ingest::new())).await;
}

/// Per-connection frame loop with shared ingest/session state.
pub async fn handle_socket_with_ingest(socket: WebSocket, peer: SocketAddr, ingest: Arc<Ingest>) {
    let source_manager = Arc::new(SourceManager::new(ingest.clone()));
    handle_socket_with_source_manager(socket, peer, ingest, source_manager).await;
}

/// Per-connection frame loop with shared ingest/session/source manager state.
pub async fn handle_socket_with_source_manager(
    socket: WebSocket,
    peer: SocketAddr,
    ingest: Arc<Ingest>,
    source_manager: Arc<SourceManager>,
) {
    handle_socket_with_source_manager_and_audit(socket, peer, ingest, source_manager, None).await;
}

async fn handle_socket_with_source_manager_and_audit(
    socket: WebSocket,
    peer: SocketAddr,
    ingest: Arc<Ingest>,
    source_manager: Arc<SourceManager>,
    audit: Option<Arc<AuditLog>>,
) {
    tracing::info!(%peer, "ws: connected");
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(64);
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });
    let mut subscriptions: HashMap<SubscriptionKey, JoinHandle<()>> = HashMap::new();

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(%peer, error = %e, "ws: recv error");
                break;
            }
        };
        match msg {
            Message::Binary(bytes) => {
                let env = match decode(&bytes) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::debug!(%peer, error = %e, "ws: decode error");
                        // Soft-drop; do not tear down the connection
                        // for a single bad frame in v0.1.
                        continue;
                    }
                };
                if let Some(reply) = handle_envelope(&env) {
                    if !queue_envelope(&out_tx, &reply, peer).await {
                        break;
                    }
                } else if !dispatch_envelope(
                    &env,
                    &ingest,
                    &source_manager,
                    audit.as_ref(),
                    &out_tx,
                    &mut subscriptions,
                    peer,
                )
                .await
                {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Ping(p) => {
                let _ = out_tx.send(Message::Pong(p)).await;
            }
            Message::Pong(_) | Message::Text(_) => {
                // v0.1 carries no text frames; ignore.
            }
        }
    }

    for (_, task) in subscriptions {
        task.abort();
    }
    drop(out_tx);
    let _ = writer.await;
    tracing::info!(%peer, "ws: closed");
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SubscriptionKey {
    sid: String,
    ch: Option<u32>,
}

impl SubscriptionKey {
    fn from_env(env: &Envelope) -> Result<Self, Envelope> {
        let Some(sid) = env.sid.clone() else {
            return Err(ctl_error(env.seq, "subscription missing sid"));
        };
        Ok(Self { sid, ch: env.ch })
    }
}

async fn dispatch_envelope(
    env: &Envelope,
    ingest: &Arc<Ingest>,
    source_manager: &Arc<SourceManager>,
    audit: Option<&Arc<AuditLog>>,
    out_tx: &mpsc::Sender<Message>,
    subscriptions: &mut HashMap<SubscriptionKey, JoinHandle<()>>,
    peer: SocketAddr,
) -> bool {
    match env.kind {
        FrameType::Sub => subscribe_session(env, ingest, out_tx, subscriptions, peer).await,
        FrameType::Unsub => unsubscribe_session(env, out_tx, subscriptions, peer).await,
        FrameType::Ctl => handle_ctl(env, source_manager, out_tx, peer).await,
        FrameType::Write => handle_write(env, source_manager, audit, out_tx, peer).await,
        _ => true,
    }
}

async fn handle_write(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    audit: Option<&Arc<AuditLog>>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    // REQ: FR-SINK-WIRE
    let sid = match parse_write_sid(env) {
        Ok(sid) => sid,
        Err(reply) => {
            audit_write(
                audit,
                peer,
                env.sid.clone().unwrap_or_else(|| "<missing>".to_string()),
                AuditResult::Denied,
                serde_json::json!({ "seq": env.seq, "reason": "invalid sid" }),
            );
            return queue_envelope(out_tx, &reply, peer).await;
        }
    };
    let ch = env.ch.unwrap_or(0);
    let body = match map_get(&env.payload, "body") {
        Some(Value::Binary(body)) => bytes::Bytes::copy_from_slice(body),
        _ => {
            audit_write(
                audit,
                peer,
                format!("{sid}/ch{ch}"),
                AuditResult::Denied,
                serde_json::json!({ "seq": env.seq, "reason": "missing body" }),
            );
            let reply = ctl_error(env.seq, "write payload.body bin is required");
            return queue_envelope(out_tx, &reply, peer).await;
        }
    };
    let target = map_str(&env.payload, "target").map(ToString::to_string);
    match source_manager.write(sid, ch, body, target).await {
        Ok(bytes_written) => {
            tracing::info!(%peer, %sid, ch, bytes_written, "ws: write-back accepted");
            audit_write(
                audit,
                peer,
                format!("{sid}/ch{ch}"),
                AuditResult::Ok,
                serde_json::json!({ "seq": env.seq, "bytes_written": bytes_written }),
            );
            let reply = write_ack(env.seq, sid, ch, bytes_written);
            queue_envelope(out_tx, &reply, peer).await
        }
        Err(err) => {
            tracing::warn!(%peer, %sid, ch, error = %err, "ws: write-back failed");
            audit_write(
                audit,
                peer,
                format!("{sid}/ch{ch}"),
                AuditResult::Error,
                serde_json::json!({ "seq": env.seq, "error": err.to_string() }),
            );
            let reply = ctl_error_with_id(
                env.seq,
                format!("write failed: {err}"),
                anyhow_error_id(&err),
            );
            queue_envelope(out_tx, &reply, peer).await
        }
    }
}

async fn handle_ctl(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    let Some(action) = map_str(&env.payload, "action") else {
        let reply = ctl_error(env.seq, "ctl action is required");
        return queue_envelope(out_tx, &reply, peer).await;
    };
    match action {
        "list" => list_sources_ctl(env, source_manager, out_tx, peer).await,
        "start" => start_source_ctl(env, source_manager, out_tx, peer).await,
        "stop" => stop_source_ctl(env, source_manager, out_tx, peer).await,
        "resume" => resume_source_ctl(env, source_manager, out_tx, peer).await,
        "restart" => restart_source_ctl(env, source_manager, out_tx, peer).await,
        "remove" => remove_source_ctl(env, source_manager, out_tx, peer).await,
        _ => {
            let reply = ctl_error(env.seq, format!("unsupported ctl action: {action}"));
            queue_envelope(out_tx, &reply, peer).await
        }
    }
}

async fn list_sources_ctl(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    let sources = source_manager.list_sources();
    let payload = Value::Map(vec![
        (
            Value::String("event".into()),
            Value::String("sources".into()),
        ),
        (
            Value::String("message".into()),
            Value::String("sources listed".into()),
        ),
        (
            Value::String("sources".into()),
            Value::Array(
                sources
                    .into_iter()
                    .map(|source| {
                        let mut row = vec![
                            (
                                Value::String("sid".into()),
                                Value::String(source.sid.to_string().into()),
                            ),
                            (
                                Value::String("name".into()),
                                Value::String(source.name.into()),
                            ),
                            (
                                Value::String("kind".into()),
                                Value::String(source.kind.into()),
                            ),
                            (
                                Value::String("status".into()),
                                Value::String(source_status_token(source.status).into()),
                            ),
                            (
                                Value::String("channels".into()),
                                Value::Array(
                                    source
                                        .channels
                                        .into_iter()
                                        .map(|ch| Value::from(u64::from(ch)))
                                        .collect(),
                                ),
                            ),
                            (
                                Value::String("bytes_in".into()),
                                Value::from(source.bytes_in),
                            ),
                            (
                                Value::String("persistent".into()),
                                Value::Boolean(source.session_dir.is_some()),
                            ),
                        ];
                        if let Some(session_dir) = source.session_dir {
                            row.push((
                                Value::String("session_dir".into()),
                                Value::String(session_dir.to_string_lossy().to_string().into()),
                            ));
                        }
                        if let Some(decoder) = source.decoder {
                            row.push((
                                Value::String("decoder".into()),
                                Value::String(decoder.into()),
                            ));
                        }
                        if let Some(encoding) = source.encoding {
                            row.push((
                                Value::String("encoding".into()),
                                Value::String(encoding.into()),
                            ));
                        }
                        if let Some(detection_mode) = source.detection_mode {
                            row.push((
                                Value::String("detection_mode".into()),
                                Value::String(detection_mode.as_str().into()),
                            ));
                        }
                        if let Some(detection) = source.detection {
                            row.push((
                                Value::String("detection".into()),
                                detection_report_value(&detection),
                            ));
                        }
                        Value::Map(row)
                    })
                    .collect(),
            ),
        ),
    ]);
    queue_envelope(
        out_tx,
        &Envelope::new(FrameType::Ctl, env.seq, payload),
        peer,
    )
    .await
}

fn detection_report_value(report: &ContentDetectionReport) -> Value {
    Value::Map(vec![
        (
            Value::String("mode".into()),
            Value::String(report.mode.as_str().into()),
        ),
        (
            Value::String("sample_bytes".into()),
            Value::from(report.sample_bytes as u64),
        ),
        (
            Value::String("configured_encoding".into()),
            Value::String(report.configured_encoding.clone().into()),
        ),
        (
            Value::String("effective_encoding".into()),
            Value::String(report.effective_encoding.clone().into()),
        ),
        (
            Value::String("sampled_encoding".into()),
            Value::String(report.sampled_encoding.clone().into()),
        ),
        (
            Value::String("encoding_candidates".into()),
            Value::Array(
                report
                    .encoding_candidates
                    .iter()
                    .map(|candidate| {
                        Value::Map(vec![
                            (
                                Value::String("label".into()),
                                Value::String(candidate.label.clone().into()),
                            ),
                            (
                                Value::String("confidence".into()),
                                Value::from(u64::from(candidate.confidence)),
                            ),
                            (
                                Value::String("had_errors".into()),
                                Value::Boolean(candidate.had_errors),
                            ),
                            (
                                Value::String("evidence".into()),
                                Value::Array(
                                    candidate
                                        .evidence
                                        .iter()
                                        .map(|item| Value::String(item.clone().into()))
                                        .collect(),
                                ),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            Value::String("log_type_candidates".into()),
            Value::Array(
                report
                    .log_type_candidates
                    .iter()
                    .map(|candidate| {
                        Value::Map(vec![
                            (
                                Value::String("tag".into()),
                                Value::String(candidate.tag.clone().into()),
                            ),
                            (
                                Value::String("kind".into()),
                                Value::String(match_kind_token(candidate.kind).into()),
                            ),
                            (
                                Value::String("pattern".into()),
                                Value::String(candidate.pattern.clone().into()),
                            ),
                            (
                                Value::String("count".into()),
                                Value::from(candidate.count as u64),
                            ),
                            (
                                Value::String("confidence".into()),
                                Value::from(u64::from(candidate.confidence)),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

fn match_kind_token(kind: wanlogger_core::classify::ClassificationMatchKind) -> &'static str {
    match kind {
        wanlogger_core::classify::ClassificationMatchKind::Contains => "contains",
        wanlogger_core::classify::ClassificationMatchKind::Regex => "regex",
    }
}

async fn start_source_ctl(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    let Some(spec_value) = map_get(&env.payload, "spec") else {
        let reply = ctl_error(env.seq, "start requires payload.spec");
        return queue_envelope(out_tx, &reply, peer).await;
    };
    let spec = match channel_spec_from_value(spec_value) {
        Ok(spec) => spec,
        Err(message) => return queue_envelope(out_tx, &ctl_error(env.seq, message), peer).await,
    };
    let start_options = match start_options_from_payload(&env.payload) {
        Ok(options) => options,
        Err(message) => return queue_envelope(out_tx, &ctl_error(env.seq, message), peer).await,
    };
    match source_manager
        .start_spec_with_options(spec, start_options)
        .await
    {
        Ok(sid) => {
            let reply = ctl_event(env.seq, "started", "source started", Some(sid));
            queue_envelope(out_tx, &reply, peer).await
        }
        Err(err) => {
            let reply = ctl_error_with_id(
                env.seq,
                format!("source start failed: {err}"),
                ErrorId::E1101SourceOpen,
            );
            queue_envelope(out_tx, &reply, peer).await
        }
    }
}

async fn stop_source_ctl(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    let sid = match parse_env_sid(env) {
        Ok(sid) => sid,
        Err(reply) => return queue_envelope(out_tx, &reply, peer).await,
    };
    if source_manager.stop(sid) {
        let reply = ctl_event(env.seq, "stopped", "source stopped", Some(sid));
        queue_envelope(out_tx, &reply, peer).await
    } else {
        let reply = ctl_error(env.seq, "source sid is not active");
        queue_envelope(out_tx, &reply, peer).await
    }
}

async fn resume_source_ctl(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    let sid = match parse_env_sid(env) {
        Ok(sid) => sid,
        Err(reply) => return queue_envelope(out_tx, &reply, peer).await,
    };
    match source_manager.resume(sid).await {
        Ok(sid) => {
            let reply = ctl_event(env.seq, "resumed", "source resumed", Some(sid));
            queue_envelope(out_tx, &reply, peer).await
        }
        Err(err) => {
            let reply = ctl_error_with_id(
                env.seq,
                format!("source resume failed: {err}"),
                lifecycle_error_id(&err),
            );
            queue_envelope(out_tx, &reply, peer).await
        }
    }
}

async fn restart_source_ctl(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    let sid = match parse_env_sid(env) {
        Ok(sid) => sid,
        Err(reply) => return queue_envelope(out_tx, &reply, peer).await,
    };
    let start_options = match start_options_from_payload(&env.payload) {
        Ok(options) => options,
        Err(message) => return queue_envelope(out_tx, &ctl_error(env.seq, message), peer).await,
    };
    match source_manager
        .restart_with_options(sid, start_options)
        .await
    {
        Ok(sid) => {
            let reply = ctl_event(env.seq, "restarted", "source restarted", Some(sid));
            queue_envelope(out_tx, &reply, peer).await
        }
        Err(err) => {
            let reply = ctl_error_with_id(
                env.seq,
                format!("source restart failed: {err}"),
                lifecycle_error_id(&err),
            );
            queue_envelope(out_tx, &reply, peer).await
        }
    }
}

async fn remove_source_ctl(
    env: &Envelope,
    source_manager: &Arc<SourceManager>,
    out_tx: &mpsc::Sender<Message>,
    peer: SocketAddr,
) -> bool {
    let sid = match parse_env_sid(env) {
        Ok(sid) => sid,
        Err(reply) => return queue_envelope(out_tx, &reply, peer).await,
    };
    if source_manager.remove(sid) {
        let reply = ctl_event(env.seq, "removed", "source removed", Some(sid));
        queue_envelope(out_tx, &reply, peer).await
    } else {
        let reply = ctl_error(env.seq, "source sid is unknown");
        queue_envelope(out_tx, &reply, peer).await
    }
}

async fn subscribe_session(
    env: &Envelope,
    ingest: &Arc<Ingest>,
    out_tx: &mpsc::Sender<Message>,
    subscriptions: &mut HashMap<SubscriptionKey, JoinHandle<()>>,
    peer: SocketAddr,
) -> bool {
    let key = match SubscriptionKey::from_env(env) {
        Ok(k) => k,
        Err(reply) => return queue_envelope(out_tx, &reply, peer).await,
    };
    if subscriptions.contains_key(&key) {
        return true;
    }
    let sid = match Uuid::parse_str(&key.sid) {
        Ok(sid) => sid,
        Err(_) => {
            let reply = ctl_error(env.seq, "subscription sid is not a UUID");
            return queue_envelope(out_tx, &reply, peer).await;
        }
    };
    let Some(session) = ingest.registry.get(&sid) else {
        let reply = ctl_error(env.seq, "subscription sid is unknown");
        return queue_envelope(out_tx, &reply, peer).await;
    };

    let mut rx = session.fanout.subscribe();
    let tx = out_tx.clone();
    let sid_for_log = key.sid.clone();
    let ch_for_log = key.ch;
    let task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(bytes) => {
                    if tx.send(Message::Binary(bytes.to_vec())).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!(sid = %sid_for_log, ch = ?ch_for_log, lagged = n, "ws: subscriber lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    subscriptions.insert(key, task);
    true
}

async fn unsubscribe_session(
    env: &Envelope,
    out_tx: &mpsc::Sender<Message>,
    subscriptions: &mut HashMap<SubscriptionKey, JoinHandle<()>>,
    peer: SocketAddr,
) -> bool {
    let key = match SubscriptionKey::from_env(env) {
        Ok(k) => k,
        Err(reply) => return queue_envelope(out_tx, &reply, peer).await,
    };
    if let Some(task) = subscriptions.remove(&key) {
        task.abort();
    }
    true
}

async fn queue_envelope(out_tx: &mpsc::Sender<Message>, env: &Envelope, peer: SocketAddr) -> bool {
    let bytes = match encode(env) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(%peer, error = %e, "ws: encode reply failed");
            return true;
        }
    };
    out_tx.send(Message::Binary(bytes)).await.is_ok()
}

fn ctl_event(seq: u64, event: &'static str, message: &'static str, sid: Option<Uuid>) -> Envelope {
    let mut payload = vec![
        (Value::String("event".into()), Value::String(event.into())),
        (
            Value::String("message".into()),
            Value::String(message.into()),
        ),
    ];
    if let Some(sid) = sid {
        payload.push((
            Value::String("sid".into()),
            Value::String(sid.to_string().into()),
        ));
    }
    let mut env = Envelope::new(FrameType::Ctl, seq, Value::Map(payload));
    if let Some(sid) = sid {
        env = env.with_sid(sid.to_string());
    }
    env
}

fn ctl_error(seq: u64, message: impl Into<String>) -> Envelope {
    ctl_error_with_id(seq, message, ErrorId::E2001WireMalformed)
}

fn ctl_error_with_id(seq: u64, message: impl Into<String>, error_id: ErrorId) -> Envelope {
    Envelope::new(
        FrameType::Ctl,
        seq,
        Value::Map(vec![
            (Value::String("event".into()), Value::String("error".into())),
            (
                Value::String("message".into()),
                Value::String(message.into().into()),
            ),
            (
                Value::String("error_id".into()),
                Value::String(error_id.code().into()),
            ),
        ]),
    )
}

fn write_ack(seq: u64, sid: Uuid, ch: u32, bytes_written: usize) -> Envelope {
    Envelope::new(
        FrameType::Ctl,
        seq,
        Value::Map(vec![
            (
                Value::String("event".into()),
                Value::String("write_ack".into()),
            ),
            (
                Value::String("message".into()),
                Value::String("write completed".into()),
            ),
            (
                Value::String("sid".into()),
                Value::String(sid.to_string().into()),
            ),
            (Value::String("ch".into()), Value::from(ch)),
            (
                Value::String("bytes_written".into()),
                Value::from(u64::try_from(bytes_written).unwrap_or(u64::MAX)),
            ),
        ]),
    )
    .with_sid(sid.to_string())
    .with_ch(ch)
}

fn anyhow_error_id(err: &anyhow::Error) -> ErrorId {
    err.downcast_ref::<WanloggerError>()
        .map_or(ErrorId::E1001PipelineGeneric, |err| err.id)
}

fn audit_write(
    audit: Option<&Arc<AuditLog>>,
    peer: SocketAddr,
    target: String,
    result: AuditResult,
    detail: serde_json::Value,
) {
    let Some(audit) = audit else {
        return;
    };
    let event = AuditEvent {
        ts: audit_ts(),
        actor: peer.to_string(),
        kind: AuditKind::WriteBack,
        target,
        result,
        detail,
    };
    if let Err(err) = audit.append(&event) {
        tracing::warn!(error = %err, "ws: failed to append write-back audit event");
    }
}

fn audit_ts() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| wanlogger_core::time::unix_ns_now().to_string())
}

fn lifecycle_error_id(err: &anyhow::Error) -> ErrorId {
    let message = err.to_string();
    if message.contains("unknown")
        || message.contains("already active")
        || message.contains("not restartable")
        || message.contains("not resumable")
    {
        ErrorId::E2001WireMalformed
    } else {
        ErrorId::E1101SourceOpen
    }
}

const fn source_status_token(status: SourceStatus) -> &'static str {
    match status {
        SourceStatus::Running => "running",
        SourceStatus::Stopped => "stopped",
        SourceStatus::Unknown => "unknown",
    }
}

fn parse_env_sid(env: &Envelope) -> Result<Uuid, Envelope> {
    let Some(sid) = env.sid.as_deref() else {
        return Err(ctl_error(env.seq, "ctl action missing sid"));
    };
    Uuid::parse_str(sid).map_err(|_| ctl_error(env.seq, "ctl sid is not a UUID"))
}

fn parse_write_sid(env: &Envelope) -> Result<Uuid, Envelope> {
    let Some(sid) = env.sid.as_deref() else {
        return Err(ctl_error(env.seq, "write missing sid"));
    };
    Uuid::parse_str(sid).map_err(|_| ctl_error(env.seq, "write sid is not a UUID"))
}

fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let Value::Map(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(k, v)| {
        if k.as_str() == Some(key) {
            Some(v)
        } else {
            None
        }
    })
}

fn map_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    map_get(value, key).and_then(Value::as_str)
}

fn required_str(value: &Value, key: &str) -> Result<String, String> {
    map_str(value, key)
        .map(ToString::to_string)
        .ok_or_else(|| format!("spec.{key} string is required"))
}

fn optional_bool(value: &Value, key: &str, default: bool) -> Result<bool, String> {
    match map_get(value, key) {
        Some(Value::Boolean(b)) => Ok(*b),
        Some(_) => Err(format!("spec.{key} bool is required")),
        None => Ok(default),
    }
}

fn optional_bool_any(value: &Value, keys: &[&str], default: bool) -> Result<bool, String> {
    for key in keys {
        if map_get(value, key).is_some() {
            return optional_bool(value, key, default);
        }
    }
    Ok(default)
}

fn optional_str(value: &Value, key: &str) -> Result<Option<String>, String> {
    match map_get(value, key) {
        Some(Value::String(s)) => Ok(s.as_str().map(str::trim).and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })),
        Some(_) => Err(format!("spec.{key} string is required")),
        None => Ok(None),
    }
}

fn optional_str_any(value: &Value, keys: &[&str]) -> Result<Option<String>, String> {
    for key in keys {
        if map_get(value, key).is_some() {
            return optional_str(value, key);
        }
    }
    Ok(None)
}

fn optional_u32(value: &Value, key: &str, default: u32) -> Result<u32, String> {
    match map_get(value, key) {
        Some(v) => v
            .as_u64()
            .ok_or_else(|| format!("spec.{key} integer is required"))
            .and_then(|n| u32::try_from(n).map_err(|_| format!("spec.{key} is out of range"))),
        None => Ok(default),
    }
}

fn optional_u32_any(value: &Value, keys: &[&str], default: u32) -> Result<u32, String> {
    for key in keys {
        if map_get(value, key).is_some() {
            return optional_u32(value, key, default);
        }
    }
    Ok(default)
}

fn optional_u32_option_any(value: &Value, keys: &[&str]) -> Result<Option<u32>, String> {
    for key in keys {
        if map_get(value, key).is_some() {
            return optional_u32(value, key, 0).map(Some);
        }
    }
    Ok(None)
}

fn required_u32(value: &Value, key: &str) -> Result<u32, String> {
    let Some(v) = map_get(value, key) else {
        return Err(format!("spec.{key} integer is required"));
    };
    let Some(n) = v.as_u64() else {
        return Err(format!("spec.{key} integer is required"));
    };
    u32::try_from(n).map_err(|_| format!("spec.{key} is out of range"))
}

fn required_u8(value: &Value, key: &str) -> Result<u8, String> {
    let Some(v) = map_get(value, key) else {
        return Err(format!("spec.{key} integer is required"));
    };
    let Some(n) = v.as_u64() else {
        return Err(format!("spec.{key} integer is required"));
    };
    u8::try_from(n).map_err(|_| format!("spec.{key} is out of range"))
}

fn required_string_array(value: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(Value::Array(items)) = map_get(value, key) else {
        return Err(format!("spec.{key} string array is required"));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToString::to_string)
                .ok_or_else(|| format!("spec.{key} items must be strings"))
        })
        .collect()
}

fn optional_payload_str(value: &Value, key: &str) -> Result<Option<String>, String> {
    match map_get(value, key) {
        Some(Value::String(s)) => Ok(s.as_str().map(str::trim).and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })),
        Some(_) => Err(format!("start.{key} string is required")),
        None => Ok(None),
    }
}

fn start_options_from_payload(value: &Value) -> Result<SourceStartOptions, String> {
    Ok(SourceStartOptions {
        encoding: optional_payload_str(value, "encoding")?,
        detection_mode: detection_mode_from_payload(value)?,
        session_name_pattern: optional_payload_str(value, "session_name_pattern")?,
        classifier: classifier_from_payload(value)?,
    })
}

fn detection_mode_from_payload(value: &Value) -> Result<Option<DetectionMode>, String> {
    let Some(raw) = optional_payload_str(value, "detection_mode")? else {
        return Ok(None);
    };
    DetectionMode::parse(&raw)
        .map(Some)
        .ok_or_else(|| "start.detection_mode must be configured, auto, suggest, or off".to_string())
}

fn classifier_from_payload(value: &Value) -> Result<Option<LogClassifier>, String> {
    let Some(raw) = map_get(value, "classifier") else {
        return Ok(None);
    };
    let Value::Array(items) = raw else {
        return Err("start.classifier array is required".to_string());
    };
    let mut rules = Vec::new();
    for item in items {
        let tag = classifier_rule_str(item, "tag")?;
        let case_sensitive = match map_get(item, "case_sensitive") {
            Some(Value::Boolean(b)) => *b,
            Some(_) => return Err("start.classifier[].case_sensitive bool is required".to_string()),
            None => false,
        };
        let contains = classifier_rule_optional_str(item, "contains")?;
        let regex = classifier_rule_optional_str(item, "regex")?;
        let Some(rule) = classifier_rule(contains, regex, tag, case_sensitive)? else {
            continue;
        };
        if !rule.is_valid() {
            return Err("start.classifier[].regex is not a valid regular expression".to_string());
        }
        rules.push(rule);
    }
    Ok(Some(LogClassifier::from_rules(rules)))
}

fn classifier_rule(
    contains: Option<String>,
    regex: Option<String>,
    tag: String,
    case_sensitive: bool,
) -> Result<Option<ClassificationRule>, String> {
    if tag.trim().is_empty() {
        return Ok(None);
    }
    match (contains, regex) {
        (Some(_), Some(_)) => {
            Err("start.classifier[] must specify only one of contains or regex".to_string())
        }
        (Some(contains), None) if !contains.trim().is_empty() => Ok(Some(
            ClassificationRule::contains_with_case(contains, tag, case_sensitive),
        )),
        (None, Some(regex)) if !regex.trim().is_empty() => Ok(Some(
            ClassificationRule::regex_with_case(regex, tag, case_sensitive),
        )),
        _ => Ok(None),
    }
}

fn classifier_rule_str(value: &Value, key: &str) -> Result<String, String> {
    map_str(value, key)
        .map(ToString::to_string)
        .ok_or_else(|| format!("start.classifier[].{key} string is required"))
}

fn classifier_rule_optional_str(value: &Value, key: &str) -> Result<Option<String>, String> {
    match map_get(value, key) {
        Some(Value::String(s)) => Ok(s.as_str().map(str::trim).and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })),
        Some(_) => Err(format!("start.classifier[].{key} string is required")),
        None => Ok(None),
    }
}

fn channel_spec_from_value(value: &Value) -> Result<ChannelSpec, String> {
    let kind = required_str(value, "kind")?;
    match kind.as_str() {
        "serial" => Ok(ChannelSpec::Serial {
            port: required_str(value, "port")?,
            baud: required_u32(value, "baud")?,
            data_bits: required_u8(value, "data_bits")?,
            parity: required_str(value, "parity")?,
            stop_bits: required_u8(value, "stop_bits")?,
            flow: required_str(value, "flow")?,
        }),
        "tcp" => Ok(ChannelSpec::Tcp {
            addr: required_str(value, "addr")?,
        }),
        "udp" => Ok(ChannelSpec::Udp {
            bind: required_str(value, "bind")?,
        }),
        "pcap" => {
            let save_mode = optional_str_any(value, &["save_mode", "save"])?
                .as_deref()
                .map(str::parse::<PcapSaveMode>)
                .transpose()?;
            let publish_mode = optional_str_any(value, &["publish_mode", "publish"])?
                .as_deref()
                .map(str::parse::<PcapPublishMode>)
                .transpose()?;
            Ok(ChannelSpec::Pcap {
                interface: required_str(value, "interface")?,
                display_name: optional_str_any(value, &["display_name", "display"])?,
                promiscuous: optional_bool_any(value, &["promiscuous", "promisc"], false)?,
                snaplen: optional_u32(value, "snaplen", DEFAULT_SNAPLEN)?,
                buffer_bytes: optional_u32_option_any(value, &["buffer_bytes", "buffer"])?,
                timeout_ms: optional_u32_any(
                    value,
                    &["timeout_ms", "timeout"],
                    DEFAULT_TIMEOUT_MS,
                )?,
                immediate: optional_bool(value, "immediate", false)?,
                filter: optional_str(value, "filter")?,
                save_mode: save_mode.unwrap_or_default(),
                pcapng_path: optional_str_any(value, &["pcapng_path", "pcapng"])?,
                publish_mode: publish_mode.unwrap_or_default(),
            })
        }
        "file" => Ok(ChannelSpec::File {
            path: required_str(value, "path")?,
            follow: optional_bool(value, "follow", false)?,
        }),
        "pipe" => Ok(ChannelSpec::Pipe {
            path: required_str(value, "path")?,
        }),
        "process" => Ok(ChannelSpec::Process {
            argv: required_string_array(value, "argv")?,
        }),
        "mock" => Ok(ChannelSpec::Mock {
            tag: required_str(value, "tag")?,
        }),
        "replay" => Ok(ChannelSpec::Replay {
            path: required_str(value, "path")?,
        }),
        "syslog" => Ok(ChannelSpec::Syslog {
            bind: required_str(value, "bind")?,
        }),
        "mqtt" => Ok(ChannelSpec::Mqtt {
            broker: required_str(value, "broker")?,
            topic: required_str(value, "topic")?,
        }),
        "http-webhook" => Ok(ChannelSpec::HttpWebhook {
            bind: required_str(value, "bind")?,
            path: required_str(value, "path")?,
        }),
        "telnet" => Ok(ChannelSpec::Telnet {
            addr: required_str(value, "addr")?,
        }),
        "ssh" => Ok(ChannelSpec::Ssh {
            addr: required_str(value, "addr")?,
            user: required_str(value, "user")?,
        }),
        "visa" => Ok(ChannelSpec::Visa {
            resource: required_str(value, "resource")?,
        }),
        "remote" => Ok(ChannelSpec::Remote {
            url: required_str(value, "url")?,
        }),
        "journald" => Ok(ChannelSpec::Journald {
            unit: map_str(value, "unit").map(ToString::to_string),
        }),
        "win-event-log" => Ok(ChannelSpec::WinEventLog {
            channel: required_str(value, "channel")?,
        }),
        "etw" => Ok(ChannelSpec::Etw {
            provider: required_str(value, "provider")?,
        }),
        "j-link-rtt" => Ok(ChannelSpec::JLinkRtt {
            channel: required_u8(value, "channel")?,
        }),
        "can-bus" => Ok(ChannelSpec::CanBus {
            iface: required_str(value, "iface")?,
        }),
        _ => Err(format!("unsupported source kind: {kind}")),
    }
}

/// Pure handler: given an inbound envelope, optionally produce a
/// reply. v0.1 only auto-replies to `ping`; everything else is
/// dispatched in later phases.
#[must_use]
pub fn handle_envelope(env: &Envelope) -> Option<Envelope> {
    match env.kind {
        FrameType::Ping => Some(Envelope::new(FrameType::Pong, env.seq, Value::Nil)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::hash_token;

    fn loopback() -> SocketAddr {
        "127.0.0.1:1234".parse().unwrap()
    }
    fn lan() -> SocketAddr {
        "192.168.1.10:1234".parse().unwrap()
    }

    // REQ: FR-WIRE-002
    #[test]
    fn auth_accepts_valid_bearer() {
        let mut v = BearerVerifier::new();
        v.add_phc(hash_token("tok").unwrap()).unwrap();
        let h = "wanlogger.v1, bearer.tok";
        assert_eq!(check_auth(Some(h), &lan(), &v, false), AuthOutcome::Bearer);
    }

    #[test]
    fn auth_rejects_bad_bearer() {
        let mut v = BearerVerifier::new();
        v.add_phc(hash_token("tok").unwrap()).unwrap();
        let h = "wanlogger.v1, bearer.nope";
        assert!(matches!(
            check_auth(Some(h), &lan(), &v, false),
            AuthOutcome::Rejected(_)
        ));
    }

    // REQ: FR-WIRE-002 (`--no-auth` only on loopback)
    #[test]
    fn no_auth_only_on_loopback() {
        let v = BearerVerifier::new();
        assert_eq!(
            check_auth(None, &loopback(), &v, true),
            AuthOutcome::LoopbackNoAuth
        );
        assert!(matches!(
            check_auth(None, &lan(), &v, true),
            AuthOutcome::Rejected(_)
        ));
        // no_auth=false on loopback still rejected
        assert!(matches!(
            check_auth(None, &loopback(), &v, false),
            AuthOutcome::Rejected(_)
        ));
    }

    #[test]
    fn ping_is_auto_replied() {
        let env = Envelope::new(FrameType::Ping, 42, Value::Nil);
        let r = handle_envelope(&env).expect("reply");
        assert_eq!(r.kind, FrameType::Pong);
        assert_eq!(r.seq, 42);
    }

    #[test]
    fn restart_payload_accepts_lifecycle_start_options() {
        // REQ: FR-WIRE-003
        let payload = Value::Map(vec![
            (
                Value::String("action".into()),
                Value::String("restart".into()),
            ),
            (
                Value::String("encoding".into()),
                Value::String("cp932".into()),
            ),
            (
                Value::String("classifier".into()),
                Value::Array(vec![
                    Value::Map(vec![
                        (
                            Value::String("contains".into()),
                            Value::String("ERROR".into()),
                        ),
                        (Value::String("tag".into()), Value::String("fault".into())),
                        (Value::String("case_sensitive".into()), Value::Boolean(true)),
                    ]),
                    Value::Map(vec![
                        (
                            Value::String("regex".into()),
                            Value::String("E-[0-9]{4}".into()),
                        ),
                        (
                            Value::String("tag".into()),
                            Value::String("error-id".into()),
                        ),
                    ]),
                ]),
            ),
            (
                Value::String("detection_mode".into()),
                Value::String("suggest".into()),
            ),
        ]);

        let options = start_options_from_payload(&payload).unwrap();
        assert_eq!(options.encoding.as_deref(), Some("cp932"));
        assert_eq!(options.detection_mode, Some(DetectionMode::Suggest));
        assert!(options.classifier.is_some());
    }

    #[test]
    fn classifier_payload_rejects_invalid_regex() {
        let payload = Value::Map(vec![(
            Value::String("classifier".into()),
            Value::Array(vec![Value::Map(vec![
                (Value::String("regex".into()), Value::String("[".into())),
                (Value::String("tag".into()), Value::String("bad".into())),
            ])]),
        )]);

        let err = start_options_from_payload(&payload).unwrap_err();
        assert!(err.contains("valid regular expression"));
    }

    #[test]
    fn channel_spec_accepts_pcap_maps() {
        let value = Value::Map(vec![
            (Value::String("kind".into()), Value::String("pcap".into())),
            (
                Value::String("interface".into()),
                Value::String("Ethernet 0".into()),
            ),
            (Value::String("promisc".into()), Value::Boolean(true)),
            (Value::String("snaplen".into()), Value::Integer(9000.into())),
            (
                Value::String("filter".into()),
                Value::String("tcp port 502".into()),
            ),
            (
                Value::String("publish".into()),
                Value::String("sampled".into()),
            ),
        ]);

        let spec = channel_spec_from_value(&value).unwrap();

        match spec {
            ChannelSpec::Pcap {
                interface,
                promiscuous,
                snaplen,
                filter,
                save_mode,
                publish_mode,
                ..
            } => {
                assert_eq!(interface, "Ethernet 0");
                assert!(promiscuous);
                assert_eq!(snaplen, 9_000);
                assert_eq!(filter.as_deref(), Some("tcp port 502"));
                assert_eq!(save_mode, PcapSaveMode::Session);
                assert_eq!(publish_mode, PcapPublishMode::Sampled);
            }
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn other_frames_are_not_auto_replied() {
        for k in [
            FrameType::Hello,
            FrameType::Sub,
            FrameType::Data,
            FrameType::Metrics,
            FrameType::PanelPriority,
        ] {
            let env = Envelope::new(k, 1, Value::Nil);
            assert!(handle_envelope(&env).is_none());
        }
    }
}
