//! Authentication: argon2id-hashed bearer tokens. **Critical path.**
//!
//! Frozen v0.1. See `docs/protocols/wire-protocol.md` § Auth and
//! [`FR-WIRE-002`].
//!
//! Connections present a bearer token via the WebSocket subprotocol
//! header (`bearer.<token>`). The server keeps an in-memory list of
//! argon2id PHC hashes and accepts the token iff
//! [`argon2::PasswordVerifier`] matches one of them in constant time.
//!
//! The CLI flag `--no-auth` is only honoured when the peer address is
//! loopback (`127.0.0.1` / `::1`). [`is_loopback_allowed`] gates that
//! decision.
//!
//! [`FR-WIRE-002`]: ../../../../docs/requirements.md

use std::net::{IpAddr, SocketAddr};

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use thiserror::Error;
use wanlogger_core::{ErrorId, WanloggerError};

/// Subprotocol prefix used by the bearer-token negotiation header.
///
/// The full header value is `bearer.<token>` and the server matches
/// any subprotocol entry that starts with this prefix.
pub const BEARER_PREFIX: &str = "bearer.";

/// Errors produced by the authentication layer.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The bearer token was rejected (no matching hash).
    #[error("E-2101: auth rejected")]
    Rejected,
    /// A stored hash failed to parse.
    #[error("E-2101: stored hash invalid: {0}")]
    InvalidHash(String),
    /// Hashing a fresh token failed.
    #[error("E-2101: hashing failed: {0}")]
    HashFailed(String),
}

impl AuthError {
    /// Stable [`ErrorId`].
    #[must_use]
    pub const fn id(&self) -> ErrorId {
        ErrorId::E2101AuthRejected
    }
}

impl From<AuthError> for WanloggerError {
    fn from(e: AuthError) -> Self {
        let id = e.id();
        WanloggerError::new(id, e.to_string())
    }
}

/// Verifier that holds a small set of argon2id PHC hashes and tries
/// each one in turn.
///
/// Tokens are kept as PHC strings (`$argon2id$v=19$...`) rather than
/// raw secrets, so the server process can dump core or be restarted
/// without leaking plaintext credentials.
#[derive(Debug, Default, Clone)]
pub struct BearerVerifier {
    hashes: Vec<String>,
}

impl BearerVerifier {
    /// Empty verifier. Every token will be rejected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pre-computed argon2id PHC string.
    ///
    /// # Errors
    /// Returns [`AuthError::InvalidHash`] if `phc` is not a valid PHC
    /// string parsable by [`argon2`].
    pub fn add_phc(&mut self, phc: impl Into<String>) -> Result<(), AuthError> {
        let s = phc.into();
        // Validate at insertion so verification never fails on shape.
        let _ = PasswordHash::new(&s).map_err(|e| AuthError::InvalidHash(e.to_string()))?;
        self.hashes.push(s);
        Ok(())
    }

    /// Number of stored hashes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    /// Whether the verifier holds no hashes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Verify a presented bearer token.
    ///
    /// Iterates the stored hashes; the first match wins.
    ///
    /// # Errors
    /// Returns [`AuthError::Rejected`] when no hash matches.
    /// Returns [`AuthError::InvalidHash`] when a stored hash cannot
    /// be parsed (should not happen because [`Self::add_phc`]
    /// validates at insertion).
    pub fn verify(&self, token: &str) -> Result<(), AuthError> {
        let argon2 = Argon2::default();
        for h in &self.hashes {
            let parsed = PasswordHash::new(h).map_err(|e| AuthError::InvalidHash(e.to_string()))?;
            if argon2.verify_password(token.as_bytes(), &parsed).is_ok() {
                return Ok(());
            }
        }
        Err(AuthError::Rejected)
    }
}

/// Hash a bearer token with argon2id default parameters.
///
/// Used by tooling that provisions tokens (e.g. CLI `wanlogger auth
/// add`). Not used on the hot path.
///
/// # Errors
/// Returns [`AuthError::HashFailed`] if the underlying argon2
/// implementation reports an error.
pub fn hash_token(token: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let phc = argon2
        .hash_password(token.as_bytes(), &salt)
        .map_err(|e| AuthError::HashFailed(e.to_string()))?;
    Ok(phc.to_string())
}

/// Returns `true` when `peer` is on the loopback interface and is
/// therefore eligible for the `--no-auth` shortcut.
///
/// REQ: FR-WIRE-002 -- `--no-auth` is rejected unless the peer is
/// `127.0.0.1` or `::1`.
#[must_use]
pub fn is_loopback_allowed(peer: &SocketAddr) -> bool {
    match peer.ip() {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// Extract the bearer token from a `Sec-WebSocket-Protocol` value.
///
/// The header is a comma-separated list; this returns the first
/// entry that starts with [`BEARER_PREFIX`], stripped of the prefix.
#[must_use]
pub fn extract_bearer(header_value: &str) -> Option<&str> {
    header_value
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix(BEARER_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    // REQ: FR-WIRE-002 (argon2id verification)
    #[test]
    fn verify_accepts_correct_token() {
        let phc = hash_token("s3cr3t").unwrap();
        let mut v = BearerVerifier::new();
        v.add_phc(phc).unwrap();
        assert!(v.verify("s3cr3t").is_ok());
    }

    #[test]
    fn verify_rejects_wrong_token() {
        let phc = hash_token("s3cr3t").unwrap();
        let mut v = BearerVerifier::new();
        v.add_phc(phc).unwrap();
        let err = v.verify("nope").unwrap_err();
        assert!(matches!(err, AuthError::Rejected));
        assert_eq!(err.id(), ErrorId::E2101AuthRejected);
    }

    #[test]
    fn empty_verifier_rejects_everything() {
        let v = BearerVerifier::new();
        assert!(v.verify("anything").is_err());
        assert!(v.is_empty());
    }

    #[test]
    fn invalid_phc_is_caught_at_insertion() {
        let mut v = BearerVerifier::new();
        let err = v.add_phc("not-a-phc").unwrap_err();
        assert!(matches!(err, AuthError::InvalidHash(_)));
    }

    #[test]
    fn multiple_hashes_any_match_passes() {
        let a = hash_token("alpha").unwrap();
        let b = hash_token("beta").unwrap();
        let mut v = BearerVerifier::new();
        v.add_phc(a).unwrap();
        v.add_phc(b).unwrap();
        assert_eq!(v.len(), 2);
        assert!(v.verify("alpha").is_ok());
        assert!(v.verify("beta").is_ok());
        assert!(v.verify("gamma").is_err());
    }

    // REQ: FR-WIRE-002 (loopback gate)
    #[test]
    fn loopback_allowed() {
        let v4: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let v6: SocketAddr = "[::1]:1".parse().unwrap();
        assert!(is_loopback_allowed(&v4));
        assert!(is_loopback_allowed(&v6));
    }

    #[test]
    fn non_loopback_denied() {
        let lan: SocketAddr = "192.168.1.10:1".parse().unwrap();
        let pub6: SocketAddr = "[2001:db8::1]:1".parse().unwrap();
        assert!(!is_loopback_allowed(&lan));
        assert!(!is_loopback_allowed(&pub6));
    }

    #[test]
    fn extract_bearer_handles_multi_value_header() {
        let h = "wanlogger.v1, bearer.abc123";
        assert_eq!(extract_bearer(h), Some("abc123"));
    }

    #[test]
    fn extract_bearer_returns_none_when_absent() {
        assert_eq!(extract_bearer("wanlogger.v1"), None);
    }

    #[test]
    fn wanlogger_error_carries_canonical_id() {
        let e: WanloggerError = AuthError::Rejected.into();
        assert_eq!(e.id, ErrorId::E2101AuthRejected);
    }
}
