use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::scenario::Scenario;
use crate::transcript::{Direction, Transcript};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Listen,
    Connect,
}

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) mode: Mode,
    pub(crate) addr: String,
}

pub(crate) async fn run(
    config: Config,
    scenario: Scenario,
    transcript: Arc<Transcript>,
) -> Result<()> {
    match config.mode {
        Mode::Listen => run_listener(&config.addr, scenario, transcript).await,
        Mode::Connect => run_connector(&config.addr, scenario, transcript).await,
    }
}

async fn run_listener(addr: &str, scenario: Scenario, transcript: Arc<Transcript>) -> Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding TCP listener {addr}"))?;
    let local = listener
        .local_addr()
        .context("reading TCP listener address")?;
    println!("wanlogger-virt-peer tcp listening {local}");
    transcript.record_event("tcp", Some(&local.to_string()), "listening")?;

    let (stream, peer) = listener.accept().await.context("accepting TCP peer")?;
    println!("wanlogger-virt-peer tcp connected {peer}");
    run_stream(stream, peer.to_string(), scenario, transcript).await
}

async fn run_connector(addr: &str, scenario: Scenario, transcript: Arc<Transcript>) -> Result<()> {
    let stream = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connecting TCP peer {addr}"))?;
    println!("wanlogger-virt-peer tcp connected {addr}");
    run_stream(stream, addr.to_string(), scenario, transcript).await
}

async fn run_stream(
    stream: TcpStream,
    peer: String,
    scenario: Scenario,
    transcript: Arc<Transcript>,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    transcript.record_event("tcp", Some(&peer), "connected")?;

    let read_task = tokio::spawn(read_loop(
        reader,
        writer.clone(),
        scenario.clone(),
        transcript.clone(),
        peer.clone(),
    ));
    let script_task = tokio::spawn(script_loop(writer, scenario, transcript, peer));

    script_task.await.context("joining TCP script task")??;
    read_task.await.context("joining TCP read task")??;
    Ok(())
}

async fn script_loop(
    writer: Arc<Mutex<OwnedWriteHalf>>,
    scenario: Scenario,
    transcript: Arc<Transcript>,
    peer: String,
) -> Result<()> {
    let payloads = scenario.scripted_payloads();
    if !payloads.is_empty() && !scenario.initial_delay().is_zero() {
        tokio::time::sleep(scenario.initial_delay()).await;
    }
    let last = payloads.len().saturating_sub(1);
    for (idx, payload) in payloads.iter().enumerate() {
        write_payload(&writer, &scenario, &transcript, &peer, payload).await?;
        if idx != last && !scenario.interval().is_zero() {
            tokio::time::sleep(scenario.interval()).await;
        }
    }
    Ok(())
}

async fn read_loop(
    mut reader: OwnedReadHalf,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    scenario: Scenario,
    transcript: Arc<Transcript>,
    peer: String,
) -> Result<()> {
    let mut buf = vec![0; scenario.read_chunk()];
    loop {
        let read = reader.read(&mut buf);
        let n = match scenario.idle_timeout() {
            Some(timeout) => match tokio::time::timeout(timeout, read).await {
                Ok(result) => result.with_context(|| format!("reading TCP peer {peer}"))?,
                Err(_) => {
                    transcript.record_event("tcp", Some(&peer), "idle-timeout")?;
                    break;
                }
            },
            None => read
                .await
                .with_context(|| format!("reading TCP peer {peer}"))?,
        };
        if n == 0 {
            transcript.record_event("tcp", Some(&peer), "eof")?;
            break;
        }
        let inbound = &buf[..n];
        transcript.record_bytes("tcp", Direction::In, Some(&peer), inbound)?;
        if let Some(reply) = scenario.echo_payload(inbound) {
            write_payload(&writer, &scenario, &transcript, &peer, &reply).await?;
        }
    }
    Ok(())
}

async fn write_payload(
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    scenario: &Scenario,
    transcript: &Transcript,
    peer: &str,
    payload: &[u8],
) -> Result<()> {
    for chunk in scenario.chunks(payload) {
        let mut writer = writer.lock().await;
        writer
            .write_all(chunk)
            .await
            .with_context(|| format!("writing TCP peer {peer}"))?;
        writer
            .flush()
            .await
            .with_context(|| format!("flushing TCP peer {peer}"))?;
        drop(writer);
        transcript.record_bytes("tcp", Direction::Out, Some(peer), chunk)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::*;
    use crate::scenario::{Eol, ScenarioConfig};

    #[tokio::test]
    async fn tcp_peer_sends_scripted_payload_and_echoes_inbound() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let scenario = Scenario::from_config(ScenarioConfig {
            send_text: vec!["hello".to_string()],
            send_hex: Vec::new(),
            repeat: 1,
            initial_delay_ms: 0,
            interval_ms: 0,
            eol: Eol::Lf,
            chunk_size: None,
            echo: true,
            ack_prefix: "ACK:".to_string(),
            read_chunk: 16,
            idle_timeout_ms: Some(1_000),
        })
        .unwrap();
        let transcript = Arc::new(Transcript::disabled());
        let server = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            run_stream(stream, peer.to_string(), scenario, transcript)
                .await
                .unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0; 64];
        let n = client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello\n");

        client.write_all(b"cmd").await.unwrap();
        let n = client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"ACK:cmd");

        drop(client);
        server.await.unwrap();
    }
}
