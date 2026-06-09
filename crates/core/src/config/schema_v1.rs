//! Config schema v1 (`config_version = 1`).
//!
//! On-disk shape of `tracemux.toml`. **Frozen v0.1.** New fields
//! must remain backwards-compatible (`#[serde(default)]` +
//! `Option<_>`); breaking changes go through
//! `crate::config::migrate` and bump `config_version`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::source::ChannelSpec;

/// Top-level v1 config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigV1 {
    /// Schema version. Must be `1`.
    pub config_version: u32,
    /// Optional node identity overrides.
    #[serde(default)]
    pub node: NodeCfg,
    /// Optional server bind config.
    #[serde(default)]
    pub server: ServerCfg,
    /// Export command defaults.
    #[serde(default)]
    pub export: ExportCfg,
    /// UI/server delivery tuning.
    #[serde(default)]
    pub ui: UiCfg,
    /// Named channel definitions.
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelCfg>,
    /// Log retention.
    #[serde(default)]
    pub retention: RetentionCfg,
}

impl Default for ConfigV1 {
    fn default() -> Self {
        Self {
            config_version: 1,
            node: NodeCfg::default(),
            server: ServerCfg::default(),
            export: ExportCfg::default(),
            ui: UiCfg::default(),
            channels: BTreeMap::new(),
            retention: RetentionCfg::default(),
        }
    }
}

/// Per-node identity overrides.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeCfg {
    /// Optional human-readable label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Server bind configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCfg {
    /// `host:port` (default `127.0.0.1:9443`).
    #[serde(default = "default_server_bind")]
    pub bind: String,
    /// Root directory for server-created session-dirs.
    #[serde(default = "default_server_session_root")]
    pub session_root: String,
    /// Default text encoding for server-side decoded records.
    #[serde(default = "default_server_encoding")]
    pub encoding: String,
    /// Content detection mode: `configured`, `auto`, `suggest`, or `off`.
    #[serde(default = "default_server_detect_mode")]
    pub detect_mode: String,
    /// Optional session-dir name pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name_pattern: Option<String>,
    /// Serial source startup settings.
    #[serde(default)]
    pub serial: SerialStartupCfg,
    /// Files containing one argon2id PHC bearer-token hash per line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_phc_files: Vec<String>,
    /// Optional TLS listener settings.
    #[serde(default)]
    pub tls: TlsCfg,
    /// Reject unauthenticated peers.
    #[serde(default = "default_server_require_auth")]
    pub require_auth: bool,
}

fn default_server_bind() -> String {
    "127.0.0.1:9443".to_string()
}

fn default_server_session_root() -> String {
    "tracemux-sessions".to_string()
}

fn default_server_encoding() -> String {
    "utf-8".to_string()
}

fn default_server_detect_mode() -> String {
    "configured".to_string()
}

const fn default_server_require_auth() -> bool {
    true
}

impl Default for ServerCfg {
    fn default() -> Self {
        Self {
            bind: default_server_bind(),
            session_root: default_server_session_root(),
            encoding: default_server_encoding(),
            detect_mode: default_server_detect_mode(),
            session_name_pattern: None,
            serial: SerialStartupCfg::default(),
            token_phc_files: Vec::new(),
            tls: TlsCfg::default(),
            require_auth: default_server_require_auth(),
        }
    }
}

/// Serial source startup settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialStartupCfg {
    /// Detect/open serial sources when `tracemux serve` starts.
    #[serde(default)]
    pub open_all: bool,
    /// Explicit serial ports. Empty means detect all host serial candidates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    /// Baud rate used by auto-started serial sources.
    #[serde(default = "default_serial_baud")]
    pub baud: u32,
    /// Data bits used by auto-started serial sources.
    #[serde(default = "default_serial_data_bits")]
    pub data_bits: u8,
    /// Parity used by auto-started serial sources (`none`, `even`, `odd`).
    #[serde(default = "default_serial_parity")]
    pub parity: String,
    /// Stop bits used by auto-started serial sources.
    #[serde(default = "default_serial_stop_bits")]
    pub stop_bits: u8,
    /// Flow control used by auto-started serial sources (`none`, `hardware`, `software`).
    #[serde(default = "default_serial_flow")]
    pub flow: String,
}

const fn default_serial_baud() -> u32 {
    115_200
}

const fn default_serial_data_bits() -> u8 {
    8
}

fn default_serial_parity() -> String {
    "none".to_string()
}

const fn default_serial_stop_bits() -> u8 {
    1
}

fn default_serial_flow() -> String {
    "none".to_string()
}

impl Default for SerialStartupCfg {
    fn default() -> Self {
        Self {
            open_all: false,
            ports: Vec::new(),
            baud: default_serial_baud(),
            data_bits: default_serial_data_bits(),
            parity: default_serial_parity(),
            stop_bits: default_serial_stop_bits(),
            flow: default_serial_flow(),
        }
    }
}

/// TLS listener settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsCfg {
    /// Whether HTTPS/WSS should be enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Directory containing `server.crt` and `server.key`, or where a
    /// self-signed pair should be generated on first start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

/// Named channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCfg {
    /// Channel spec.
    pub spec: ChannelSpec,
    /// Optional human label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional default local-echo mode for interactive terminals
    /// (`auto`, `on`, `off`). Consumed by clients; the server does not echo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_echo: Option<String>,
    /// Optional default line ending the terminal sends on Enter
    /// (`auto`, `cr`, `lf`, `crlf`). Consumed by clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newline: Option<String>,
}

/// Export command defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportCfg {
    /// Default export timezone when `tracemux export --tz` is omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Default export text encoding when `tracemux export --encoding` is omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

/// UI/server delivery tuning.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiCfg {
    /// Minimum milliseconds between live WSS subscription sends (`0` disables).
    #[serde(default)]
    pub live_flush_ms: u64,
}

/// Log retention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionCfg {
    /// Days to keep session-dirs (`0` disables).
    pub keep_days: u32,
}

impl Default for RetentionCfg {
    fn default() -> Self {
        Self { keep_days: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let c = ConfigV1::default();
        let s = toml::to_string(&c).unwrap();
        let c2: ConfigV1 = toml::from_str(&s).unwrap();
        assert_eq!(c2.config_version, 1);
        assert_eq!(c2.server.bind, "127.0.0.1:9443");
        assert_eq!(c2.server.session_root, "tracemux-sessions");
        assert_eq!(c2.server.encoding, "utf-8");
        assert_eq!(c2.server.detect_mode, "configured");
        assert!(!c2.server.serial.open_all);
        assert_eq!(c2.server.serial.baud, 115_200);
        assert!(c2.server.token_phc_files.is_empty());
        assert!(!c2.server.tls.enabled);
        assert!(c2.export.timezone.is_none());
        assert!(c2.export.encoding.is_none());
        assert_eq!(c2.ui.live_flush_ms, 0);
        assert_eq!(c2.retention.keep_days, 0);
    }

    #[test]
    fn parses_with_a_channel() {
        let s = r#"
            config_version = 1
            [server]
            bind = "0.0.0.0:9443"
            require_auth = false
            [channels.lab]
            label = "lab tcp"
            [channels.lab.spec]
            kind = "tcp"
            addr = "10.0.0.1:5555"
        "#;
        let c: ConfigV1 = toml::from_str(s).unwrap();
        assert_eq!(c.channels.len(), 1);
        let ch = c.channels.get("lab").unwrap();
        match &ch.spec {
            ChannelSpec::Tcp { addr } => assert_eq!(addr, "10.0.0.1:5555"),
            other => panic!("wrong: {other:?}"),
        }
    }

    #[test]
    fn partial_server_table_uses_secure_defaults() {
        let s = r#"
            config_version = 1
            [server]
            bind = "0.0.0.0:9443"
        "#;
        let c: ConfigV1 = toml::from_str(s).unwrap();
        assert_eq!(c.server.bind, "0.0.0.0:9443");
        assert_eq!(c.server.session_root, "tracemux-sessions");
        assert_eq!(c.server.encoding, "utf-8");
        assert_eq!(c.server.detect_mode, "configured");
        assert!(!c.server.serial.open_all);
        assert_eq!(c.server.serial.baud, 115_200);
        assert!(c.server.token_phc_files.is_empty());
        assert!(!c.server.tls.enabled);
        assert!(c.server.require_auth);
    }

    #[test]
    fn parses_server_runtime_settings() {
        let s = r#"
            config_version = 1
            [server]
            bind = "127.0.0.1:9443"
            session_root = "sessions"
            encoding = "shift_jis"
            detect_mode = "suggest"
            session_name_pattern = "{prefix}-{kind}-{iface}"
            token_phc_files = ["tokens.phc", "ops.phc"]
            require_auth = true

            [server.serial]
            open_all = true
            ports = ["COM7", "COM8"]
            baud = 9600
            data_bits = 7
            parity = "even"
            stop_bits = 2
            flow = "hardware"

            [server.tls]
            enabled = true
            dir = "tls-state"

            [export]
            timezone = "Asia/Tokyo"
            encoding = "utf-8"

            [ui]
            live_flush_ms = 16

            [retention]
            keep_days = 14
        "#;
        let c: ConfigV1 = toml::from_str(s).unwrap();
        assert_eq!(c.server.session_root, "sessions");
        assert_eq!(c.server.encoding, "shift_jis");
        assert_eq!(c.server.detect_mode, "suggest");
        assert_eq!(
            c.server.session_name_pattern.as_deref(),
            Some("{prefix}-{kind}-{iface}")
        );
        assert!(c.server.serial.open_all);
        assert_eq!(c.server.serial.ports, ["COM7", "COM8"]);
        assert_eq!(c.server.serial.baud, 9_600);
        assert_eq!(c.server.serial.data_bits, 7);
        assert_eq!(c.server.serial.parity, "even");
        assert_eq!(c.server.serial.stop_bits, 2);
        assert_eq!(c.server.serial.flow, "hardware");
        assert_eq!(c.server.token_phc_files, ["tokens.phc", "ops.phc"]);
        assert!(c.server.tls.enabled);
        assert_eq!(c.server.tls.dir.as_deref(), Some("tls-state"));
        assert_eq!(c.export.timezone.as_deref(), Some("Asia/Tokyo"));
        assert_eq!(c.export.encoding.as_deref(), Some("utf-8"));
        assert_eq!(c.ui.live_flush_ms, 16);
        assert_eq!(c.retention.keep_days, 14);
    }
}
