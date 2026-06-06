//! Process [`Source`] — spawns a child and captures its
//! stdout / stderr.
//!
//! - stdout bytes are emitted as [`Frame::Bytes`].
//! - stderr bytes are emitted as
//!   [`Frame::Other { kind: "stderr", data }`].
//! - When the child exits, [`ControlEvt::Eof`] is queued and
//!   subsequent `recv()` returns `None`.
//!
//! Stdout and stderr are drained by independent tokio tasks that
//! send frames to an unbounded channel. This avoids the `select!`
//! race condition where one branch can consume and drop buffered
//! data from the other pipe on Linux.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::process::Stdio;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::sink::process::ProcessSink;
use crate::{ErrorId, Result, TraceMuxError};

const READ_CHUNK: usize = 8 * 1024;

/// Process source.
#[derive(Debug)]
pub struct ProcessSource {
    argv: Vec<String>,
    child: Option<Child>,
    rx: Option<mpsc::UnboundedReceiver<Result<Frame>>>,
    pending_ctl: VecDeque<ControlEvt>,
    eof: bool,
}

impl ProcessSource {
    /// Construct from an argv-style command.
    ///
    /// # Panics
    /// Never directly; `open()` reports a missing `argv[0]` via
    /// [`ErrorId::E1101SourceOpen`].
    #[must_use]
    pub fn new(argv: Vec<String>) -> Self {
        Self {
            argv,
            child: None,
            rx: None,
            pending_ctl: VecDeque::new(),
            eof: false,
        }
    }

    /// Spawn a child with piped stdin and split it into source/sink halves.
    ///
    /// The returned source is already open; a later [`Source::open`] call is
    /// a no-op so the source can be passed to the server runner.
    pub fn spawn_duplex(argv: Vec<String>) -> Result<(Self, ProcessSink)> {
        let mut source = Self::new(argv);
        let stdin = source.spawn(true)?;
        let stdin = stdin.ok_or_else(|| {
            TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                "process source: child stdin was not piped",
            )
        })?;
        let iface = source.argv.join(" ");
        Ok((source, ProcessSink::new(iface, stdin)))
    }

    fn spawn(&mut self, pipe_stdin: bool) -> Result<Option<ChildStdin>> {
        if self.argv.is_empty() {
            return Err(TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                "process source: argv is empty",
            ));
        }
        if self.child.is_some() {
            return Ok(None);
        }
        let mut cmd = Command::new(&self.argv[0]);
        cmd.args(&self.argv[1..]);
        cmd.stdin(if pipe_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                format!("spawn {}: {e}", self.argv[0]),
            )
            .with_source(e)
        })?;
        let stdin = child.stdin.take();

        // Drain stdout and stderr in independent tasks to avoid select! races.
        let (tx, rx) = mpsc::unbounded_channel::<Result<Frame>>();
        self.rx = Some(rx);

        if let Some(mut stdout) = child.stdout.take() {
            let tx2 = tx.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; READ_CHUNK];
                loop {
                    match stdout.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let data = Bytes::copy_from_slice(&buf[..n]);
                            if tx2.send(Ok(Frame::Bytes(data))).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx2.send(Err(read_err("stdout", e)));
                            break;
                        }
                    }
                }
            });
        }

        if let Some(mut stderr) = child.stderr.take() {
            let tx2 = tx.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; READ_CHUNK];
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let data = Bytes::copy_from_slice(&buf[..n]);
                            if tx2
                                .send(Ok(Frame::Other {
                                    kind: "stderr",
                                    data,
                                }))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = tx2.send(Err(read_err("stderr", e)));
                            break;
                        }
                    }
                }
            });
        }

        // Drop the original sender so the channel closes when both tasks exit.
        drop(tx);

        self.child = Some(child);
        self.pending_ctl.push_back(ControlEvt::Connected);
        Ok(stdin)
    }
}

#[async_trait]
impl Source for ProcessSource {
    async fn open(&mut self) -> Result<()> {
        self.spawn(false)?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        if self.eof {
            return Ok(None);
        }
        let Some(rx) = self.rx.as_mut() else {
            self.eof = true;
            self.pending_ctl.push_back(ControlEvt::Eof);
            return Ok(None);
        };
        match rx.recv().await {
            Some(Ok(frame)) => Ok(Some(frame)),
            Some(Err(e)) => Err(e),
            None => {
                self.eof = true;
                self.pending_ctl.push_back(ControlEvt::Eof);
                Ok(None)
            }
        }
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(self.pending_ctl.pop_front())
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "process".into(),
            iface: self.argv.join(" "),
            tags: BTreeMap::new(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(mut c) = self.child.take() {
            let _ = c.start_kill();
        }
        self.rx = None;
        Ok(())
    }
}

fn read_err(stream: &str, e: std::io::Error) -> TraceMuxError {
    TraceMuxError::new(
        ErrorId::E1102SourceClosed,
        format!("process {stream} read: {e}"),
    )
    .with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::Sink;

    fn echo_argv(text: &str) -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".into(), "/C".into(), format!("echo {text}")]
        } else {
            vec!["sh".into(), "-c".into(), format!("printf '{text}'")]
        }
    }

    fn cat_argv() -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".into(), "/C".into(), "more".into()]
        } else {
            vec!["cat".into()]
        }
    }

    #[tokio::test]
    async fn captures_stdout_then_eof() {
        let mut src = ProcessSource::new(echo_argv("hello"));
        src.open().await.unwrap();
        let mut got: Vec<u8> = Vec::new();
        loop {
            match src.recv().await.unwrap() {
                Some(Frame::Bytes(b)) => got.extend_from_slice(&b),
                Some(Frame::Other { data, .. }) => got.extend_from_slice(&data),
                Some(_) => {}
                None => break,
            }
        }
        let s = String::from_utf8_lossy(&got);
        assert!(s.contains("hello"), "stdout contained: {s:?}");
    }

    #[tokio::test]
    async fn empty_argv_is_e1101() {
        let mut src = ProcessSource::new(Vec::new());
        let err = src.open().await.unwrap_err();
        assert_eq!(err.id, ErrorId::E1101SourceOpen);
    }

    // REQ: FR-SINK-PROCESS
    #[tokio::test]
    async fn duplex_sink_writes_to_child_stdin() {
        let (mut src, mut sink) = ProcessSource::spawn_duplex(cat_argv()).unwrap();
        sink.write(Bytes::from_static(b"hello\n")).await.unwrap();
        sink.close().await.unwrap();

        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), src.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        match frame {
            Frame::Bytes(bytes) | Frame::Other { data: bytes, .. } => {
                let text = String::from_utf8_lossy(&bytes);
                assert!(text.contains("hello"), "process output was {text:?}");
            }
            other => panic!("unexpected frame: {other:?}"),
        }
        src.close().await.unwrap();
    }
}
