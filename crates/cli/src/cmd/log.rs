//! `tracemux log` ? record a channel into a v0.1 session-dir.
//!
//! Layout (see `docs/protocols/log-format.md`):
//! ```text
//! {prefix}_{kind}_{iface}_{YYYYMMDD-HHMMSS}/
//!   meta.toml
//!   raw.bin
//!   index.jsonl
//! ```
//!
//! v0.1 writes the **plain** raw.bin path (the WAL/group-commit/zstd
//! pipeline lives in `tracemux-core::log::wal` / `group_commit` and
//! is a frozen critical path applied by the server when bound).

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracemux_core::classify::{ClassificationRule, LogClassifier};
use tracemux_core::codec::decode;
use tracemux_core::log::index::{Dir, IndexEntry, IndexWriter, Kind};
use tracemux_core::log::raw::RawWriter;
use tracemux_core::session_name::{
    render_session_name, SessionNameParts, DEFAULT_CLI_SESSION_NAME_PATTERN,
};
use tracemux_core::source::{ChannelSpec, ControlEvt, Frame};
use tracemux_core::time::{ClockQuality, ClockSource, DualTimestamp};
use uuid::Uuid;

use super::spec;

#[derive(Debug, Serialize)]
struct MetaToml {
    prefix: String,
    spec: ChannelSpec,
    sid: Uuid,
    started: String,
    decoder: String,
    encoding: String,
}

/// Options for the `log` subcommand.
#[derive(Debug, Clone)]
pub struct Options {
    /// Channel spec URI.
    pub spec: String,
    /// Output prefix.
    pub prefix: Option<String>,
    /// Session-dir name pattern.
    pub name_pattern: Option<String>,
    /// Encoding label used for classification matching.
    pub encoding: String,
    /// Classifier rules in `contains=tag` form.
    pub classify: Vec<String>,
    /// Regex classifier rules in `regex=tag` form.
    pub classify_regex: Vec<String>,
}

/// Run the `log` subcommand.
///
/// # Errors
/// Returns an `anyhow::Error` for spec / I/O / source failure.
pub async fn run(options: Options) -> Result<()> {
    let s = spec::parse(&options.spec).context("parsing channel spec")?;
    let prefix = options.prefix.as_deref().unwrap_or("tracemux");
    let classifier = classifier_from_specs(&options.classify, &options.classify_regex)?;
    let now = OffsetDateTime::now_utc();
    let stamp = format_session_stamp(now);
    let kind = spec::kind_tag(&s);
    let iface = spec::iface_tag(&s);
    let unix_ns = tracemux_core::time::unix_ns_now();
    let dir_name = render_session_name(
        options
            .name_pattern
            .as_deref()
            .unwrap_or(DEFAULT_CLI_SESSION_NAME_PATTERN),
        &SessionNameParts {
            prefix,
            kind,
            iface: &iface,
            timestamp: &stamp,
            unix_ns,
        },
    );
    let dir = PathBuf::from(&dir_name);
    std::fs::create_dir_all(&dir).context("creating session-dir")?;
    tracing::info!(dir = %dir.display(), "log: opened session-dir");

    let sid = Uuid::new_v4();
    let started = now.format(&Rfc3339).unwrap_or_else(|_| stamp.clone());
    let encoding = normalized_encoding(&options.encoding);
    let meta = MetaToml {
        prefix: prefix.to_string(),
        spec: s.clone(),
        sid,
        started,
        decoder: format!("utf8-text:{encoding}"),
        encoding: encoding.clone(),
    };
    std::fs::write(
        dir.join("meta.toml"),
        toml::to_string_pretty(&meta).context("serialising meta.toml")?,
    )
    .context("writing meta.toml")?;

    let mut raw = RawWriter::create(&dir).context("opening raw.bin")?;
    let mut index = IndexWriter::create(&dir).context("opening index.jsonl")?;

    let mut source = spec::open(&s).context("opening source")?;
    source.open().await.context("Source::open failed")?;

    let mut count = 0u64;
    loop {
        match source.recv().await? {
            Some(frame) => {
                let (data, kind) = frame_payload(&frame);
                let (off, len) = raw.append(&data).context("raw append")?;
                let ts = synth_dual_ts();
                let mut e = IndexEntry::from_envelope(&ts, sid, Dir::In, kind, off, len);
                e.source = Some(format!("{}:{}", spec::kind_tag(&s), spec::iface_tag(&s)));
                if !classifier.is_empty() {
                    let (text, _) = decode(&data, &encoding);
                    e.tags = classifier.tags_for_text(&text);
                }
                index.append(&e).context("index append")?;
                count += 1;
                if count % 256 == 0 {
                    raw.flush().ok();
                    index.flush().ok();
                }
            }
            None => {
                tracing::info!("log: source returned None");
                break;
            }
        }
        match source.recv_ctl().await? {
            Some(ControlEvt::Eof) => {
                tracing::info!("log: EOF");
                break;
            }
            Some(ControlEvt::Disconnected { reason }) => {
                tracing::warn!(?reason, "log: disconnected");
                break;
            }
            Some(ControlEvt::Error { id, message }) => {
                tracing::error!(code = id.code(), %message, "log: source error");
                break;
            }
            Some(other) => tracing::debug!(?other, "log: ctl"),
            None => {}
        }
    }

    raw.flush().ok();
    index.flush().ok();
    source.close().await.ok();
    tracing::info!(records = count, "log: session closed");
    Ok(())
}

fn format_session_stamp(now: OffsetDateTime) -> String {
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn normalized_encoding(encoding: &str) -> String {
    let encoding = encoding.trim();
    if encoding.is_empty() {
        "utf-8".to_string()
    } else {
        encoding.to_ascii_lowercase()
    }
}

pub(crate) fn classifier_from_specs(
    specs: &[String],
    regex_specs: &[String],
) -> Result<LogClassifier> {
    let mut rules = Vec::new();
    for spec in specs {
        let Some((contains, tag)) = spec.split_once('=') else {
            bail!("--classify must use CONTAINS=TAG syntax: {spec}");
        };
        let contains = contains.trim();
        let tag = tag.trim();
        if contains.is_empty() || tag.is_empty() {
            bail!("--classify requires non-empty CONTAINS and TAG: {spec}");
        }
        rules.push(ClassificationRule::contains(contains, tag));
    }
    for spec in regex_specs {
        let Some((regex, tag)) = spec.split_once('=') else {
            bail!("--classify-regex must use REGEX=TAG syntax: {spec}");
        };
        let regex = regex.trim();
        let tag = tag.trim();
        if regex.is_empty() || tag.is_empty() {
            bail!("--classify-regex requires non-empty REGEX and TAG: {spec}");
        }
        let rule = ClassificationRule::regex(regex, tag);
        if !rule.is_valid() {
            bail!("--classify-regex contains an invalid regular expression: {regex}");
        }
        rules.push(rule);
    }
    Ok(LogClassifier::from_rules(rules))
}

fn frame_payload(f: &Frame) -> (Vec<u8>, Kind) {
    match f {
        Frame::Bytes(b) => (b.to_vec(), Kind::Bytes),
        Frame::Datagram { data, .. } => (data.to_vec(), Kind::Datagram),
        Frame::Ssh { data, .. } | Frame::Visa { data, .. } | Frame::Other { data, .. } => {
            (data.to_vec(), Kind::Frame)
        }
        _ => (Vec::new(), Kind::Frame),
    }
}

/// Produce a CLI-side [`DualTimestamp`] envelope.
///
/// The CLI is not the canonical clock source; the server fills in
/// `ts_ingest`. For the standalone `log` subcommand, we approximate
/// both with `now` and mark the quality as `BestEffort`.
fn synth_dual_ts() -> DualTimestamp {
    let now_ns = tracemux_core::time::unix_ns_now();
    DualTimestamp {
        ts_origin_ns: now_ns,
        ts_ingest_ns: now_ns,
        mono_ns: 0,
        boot_id: Uuid::nil(),
        node_id: Uuid::nil(),
        clock_offset_ms: 0,
        clock_quality: ClockQuality::BestEffort,
        drift_ppm: 0.0,
        clock_source: ClockSource::System,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_specs_parse() {
        // REQ: FR-CLI-005
        let specs = vec!["ERROR=fault".to_string(), "WARN=warning".to_string()];
        let classifier = classifier_from_specs(&specs, &[]).unwrap();

        assert_eq!(
            classifier.tags_for_text("warn and error"),
            vec!["fault", "warning"]
        );
    }

    #[test]
    fn classifier_specs_reject_invalid_input() {
        assert!(classifier_from_specs(&["missing-equals".to_string()], &[]).is_err());
        assert!(classifier_from_specs(&["=tag".to_string()], &[]).is_err());
        assert!(classifier_from_specs(&["needle=".to_string()], &[]).is_err());
        assert!(classifier_from_specs(&[], &["[=bad".to_string()]).is_err());
    }

    #[test]
    fn classifier_specs_parse_regex() {
        let specs = vec!["E-[0-9]{4}=error-id".to_string()];
        let classifier = classifier_from_specs(&[], &specs).unwrap();

        assert_eq!(
            classifier.tags_for_text("failed with e-2001"),
            vec!["error-id"]
        );
    }
}
