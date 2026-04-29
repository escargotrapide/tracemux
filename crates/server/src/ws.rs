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
//! * Minimal frame loop: decode [`crate::wire::Envelope`], reply to
//!   `ping` with `pong`, log everything else (handlers for `sub` /
//!   `data` / `clientlog` / ... live in [`crate::ingest`] /
//!   [`crate::clientlog`] and are wired in later phases).

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rmpv::Value;
use std::net::SocketAddr;

use crate::auth::{extract_bearer, is_loopback_allowed, BearerVerifier};
use crate::ratelimit::ConnCounter;
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
}

impl WsState {
    /// Build a state for the given verifier and policy.
    #[must_use]
    pub fn new(auth: BearerVerifier, no_auth: bool, conns: Arc<ConnCounter>) -> Self {
        Self {
            auth: Arc::new(auth),
            no_auth,
            conns,
        }
    }
}

/// Attach the `/ws` route to a router.
#[must_use]
pub fn router(state: WsState) -> Router {
    Router::new().route("/ws", get(ws_handler)).with_state(state)
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
            let _g = guard;
            handle_socket(socket, peer).await;
            drop(_g);
        })
}

/// Per-connection frame loop. Public so integration tests in
/// `tests/` can spin it up directly.
pub async fn handle_socket(mut socket: WebSocket, peer: SocketAddr) {
    tracing::info!(%peer, "ws: connected");
    while let Some(msg) = socket.recv().await {
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
                    let bytes = match encode(&reply) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::warn!(%peer, error = %e, "ws: encode reply failed");
                            continue;
                        }
                    };
                    if socket.send(Message::Binary(bytes)).await.is_err() {
                        break;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Ping(p) => {
                let _ = socket.send(Message::Pong(p)).await;
            }
            Message::Pong(_) | Message::Text(_) => {
                // v0.1 carries no text frames; ignore.
            }
        }
    }
    tracing::info!(%peer, "ws: closed");
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

