//! PTY [`Source`] — spawns a child attached to a pseudo-console
//! (Windows ConPTY / Unix openpty) so it runs as a real interactive
//! terminal.
//!
//! Unlike [`ProcessSource`](super::process::ProcessSource), which uses
//! pipes (and therefore `isatty = false`), a PTY child:
//!
//! - sees a real terminal, so it echoes stdin, emits a single merged VT
//!   byte stream (colour, cursor control, screen clears), and runs
//!   full-screen TUIs;
//! - honours a window size that can be changed at runtime via
//!   [`PtySink::ctl`](crate::sink::pty::PtySink) with kind `"resize"`.
//!
//! Output bytes are emitted as [`Frame::Bytes`]. When the child exits,
//! [`ControlEvt::Eof`] is queued and `recv()` returns `None`.
//!
//! The real implementation requires the `pty` crate feature (which pulls
//! in `portable-pty`). Without it, [`PtySource::spawn_duplex`] returns
//! [`ErrorId::E1107PtyUnavailable`], mirroring the serial source pattern.
//!
//! REQ: FR-SRC-PTY

use std::collections::BTreeMap;
use std::collections::VecDeque;

use async_trait::async_trait;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::sink::pty::PtySink;
use crate::{ErrorId, Result, TraceMuxError};

#[cfg(feature = "pty")]
use {
    bytes::Bytes,
    portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize},
    std::io::Read,
    std::sync::{Arc, Mutex},
    tokio::sync::mpsc,
};

#[cfg(feature = "pty")]
const READ_CHUNK: usize = 8 * 1024;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// Clamp a requested terminal dimension to a sane, non-zero range.
#[must_use]
pub fn clamp_dim(value: u16) -> u16 {
    value.clamp(1, 10_000)
}

fn resolve_dim(value: u16, default: u16) -> u16 {
    if value == 0 {
        default
    } else {
        clamp_dim(value)
    }
}

/// PTY source.
pub struct PtySource {
    argv: Vec<String>,
    cols: u16,
    rows: u16,
    eof: bool,
    pending_ctl: VecDeque<ControlEvt>,
    #[cfg(feature = "pty")]
    child: Option<Box<dyn Child + Send + Sync>>,
    #[cfg(feature = "pty")]
    rx: Option<mpsc::UnboundedReceiver<Result<Frame>>>,
}

impl std::fmt::Debug for PtySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySource")
            .field("argv", &self.argv)
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("eof", &self.eof)
            .finish_non_exhaustive()
    }
}

impl PtySource {
    /// Construct from an argv-style command with an initial terminal size.
    ///
    /// `cols`/`rows` of `0` fall back to an 80x24 default.
    #[must_use]
    pub fn new(argv: Vec<String>, cols: u16, rows: u16) -> Self {
        Self {
            argv,
            cols: resolve_dim(cols, DEFAULT_COLS),
            rows: resolve_dim(rows, DEFAULT_ROWS),
            eof: false,
            pending_ctl: VecDeque::new(),
            #[cfg(feature = "pty")]
            child: None,
            #[cfg(feature = "pty")]
            rx: None,
        }
    }

    /// Spawn the child attached to a PTY and split it into source/sink halves.
    ///
    /// The returned source is already open; a later [`Source::open`] call is a
    /// no-op so the source can be passed to the server runner.
    ///
    /// # Errors
    /// Returns [`ErrorId::E1107PtyUnavailable`] if the PTY cannot be allocated,
    /// the child cannot be spawned, or the `pty` feature is disabled; and
    /// [`ErrorId::E1101SourceOpen`] if `argv` is empty.
    #[cfg(feature = "pty")]
    pub fn spawn_duplex(argv: Vec<String>, cols: u16, rows: u16) -> Result<(Self, PtySink)> {
        let mut source = Self::new(argv, cols, rows);
        let sink = source.spawn()?;
        Ok((source, sink))
    }

    /// Return a clear error for duplex PTY when the feature is disabled.
    #[cfg(not(feature = "pty"))]
    pub fn spawn_duplex(argv: Vec<String>, _cols: u16, _rows: u16) -> Result<(Self, PtySink)> {
        let prog = argv.into_iter().next().unwrap_or_default();
        Err(TraceMuxError::new(
            ErrorId::E1107PtyUnavailable,
            format!("pty source {prog} requires the `pty` feature"),
        ))
    }

    #[cfg(feature = "pty")]
    fn spawn(&mut self) -> Result<PtySink> {
        if self.argv.is_empty() {
            return Err(TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                "pty source: argv is empty",
            ));
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: self.rows,
                cols: self.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| {
                TraceMuxError::new(ErrorId::E1107PtyUnavailable, format!("openpty: {e}"))
            })?;

        let mut cmd = CommandBuilder::new(&self.argv[0]);
        cmd.args(&self.argv[1..]);
        let child = pair.slave.spawn_command(cmd).map_err(|e| {
            TraceMuxError::new(
                ErrorId::E1107PtyUnavailable,
                format!("spawn {} in pty: {e}", self.argv[0]),
            )
        })?;
        // The slave handle is not needed once the child owns it; dropping it
        // lets EOF propagate cleanly when the child exits.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| {
            TraceMuxError::new(ErrorId::E1107PtyUnavailable, format!("pty reader: {e}"))
        })?;
        let writer = pair.master.take_writer().map_err(|e| {
            TraceMuxError::new(ErrorId::E1107PtyUnavailable, format!("pty writer: {e}"))
        })?;

        // The master end is shared so the sink can resize the live terminal.
        let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(pair.master));

        // Drain the merged VT stream on a blocking thread (portable-pty I/O is
        // synchronous) and forward chunks to an unbounded channel.
        let (tx, rx) = mpsc::unbounded_channel::<Result<Frame>>();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = Bytes::copy_from_slice(&buf[..n]);
                        if tx.send(Ok(Frame::Bytes(data))).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(TraceMuxError::new(
                            ErrorId::E1102SourceClosed,
                            format!("pty read: {e}"),
                        )));
                        break;
                    }
                }
            }
        });

        self.child = Some(child);
        self.pending_ctl.push_back(ControlEvt::Connected);
        let iface = self.argv.join(" ");
        Ok(PtySink::new(iface, writer, master, self.cols, self.rows))
    }
}

#[async_trait]
impl Source for PtySource {
    async fn open(&mut self) -> Result<()> {
        #[cfg(feature = "pty")]
        {
            if self.child.is_some() {
                return Ok(());
            }
            // A standalone open (no sink) still needs the child running; drop
            // the sink half since this path has no write-back consumer.
            let _sink = self.spawn()?;
            Ok(())
        }
        #[cfg(not(feature = "pty"))]
        {
            let prog = self.argv.first().cloned().unwrap_or_default();
            Err(TraceMuxError::new(
                ErrorId::E1107PtyUnavailable,
                format!("pty source {prog} requires the `pty` feature"),
            ))
        }
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        if self.eof {
            return Ok(None);
        }
        #[cfg(feature = "pty")]
        {
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
        #[cfg(not(feature = "pty"))]
        {
            self.eof = true;
            self.pending_ctl.push_back(ControlEvt::Eof);
            Ok(None)
        }
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(self.pending_ctl.pop_front())
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "pty".into(),
            iface: self.argv.join(" "),
            tags: BTreeMap::new(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        #[cfg(feature = "pty")]
        {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
            }
            self.rx = None;
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "pty"))]
mod tests {
    use super::*;
    use crate::sink::Sink;

    fn shell_argv() -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd.exe".into(), "/K".into(), "echo ready".into()]
        } else {
            vec!["sh".into()]
        }
    }

    #[test]
    fn clamp_dim_bounds() {
        assert_eq!(clamp_dim(0), 1);
        assert_eq!(clamp_dim(80), 80);
        assert_eq!(clamp_dim(60_000), 10_000);
    }

    #[test]
    fn empty_argv_is_rejected() {
        let mut src = PtySource::new(Vec::new(), 80, 24);
        let err = src.spawn().unwrap_err();
        assert_eq!(err.id, ErrorId::E1101SourceOpen);
    }

    #[tokio::test]
    async fn spawns_pty_and_reads_output() {
        let (mut src, mut sink) = PtySource::spawn_duplex(shell_argv(), 80, 24).unwrap();
        // A resize ctl must be accepted by the paired sink.
        sink.ctl("resize", Some(Bytes::from_static(b"100x40")))
            .await
            .unwrap();

        // Read some output; a real terminal echoes/prints promptly.
        let mut got: Vec<u8> = Vec::new();
        for _ in 0..50 {
            match tokio::time::timeout(std::time::Duration::from_millis(500), src.recv()).await {
                Ok(Ok(Some(Frame::Bytes(b)))) => {
                    got.extend_from_slice(&b);
                    if !got.is_empty() {
                        break;
                    }
                }
                Ok(Ok(Some(_))) => {}
                // EOF, a recv error, or a timeout all end the read loop.
                Ok(Ok(None) | Err(_)) | Err(_) => break,
            }
        }
        src.close().await.unwrap();
        assert!(!got.is_empty(), "pty produced no output");
    }
}

#[cfg(all(test, not(feature = "pty")))]
mod tests_stub {
    use super::*;

    #[test]
    fn spawn_duplex_without_feature_errors() {
        let err = PtySource::spawn_duplex(vec!["cmd.exe".into()], 80, 24).unwrap_err();
        assert_eq!(err.id, ErrorId::E1107PtyUnavailable);
    }
}
