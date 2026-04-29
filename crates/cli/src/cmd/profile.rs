//! `wanlogger profile` ? manage saved channel profiles.
//!
//! Profiles are stored as TOML under a base directory (`profiles_dir`).
//! Each file is `<name>.toml` and contains a single `spec` table that
//! deserialises into [`wanlogger_core::source::ChannelSpec`].

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use wanlogger_core::source::ChannelSpec;

/// Subcommand action.
#[derive(Debug, Clone)]
pub enum Action {
    /// List all profile names.
    List,
    /// Print a single profile.
    Show {
        /// Profile name.
        name: String,
    },
    /// Save / overwrite a profile from a spec URI.
    Set {
        /// Profile name.
        name: String,
        /// Spec URI (see [`crate::cmd::spec`]).
        spec: String,
    },
    /// Delete a profile.
    Del {
        /// Profile name.
        name: String,
    },
}

/// On-disk profile shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// The channel spec.
    pub spec: ChannelSpec,
}

/// Run a profile action against `dir`.
///
/// # Errors
/// Returns `anyhow::Error` for I/O, parse, or unknown-name failure.
pub fn run(dir: &Path, action: Action) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    match action {
        Action::List => {
            for n in list(dir)? {
                println!("{n}");
            }
        }
        Action::Show { name } => {
            let p = read(dir, &name)?;
            print!("{}", toml::to_string_pretty(&p)?);
        }
        Action::Set { name, spec } => {
            let parsed = super::spec::parse(&spec)?;
            write(dir, &name, &Profile { spec: parsed })?;
            tracing::info!(name = %name, dir = %dir.display(), "profile saved");
        }
        Action::Del { name } => {
            del(dir, &name)?;
            tracing::info!(name = %name, "profile deleted");
        }
    }
    Ok(())
}

/// Default profiles directory, relative to OS conventions:
/// * Windows: `%APPDATA%\wanlogger\profiles`
/// * Unix:    `$XDG_CONFIG_HOME/wanlogger/profiles` or
///   `~/.config/wanlogger/profiles`.
#[must_use]
pub fn default_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join("wanlogger")
            .join("profiles")
    } else {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| PathBuf::from(h).join(".config"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("wanlogger").join("profiles")
    }
}

fn list(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for ent in std::fs::read_dir(dir)? {
        let ent = ent?;
        let p = ent.path();
        if p.extension().and_then(|s| s.to_str()) == Some("toml") {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                out.push(stem.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read(dir: &Path, name: &str) -> Result<Profile> {
    let path = file_for(dir, name)?;
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let p: Profile = toml::from_str(&body).context("parsing profile TOML")?;
    Ok(p)
}

fn write(dir: &Path, name: &str, p: &Profile) -> Result<()> {
    let path = file_for(dir, name)?;
    let body = toml::to_string_pretty(p).context("serialising profile")?;
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))
}

fn del(dir: &Path, name: &str) -> Result<()> {
    let path = file_for(dir, name)?;
    if !path.exists() {
        return Err(anyhow!("profile not found: {name}"));
    }
    std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))
}

fn file_for(dir: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.contains(|c: char| {
            !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    {
        return Err(anyhow!("invalid profile name: {name:?}"));
    }
    Ok(dir.join(format!("{name}.toml")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "wanlogger-cli-profile-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn set_show_list_del_round_trip() {
        let dir = tempdir();
        run(
            &dir,
            Action::Set {
                name: "lab1".to_string(),
                spec: "tcp://10.0.0.1:5555".to_string(),
            },
        )
        .unwrap();
        let names = list(&dir).unwrap();
        assert_eq!(names, vec!["lab1".to_string()]);
        let p = read(&dir, "lab1").unwrap();
        match p.spec {
            ChannelSpec::Tcp { addr } => assert_eq!(addr, "10.0.0.1:5555"),
            other => panic!("wrong: {other:?}"),
        }
        del(&dir, "lab1").unwrap();
        assert!(list(&dir).unwrap().is_empty());
    }

    #[test]
    fn invalid_name_rejected() {
        let dir = tempdir();
        let r = run(
            &dir,
            Action::Set {
                name: "../etc/passwd".to_string(),
                spec: "tcp://x:1".to_string(),
            },
        );
        assert!(r.is_err());
    }
}
