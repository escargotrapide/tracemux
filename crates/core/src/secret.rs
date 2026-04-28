//! `secret://name` resolver backed by the OS keyring (`keyring` crate).
//! v0.1 stub. **Critical path** — see SECURITY.md.

use serde::{Deserialize, Serialize};

/// Reference to a secret stored in the OS keyring.
///
/// Configuration files store this as a `"secret://name"` string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(pub String);

impl SecretRef {
    /// Parse a `"secret://name"` string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        s.strip_prefix("secret://")
            .map(|name| Self(name.to_owned()))
    }
}

/// Newtype that hides the wrapped value from `Debug` / `Display` /
/// logging. Use this for any in-memory secret value.
pub struct Redact<T>(pub T);

impl<T> std::fmt::Debug for Redact<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}
