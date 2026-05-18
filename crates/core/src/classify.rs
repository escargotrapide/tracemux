//! Log classification helpers.
//!
//! v0.1 classification is intentionally lightweight: a rule checks
//! whether decoded text contains a configured substring and, on match,
//! adds a tag to the record/index entry. The frozen `Decoder` and
//! log-format surfaces are unchanged; callers attach returned tags to
//! existing `Record::tags` or `IndexEntry::tags` fields.

use std::collections::BTreeSet;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::decoder::Decoder;
use crate::Result;

/// One substring-based classification rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassificationRule {
    /// Substring to search for in decoded log text.
    pub contains: String,
    /// Tag/log-type name to add when the substring matches.
    pub tag: String,
    /// Whether matching should be case-sensitive.
    #[serde(default)]
    pub case_sensitive: bool,
}

impl ClassificationRule {
    /// Construct a case-insensitive substring rule.
    #[must_use]
    pub fn contains(needle: impl Into<String>, tag: impl Into<String>) -> Self {
        Self {
            contains: needle.into(),
            tag: tag.into(),
            case_sensitive: false,
        }
    }

    /// Construct a substring rule with an explicit case-sensitivity flag.
    #[must_use]
    pub fn contains_with_case(
        needle: impl Into<String>,
        tag: impl Into<String>,
        case_sensitive: bool,
    ) -> Self {
        Self {
            contains: needle.into(),
            tag: tag.into(),
            case_sensitive,
        }
    }

    fn matches(&self, text: &str) -> bool {
        if self.contains.is_empty() || self.tag.is_empty() {
            return false;
        }
        if self.case_sensitive {
            text.contains(&self.contains)
        } else {
            text.to_lowercase().contains(&self.contains.to_lowercase())
        }
    }
}

/// Ordered set of log classification rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogClassifier {
    rules: Vec<ClassificationRule>,
}

impl LogClassifier {
    /// Construct an empty classifier.
    #[must_use]
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Construct from explicit rules.
    #[must_use]
    pub fn from_rules(rules: Vec<ClassificationRule>) -> Self {
        Self { rules }
    }

    /// Whether no rules are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Return matching tags in rule order, deduplicated.
    #[must_use]
    pub fn tags_for_text(&self, text: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for rule in &self.rules {
            if !rule.matches(text) || !seen.insert(rule.tag.clone()) {
                continue;
            }
            out.push(rule.tag.clone());
        }
        out
    }

    /// Borrow configured rules.
    #[must_use]
    pub fn rules(&self) -> &[ClassificationRule] {
        &self.rules
    }
}

/// Decoder decorator that adds classification tags to decoded records.
#[derive(Debug, Clone)]
pub struct ClassifyingDecoder<D> {
    inner: D,
    classifier: LogClassifier,
}

impl<D> ClassifyingDecoder<D> {
    /// Wrap an existing decoder with a classifier.
    #[must_use]
    pub fn new(inner: D, classifier: LogClassifier) -> Self {
        Self { inner, classifier }
    }

    /// Borrow the wrapped decoder.
    #[must_use]
    pub const fn inner(&self) -> &D {
        &self.inner
    }

    /// Borrow the classifier used by this wrapper.
    #[must_use]
    pub const fn classifier(&self) -> &LogClassifier {
        &self.classifier
    }
}

impl<D> Decoder for ClassifyingDecoder<D>
where
    D: Decoder,
{
    fn decode(&mut self, frame: Bytes) -> Result<Option<crate::decoder::Record>> {
        let Some(mut record) = self.inner.decode(frame)? else {
            return Ok(None);
        };
        if let Some(text) = record.text.as_deref() {
            let mut seen = record.tags.iter().cloned().collect::<BTreeSet<_>>();
            for tag in self.classifier.tags_for_text(text) {
                if seen.insert(tag.clone()) {
                    record.tags.push(tag);
                }
            }
        }
        Ok(Some(record))
    }

    fn kind(&self) -> &'static str {
        "classifying"
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;
    use crate::decoder::passthrough::PassthroughDecoder;

    #[test]
    fn matches_substrings_case_insensitively() {
        // REQ: FR-CLI-005
        let classifier =
            LogClassifier::from_rules(vec![ClassificationRule::contains("error", "fault")]);

        assert_eq!(classifier.tags_for_text("ERROR: motor stop"), vec!["fault"]);
    }

    #[test]
    fn honors_case_sensitive_rules_and_dedups_tags() {
        // REQ: FR-CLI-005
        let classifier = LogClassifier::from_rules(vec![
            ClassificationRule::contains_with_case("ERR", "fault", true),
            ClassificationRule::contains("err", "fault"),
            ClassificationRule::contains("WARN", "warn"),
        ]);

        assert_eq!(
            classifier.tags_for_text("err and warn"),
            vec!["fault", "warn"]
        );
    }

    #[test]
    fn ignores_empty_rules() {
        let classifier = LogClassifier::from_rules(vec![
            ClassificationRule::contains("", "empty"),
            ClassificationRule::contains("x", ""),
        ]);

        assert!(classifier.tags_for_text("x").is_empty());
    }

    #[test]
    fn classifying_decoder_adds_tags_to_records() {
        // REQ: FR-CLI-005
        let classifier =
            LogClassifier::from_rules(vec![ClassificationRule::contains("ERROR", "fault")]);
        let mut decoder = ClassifyingDecoder::new(PassthroughDecoder::new(), classifier);

        let record = decoder
            .decode(Bytes::from_static(b"error: overcurrent"))
            .unwrap()
            .unwrap();

        assert_eq!(record.tags, vec!["fault"]);
    }
}
