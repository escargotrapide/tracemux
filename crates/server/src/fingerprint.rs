//! TOFU fingerprint pin store. **Critical path.** Frozen v0.1.
//!
//! Clients (browser / Tauri / CLI) pin the server's leaf-certificate
//! SHA-256 fingerprint on first connection. Subsequent connections
//! verify that the presented certificate hashes to the same value;
//! a mismatch surfaces as [`E-2103`].
//!
//! The store is a tiny JSON file keyed by `host:port` (or any
//! caller-defined label):
//!
//! ```json
//! {
//!   "schema": "wanlogger/tofu/v1",
//!   "pins": {
//!     "127.0.0.1:7443": "sha256:ab12...beef"
//!   }
//! }
//! ```
//!
//! [`E-2103`]: wanlogger_core::ErrorId::E2103TofuMismatch

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use wanlogger_core::{ErrorId, WanloggerError};

/// Schema string written into the pin file. Bump when the on-disk
/// shape changes.
pub const SCHEMA: &str = "wanlogger/tofu/v1";

/// Errors produced by the TOFU pin store.
#[derive(Debug, Error)]
pub enum FingerprintError {
    /// I/O while reading or writing the pin file.
    #[error("E-2103: tofu I/O at {path}: {source}")]
    Io {
        /// Path that was being accessed.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// JSON parse / serialise error.
    #[error("E-2103: tofu json: {0}")]
    Json(String),
    /// Schema mismatch in an existing pin file.
    #[error("E-2103: tofu schema mismatch: got {0}")]
    Schema(String),
    /// The presented certificate does not match the pinned value.
    #[error("E-2103: tofu fingerprint mismatch for {host}: pinned {pinned}, got {got}")]
    Mismatch {
        /// Host:port label.
        host: String,
        /// Pinned fingerprint.
        pinned: String,
        /// Observed fingerprint.
        got: String,
    },
}

impl FingerprintError {
    /// Stable [`ErrorId`].
    #[must_use]
    pub const fn id(&self) -> ErrorId {
        ErrorId::E2103TofuMismatch
    }
}

impl From<FingerprintError> for WanloggerError {
    fn from(e: FingerprintError) -> Self {
        let id = e.id();
        WanloggerError::new(id, e.to_string())
    }
}

/// Compute the canonical fingerprint of a DER-encoded certificate.
///
/// The format is `sha256:<lowercase hex>` so the value is greppable
/// in logs and config files.
#[must_use]
pub fn fingerprint_der(cert_der: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert_der);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// On-disk shape of the pin store.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinFile {
    /// Schema string. Must equal [`SCHEMA`].
    #[serde(default = "default_schema")]
    pub schema: String,
    /// Map of `host:port` → `sha256:<hex>`.
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
}

fn default_schema() -> String {
    SCHEMA.to_string()
}

impl PinFile {
    /// Empty pin file with the current [`SCHEMA`].
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema: SCHEMA.to_string(),
            pins: BTreeMap::new(),
        }
    }
}

/// File-backed TOFU pin store.
#[derive(Debug, Clone)]
pub struct PinStore {
    path: PathBuf,
    inner: PinFile,
}

impl PinStore {
    /// Open an existing store, or create an empty one at `path`.
    ///
    /// # Errors
    /// Returns [`FingerprintError::Io`] / [`FingerprintError::Json`] /
    /// [`FingerprintError::Schema`].
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, FingerprintError> {
        let path = path.into();
        let inner = if path.exists() {
            let bytes = std::fs::read(&path).map_err(|e| FingerprintError::Io {
                path: path.clone(),
                source: e,
            })?;
            let pf: PinFile = serde_json::from_slice(&bytes)
                .map_err(|e| FingerprintError::Json(e.to_string()))?;
            if pf.schema != SCHEMA {
                return Err(FingerprintError::Schema(pf.schema));
            }
            pf
        } else {
            PinFile::empty()
        };
        Ok(Self { path, inner })
    }

    /// Backing file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of stored pins.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.pins.len()
    }

    /// Whether the store has zero pins.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.pins.is_empty()
    }

    /// Get the pinned fingerprint for `host`, if any.
    #[must_use]
    pub fn get(&self, host: &str) -> Option<&str> {
        self.inner.pins.get(host).map(String::as_str)
    }

    /// Verify or trust-on-first-use.
    ///
    /// On first contact with `host` the fingerprint is recorded and
    /// the store is flushed to disk. On subsequent contacts the
    /// presented fingerprint must match exactly, otherwise
    /// [`FingerprintError::Mismatch`] is returned.
    ///
    /// # Errors
    /// See variants of [`FingerprintError`].
    pub fn verify_or_pin(
        &mut self,
        host: &str,
        cert_der: &[u8],
    ) -> Result<PinOutcome, FingerprintError> {
        let fp = fingerprint_der(cert_der);
        if let Some(pinned) = self.inner.pins.get(host) {
            if pinned == &fp {
                return Ok(PinOutcome::Matched);
            }
            return Err(FingerprintError::Mismatch {
                host: host.to_string(),
                pinned: pinned.clone(),
                got: fp,
            });
        }
        self.inner.pins.insert(host.to_string(), fp);
        self.save()?;
        Ok(PinOutcome::Pinned)
    }

    /// Force the pinned fingerprint for `host`. Mostly for tests and
    /// the `wanlogger trust` admin command.
    ///
    /// # Errors
    /// Returns [`FingerprintError::Io`] / [`FingerprintError::Json`].
    pub fn force_pin(&mut self, host: &str, fp: &str) -> Result<(), FingerprintError> {
        self.inner.pins.insert(host.to_string(), fp.to_string());
        self.save()
    }

    /// Persist the store to disk.
    ///
    /// # Errors
    /// Returns [`FingerprintError::Io`] / [`FingerprintError::Json`].
    pub fn save(&self) -> Result<(), FingerprintError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| FingerprintError::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
        }
        let bytes = serde_json::to_vec_pretty(&self.inner)
            .map_err(|e| FingerprintError::Json(e.to_string()))?;
        std::fs::write(&self.path, bytes).map_err(|e| FingerprintError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        Ok(())
    }
}

/// Outcome of [`PinStore::verify_or_pin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinOutcome {
    /// First contact: the fingerprint was recorded.
    Pinned,
    /// Subsequent contact: the fingerprint matched the pinned value.
    Matched,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "wanlogger-fp-{}-{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        p
    }

    #[test]
    fn fingerprint_format_is_stable() {
        let fp = fingerprint_der(b"hello world");
        // Known SHA-256 of "hello world"
        assert_eq!(
            fp,
            "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn first_contact_pins_then_matches() {
        let path = tmp("first");
        let mut store = PinStore::open(&path).unwrap();
        assert!(store.is_empty());
        let cert = b"DERDERDER";
        assert_eq!(store.verify_or_pin("h:1", cert).unwrap(), PinOutcome::Pinned);
        assert_eq!(store.len(), 1);
        // Second time matches.
        assert_eq!(store.verify_or_pin("h:1", cert).unwrap(), PinOutcome::Matched);

        // Reopen and confirm persistence.
        let store2 = PinStore::open(&path).unwrap();
        assert_eq!(store2.get("h:1"), Some(fingerprint_der(cert).as_str()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn mismatch_is_rejected() {
        let path = tmp("mismatch");
        let mut store = PinStore::open(&path).unwrap();
        store.verify_or_pin("h:1", b"original").unwrap();
        let err = store.verify_or_pin("h:1", b"different").unwrap_err();
        assert!(matches!(err, FingerprintError::Mismatch { .. }));
        assert_eq!(err.id(), ErrorId::E2103TofuMismatch);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn schema_mismatch_rejected() {
        let path = tmp("schema");
        std::fs::write(&path, br#"{"schema":"wanlogger/tofu/v999","pins":{}}"#).unwrap();
        let err = PinStore::open(&path).unwrap_err();
        assert!(matches!(err, FingerprintError::Schema(_)));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn force_pin_overrides() {
        let path = tmp("force");
        let mut store = PinStore::open(&path).unwrap();
        store.force_pin("h:1", "sha256:deadbeef").unwrap();
        assert_eq!(store.get("h:1"), Some("sha256:deadbeef"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn wanlogger_error_carries_canonical_id() {
        let e: WanloggerError = FingerprintError::Mismatch {
            host: "h".into(),
            pinned: "p".into(),
            got: "g".into(),
        }
        .into();
        assert_eq!(e.id, ErrorId::E2103TofuMismatch);
    }
}

