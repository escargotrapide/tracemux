//! Process [`Source`] — spawns a child and captures its
//! stdout / stderr.
//!
//! - stdout bytes are emitted as [`Frame::Bytes`].
//! - stderr bytes are emitted as
//!   [`Frame::Other { kind: "stderr", data }`].
//! - When the child exits, [`ControlEvt::Eof`] is queued and
//!   subsequent `recv()` returns `None`.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::process::Stdio;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

const READ_CHUNK: usize = 8 * 1024;

/// Process source.
#[derive(Debug)]
pub struct ProcessSource {
    argv: Vec<String>,
    child: Option<Child>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
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
            stdout: None,
            stderr: None,
            pending_ctl: VecDeque::new(),
            eof: false,
        }
    }
}

#[async_trait]
impl Source for ProcessSource {
    async fn open(&mut self) -> Result<()> {
        if self.argv.is_empty() {
            return Err(WanloggerError::new(
                ErrorId::E1101SourceOpen,
                "process source: argv is empty",
            ));
        }
        let mut cmd = Command::new(&self.argv[0]);
        cmd.args(&self.argv[1..]);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            WanloggerError::new(
                ErrorId::E1101SourceOpen,
                format!("spawn {}: {e}", self.argv[0]),
            )
            .with_source(e)
        })?;
        self.stdout = child.stdout.take();
        self.stderr = child.stderr.take();
        self.child = Some(child);
        self.pending_ctl.push_back(ControlEvt::Connected);
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        if self.eof {
            return Ok(None);
        }
        let mut out_buf = vec![0u8; READ_CHUNK];
        let mut err_buf = vec![0u8; READ_CHUNK];
        // Race stdout vs stderr; whichever delivers first wins.
        let n_out = self.stdout.as_mut();
        let n_err = self.stderr.as_mut();
        let frame = match (n_out, n_err) {
            (Some(out), Some(err)) => tokio::select! {
                r = out.read(&mut out_buf) => match r {
                    Ok(0) => None,
                    Ok(n) => { out_buf.truncate(n); Some(Frame::Bytes(Bytes::from(out_buf))) }
                    Err(e) => return Err(read_err("stdout", e)),
                },
                r = err.read(&mut err_buf) => match r {
                    Ok(0) => None,
                    Ok(n) => {
                        err_buf.truncate(n);
                        Some(Frame::Other { kind: "stderr", data: Bytes::from(err_buf) })
                    }
                    Err(e) => return Err(read_err("stderr", e)),
                },
            },
            (Some(out), None) => match out.read(&mut out_buf).await {
                Ok(0) => None,
                Ok(n) => {
                    out_buf.truncate(n);
                    Some(Frame::Bytes(Bytes::from(out_buf)))
                }
                Err(e) => return Err(read_err("stdout", e)),
            },
            (None, Some(err)) => match err.read(&mut err_buf).await {
                Ok(0) => None,
                Ok(n) => {
                    err_buf.truncate(n);
                    Some(Frame::Other {
                        kind: "stderr",
                        data: Bytes::from(err_buf),
                    })
                }
                Err(e) => return Err(read_err("stderr", e)),
            },
            (None, None) => None,
        };
        if frame.is_none() {
            self.eof = true;
            self.pending_ctl.push_back(ControlEvt::Eof);
        }
        Ok(frame)
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
        self.stdout = None;
        self.stderr = None;
        Ok(())
    }
}

fn read_err(stream: &str, e: std::io::Error) -> WanloggerError {
    WanloggerError::new(
        ErrorId::E1102SourceClosed,
        format!("process {stream} read: {e}"),
    )
    .with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_argv(text: &str) -> Vec<String> {
        if cfg!(windows) {
            vec![
                "cmd".into(),
                "/C".into(),
                format!("echo {text}"),
            ]
        } else {
            vec!["sh".into(), "-c".into(), format!("printf '{text}'")]
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
}
