//! `wanlogger connect` ? open a channel and pipe frames to stdout.

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use wanlogger_core::source::{ControlEvt, Frame};

use super::spec;

/// Run the `connect` subcommand.
///
/// Pipes [`Frame::Bytes`] / [`Frame::Datagram`] payloads to stdout
/// verbatim until EOF or Ctrl-C. Datagram source addresses and other
/// frame kinds are logged at INFO level via `tracing` so they don't
/// pollute binary stdout.
///
/// # Errors
/// Returns an `anyhow::Error` if the spec cannot be parsed, the
/// source cannot be opened, or stdout cannot be written.
pub async fn run(spec_str: &str) -> Result<()> {
    let s = spec::parse(spec_str).context("parsing channel spec")?;
    let mut source = spec::open(&s).context("opening source")?;
    source.open().await.context("Source::open failed")?;
    let meta = source.metadata();
    tracing::info!(kind = %meta.kind, iface = %meta.iface, "connect: opened");

    let mut stdout = tokio::io::stdout();
    loop {
        match source.recv().await? {
            Some(Frame::Bytes(b)) => stdout.write_all(&b).await?,
            Some(Frame::Datagram { src, data }) => {
                if let Some(src) = src.as_deref() {
                    tracing::debug!(%src, "connect: datagram");
                }
                stdout.write_all(&data).await?;
            }
            Some(Frame::Other { kind, data }) => {
                tracing::debug!(kind, "connect: other frame");
                stdout.write_all(&data).await?;
            }
            Some(Frame::Ssh { stream, data }) => {
                tracing::debug!(stream, "connect: ssh frame");
                stdout.write_all(&data).await?;
            }
            Some(Frame::Visa { eom, data }) => {
                tracing::debug!(eom, "connect: visa frame");
                stdout.write_all(&data).await?;
            }
            Some(_) => tracing::debug!("connect: unknown frame variant"),
            None => {
                tracing::info!("connect: source returned None");
                break;
            }
        }
        stdout.flush().await?;
        match source.recv_ctl().await? {
            Some(ControlEvt::Eof) => {
                tracing::info!("connect: EOF");
                break;
            }
            Some(ControlEvt::Disconnected { reason }) => {
                tracing::warn!(?reason, "connect: disconnected");
                break;
            }
            Some(ControlEvt::Error { id, message }) => {
                tracing::error!(code = id.code(), %message, "connect: source error");
                break;
            }
            Some(other) => tracing::debug!(?other, "connect: ctl"),
            None => {}
        }
    }
    source.close().await?;
    Ok(())
}
