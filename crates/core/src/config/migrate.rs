//! Config schema migration (v1 to vN). **Critical path.**

use thiserror::Error;

use super::schema_v1::ConfigV1;

/// Latest supported config schema version.
pub const CURRENT_CONFIG_VERSION: u32 = 1;

/// Error returned while detecting or migrating a config file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigMigrationError {
    /// TOML parsing failed before migration could complete.
    #[error("parsing config: {0}")]
    Parse(#[from] toml::de::Error),
    /// The top-level `config_version` field is missing.
    #[error("missing config_version")]
    MissingVersion,
    /// The top-level `config_version` field is not a positive integer.
    #[error("config_version must be a positive integer")]
    InvalidVersion,
    /// The file declares a version this binary cannot read.
    #[error("unsupported config_version {found}; expected {CURRENT_CONFIG_VERSION}")]
    UnsupportedVersion {
        /// Version declared by the file.
        found: u32,
    },
}

/// Detect the top-level schema version before deserializing a concrete schema.
///
/// Future migrations should keep this function strict so that unknown schemas
/// fail before any lossy deserialization happens.
pub fn detect_config_version(body: &str) -> Result<u32, ConfigMigrationError> {
    let value: toml::Value = toml::from_str(body)?;
    let Some(version) = value.get("config_version") else {
        return Err(ConfigMigrationError::MissingVersion);
    };
    let Some(version) = version.as_integer() else {
        return Err(ConfigMigrationError::InvalidVersion);
    };
    if version <= 0 || version > i64::from(u32::MAX) {
        return Err(ConfigMigrationError::InvalidVersion);
    }
    Ok(version as u32)
}

/// Migrate a TOML config document to the latest in-memory schema.
///
/// v0.1 has only `ConfigV1`, so version 1 is an identity migration. This
/// dispatch point is intentionally present now to keep future version bumps
/// localized and testable.
pub fn migrate_config_to_latest(body: &str) -> Result<ConfigV1, ConfigMigrationError> {
    match detect_config_version(body)? {
        1 => Ok(toml::from_str(body)?),
        found => Err(ConfigMigrationError::UnsupportedVersion { found }),
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_config_version, migrate_config_to_latest, ConfigMigrationError};

    #[test]
    fn detects_version_before_schema_deserialization() {
        let body = r#"
			config_version = 1
			[server]
			bind = "127.0.0.1:9443"
		"#;

        assert_eq!(detect_config_version(body).unwrap(), 1);
    }

    #[test]
    fn migrates_v1_as_identity() {
        let body = r#"
			config_version = 1
			[server]
			bind = "127.0.0.1:9443"
			require_auth = false
			[channels.demo]
			label = "demo"
			[channels.demo.spec]
			kind = "mock"
			tag = "demo"
		"#;

        let config = migrate_config_to_latest(body).unwrap();
        assert_eq!(config.config_version, 1);
        assert_eq!(config.server.bind, "127.0.0.1:9443");
        assert!(config.channels.contains_key("demo"));
    }

    #[test]
    fn rejects_missing_version() {
        let err = detect_config_version("[server]\nbind = '127.0.0.1:9443'").unwrap_err();
        assert!(matches!(err, ConfigMigrationError::MissingVersion));
    }

    #[test]
    fn rejects_invalid_version_type() {
        let err = detect_config_version("config_version = '1'").unwrap_err();
        assert!(matches!(err, ConfigMigrationError::InvalidVersion));
    }

    #[test]
    fn rejects_unknown_future_version_without_schema_parse() {
        let body = r"
			config_version = 2
			[future]
			field = true
        ";

        let err = migrate_config_to_latest(body).unwrap_err();
        assert!(matches!(
            err,
            ConfigMigrationError::UnsupportedVersion { found: 2 }
        ));
    }
}
