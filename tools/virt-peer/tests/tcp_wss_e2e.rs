//! End-to-end test for `wanlogger-virt-peer tcp` through the server WSS runner.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures::{SinkExt, StreamExt};
use rmpv::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use wanlogger_server::auth::BearerVerifier;
use wanlogger_server::ingest::Ingest;
use wanlogger_server::ratelimit::ConnCounter;
use wanlogger_server::source_manager::SourceManager;
use wanlogger_server::wire::{decode, encode, Envelope, FrameType};
use wanlogger_server::ws::{self, WsState};

type WsClient = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    const fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    async fn wait_success(mut self) -> Result<()> {
        let mut child = self.child.take().expect("child already taken");
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .context("waiting for virt-peer process timed out")?
            .context("waiting for virt-peer process")?;
        if !status.success() {
            bail!("virt-peer exited with {status}");
        }
        Ok(())
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.start_kill();
        }
    }
}

// REQ: FR-WIRE-001
// REQ: FR-LOG-001
#[tokio::test]
async fn virt_peer_tcp_flows_through_wss_and_session_dir() -> Result<()> {
    let root = tempfile::tempdir().context("creating E2E tempdir")?;
    let transcript = root.path().join("peer-transcript.jsonl");
    let (peer, peer_addr) = spawn_virt_peer_tcp(&transcript).await?;

    let sessions = root.path().join("sessions");
    let manager = Arc::new(SourceManager::with_session_root(
        Arc::new(Ingest::new()),
        &sessions,
    ));
    let server_addr = spawn_server(manager.clone()).await?;
    let (mut socket, _) = tokio_tungstenite::connect_async(ws_request(server_addr)?)
        .await
        .context("connecting WSS test client")?;

    let sid = start_tcp_source(&mut socket, peer_addr).await?;
    subscribe_and_wait_barrier(&mut socket, sid).await?;
    let data = read_until(&mut socket, FrameType::Data).await?;

    assert_eq!(data.sid.as_deref(), Some(sid.to_string().as_str()));
    assert_eq!(data.ch, Some(0));
    assert_eq!(map_str(&data.payload, "dir"), Some("in"));
    assert_eq!(map_str(&data.payload, "kind"), Some("bytes"));
    assert_eq!(
        map_bin(&data.payload, "body"),
        Some(b"virt-peer-e2e\n".as_slice())
    );

    let stats = tokio::time::timeout(Duration::from_secs(5), manager.wait(sid))
        .await
        .context("waiting for tcp source completion timed out")?
        .context("tcp source task handle missing")??;
    assert_eq!(stats.sid, sid);
    assert_eq!(stats.raw_frames, 1);
    assert_eq!(stats.decoded_records, 1);

    let session_dir = single_session_dir(&sessions)?;
    assert_eq!(
        std::fs::read(session_dir.join("raw.bin"))?,
        b"virt-peer-e2e\n"
    );
    let index = std::fs::read_to_string(session_dir.join("index.jsonl"))?;
    assert!(index.contains("\"kind\":\"bytes\""), "index was: {index}");
    let lines = std::fs::read_to_string(session_dir.join("lines.jsonl"))?;
    assert!(lines.contains("virt-peer-e2e"), "lines were: {lines}");

    let transcript_body = std::fs::read_to_string(&transcript)?;
    assert!(
        transcript_body.contains("\"bytes_hex\":\"766972742d706565722d6532650a\""),
        "transcript was: {transcript_body}"
    );

    peer.wait_success().await?;
    assert!(manager.remove(sid));
    Ok(())
}

async fn spawn_server(manager: Arc<SourceManager>) -> Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("binding test server")?;
    let addr = listener.local_addr().context("reading test server addr")?;
    let state = WsState::with_source_manager(
        BearerVerifier::new(),
        true,
        Arc::new(ConnCounter::new(8)),
        manager,
    );
    let app = ws::router(state);
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("test server failed");
    });
    Ok(addr)
}

async fn spawn_virt_peer_tcp(transcript: &Path) -> Result<(ChildGuard, SocketAddr)> {
    let mut child = Command::new(virt_peer_exe())
        .args([
            "--log-filter",
            "warn",
            "tcp",
            "--addr",
            "127.0.0.1:0",
            "--send",
            "virt-peer-e2e",
            "--eol",
            "lf",
            "--initial-delay-ms",
            "500",
            "--idle-timeout-ms",
            "1000",
            "--transcript",
        ])
        .arg(transcript)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawning wanlogger-virt-peer")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("virt-peer stdout was not piped"))?;
    let mut lines = BufReader::new(stdout).lines();
    let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
        .await
        .context("waiting for virt-peer listening line timed out")?
        .context("reading virt-peer stdout")?
        .ok_or_else(|| anyhow!("virt-peer exited before printing listening line"))?;
    let addr = line
        .strip_prefix("wanlogger-virt-peer tcp listening ")
        .ok_or_else(|| anyhow!("unexpected virt-peer listening line: {line}"))?
        .parse::<SocketAddr>()
        .with_context(|| format!("parsing virt-peer address from {line}"))?;

    tokio::spawn(async move { while matches!(lines.next_line().await, Ok(Some(_))) {} });
    Ok((ChildGuard::new(child), addr))
}

fn virt_peer_exe() -> PathBuf {
    option_env!("CARGO_BIN_EXE_wanlogger-virt-peer").map_or_else(
        || {
            let mut path = std::env::current_exe().expect("current test exe path");
            path.pop();
            if path.ends_with("deps") {
                path.pop();
            }
            path.push(format!(
                "wanlogger-virt-peer{}",
                std::env::consts::EXE_SUFFIX
            ));
            path
        },
        PathBuf::from,
    )
}

fn ws_request(
    addr: SocketAddr,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request> {
    let url = format!("ws://{addr}/ws");
    let mut req = url.into_client_request().context("building WSS request")?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static("wanlogger.v1"),
    );
    Ok(req)
}

async fn start_tcp_source(socket: &mut WsClient, peer_addr: SocketAddr) -> Result<uuid::Uuid> {
    let start = Envelope::new(
        FrameType::Ctl,
        10,
        value_map(vec![
            ("action", value_str("start")),
            (
                "spec",
                value_map(vec![
                    ("kind", value_str("tcp")),
                    ("addr", value_str(&peer_addr.to_string())),
                ]),
            ),
        ]),
    );
    send_env(socket, &start).await?;
    let started = read_env(socket).await?;
    assert_eq!(started.kind, FrameType::Ctl);
    assert_eq!(map_str(&started.payload, "event"), Some("started"));
    map_str(&started.payload, "sid")
        .ok_or_else(|| anyhow!("started ctl missing sid"))?
        .parse()
        .context("parsing started sid")
}

async fn subscribe_and_wait_barrier(socket: &mut WsClient, sid: uuid::Uuid) -> Result<()> {
    let sub = Envelope::new(FrameType::Sub, 11, Value::Map(vec![]))
        .with_sid(sid.to_string())
        .with_ch(0);
    send_env(socket, &sub).await?;
    let ping = Envelope::new(FrameType::Ping, 12, Value::Nil);
    send_env(socket, &ping).await?;
    let pong = read_until(socket, FrameType::Pong).await?;
    assert_eq!(pong.seq, 12);
    Ok(())
}

async fn send_env(socket: &mut WsClient, env: &Envelope) -> Result<()> {
    socket
        .send(Message::Binary(encode(env)?))
        .await
        .context("sending WSS envelope")
}

async fn read_until(socket: &mut WsClient, kind: FrameType) -> Result<Envelope> {
    for _ in 0..8 {
        let env = read_env(socket).await?;
        if env.kind == kind {
            return Ok(env);
        }
    }
    bail!("did not receive {kind:?} within frame budget")
}

async fn read_env(socket: &mut WsClient) -> Result<Envelope> {
    let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .context("waiting for WSS frame timed out")?
        .ok_or_else(|| anyhow!("WSS stream ended"))?
        .context("reading WSS frame")?;
    let Message::Binary(bytes) = msg else {
        bail!("expected binary WSS frame, got {msg:?}");
    };
    Ok(decode(&bytes)?)
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

fn map_get<'a>(payload: &'a Value, key: &str) -> Option<&'a Value> {
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

fn map_str<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    map_get(payload, key).and_then(Value::as_str)
}

fn map_bin<'a>(payload: &'a Value, key: &str) -> Option<&'a [u8]> {
    match map_get(payload, key) {
        Some(Value::Binary(bytes)) => Some(bytes.as_slice()),
        _ => None,
    }
}

fn single_session_dir(root: &Path) -> Result<PathBuf> {
    let dirs: Vec<_> = std::fs::read_dir(root)
        .with_context(|| format!("reading session root {}", root.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|path| path.is_dir())
        .collect();
    match dirs.as_slice() {
        [dir] => Ok(dir.clone()),
        _ => bail!(
            "expected one session dir under {}, got {dirs:?}",
            root.display()
        ),
    }
}
