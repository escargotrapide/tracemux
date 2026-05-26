//! Text encoding resolution shared by text-like exporters.

use std::path::Path;

use serde::Deserialize;

const DEFAULT_EXPORT_ENCODING: &str = "utf-8";

#[derive(Debug, Deserialize)]
struct SessionMeta {
    decoder: Option<String>,
    encoding: Option<String>,
}

// REQ: FR-EXP-001
pub(crate) fn resolve_text_encoding(src: &Path, requested: Option<&str>) -> String {
    requested
        .and_then(non_empty)
        .or_else(|| meta_text_encoding(src))
        .unwrap_or_else(|| DEFAULT_EXPORT_ENCODING.to_string())
        .to_ascii_lowercase()
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn meta_text_encoding(src: &Path) -> Option<String> {
    let text = std::fs::read_to_string(src.join("meta.toml")).ok()?;
    let meta: SessionMeta = toml::from_str(&text).ok()?;
    meta.encoding
        .as_deref()
        .and_then(non_empty)
        .or_else(|| decoder_text_encoding(meta.decoder.as_deref()))
}

fn decoder_text_encoding(decoder: Option<&str>) -> Option<String> {
    decoder?
        .trim()
        .strip_prefix("utf8-text:")
        .and_then(non_empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    // REQ: FR-EXP-001
    #[test]
    fn requested_encoding_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("meta.toml"),
            "decoder = \"utf8-text:shift_jis\"\n",
        )
        .unwrap();

        assert_eq!(resolve_text_encoding(dir.path(), Some(" CP932 ")), "cp932");
    }

    // REQ: FR-EXP-001
    #[test]
    fn decoder_label_supplies_encoding() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("meta.toml"),
            "decoder = \"utf8-text:shift_jis\"\n",
        )
        .unwrap();

        assert_eq!(resolve_text_encoding(dir.path(), None), "shift_jis");
    }
}
