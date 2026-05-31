//! `tracemux connect` ? open a channel and pipe frames to stdout.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::io::AsyncWriteExt;
use tracemux_core::log::index::{Dir, IndexEntry, IndexWriter, Kind};
use tracemux_core::log::raw::RawWriter;
use tracemux_core::source::{ControlEvt, Frame};
use tracemux_core::time::{ClockQuality, ClockSource, DualTimestamp};
use uuid::Uuid;

use super::spec;

// REQ: FR-CLI-010
/// Options for the `connect` subcommand.
#[derive(Debug, Clone)]
pub struct Options {
    /// Channel spec URI.
    pub spec: String,
    /// Optional session-dir receiving a copy of inbound bytes.
    pub save: Option<PathBuf>,
    /// Text encoding recorded in metadata for saved sessions.
    pub encoding: String,
}

#[derive(Debug, Serialize)]
struct MetaToml {
    log_format_version: &'static str,
    command: &'static str,
    spec: tracemux_core::source::ChannelSpec,
    sid: Uuid,
    started: String,
    decoder: String,
    encoding: String,
}

struct ConnectRecorder {
    sid: Uuid,
    source: String,
    raw: RawWriter,
    index: IndexWriter,
    count: u64,
}

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
pub async fn run(options: Options) -> Result<()> {
    let s = spec::parse(&options.spec).context("parsing channel spec")?;
    let mut source = spec::open(&s).context("opening source")?;
    source.open().await.context("Source::open failed")?;
    let meta = source.metadata();
    tracing::info!(kind = %meta.kind, iface = %meta.iface, "connect: opened");
    let mut recorder = match options.save.as_deref() {
        Some(path) => Some(ConnectRecorder::create(path, &s, &options.encoding)?),
        None => None,
    };

    let mut stdout = tokio::io::stdout();
    loop {
        match source.recv().await? {
            Some(Frame::Bytes(bytes)) => {
                write_payload(&mut stdout, recorder.as_mut(), bytes, Kind::Bytes).await?;
            }
            Some(Frame::Datagram { src, data }) => {
                if let Some(src) = src.as_deref() {
                    tracing::debug!(%src, "connect: datagram");
                }
                write_payload(&mut stdout, recorder.as_mut(), data, Kind::Datagram).await?;
            }
            Some(Frame::Other { kind, data }) => {
                tracing::debug!(kind, "connect: other frame");
                write_payload(&mut stdout, recorder.as_mut(), data, Kind::Frame).await?;
            }
            Some(Frame::Ssh { stream, data }) => {
                tracing::debug!(stream, "connect: ssh frame");
                write_payload(&mut stdout, recorder.as_mut(), data, Kind::Frame).await?;
            }
            Some(Frame::Visa { eom, data }) => {
                tracing::debug!(eom, "connect: visa frame");
                write_payload(&mut stdout, recorder.as_mut(), data, Kind::Frame).await?;
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
    if let Some(recorder) = recorder.as_mut() {
        recorder.flush()?;
    }
    source.close().await?;
    Ok(())
}

async fn write_payload(
    stdout: &mut tokio::io::Stdout,
    recorder: Option<&mut ConnectRecorder>,
    data: Bytes,
    kind: Kind,
) -> Result<()> {
    stdout.write_all(&data).await?;
    if let Some(recorder) = recorder {
        recorder.append(&data, kind)?;
    }
    Ok(())
}

impl ConnectRecorder {
    fn create(
        dir: &Path,
        channel_spec: &tracemux_core::source::ChannelSpec,
        encoding: &str,
    ) -> Result<Self> {
        if dir.exists() {
            if !dir.is_dir() {
                bail!("--save path is not a directory: {}", dir.display());
            }
            let non_empty = std::fs::read_dir(dir)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false);
            if non_empty {
                bail!(
                    "--save session-dir is non-empty; refusing to overwrite: {}",
                    dir.display()
                );
            }
        }
        std::fs::create_dir_all(dir).context("creating --save session-dir")?;
        let sid = Uuid::new_v4();
        let encoding = normalized_encoding(encoding);
        let decoder = format!("utf8-text:{encoding}");
        write_meta(dir, channel_spec, sid, &decoder, &encoding)?;
        let raw = RawWriter::create(dir).context("opening raw.bin")?;
        let index = IndexWriter::create(dir).context("opening index.jsonl")?;
        Ok(Self {
            sid,
            source: format!(
                "{}:{}",
                spec::kind_tag(channel_spec),
                spec::iface_tag(channel_spec)
            ),
            raw,
            index,
            count: 0,
        })
    }

    fn append(&mut self, data: &[u8], kind: Kind) -> Result<()> {
        let (off, len) = self.raw.append(data).context("raw append")?;
        let mut entry =
            IndexEntry::from_envelope(&synth_dual_ts(), self.sid, Dir::In, kind, off, len);
        entry.source = Some(self.source.clone());
        self.index.append(&entry).context("index append")?;
        self.count += 1;
        if self.count % 256 == 0 {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.raw.flush().context("flush raw.bin")?;
        self.index.flush().context("flush index.jsonl")?;
        Ok(())
    }
}

fn write_meta(
    dir: &Path,
    channel_spec: &tracemux_core::source::ChannelSpec,
    sid: Uuid,
    decoder: &str,
    encoding: &str,
) -> Result<()> {
    let started = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| tracemux_core::time::unix_ns_now().to_string());
    let meta = MetaToml {
        log_format_version: "1.0.0",
        command: "connect",
        spec: channel_spec.clone(),
        sid,
        started,
        decoder: decoder.to_string(),
        encoding: encoding.to_string(),
    };
    std::fs::write(
        dir.join("meta.toml"),
        toml::to_string_pretty(&meta).context("serialising meta.toml")?,
    )
    .context("writing meta.toml")
}

fn normalized_encoding(encoding: &str) -> String {
    let encoding = encoding.trim();
    if encoding.is_empty() {
        "utf-8".to_string()
    } else {
        encoding.to_ascii_lowercase()
    }
}

fn synth_dual_ts() -> DualTimestamp {
    let now_ns = tracemux_core::time::unix_ns_now();
    DualTimestamp {
        ts_origin_ns: now_ns,
        ts_ingest_ns: now_ns,
        mono_ns: 0,
        boot_id: Uuid::nil(),
        node_id: Uuid::nil(),
        clock_offset_ms: 0,
        clock_quality: ClockQuality::BestEffort,
        drift_ppm: 0.0,
        clock_source: ClockSource::System,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracemux_core::log::raw::RawReader;

    // REQ: FR-CLI-010
    #[tokio::test]
    async fn save_writes_session_dir_while_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.bin");
        let session = dir.path().join("session");
        std::fs::write(&input, b"hello").unwrap();

        run(Options {
            spec: format!("file:///{}", input.display()),
            save: Some(session.clone()),
            encoding: "shift_jis".to_string(),
        })
        .await
        .unwrap();

        let meta = std::fs::read_to_string(session.join("meta.toml")).unwrap();
        assert!(meta.contains("encoding = \"shift_jis\""));
        assert!(meta.contains("decoder = \"utf8-text:shift_jis\""));

        let index = std::fs::read_to_string(session.join("index.jsonl")).unwrap();
        let entry: IndexEntry = serde_json::from_str(index.lines().next().unwrap()).unwrap();
        assert_eq!(entry.kind, Kind::Bytes);
        assert_eq!(entry.dir, Dir::In);

        let mut raw = RawReader::open(&session).unwrap();
        assert_eq!(raw.read_at(entry.off, entry.len).unwrap(), b"hello");
    }

    // REQ: FR-CLI-010
    #[test]
    fn rejects_non_empty_save_dir() {
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().join("session");
        std::fs::create_dir(&save).unwrap();
        std::fs::write(save.join("existing"), b"data").unwrap();
        let spec = spec::parse("mock://unit").unwrap();
        let err = match ConnectRecorder::create(&save, &spec, "utf-8") {
            Ok(_) => panic!("expected non-empty save dir to fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("non-empty"));
    }
}
