//! Config schema v1 (`config_version = 1`). v0.1 stub.

use serde::{Deserialize, Serialize};

/// Top-level v1 config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigV1 {
    /// Schema version. Must be `1`.
    pub config_version: u32,
}

impl Default for ConfigV1 {
    fn default() -> Self {
        Self { config_version: 1 }
    }
}
