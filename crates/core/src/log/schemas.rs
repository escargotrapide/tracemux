//! Schema registry.
//!
//! On-disk layout: one file per schema under `session-dir/schemas/`,
//! filename is `<id>.json`. The registry deduplicates writes (a given
//! id is written exactly once per session).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Filesystem-backed schema registry.
pub struct SchemaRegistry {
    dir: PathBuf,
    written: HashSet<String>,
}

impl SchemaRegistry {
    /// Create (or open) `session_dir/schemas/`.
    ///
    /// # Errors
    /// Returns `io::Error` if the directory cannot be created. Already
    /// present schema files are noted so re-registration is a no-op.
    pub fn create(session_dir: &Path) -> io::Result<Self> {
        let dir = session_dir.join("schemas");
        fs::create_dir_all(&dir)?;
        let mut written = HashSet::new();
        for ent in fs::read_dir(&dir)? {
            let ent = ent?;
            if let Some(name) = ent.path().file_name().and_then(|s| s.to_str()) {
                if let Some(id) = name.strip_suffix(".json") {
                    written.insert(id.to_string());
                }
            }
        }
        Ok(Self { dir, written })
    }

    /// Register `schema_json` under `id`. Idempotent: a second call
    /// with the same `id` is a no-op (does **not** overwrite).
    ///
    /// # Errors
    /// Returns `io::Error` if the id is invalid (path-traversal-ish)
    /// or the write fails.
    pub fn register(&mut self, id: &str, schema_json: &[u8]) -> io::Result<PathBuf> {
        if !is_safe_id(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "schema id contains forbidden characters",
            ));
        }
        let path = self.dir.join(format!("{id}.json"));
        if !self.written.contains(id) {
            fs::write(&path, schema_json)?;
            self.written.insert(id.to_string());
        }
        Ok(path)
    }

    /// True if `id` has already been registered this session.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.written.contains(id)
    }

    /// Path to the schemas directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn is_safe_id(id: &str) -> bool {
    if id.is_empty() || id == "." || id == ".." {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        && !id.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("wanlogger-schema-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn register_writes_once() {
        let dir = tempdir();
        let mut r = SchemaRegistry::create(&dir).unwrap();
        let path = r.register("nmea:gprmc", br#"{"type":"object"}"#).unwrap();
        assert!(path.exists());
        r.register("nmea:gprmc", b"OVERWRITTEN").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert_eq!(body, r#"{"type":"object"}"#);
        assert!(r.contains("nmea:gprmc"));
    }

    #[test]
    fn rejects_bad_id() {
        let dir = tempdir();
        let mut r = SchemaRegistry::create(&dir).unwrap();
        assert!(r.register("../etc/passwd", b"{}").is_err());
        assert!(r.register("a/b", b"{}").is_err());
        assert!(r.register("", b"{}").is_err());
    }

    #[test]
    fn reopen_remembers_existing() {
        let dir = tempdir();
        {
            let mut r = SchemaRegistry::create(&dir).unwrap();
            r.register("foo.v1", b"{}").unwrap();
        }
        let r = SchemaRegistry::create(&dir).unwrap();
        assert!(r.contains("foo.v1"));
    }
}
