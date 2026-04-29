//! Integration test for the WSS handler.
//!
//! Spins up the public router on `127.0.0.1:0` and walks through:
//!
//! 1. subprotocol negotiation (`wanlogger.v1`)
//! 2. loopback `--no-auth` accept path (FR-WIRE-002)
//! 3. ping/pong round-trip via the wire envelope (FR-WIRE-001)
//! 4. connection cap (`MAX_CONNS`) enforcement
//!
//! No TLS in v0.1; that lives in [`wanlogger_server::tls`] and is
//! exercised separately.

use std::net::SocketAddr;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use rmpv::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use wanlogger_server::auth::BearerVerifier;
use wanlogger_server::ratelimit::ConnCounter;
use wanlogger_server::wire::{decode, encode, Envelope, FrameType};
use wanlogger_server::ws::{self, WsState};

async fn spawn_server(no_auth: bool, max_conns: u32) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = WsState::new(
        BearerVerifier::new(),
        no_auth,
        Arc::new(ConnCounter::new(max_conns)),
    );
    let app = ws::router(state);
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    addr
}

fn ws_request(addr: SocketAddr) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let url = format!("ws://{addr}/ws");
    let mut req = url.into_client_request().unwrap();
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("wanlogger.v1"),
    );
    req
}

// REQ: FR-WIRE-001, FR-WIRE-002
#[tokio::test]
async fn loopback_no_auth_ping_pong_round_trip() {
    let addr = spawn_server(true, 8).await;
    let req = ws_request(addr);

    let (mut socket, response) = tokio_tungstenite::connect_async(req)
        .await
        .expect("connect");
    assert_eq!(
        response
            .headers()
            .get("sec-websocket-protocol")
            .map(HeaderValue::to_str)
            .transpose()
            .ok()
            .flatten(),
        Some("wanlogger.v1")
    );

    let ping = Envelope::new(FrameType::Ping, 17, Value::Nil);
    socket
        .send(Message::Binary(encode(&ping).unwrap()))
        .await
        .unwrap();

    let msg = socket.next().await.expect("frame").expect("ok");
    let bytes = match msg {
        Message::Binary(b) => b,
        other => panic!("expected binary, got {other:?}"),
    };
    let pong = decode(&bytes).expect("decode");
    assert_eq!(pong.kind, FrameType::Pong);
    assert_eq!(pong.seq, 17);

    socket.close(None).await.ok();
}

// REQ: FR-WIRE-002 (no_auth=false from non-loopback peer rejects)
#[tokio::test]
async fn missing_bearer_is_rejected_when_auth_required() {
    // no_auth=false + empty verifier → every connection rejected.
    let addr = spawn_server(false, 8).await;
    let req = ws_request(addr);

    let err = tokio_tungstenite::connect_async(req)
        .await
        .expect_err("must fail");
    let s = err.to_string();
    assert!(
        s.contains("401") || s.to_lowercase().contains("unauthorized"),
        "unexpected error: {s}"
    );
}

// DoS guard: connection cap.
#[tokio::test]
async fn connection_cap_returns_503() {
    let addr = spawn_server(true, 1).await;

    let s1 = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("first")
        .0;

    // Second attempt must be refused with 503.
    let err = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect_err("must fail");
    let s = err.to_string();
    assert!(
        s.contains("503") || s.to_lowercase().contains("service"),
        "unexpected error: {s}"
    );

    drop(s1);
}
