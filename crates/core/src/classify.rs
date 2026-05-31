//! Log classification helpers.
//!
//! v0.1 classification is intentionally lightweight: a rule checks
//! whether decoded text contains a configured substring or matches a
//! configured regular expression and, on match, adds a tag to the
//! record/index entry. The frozen `Decoder` and log-format surfaces are
//! unchanged; callers attach returned tags to existing `Record::tags` or
//! `IndexEntry::tags` fields.

use std::collections::BTreeSet;

use bytes::Bytes;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::decoder::Decoder;
use crate::Result;

/// Pattern kind used by one classification rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClassificationMatchKind {
    /// Plain substring matching.
    Contains,
    /// Rust regular expression matching.
    Regex,
}

/// One text-pattern classification rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassificationRule {
    /// Substring to search for in decoded log text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contains: String,
    /// Regular expression to search for in decoded log text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
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
            regex: None,
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
            regex: None,
            tag: tag.into(),
            case_sensitive,
        }
    }

    /// Construct a case-insensitive regular expression rule.
    #[must_use]
    pub fn regex(pattern: impl Into<String>, tag: impl Into<String>) -> Self {
        Self {
            contains: String::new(),
            regex: Some(pattern.into()),
            tag: tag.into(),
            case_sensitive: false,
        }
    }

    /// Construct a regular expression rule with an explicit
    /// case-sensitivity flag.
    #[must_use]
    pub fn regex_with_case(
        pattern: impl Into<String>,
        tag: impl Into<String>,
        case_sensitive: bool,
    ) -> Self {
        Self {
            contains: String::new(),
            regex: Some(pattern.into()),
            tag: tag.into(),
            case_sensitive,
        }
    }

    /// Pattern kind selected by this rule.
    #[must_use]
    pub fn match_kind(&self) -> ClassificationMatchKind {
        if self
            .regex
            .as_deref()
            .is_some_and(|pattern| !pattern.is_empty())
        {
            ClassificationMatchKind::Regex
        } else {
            ClassificationMatchKind::Contains
        }
    }

    /// Pattern text used by this rule.
    #[must_use]
    pub fn pattern(&self) -> &str {
        self.regex
            .as_deref()
            .filter(|pattern| !pattern.is_empty())
            .unwrap_or(&self.contains)
    }

    /// Whether this rule can be evaluated.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.match_kind() == ClassificationMatchKind::Contains || compile_regex(self).is_ok()
    }

    fn is_active(&self) -> bool {
        !self.pattern().is_empty() && !self.tag.is_empty()
    }
}

/// One classification match produced by a classifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationMatch {
    /// Matching rule kind.
    pub kind: ClassificationMatchKind,
    /// Pattern text that matched.
    pub pattern: String,
    /// Tag/log-type name added by the rule.
    pub tag: String,
    /// Number of non-overlapping matches in the text.
    pub count: usize,
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
        self.compile().tags_for_text(text)
    }

    /// Return matching rule details in rule order.
    #[must_use]
    pub fn matches_for_text(&self, text: &str) -> Vec<ClassificationMatch> {
        self.compile().matches_for_text(text)
    }

    /// Compile rules for repeated matching.
    #[must_use]
    pub fn compile(&self) -> CompiledLogClassifier {
        CompiledLogClassifier::new(self.clone())
    }

    /// Borrow configured rules.
    #[must_use]
    pub fn rules(&self) -> &[ClassificationRule] {
        &self.rules
    }
}

/// Classifier with regex rules compiled for repeated matching.
#[derive(Debug, Clone)]
pub struct CompiledLogClassifier {
    source: LogClassifier,
    rules: Vec<CompiledClassificationRule>,
}

impl CompiledLogClassifier {
    /// Compile all rules in `source`.
    #[must_use]
    pub fn new(source: LogClassifier) -> Self {
        let rules = source
            .rules
            .iter()
            .filter(|rule| rule.is_active())
            .map(CompiledClassificationRule::new)
            .collect();
        Self { source, rules }
    }

    /// Borrow the source rule set used to build this classifier.
    #[must_use]
    pub const fn source(&self) -> &LogClassifier {
        &self.source
    }

    /// Whether no active rules were compiled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Return matching tags in rule order, deduplicated.
    #[must_use]
    pub fn tags_for_text(&self, text: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for matched in self.matches_for_text(text) {
            if !seen.insert(matched.tag.clone()) {
                continue;
            }
            out.push(matched.tag);
        }
        out
    }

    /// Return matching rule details in rule order.
    #[must_use]
    pub fn matches_for_text(&self, text: &str) -> Vec<ClassificationMatch> {
        self.rules
            .iter()
            .filter_map(|rule| rule.matches(text))
            .collect()
    }
}

#[derive(Debug, Clone)]
struct CompiledClassificationRule {
    rule: ClassificationRule,
    matcher: RuleMatcher,
}

impl CompiledClassificationRule {
    fn new(rule: &ClassificationRule) -> Self {
        let matcher = match rule.match_kind() {
            ClassificationMatchKind::Contains => {
                let needle = if rule.case_sensitive {
                    rule.contains.clone()
                } else {
                    rule.contains.to_lowercase()
                };
                RuleMatcher::Contains { needle }
            }
            ClassificationMatchKind::Regex => {
                compile_regex(rule).map_or_else(|_| RuleMatcher::InvalidRegex, RuleMatcher::Regex)
            }
        };
        Self {
            rule: rule.clone(),
            matcher,
        }
    }

    fn matches(&self, text: &str) -> Option<ClassificationMatch> {
        let count = match &self.matcher {
            RuleMatcher::Contains { needle } => {
                let haystack = if self.rule.case_sensitive {
                    text.to_string()
                } else {
                    text.to_lowercase()
                };
                count_substrings(&haystack, needle)
            }
            RuleMatcher::Regex(regex) => regex.find_iter(text).count(),
            RuleMatcher::InvalidRegex => 0,
        };
        (count > 0).then(|| ClassificationMatch {
            kind: self.rule.match_kind(),
            pattern: self.rule.pattern().to_string(),
            tag: self.rule.tag.clone(),
            count,
        })
    }
}

#[derive(Debug, Clone)]
enum RuleMatcher {
    Contains { needle: String },
    Regex(Regex),
    InvalidRegex,
}

fn compile_regex(rule: &ClassificationRule) -> std::result::Result<Regex, regex::Error> {
    RegexBuilder::new(rule.pattern())
        .case_insensitive(!rule.case_sensitive)
        .build()
}

fn count_substrings(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.match_indices(needle).count()
}

/// Decoder decorator that adds classification tags to decoded records.
#[derive(Debug, Clone)]
pub struct ClassifyingDecoder<D> {
    inner: D,
    classifier: LogClassifier,
    compiled: CompiledLogClassifier,
}

impl<D> ClassifyingDecoder<D> {
    /// Wrap an existing decoder with a classifier.
    #[must_use]
    pub fn new(inner: D, classifier: LogClassifier) -> Self {
        let compiled = classifier.compile();
        Self {
            inner,
            classifier,
            compiled,
        }
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
            for tag in self.compiled.tags_for_text(text) {
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
    fn matches_regex_case_insensitively() {
        // REQ: FR-CLI-005
        let classifier =
            LogClassifier::from_rules(vec![ClassificationRule::regex(r"e-[0-9]{4}", "error-id")]);

        assert_eq!(
            classifier.tags_for_text("failed with E-2001"),
            vec!["error-id"]
        );
        assert_eq!(classifier.matches_for_text("E-1001 and e-2002")[0].count, 2);
    }

    #[test]
    fn invalid_regex_is_ignored() {
        let classifier = LogClassifier::from_rules(vec![ClassificationRule::regex("[", "bad")]);

        assert!(classifier.tags_for_text("[").is_empty());
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
