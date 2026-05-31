//! Content detection for raw log samples.
//!
//! This module deliberately stays above the frozen source/framer/decoder
//! traits. It inspects a bounded byte sample, proposes a text encoding,
//! and evaluates configured log-type classification patterns against the
//! decoded sample.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::classify::{ClassificationMatchKind, LogClassifier};
use crate::codec::decode;

/// Encodings considered by content detection and exposed by the UI.
pub const SUPPORTED_TEXT_ENCODINGS: &[&str] =
    &["utf-8", "shift_jis", "cp932", "euc-jp", "iso-2022-jp"];

/// Default maximum number of raw bytes sampled for content detection.
pub const DEFAULT_MAX_SAMPLE_BYTES: usize = 64 * 1024;

/// Default minimum confidence required before auto-applying an encoding.
pub const DEFAULT_MIN_ENCODING_CONFIDENCE: u8 = 80;

/// Default minimum confidence gap between first and second candidates.
pub const DEFAULT_MIN_ENCODING_DELTA: u8 = 8;

/// How content detection should influence source startup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetectionMode {
    /// Use configured defaults and configured pattern rules.
    #[default]
    Configured,
    /// Apply high-confidence detected values automatically.
    Auto,
    /// Report candidates but keep configured values active.
    Suggest,
    /// Disable content detection.
    Off,
}

impl DetectionMode {
    /// Stable wire/config token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Auto => "auto",
            Self::Suggest => "suggest",
            Self::Off => "off",
        }
    }

    /// Parse a user/wire token.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "configured" | "default" | "defaults" => Some(Self::Configured),
            "auto" | "automatic" => Some(Self::Auto),
            "suggest" | "suggestion" | "suggestions" => Some(Self::Suggest),
            "off" | "none" | "disabled" => Some(Self::Off),
            _ => None,
        }
    }
}

impl fmt::Display for DetectionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Settings used for one content detection pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDetectionSettings {
    /// Detection mode requested by the user or server default.
    pub mode: DetectionMode,
    /// Encoding configured before content detection runs.
    pub configured_encoding: String,
    /// Pattern rules used for log-type candidates.
    pub classifier: LogClassifier,
    /// Minimum confidence required before auto-applying an encoding.
    pub min_encoding_confidence: u8,
    /// Minimum confidence gap between first and second candidates.
    pub min_encoding_delta: u8,
}

impl Default for ContentDetectionSettings {
    fn default() -> Self {
        Self {
            mode: DetectionMode::Configured,
            configured_encoding: "utf-8".to_string(),
            classifier: LogClassifier::new(),
            min_encoding_confidence: DEFAULT_MIN_ENCODING_CONFIDENCE,
            min_encoding_delta: DEFAULT_MIN_ENCODING_DELTA,
        }
    }
}

/// Encoding candidate inferred from a sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodingCandidate {
    /// Encoding label.
    pub label: String,
    /// Confidence in the range `0..=100`.
    pub confidence: u8,
    /// Whether the decoder reported malformed input.
    pub had_errors: bool,
    /// Short evidence tokens that explain the score.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

/// Log-type candidate inferred from configured pattern rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogTypeCandidate {
    /// Tag/log-type name produced by the matching rule.
    pub tag: String,
    /// Matching rule kind.
    pub kind: ClassificationMatchKind,
    /// Pattern text that matched.
    pub pattern: String,
    /// Number of non-overlapping sample matches.
    pub count: usize,
    /// Confidence in the range `0..=100`.
    pub confidence: u8,
}

/// Result of one content detection pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentDetectionReport {
    /// Detection mode used for this pass.
    pub mode: DetectionMode,
    /// Number of bytes inspected.
    pub sample_bytes: usize,
    /// Encoding configured before detection.
    pub configured_encoding: String,
    /// Encoding applied to the live pipeline.
    pub effective_encoding: String,
    /// Encoding used to evaluate log-type candidates.
    pub sampled_encoding: String,
    /// Ordered encoding candidates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encoding_candidates: Vec<EncodingCandidate>,
    /// Ordered log-type candidates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log_type_candidates: Vec<LogTypeCandidate>,
}

/// Inspect `sample` and return conservative content-detection metadata.
// REQ: FR-CLI-011
#[must_use]
pub fn detect_content(
    sample: &[u8],
    settings: &ContentDetectionSettings,
) -> ContentDetectionReport {
    let configured_encoding = canonical_encoding_label(&settings.configured_encoding);
    let sample = &sample[..sample.len().min(DEFAULT_MAX_SAMPLE_BYTES)];

    if settings.mode == DetectionMode::Off {
        return ContentDetectionReport {
            mode: settings.mode,
            sample_bytes: sample.len(),
            configured_encoding: configured_encoding.clone(),
            effective_encoding: configured_encoding.clone(),
            sampled_encoding: configured_encoding,
            encoding_candidates: Vec::new(),
            log_type_candidates: Vec::new(),
        };
    }

    let encoding_candidates = detect_encodings(sample);
    let effective_encoding =
        effective_encoding(&configured_encoding, &encoding_candidates, settings);
    let sampled_encoding = sampled_encoding(&configured_encoding, &encoding_candidates, settings);
    let log_type_candidates = match settings.mode {
        DetectionMode::Auto | DetectionMode::Suggest => {
            detect_log_types(sample, &sampled_encoding, &settings.classifier)
        }
        DetectionMode::Configured | DetectionMode::Off => Vec::new(),
    };

    ContentDetectionReport {
        mode: settings.mode,
        sample_bytes: sample.len(),
        configured_encoding,
        effective_encoding,
        sampled_encoding,
        encoding_candidates,
        log_type_candidates,
    }
}

/// Return scored encoding candidates for `sample`.
#[must_use]
pub fn detect_encodings(sample: &[u8]) -> Vec<EncodingCandidate> {
    let mut candidates = SUPPORTED_TEXT_ENCODINGS
        .iter()
        .map(|label| score_encoding(sample, label))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .confidence
            .cmp(&left.confidence)
            .then_with(|| label_rank(&left.label).cmp(&label_rank(&right.label)))
    });
    candidates
}

/// Evaluate configured log-type rules against `sample` decoded as `encoding`.
#[must_use]
pub fn detect_log_types(
    sample: &[u8],
    encoding: &str,
    classifier: &LogClassifier,
) -> Vec<LogTypeCandidate> {
    if sample.is_empty() || classifier.is_empty() {
        return Vec::new();
    }
    let (text, had_errors) = decode(sample, encoding);
    if had_errors && text.is_empty() {
        return Vec::new();
    }
    classifier
        .compile()
        .matches_for_text(&text)
        .into_iter()
        .map(|matched| LogTypeCandidate {
            tag: matched.tag,
            kind: matched.kind,
            pattern: matched.pattern,
            count: matched.count,
            confidence: log_type_confidence(matched.count),
        })
        .collect()
}

fn effective_encoding(
    configured: &str,
    candidates: &[EncodingCandidate],
    settings: &ContentDetectionSettings,
) -> String {
    if settings.mode != DetectionMode::Auto {
        return configured.to_string();
    }
    let Some(best) = candidates.first() else {
        return configured.to_string();
    };
    let second = candidates
        .iter()
        .skip(1)
        .find(|candidate| encoding_family(&candidate.label) != encoding_family(&best.label))
        .map_or(0, |candidate| candidate.confidence);
    if best.confidence >= settings.min_encoding_confidence
        && best.confidence.saturating_sub(second) >= settings.min_encoding_delta
    {
        best.label.clone()
    } else {
        configured.to_string()
    }
}

fn sampled_encoding(
    configured: &str,
    candidates: &[EncodingCandidate],
    settings: &ContentDetectionSettings,
) -> String {
    if settings.mode == DetectionMode::Configured {
        return configured.to_string();
    }
    candidates
        .first()
        .filter(|candidate| candidate.confidence >= settings.min_encoding_confidence)
        .map_or_else(
            || configured.to_string(),
            |candidate| candidate.label.clone(),
        )
}

fn score_encoding(sample: &[u8], label: &str) -> EncodingCandidate {
    if sample.is_empty() {
        return EncodingCandidate {
            label: label.to_string(),
            confidence: 0,
            had_errors: false,
            evidence: vec!["empty-sample".to_string()],
        };
    }

    let (text, had_errors) = decode(sample, label);
    let mut evidence = Vec::new();
    let mut confidence = text_confidence(sample, &text, had_errors, &mut evidence);

    if label == "utf-8" && sample.starts_with(&[0xEF, 0xBB, 0xBF]) {
        confidence = confidence.max(100);
        evidence.push("utf8-bom".to_string());
    }
    if label == "iso-2022-jp" && looks_like_iso_2022_jp(sample) {
        confidence = confidence.max(95);
        evidence.push("iso-2022-jp-escape".to_string());
    }
    if label == "utf-8" && std::str::from_utf8(sample).is_ok() && !had_errors {
        confidence = confidence.saturating_add(4).min(100);
        evidence.push("valid-utf8".to_string());
    }
    if has_japanese_text(&text) {
        confidence = confidence.saturating_add(4).min(100);
        evidence.push("japanese-text".to_string());
    }

    EncodingCandidate {
        label: label.to_string(),
        confidence,
        had_errors,
        evidence,
    }
}

fn text_confidence(sample: &[u8], text: &str, had_errors: bool, evidence: &mut Vec<String>) -> u8 {
    let total_chars = text.chars().count().max(1);
    let replacement_chars = text.chars().filter(|ch| *ch == '\u{FFFD}').count();
    let control_chars = text
        .chars()
        .filter(|ch| ch.is_control() && !matches!(*ch, '\r' | '\n' | '\t'))
        .count();
    let has_nul_byte = sample.contains(&0);
    let printable_chars = total_chars.saturating_sub(replacement_chars + control_chars);
    let printable_ratio = (printable_chars * 100 / total_chars) as u8;

    let mut confidence: u8 = if had_errors { 28 } else { 68 };
    confidence = confidence.saturating_add(printable_ratio / 4).min(100);

    if had_errors {
        evidence.push("decode-errors".to_string());
    } else {
        evidence.push("decode-clean".to_string());
    }
    if replacement_chars > 0 {
        confidence = confidence.saturating_sub((replacement_chars.min(5) as u8) * 8);
        evidence.push("replacement-chars".to_string());
    }
    if control_chars > 0 {
        confidence = confidence.saturating_sub((control_chars.min(8) as u8) * 4);
        evidence.push("control-chars".to_string());
    }
    if has_nul_byte {
        confidence = confidence.min(25);
        evidence.push("nul-bytes".to_string());
    }
    confidence
}

fn log_type_confidence(count: usize) -> u8 {
    55u8.saturating_add((count.min(3) as u8) * 15).min(100)
}

fn canonical_encoding_label(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    if SUPPORTED_TEXT_ENCODINGS.contains(&normalized.as_str()) {
        normalized
    } else {
        "utf-8".to_string()
    }
}

fn label_rank(label: &str) -> usize {
    SUPPORTED_TEXT_ENCODINGS
        .iter()
        .position(|candidate| *candidate == label)
        .unwrap_or(usize::MAX)
}

fn encoding_family(label: &str) -> &str {
    match label {
        "shift_jis" | "cp932" => "shift-jis-family",
        other => other,
    }
}

fn looks_like_iso_2022_jp(sample: &[u8]) -> bool {
    sample.windows(3).any(|window| {
        matches!(
            window,
            [0x1B, b'$', b'B' | b'@'] | [0x1B, b'(', b'B' | b'J']
        )
    })
}

fn has_japanese_text(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xFF66..=0xFF9F
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::ClassificationRule;
    use crate::codec::encode_text;

    #[test]
    fn detection_mode_parses_aliases() {
        assert_eq!(DetectionMode::parse("auto"), Some(DetectionMode::Auto));
        assert_eq!(
            DetectionMode::parse("default"),
            Some(DetectionMode::Configured)
        );
        assert_eq!(DetectionMode::parse("disabled"), Some(DetectionMode::Off));
    }

    #[test]
    fn detect_encodings_prefers_utf8_bom() {
        let candidates = detect_encodings(b"\xEF\xBB\xBFhello\n");
        assert_eq!(candidates[0].label, "utf-8");
        assert_eq!(candidates[0].confidence, 100);
    }

    #[test]
    fn detect_content_auto_falls_back_on_ascii_ambiguity() {
        // REQ: FR-CLI-011
        let settings = ContentDetectionSettings {
            mode: DetectionMode::Auto,
            configured_encoding: "shift_jis".to_string(),
            ..ContentDetectionSettings::default()
        };
        let report = detect_content(b"plain ascii log\n", &settings);

        assert_eq!(report.effective_encoding, "shift_jis");
    }

    #[test]
    fn detect_content_auto_applies_shift_jis_when_confident() {
        // REQ: FR-CLI-011
        let (sample, had_errors) = encode_text("エラー: モータ停止\n", "shift_jis");
        assert!(!had_errors);
        let settings = ContentDetectionSettings {
            mode: DetectionMode::Auto,
            configured_encoding: "utf-8".to_string(),
            ..ContentDetectionSettings::default()
        };
        let report = detect_content(&sample, &settings);

        assert_eq!(report.effective_encoding, "shift_jis");
    }

    #[test]
    fn detect_log_types_uses_regex_rules() {
        // REQ: FR-CLI-011
        let classifier =
            LogClassifier::from_rules(vec![ClassificationRule::regex(r"E-[0-9]{4}", "error-id")]);
        let candidates = detect_log_types(b"E-1001 and E-2002", "utf-8", &classifier);

        assert_eq!(candidates[0].tag, "error-id");
        assert_eq!(candidates[0].kind, ClassificationMatchKind::Regex);
        assert_eq!(candidates[0].count, 2);
    }
}
