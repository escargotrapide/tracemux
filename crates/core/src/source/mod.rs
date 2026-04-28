//! `Source` trait — produces frames from a transport. **Frozen v0.1.**
//!
//! See `.github/skills/add-source/SKILL.md` for the playbook.
//!
//! A `Source` does not parse semantics; that is the job of [`Framer`]
//! and then [`Decoder`]. Source-only transports (pcap, RTT, CAN sniff)
//! implement `Source` only — they do not implement [`crate::sink::Sink`].
//!
//! [`Framer`]: crate::framer::Framer
//! [`Decoder`]: crate::decoder::Decoder

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::Result;

/// Producer-side frame.
///
/// **Frozen v0.1.** New variants require an ADR + wire-protocol bump
/// (because `kind` strings are part of the wire schema).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Frame {
    /// Raw bytes (default for byte-stream transports).
    Bytes(Bytes),
    /// One datagram with optional source address.
    Datagram {
        /// Source address (transport-specific string), if any.
        src: Option<String>,
        /// Datagram payload.
        data: Bytes,
    },
    /// SSH-style multiplexed stream id + bytes.
    Ssh {
        /// SSH stream id (0=stdin/stdout, 1=stderr, …).
        stream: u8,
        /// Bytes.
        data: Bytes,
    },
    /// VISA-style instrument response with End-Of-Message flag.
    Visa {
        /// True when the instrument signals end-of-message.
        eom: bool,
        /// Bytes.
        data: Bytes,
    },
    /// Other / extension. The `kind` is part of the wire schema.
    Other {
        /// Stable kind string.
        kind: &'static str,
        /// Bytes.
        data: Bytes,
    },
}

/// Out-of-band control event from a [`Source`].
///
/// **Frozen v0.1.**
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ControlEvt {
    /// Channel just connected.
    Connected,
    /// Channel disconnected (reconnect may be in progress).
    Disconnected {
        /// Optional human-readable reason.
        reason: Option<String>,
    },
    /// End of stream (no further frames).
    Eof,
    /// Error event (with stable id from [`crate::error_id`]).
    Error {
        /// Error id.
        id: crate::ErrorId,
        /// Human-readable detail.
        message: String,
    },
    /// Terminal resize event (rows / cols).
    Resize {
        /// Rows.
        rows: u16,
        /// Cols.
        cols: u16,
    },
    /// VISA Service Request.
    Srq,
    /// Modem / line-state change (DCD/DSR/CTS/RI bitmask, transport-specific).
    LineState(u32),
    /// Custom event with stable kind string.
    Custom {
        /// Kind.
        kind: &'static str,
        /// Optional payload.
        data: Option<Bytes>,
    },
}

/// Static metadata about a channel (filled on `open`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMeta {
    /// Source kind, e.g. `"serial"`, `"tcp"`.
    pub kind: String,
    /// Interface descriptor, e.g. `"COM3"`, `"127.0.0.1:5555"`.
    pub iface: String,
    /// Free-form key/value tags.
    #[serde(default)]
    pub tags: std::collections::BTreeMap<String, String>,
}

/// Configuration for opening a channel.
///
/// **Frozen v0.1.** Variants are part of the on-disk config schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ChannelSpec {
    /// Serial port.
    Serial {
        /// Port name (`COM3`, `/dev/ttyUSB0`).
        port: String,
        /// Baud rate.
        baud: u32,
        /// Data bits (5..=8).
        data_bits: u8,
        /// Parity (`"none"`, `"even"`, `"odd"`).
        parity: String,
        /// Stop bits (1 or 2).
        stop_bits: u8,
        /// Flow control (`"none"`, `"hardware"`, `"software"`).
        flow: String,
    },
    /// TCP client.
    Tcp {
        /// Host:port.
        addr: String,
    },
    /// UDP listener / sender.
    Udp {
        /// Bind address.
        bind: String,
    },
    /// Local file (tail mode).
    File {
        /// Path.
        path: String,
        /// True to follow new bytes.
        follow: bool,
    },
    /// Named pipe / Unix domain socket.
    Pipe {
        /// Path.
        path: String,
    },
    /// Spawn a child process and capture its stdout/stderr.
    Process {
        /// Argv.
        argv: Vec<String>,
    },
    /// In-memory mock for tests.
    Mock {
        /// Free-form tag.
        tag: String,
    },
    /// Replay from an existing `session-dir/`.
    Replay {
        /// Session directory.
        path: String,
    },
    /// Syslog (UDP/TCP/TLS).
    Syslog {
        /// Bind address.
        bind: String,
    },
    /// MQTT subscriber.
    Mqtt {
        /// Broker URL.
        broker: String,
        /// Topic.
        topic: String,
    },
    /// HTTP webhook receiver.
    HttpWebhook {
        /// Bind address.
        bind: String,
        /// Path prefix.
        path: String,
    },
    /// Telnet — stub in v0.1.
    Telnet {
        /// Host:port.
        addr: String,
    },
    /// SSH — stub in v0.1.
    Ssh {
        /// Host:port.
        addr: String,
        /// Username.
        user: String,
    },
    /// VISA — stub in v0.1.
    Visa {
        /// Resource string.
        resource: String,
    },
    /// Remote `wanlogger serve` proxy — stub in v0.1.
    Remote {
        /// `wss://host:port/ws`.
        url: String,
    },
    /// systemd journald — stub in v0.1.
    Journald {
        /// Optional unit filter.
        unit: Option<String>,
    },
    /// Windows Event Log — stub in v0.1.
    WinEventLog {
        /// Channel name.
        channel: String,
    },
    /// Windows ETW — stub in v0.1.
    Etw {
        /// Provider GUID.
        provider: String,
    },
    /// J-Link RTT — stub in v0.1.
    JLinkRtt {
        /// Channel index.
        channel: u8,
    },
    /// CAN bus — stub in v0.1.
    CanBus {
        /// Interface name.
        iface: String,
    },
}

/// A producer of [`Frame`]s + [`ControlEvt`]s.
///
/// **Frozen v0.1.** Implementations live under
/// `crates/core/src/source/<name>.rs`. Add new ones via the
/// `add-source` skill.
#[async_trait]
pub trait Source: Send + Sync + 'static {
    /// Open the underlying transport. Must be idempotent / re-openable.
    async fn open(&mut self) -> Result<()>;

    /// Receive the next frame. `None` on graceful EOF.
    async fn recv(&mut self) -> Result<Option<Frame>>;

    /// Drain the next control event, if any (non-blocking semantics
    /// preferred; implementations may use a separate channel).
    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>>;

    /// Channel metadata (after `open`).
    fn metadata(&self) -> ChannelMeta;

    /// Close the channel. Must be idempotent.
    async fn close(&mut self) -> Result<()>;
}

// ---- v0.1 source impls (most are stubs; keep modules compiling) ----

pub mod can_stub;
pub mod etw_stub;
pub mod file;
pub mod http_webhook;
pub mod journald_stub;
pub mod mock;
pub mod mqtt;
pub mod pipe;
pub mod process;
pub mod remote_stub;
pub mod replay;
pub mod rtt_stub;
pub mod serial;
pub mod ssh_stub;
pub mod syslog;
pub mod tcp;
pub mod telnet_stub;
pub mod udp;
pub mod visa_stub;
pub mod wineventlog_stub;
