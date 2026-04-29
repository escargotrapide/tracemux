//! `secret://name` resolver backed by the OS keyring (`keyring` crate).
//! **Critical path** -- see SECURITY.md.
//!
//! Configuration files only ever store `secret://<name>` strings. The
//! actual secret value lives in the OS keyring under
//! `service = "wanlogger"`, `username = <name>`. The
//! [`SecretResolver`] trait abstracts the backend so tests can use an
//! in-memory store.
//!
//! Secrets returned to callers are wrapped in [`Redact`], which hides
//! the value from `Debug` / `Display`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error_id::{ErrorId, WanloggerError};

/// Service name used in every keyring entry written by wanlogger.
pub const KEYRING_SERVICE: &str = "wanlogger";

/// Reference to a secret stored in the OS keyring.
///
/// Configuration files store this as a `"secret://name"` string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRef(pub String);

impl SecretRef {
    /// Parse a `"secret://name"` string. Names must be non-empty and
    /// contain only `[A-Za-z0-9._-]`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let name = s.strip_prefix("secret://")?;
        if name.is_empty() || !name.chars().all(is_safe_name_char) {
            return None;
        }
        Some(Self(name.to_owned()))
    }

    /// The bare name (without `secret://` prefix).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.0
    }
}

fn is_safe_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'
}

/// Newtype that hides the wrapped value from `Debug` / `Display` /
/// logging. Use this for any in-memory secret value.
pub struct Redact<T>(pub T);

impl<T> std::fmt::Debug for Redact<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl<T> std::fmt::Display for Redact<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl<T: Clone> Clone for Redact<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Backend trait for resolving / writing secrets.
pub trait SecretResolver: Send + Sync {
    /// Read `name` from the backend.
    fn get(&self, name: &str) -> Result<Redact<String>, WanloggerError>;
    /// Write `name = value` to the backend.
    fn set(&self, name: &str, value: &str) -> Result<(), WanloggerError>;
    /// Remove `name` from the backend. Idempotent.
    fn delete(&self, name: &str) -> Result<(), WanloggerError>;
}

/// In-memory backend for tests. **Never use in production.**
#[derive(Default)]
pub struct MemorySecretResolver {
    inner: Mutex<HashMap<String, String>>,
}

impl MemorySecretResolver {
    /// Construct an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretResolver for MemorySecretResolver {
    fn get(&self, name: &str) -> Result<Redact<String>, WanloggerError> {
        self.inner
            .lock()
            .expect("poisoned")
            .get(name)
            .cloned()
            .map(Redact)
            .ok_or_else(|| {
                WanloggerError::new(
                    ErrorId::E1001PipelineGeneric,
                    format!("secret '{name}' not found"),
                )
            })
    }
    fn set(&self, name: &str, value: &str) -> Result<(), WanloggerError> {
        self.inner
            .lock()
            .expect("poisoned")
            .insert(name.to_owned(), value.to_owned());
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<(), WanloggerError> {
        self.inner.lock().expect("poisoned").remove(name);
        Ok(())
    }
}

/// OS keyring backend (production default).
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringResolver;

impl KeyringResolver {
    /// Construct.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl SecretResolver for KeyringResolver {
    fn get(&self, name: &str) -> Result<Redact<String>, WanloggerError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, name).map_err(map_kr)?;
        let v = entry.get_password().map_err(map_kr)?;
        Ok(Redact(v))
    }
    fn set(&self, name: &str, value: &str) -> Result<(), WanloggerError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, name).map_err(map_kr)?;
        entry.set_password(value).map_err(map_kr)
    }
    fn delete(&self, name: &str) -> Result<(), WanloggerError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, name).map_err(map_kr)?;
        match entry.delete_password() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_kr(e)),
        }
    }
}

fn map_kr(e: keyring::Error) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, format!("keyring: {e}")).with_source(e)
}

/// Resolve a [`SecretRef`] against `backend`.
///
/// REQ: FR-SEC-001 -- TOML never carries the secret value.
pub fn resolve<R: SecretResolver + ?Sized>(
    backend: &R,
    sref: &SecretRef,
) -> Result<Redact<String>, WanloggerError> {
    backend.get(sref.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    // REQ: FR-SEC-001
    #[test]
    fn parse_accepts_simple_name() {
        let s = SecretRef::parse("secret://my-token.v2").unwrap();
        assert_eq!(s.name(), "my-token.v2");
    }

    // REQ: FR-SEC-001
    #[test]
    fn parse_rejects_empty_name() {
        assert!(SecretRef::parse("secret://").is_none());
    }

    // REQ: FR-SEC-001
    #[test]
    fn parse_rejects_unsafe_chars() {
        assert!(SecretRef::parse("secret://a/b").is_none());
        assert!(SecretRef::parse("secret://a b").is_none());
        assert!(SecretRef::parse("secret://a:b").is_none());
    }

    // REQ: FR-SEC-001
    #[test]
    fn parse_rejects_missing_scheme() {
        assert!(SecretRef::parse("my-token").is_none());
        assert!(SecretRef::parse("env://my-token").is_none());
    }

    // REQ: FR-SEC-001
    #[test]
    fn redact_hides_value_in_debug_and_display() {
        let r = Redact("hunter2".to_string());
        assert_eq!(format!("{r:?}"), "<redacted>");
        assert_eq!(format!("{r}"), "<redacted>");
    }

    // REQ: FR-SEC-001
    #[test]
    fn memory_backend_round_trips() {
        let m = MemorySecretResolver::new();
        m.set("api", "abc123").unwrap();
        let v = m.get("api").unwrap();
        assert_eq!(v.0, "abc123");
        m.delete("api").unwrap();
        assert!(m.get("api").is_err());
    }

    // REQ: FR-SEC-001
    #[test]
    fn resolve_uses_backend() {
        let m = MemorySecretResolver::new();
        m.set("token", "shhh").unwrap();
        let s = SecretRef::parse("secret://token").unwrap();
        let v = resolve(&m, &s).unwrap();
        assert_eq!(v.0, "shhh");
    }

    // REQ: FR-SEC-001
    #[test]
    fn missing_secret_carries_canonical_id() {
        let m = MemorySecretResolver::new();
        let err = m.get("nope").unwrap_err();
        assert_eq!(err.id, ErrorId::E1001PipelineGeneric);
    }
}
