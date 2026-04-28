//! Config schema v1 (`config_version = 1`).
//!
//! On-disk shape of `wanlogger.toml`. **Frozen v0.1.** New fields
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
    pub bind: String,
    /// Reject unauthenticated peers.
    #[serde(default)]
    pub require_auth: bool,
}

impl Default for ServerCfg {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:9443".to_string(),
            require_auth: true,
        }
    }
}

/// Named channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCfg {
    /// Channel spec.
    pub spec: ChannelSpec,
    /// Optional human label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
}
