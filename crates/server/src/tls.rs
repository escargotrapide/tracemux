//! TLS setup: `rustls` + `rcgen` self-signed cert. **Critical path.**
//!
//! Frozen v0.1. See `docs/protocols/wire-protocol.md` and
//! [`SECURITY.md`](../../../../SECURITY.md).
//!
//! On first start the server generates a self-signed certificate
//! covering the configured SANs (default: `localhost`, `127.0.0.1`,
//! `::1`) and persists it as PEM under the config directory. The
//! cert/key are loaded into a [`rustls::ServerConfig`] suitable for
//! [`axum_server`].
//!
//! TOFU pinning of the cert by its SHA-256 fingerprint lives in
//! [`crate::fingerprint`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::{generate_simple_self_signed, CertifiedKey};
use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use thiserror::Error;
use tracemux_core::{ErrorId, TraceMuxError};

/// Default Subject Alternative Names baked into a freshly generated
/// self-signed certificate.
pub const DEFAULT_SANS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Filenames used by [`load_or_generate`].
pub mod files {
    /// Certificate PEM filename.
    pub const CERT: &str = "server.crt";
    /// Private key PEM filename.
    pub const KEY: &str = "server.key";
}

/// Errors produced by the TLS setup layer.
#[derive(Debug, Error)]
pub enum TlsError {
    /// Certificate / key generation via `rcgen` failed.
    #[error("E-2102: certificate generation failed: {0}")]
    Generate(String),
    /// I/O while reading or writing the cert / key.
    #[error("E-2102: tls I/O at {path}: {source}")]
    Io {
        /// Path that was being read or written.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// PEM parsing of an existing cert / key failed.
    #[error("E-2102: pem parse: {0}")]
    Pem(String),
    /// `rustls` config construction failed.
    #[error("E-2102: rustls config: {0}")]
    Rustls(String),
}

impl TlsError {
    /// Stable [`ErrorId`].
    #[must_use]
    pub const fn id(&self) -> ErrorId {
        ErrorId::E2102TlsHandshake
    }
}

impl From<TlsError> for TraceMuxError {
    fn from(e: TlsError) -> Self {
        let id = e.id();
        TraceMuxError::new(id, e.to_string())
    }
}

/// A loaded certificate + private key bundle in PEM form.
#[derive(Debug, Clone)]
pub struct CertBundle {
    /// PEM-encoded certificate chain (single self-signed leaf in v0.1).
    pub cert_pem: String,
    /// PEM-encoded PKCS#8 private key.
    pub key_pem: String,
}

impl CertBundle {
    /// Generate a fresh self-signed bundle covering `sans`.
    ///
    /// # Errors
    /// Returns [`TlsError::Generate`] when `rcgen` fails.
    pub fn generate(sans: &[&str]) -> Result<Self, TlsError> {
        let names: Vec<String> = sans.iter().map(|s| (*s).to_string()).collect();
        let CertifiedKey { cert, key_pair } =
            generate_simple_self_signed(names).map_err(|e| TlsError::Generate(e.to_string()))?;
        Ok(Self {
            cert_pem: cert.pem(),
            key_pem: key_pair.serialize_pem(),
        })
    }

    /// Load a bundle from disk.
    ///
    /// # Errors
    /// Returns [`TlsError::Io`] on read failures.
    pub fn load(cert_path: &Path, key_path: &Path) -> Result<Self, TlsError> {
        let cert_pem = std::fs::read_to_string(cert_path).map_err(|e| TlsError::Io {
            path: cert_path.to_path_buf(),
            source: e,
        })?;
        let key_pem = std::fs::read_to_string(key_path).map_err(|e| TlsError::Io {
            path: key_path.to_path_buf(),
            source: e,
        })?;
        Ok(Self { cert_pem, key_pem })
    }

    /// Persist the bundle to disk. Creates `dir` if missing.
    ///
    /// # Errors
    /// Returns [`TlsError::Io`] on write failures.
    pub fn save(&self, dir: &Path) -> Result<(), TlsError> {
        std::fs::create_dir_all(dir).map_err(|e| TlsError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let cert_path = dir.join(files::CERT);
        let key_path = dir.join(files::KEY);
        std::fs::write(&cert_path, &self.cert_pem).map_err(|e| TlsError::Io {
            path: cert_path,
            source: e,
        })?;
        std::fs::write(&key_path, &self.key_pem).map_err(|e| TlsError::Io {
            path: key_path,
            source: e,
        })?;
        Ok(())
    }

    /// Parse cert / key into [`rustls`] types.
    ///
    /// # Errors
    /// Returns [`TlsError::Pem`] when either PEM is malformed.
    pub fn parse(
        &self,
    ) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TlsError> {
        let certs: Vec<CertificateDer<'static>> =
            CertificateDer::pem_slice_iter(self.cert_pem.as_bytes())
                .collect::<Result<_, _>>()
                .map_err(|e| TlsError::Pem(format!("certs: {e}")))?;
        if certs.is_empty() {
            return Err(TlsError::Pem("no certificates in PEM".into()));
        }

        let key = PrivateKeyDer::from_pem_slice(self.key_pem.as_bytes())
            .map_err(|e| TlsError::Pem(format!("key: {e}")))?;

        Ok((certs, key))
    }
}

/// Load an existing bundle from `dir`, or generate one (covering
/// [`DEFAULT_SANS`]) and persist it.
///
/// # Errors
/// Propagates [`TlsError::Generate`], [`TlsError::Io`], or
/// [`TlsError::Pem`].
pub fn load_or_generate(dir: &Path) -> Result<CertBundle, TlsError> {
    let cert_path = dir.join(files::CERT);
    let key_path = dir.join(files::KEY);
    if cert_path.exists() && key_path.exists() {
        CertBundle::load(&cert_path, &key_path)
    } else {
        let b = CertBundle::generate(DEFAULT_SANS)?;
        b.save(dir)?;
        Ok(b)
    }
}

/// Build a [`rustls::ServerConfig`] from a [`CertBundle`].
///
/// Uses the `rustls` default crypto provider; no client auth in v0.1.
///
/// # Errors
/// Returns [`TlsError::Rustls`] if the bundle cannot be installed.
pub fn build_server_config(bundle: &CertBundle) -> Result<Arc<ServerConfig>, TlsError> {
    let (certs, key) = bundle.parse()?;
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| TlsError::Rustls(e.to_string()))?;
    Ok(Arc::new(cfg))
}

/// Return the DER bytes of the leaf certificate, suitable for
/// fingerprinting (see [`crate::fingerprint`]).
///
/// # Errors
/// Returns [`TlsError::Pem`] when the bundle does not parse.
pub fn leaf_cert_der(bundle: &CertBundle) -> Result<Vec<u8>, TlsError> {
    let (certs, _) = bundle.parse()?;
    Ok(certs.into_iter().next().unwrap().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_round_trip_through_rustls() {
        let b = CertBundle::generate(DEFAULT_SANS).unwrap();
        assert!(b.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(b.key_pem.contains("PRIVATE KEY"));
        let (certs, _key) = b.parse().expect("parse");
        assert_eq!(certs.len(), 1);
        let _cfg = build_server_config(&b).expect("server config");
    }

    #[test]
    fn load_or_generate_persists_then_loads() {
        let tmp = tempdir();
        let a = load_or_generate(&tmp).expect("first");
        let b = load_or_generate(&tmp).expect("second");
        // Second call must read the same bytes back; not regenerate.
        assert_eq!(a.cert_pem, b.cert_pem);
        assert_eq!(a.key_pem, b.key_pem);
    }

    #[test]
    fn parse_rejects_garbage_pem() {
        let bad = CertBundle {
            cert_pem: "not a pem".into(),
            key_pem: "neither".into(),
        };
        assert!(bad.parse().is_err());
    }

    #[test]
    fn tracemux_error_carries_canonical_id() {
        let e: TraceMuxError = TlsError::Generate("x".into()).into();
        assert_eq!(e.id, ErrorId::E2102TlsHandshake);
    }

    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "tracemux-tls-test-{}",
            std::process::id() as u64 ^ rand_u64()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn rand_u64() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }
}
