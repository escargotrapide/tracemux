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

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use rmpv::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use wanlogger_core::session::registry::SessionState;
use wanlogger_server::auth::BearerVerifier;
use wanlogger_server::ingest::Ingest;
use wanlogger_server::ratelimit::ConnCounter;
use wanlogger_server::source_manager::SourceManager;
use wanlogger_server::wire::{decode, encode, Envelope, FrameType};
use wanlogger_server::ws::{self, WsState};

async fn spawn_server(no_auth: bool, max_conns: u32) -> SocketAddr {
    spawn_server_with_ingest(no_auth, max_conns, Arc::new(Ingest::new())).await
}

async fn spawn_server_with_ingest(
    no_auth: bool,
    max_conns: u32,
    ingest: Arc<Ingest>,
) -> SocketAddr {
    spawn_server_with_manager(no_auth, max_conns, Arc::new(SourceManager::new(ingest))).await
}

async fn spawn_server_with_manager(
    no_auth: bool,
    max_conns: u32,
    source_manager: Arc<SourceManager>,
) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = WsState::with_source_manager(
        BearerVerifier::new(),
        no_auth,
        Arc::new(ConnCounter::new(max_conns)),
        source_manager,
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

fn payload_str<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    value_get(payload, key).and_then(Value::as_str)
}

fn value_get<'a>(payload: &'a Value, key: &str) -> Option<&'a Value> {
    let Value::Map(entries) = payload else {
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

fn value_str(s: &str) -> Value {
    Value::String(s.into())
}

fn value_map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(k, v)| (value_str(k), v))
            .collect(),
    )
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

// REQ: FR-WIRE-001
#[tokio::test]
async fn subscribed_client_receives_ingested_wire_frame() {
    let ingest = Arc::new(Ingest::new());
    let sid = ingest.register_session(SessionState::new("mock", "loopback"));
    let addr = spawn_server_with_ingest(true, 8, ingest.clone()).await;

    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let sub = Envelope::new(FrameType::Sub, 1, Value::Map(vec![]))
        .with_sid(sid.to_string())
        .with_ch(0);
    socket
        .send(Message::Binary(encode(&sub).unwrap()))
        .await
        .unwrap();

    // A ping after sub acts as an ordering barrier: once pong arrives,
    // the server has processed the preceding subscription.
    let ping = Envelope::new(FrameType::Ping, 2, Value::Nil);
    socket
        .send(Message::Binary(encode(&ping).unwrap()))
        .await
        .unwrap();
    let pong_msg = socket.next().await.expect("pong frame").expect("pong ok");
    let Message::Binary(pong_bytes) = pong_msg else {
        panic!("expected binary pong, got {pong_msg:?}");
    };
    assert_eq!(decode(&pong_bytes).unwrap().kind, FrameType::Pong);

    let data = Envelope::new(
        FrameType::Data,
        99,
        Value::Map(vec![(
            Value::String("body".into()),
            Value::Binary(vec![1, 2, 3]),
        )]),
    )
    .with_sid(sid.to_string())
    .with_ch(0);
    let encoded = encode(&data).unwrap();
    assert_eq!(ingest.publish_wire(sid, Bytes::from(encoded)).unwrap(), 1);

    let msg = socket.next().await.expect("data frame").expect("data ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary data, got {msg:?}");
    };
    let got = decode(&bytes).expect("decode data");
    assert_eq!(got.kind, FrameType::Data);
    assert_eq!(got.seq, 99);
    let sid_text = sid.to_string();
    assert_eq!(got.sid.as_deref(), Some(sid_text.as_str()));
    assert_eq!(got.ch, Some(0));
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn unknown_subscription_returns_ctl_error() {
    let addr = spawn_server(true, 8).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let sub = Envelope::new(FrameType::Sub, 3, Value::Map(vec![]))
        .with_sid(uuid::Uuid::new_v4().to_string())
        .with_ch(0);
    socket
        .send(Message::Binary(encode(&sub).unwrap()))
        .await
        .unwrap();

    let msg = socket.next().await.expect("ctl frame").expect("ctl ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary ctl, got {msg:?}");
    };
    let got = decode(&bytes).expect("decode ctl");
    assert_eq!(got.kind, FrameType::Ctl);
    assert_eq!(got.seq, 3);
    assert_eq!(payload_str(&got.payload, "event"), Some("error"));
    assert_eq!(payload_str(&got.payload, "error_id"), Some("E-2001"));
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn ctl_list_sources_returns_registered_sessions() {
    let ingest = Arc::new(Ingest::new());
    let mut session = SessionState::new("mock", "loopback");
    session.label = Some("Demo source".to_string());
    let sid = ingest.register_session(session);
    ingest.record_frame(sid, 12);
    let addr = spawn_server_with_ingest(true, 8, ingest).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let list = Envelope::new(
        FrameType::Ctl,
        4,
        value_map(vec![("action", value_str("list"))]),
    );
    socket
        .send(Message::Binary(encode(&list).unwrap()))
        .await
        .unwrap();

    let msg = socket.next().await.expect("ctl frame").expect("ctl ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary ctl, got {msg:?}");
    };
    let got = decode(&bytes).expect("decode ctl");
    assert_eq!(got.kind, FrameType::Ctl);
    assert_eq!(got.seq, 4);
    assert_eq!(payload_str(&got.payload, "event"), Some("sources"));
    let sources = value_get(&got.payload, "sources")
        .and_then(Value::as_array)
        .expect("sources array");
    assert_eq!(sources.len(), 1);
    let row = &sources[0];
    let sid_text = sid.to_string();
    assert_eq!(payload_str(row, "sid"), Some(sid_text.as_str()));
    assert_eq!(payload_str(row, "name"), Some("Demo source"));
    assert_eq!(payload_str(row, "kind"), Some("mock"));
    assert_eq!(payload_str(row, "status"), Some("unknown"));
    assert_eq!(value_get(row, "bytes_in").and_then(Value::as_u64), Some(12));
    let channels = value_get(row, "channels")
        .and_then(Value::as_array)
        .expect("channels array");
    assert_eq!(channels.first().and_then(Value::as_u64), Some(0));
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn malformed_ctl_lifecycle_commands_return_ctl_errors() {
    let addr = spawn_server(true, 8).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let cases = vec![
        (30, value_map(vec![]), None),
        (31, value_map(vec![("action", value_str("start"))]), None),
        (32, value_map(vec![("action", value_str("stop"))]), None),
        (37, value_map(vec![("action", value_str("resume"))]), None),
        (38, value_map(vec![("action", value_str("restart"))]), None),
        (
            33,
            value_map(vec![("action", value_str("remove"))]),
            Some("not-a-uuid".to_string()),
        ),
    ];

    for (seq, payload, sid) in cases {
        let mut env = Envelope::new(FrameType::Ctl, seq, payload);
        if let Some(sid) = sid {
            env = env.with_sid(sid);
        }
        socket
            .send(Message::Binary(encode(&env).unwrap()))
            .await
            .unwrap();

        let msg = socket.next().await.expect("ctl frame").expect("ctl ok");
        let Message::Binary(bytes) = msg else {
            panic!("expected binary ctl, got {msg:?}");
        };
        let got = decode(&bytes).expect("decode ctl");
        assert_eq!(got.kind, FrameType::Ctl);
        assert_eq!(got.seq, seq);
        assert_eq!(payload_str(&got.payload, "event"), Some("error"));
        assert_eq!(payload_str(&got.payload, "error_id"), Some("E-2001"));
    }
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn ctl_start_source_open_failure_returns_source_error() {
    let manager = Arc::new(SourceManager::new(Arc::new(Ingest::new())));
    let addr = spawn_server_with_manager(true, 8, manager.clone()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");
    let missing = std::env::temp_dir()
        .join(format!("wanlogger-missing-{}.log", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .to_string();

    let start = Envelope::new(
        FrameType::Ctl,
        34,
        value_map(vec![
            ("action", value_str("start")),
            (
                "spec",
                value_map(vec![
                    ("kind", value_str("file")),
                    ("path", value_str(&missing)),
                    ("follow", Value::Boolean(false)),
                ]),
            ),
        ]),
    );
    socket
        .send(Message::Binary(encode(&start).unwrap()))
        .await
        .unwrap();

    let msg = socket.next().await.expect("ctl frame").expect("ctl ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary ctl, got {msg:?}");
    };
    let got = decode(&bytes).expect("decode ctl");
    assert_eq!(got.kind, FrameType::Ctl);
    assert_eq!(got.seq, 34);
    assert_eq!(payload_str(&got.payload, "event"), Some("error"));
    assert_eq!(payload_str(&got.payload, "error_id"), Some("E-1101"));
    assert!(manager.active_ids().is_empty());
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn ctl_unknown_stop_and_remove_return_errors() {
    let addr = spawn_server(true, 8).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");
    let sid = uuid::Uuid::new_v4().to_string();

    for (seq, action) in [(35, "stop"), (36, "remove")] {
        let env = Envelope::new(
            FrameType::Ctl,
            seq,
            value_map(vec![("action", value_str(action))]),
        )
        .with_sid(sid.clone());
        socket
            .send(Message::Binary(encode(&env).unwrap()))
            .await
            .unwrap();

        let msg = socket.next().await.expect("ctl frame").expect("ctl ok");
        let Message::Binary(bytes) = msg else {
            panic!("expected binary ctl, got {msg:?}");
        };
        let got = decode(&bytes).expect("decode ctl");
        assert_eq!(got.kind, FrameType::Ctl);
        assert_eq!(got.seq, seq);
        assert_eq!(payload_str(&got.payload, "event"), Some("error"));
        assert_eq!(payload_str(&got.payload, "error_id"), Some("E-2001"));
    }
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn ctl_start_mock_source_returns_registered_sid() {
    let manager = Arc::new(SourceManager::new(Arc::new(Ingest::new())));
    let addr = spawn_server_with_manager(true, 8, manager.clone()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let start = Envelope::new(
        FrameType::Ctl,
        10,
        Value::Map(vec![
            (
                Value::String("action".into()),
                Value::String("start".into()),
            ),
            (
                Value::String("spec".into()),
                Value::Map(vec![
                    (Value::String("kind".into()), Value::String("mock".into())),
                    (Value::String("tag".into()), Value::String("ctl".into())),
                ]),
            ),
        ]),
    );
    socket
        .send(Message::Binary(encode(&start).unwrap()))
        .await
        .unwrap();

    let msg = socket.next().await.expect("ctl frame").expect("ctl ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary ctl, got {msg:?}");
    };
    let got = decode(&bytes).expect("decode ctl");
    assert_eq!(got.kind, FrameType::Ctl);
    assert_eq!(got.seq, 10);
    assert_eq!(payload_str(&got.payload, "event"), Some("started"));
    let sid = payload_str(&got.payload, "sid")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .expect("started sid");
    assert_eq!(got.sid.as_deref(), Some(sid.to_string().as_str()));
    assert!(manager.ingest().registry.get(&sid).is_some());
    let stats = manager.wait(sid).await.expect("task handle").unwrap();
    assert_eq!(stats.sid, sid);
    assert!(manager.remove(sid));
}

// REQ: FR-WIRE-001, FR-LOG-001
#[tokio::test]
async fn ctl_start_file_source_persists_session_dir() {
    let root = std::env::temp_dir().join(format!("wanlogger-ws-persist-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("input.log");
    std::fs::write(&input, b"via-ws\n").unwrap();
    let input_text = input.to_string_lossy().to_string();
    let sessions = root.join("sessions");
    let manager = Arc::new(SourceManager::with_session_root(
        Arc::new(Ingest::new()),
        &sessions,
    ));
    let addr = spawn_server_with_manager(true, 8, manager.clone()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let start = Envelope::new(
        FrameType::Ctl,
        40,
        value_map(vec![
            ("action", value_str("start")),
            (
                "spec",
                value_map(vec![
                    ("kind", value_str("file")),
                    ("path", value_str(&input_text)),
                    ("follow", Value::Boolean(false)),
                ]),
            ),
        ]),
    );
    socket
        .send(Message::Binary(encode(&start).unwrap()))
        .await
        .unwrap();

    let msg = socket
        .next()
        .await
        .expect("started frame")
        .expect("start ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary started ctl, got {msg:?}");
    };
    let started = decode(&bytes).expect("decode started");
    assert_eq!(payload_str(&started.payload, "event"), Some("started"));
    let sid = payload_str(&started.payload, "sid")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .expect("started sid");
    manager.wait(sid).await.expect("task handle").unwrap();

    let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(session_dirs.len(), 1);
    let dir = &session_dirs[0];
    assert_eq!(std::fs::read(dir.join("raw.bin")).unwrap(), b"via-ws\n");
    let index = std::fs::read_to_string(dir.join("index.jsonl")).unwrap();
    let index_row: serde_json::Value = serde_json::from_str(index.trim()).unwrap();
    assert_eq!(index_row["sid"], sid.to_string());
    assert_eq!(index_row["kind"], "bytes");
    assert!(std::fs::read_to_string(dir.join("lines.jsonl"))
        .unwrap()
        .contains("via-ws"));
    assert!(std::fs::read_to_string(dir.join("meta.toml"))
        .unwrap()
        .contains(&sid.to_string()));
    assert!(manager.remove(sid));
}

// REQ: FR-WIRE-001, FR-LOG-001
#[tokio::test]
async fn ctl_resume_completed_file_source_reuses_sid_and_session_dir() {
    let root = std::env::temp_dir().join(format!("wanlogger-ws-resume-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let input = root.join("input.log");
    std::fs::write(&input, b"again-ws\n").unwrap();
    let input_text = input.to_string_lossy().to_string();
    let sessions = root.join("sessions");
    let manager = Arc::new(SourceManager::with_session_root(
        Arc::new(Ingest::new()),
        &sessions,
    ));
    let addr = spawn_server_with_manager(true, 8, manager.clone()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let start = Envelope::new(
        FrameType::Ctl,
        50,
        value_map(vec![
            ("action", value_str("start")),
            (
                "spec",
                value_map(vec![
                    ("kind", value_str("file")),
                    ("path", value_str(&input_text)),
                    ("follow", Value::Boolean(false)),
                ]),
            ),
        ]),
    );
    socket
        .send(Message::Binary(encode(&start).unwrap()))
        .await
        .unwrap();
    let msg = socket
        .next()
        .await
        .expect("started frame")
        .expect("start ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary started ctl, got {msg:?}");
    };
    let started = decode(&bytes).expect("decode started");
    assert_eq!(payload_str(&started.payload, "event"), Some("started"));
    let sid = payload_str(&started.payload, "sid")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .expect("started sid");
    manager.wait(sid).await.expect("task handle").unwrap();

    let resume = Envelope::new(
        FrameType::Ctl,
        51,
        value_map(vec![("action", value_str("resume"))]),
    )
    .with_sid(sid.to_string());
    socket
        .send(Message::Binary(encode(&resume).unwrap()))
        .await
        .unwrap();
    let msg = socket
        .next()
        .await
        .expect("resumed frame")
        .expect("resume ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary resumed ctl, got {msg:?}");
    };
    let resumed = decode(&bytes).expect("decode resumed");
    assert_eq!(resumed.seq, 51);
    assert_eq!(resumed.sid.as_deref(), Some(sid.to_string().as_str()));
    assert_eq!(payload_str(&resumed.payload, "event"), Some("resumed"));
    manager.wait(sid).await.expect("task handle").unwrap();

    let session_dirs: Vec<_> = std::fs::read_dir(&sessions)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(session_dirs.len(), 1);
    assert_eq!(
        std::fs::read(session_dirs[0].join("raw.bin")).unwrap(),
        b"again-ws\nagain-ws\n"
    );
    assert!(manager.remove(sid));
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn ctl_stop_and_remove_source_lifecycle() {
    let manager = Arc::new(SourceManager::new(Arc::new(Ingest::new())));
    let addr = spawn_server_with_manager(true, 8, manager.clone()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let start = Envelope::new(
        FrameType::Ctl,
        20,
        Value::Map(vec![
            (
                Value::String("action".into()),
                Value::String("start".into()),
            ),
            (
                Value::String("spec".into()),
                Value::Map(vec![
                    (Value::String("kind".into()), Value::String("udp".into())),
                    (
                        Value::String("bind".into()),
                        Value::String("127.0.0.1:0".into()),
                    ),
                ]),
            ),
        ]),
    );
    socket
        .send(Message::Binary(encode(&start).unwrap()))
        .await
        .unwrap();

    let msg = socket
        .next()
        .await
        .expect("started frame")
        .expect("start ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary started ctl, got {msg:?}");
    };
    let started = decode(&bytes).expect("decode started");
    assert_eq!(payload_str(&started.payload, "event"), Some("started"));
    let sid = payload_str(&started.payload, "sid")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .expect("started sid");
    assert!(manager.active_ids().contains(&sid));

    let stop = Envelope::new(
        FrameType::Ctl,
        21,
        Value::Map(vec![(
            Value::String("action".into()),
            Value::String("stop".into()),
        )]),
    )
    .with_sid(sid.to_string());
    socket
        .send(Message::Binary(encode(&stop).unwrap()))
        .await
        .unwrap();
    let msg = socket
        .next()
        .await
        .expect("stopped frame")
        .expect("stop ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary stopped ctl, got {msg:?}");
    };
    let stopped = decode(&bytes).expect("decode stopped");
    assert_eq!(stopped.seq, 21);
    assert_eq!(payload_str(&stopped.payload, "event"), Some("stopped"));
    assert!(!manager.active_ids().contains(&sid));
    assert!(manager.ingest().registry.get(&sid).is_some());

    let remove = Envelope::new(
        FrameType::Ctl,
        22,
        Value::Map(vec![(
            Value::String("action".into()),
            Value::String("remove".into()),
        )]),
    )
    .with_sid(sid.to_string());
    socket
        .send(Message::Binary(encode(&remove).unwrap()))
        .await
        .unwrap();
    let msg = socket
        .next()
        .await
        .expect("removed frame")
        .expect("remove ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary removed ctl, got {msg:?}");
    };
    let removed = decode(&bytes).expect("decode removed");
    assert_eq!(removed.seq, 22);
    assert_eq!(payload_str(&removed.payload, "event"), Some("removed"));
    assert!(manager.ingest().registry.get(&sid).is_none());
}

// REQ: FR-WIRE-001
#[tokio::test]
async fn ctl_restart_stopped_source_reuses_sid() {
    let manager = Arc::new(SourceManager::new(Arc::new(Ingest::new())));
    let addr = spawn_server_with_manager(true, 8, manager.clone()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(addr))
        .await
        .expect("connect");

    let start = Envelope::new(
        FrameType::Ctl,
        60,
        value_map(vec![
            ("action", value_str("start")),
            (
                "spec",
                value_map(vec![
                    ("kind", value_str("udp")),
                    ("bind", value_str("127.0.0.1:0")),
                ]),
            ),
        ]),
    );
    socket
        .send(Message::Binary(encode(&start).unwrap()))
        .await
        .unwrap();
    let msg = socket
        .next()
        .await
        .expect("started frame")
        .expect("start ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary started ctl, got {msg:?}");
    };
    let started = decode(&bytes).expect("decode started");
    let sid = payload_str(&started.payload, "sid")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .expect("started sid");

    let stop = Envelope::new(
        FrameType::Ctl,
        61,
        value_map(vec![("action", value_str("stop"))]),
    )
    .with_sid(sid.to_string());
    socket
        .send(Message::Binary(encode(&stop).unwrap()))
        .await
        .unwrap();
    let msg = socket
        .next()
        .await
        .expect("stopped frame")
        .expect("stop ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary stopped ctl, got {msg:?}");
    };
    let stopped = decode(&bytes).expect("decode stopped");
    assert_eq!(payload_str(&stopped.payload, "event"), Some("stopped"));

    let restart = Envelope::new(
        FrameType::Ctl,
        62,
        value_map(vec![("action", value_str("restart"))]),
    )
    .with_sid(sid.to_string());
    socket
        .send(Message::Binary(encode(&restart).unwrap()))
        .await
        .unwrap();
    let msg = socket
        .next()
        .await
        .expect("restarted frame")
        .expect("restart ok");
    let Message::Binary(bytes) = msg else {
        panic!("expected binary restarted ctl, got {msg:?}");
    };
    let restarted = decode(&bytes).expect("decode restarted");
    assert_eq!(restarted.seq, 62);
    assert_eq!(restarted.sid.as_deref(), Some(sid.to_string().as_str()));
    assert_eq!(payload_str(&restarted.payload, "event"), Some("restarted"));
    assert!(manager.active_ids().contains(&sid));

    assert!(manager.remove(sid));
    assert!(manager.ingest().registry.get(&sid).is_none());
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
