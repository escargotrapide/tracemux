//! Safe session-dir name templating.
//!
//! This module keeps log-saving filename patterns out of the frozen
//! `Source` / `LogSink` trait surfaces. Callers provide a small set of
//! known tokens, and the rendered result is sanitised so it is safe as
//! a single directory name on Windows and Unix-like systems.

/// Default `wanlogger log` session-dir naming pattern.
pub const DEFAULT_CLI_SESSION_NAME_PATTERN: &str = "{prefix}_{kind}_{iface}_{timestamp}";

/// Default `wanlogger serve` session-dir naming pattern.
pub const DEFAULT_SERVER_SESSION_NAME_PATTERN: &str = "wanlogger_{kind}_{iface}_{unix_ns}";

/// Values available to a session-dir name pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionNameParts<'a> {
    /// User-visible prefix, typically `wanlogger` or a CLI `--prefix` value.
    pub prefix: &'a str,
    /// Source kind token such as `serial`, `tcp`, or `file`.
    pub kind: &'a str,
    /// Source interface token such as `COM7` or `127.0.0.1-5555`.
    pub iface: &'a str,
    /// Human-readable compact timestamp, e.g. `20260519-153045`.
    pub timestamp: &'a str,
    /// Nanoseconds since UNIX epoch as a collision-resistant token.
    pub unix_ns: i64,
}

/// Render and sanitise a session-dir name pattern.
///
/// Recognised placeholders are `{prefix}`, `{kind}`, `{iface}`,
/// `{timestamp}`, and `{unix_ns}`. Unknown placeholders are preserved
/// as literal text before sanitisation, which makes the function
/// forwards-compatible with future tokens without creating path
/// separators.
#[must_use]
pub fn render_session_name(pattern: &str, parts: &SessionNameParts<'_>) -> String {
    let pattern = if pattern.trim().is_empty() {
        DEFAULT_CLI_SESSION_NAME_PATTERN
    } else {
        pattern.trim()
    };
    let unix_ns = parts.unix_ns.to_string();
    let rendered = pattern
        .replace("{prefix}", parts.prefix)
        .replace("{kind}", parts.kind)
        .replace("{iface}", parts.iface)
        .replace("{timestamp}", parts.timestamp)
        .replace("{unix_ns}", &unix_ns);
    sanitize_session_name(&rendered)
}

/// Sanitise an arbitrary string as one filesystem directory name.
#[must_use]
pub fn sanitize_session_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len().min(160));
    let mut previous_dash = false;
    for ch in input.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            ch
        } else {
            '-'
        };
        if mapped == '-' {
            if !previous_dash {
                out.push(mapped);
            }
            previous_dash = true;
        } else {
            out.push(mapped);
            previous_dash = false;
        }
    }
    let mut trimmed = out.trim_matches(|ch| matches!(ch, '-' | '.')).to_string();
    if trimmed.len() > 160 {
        trimmed.truncate(160);
        trimmed = trimmed
            .trim_matches(|ch| matches!(ch, '-' | '.'))
            .to_string();
    }
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> SessionNameParts<'static> {
        SessionNameParts {
            prefix: "capture",
            kind: "serial",
            iface: "COM7",
            timestamp: "20260519-153045",
            unix_ns: 1_768_999_111_222_333_444,
        }
    }

    #[test]
    fn renders_default_cli_pattern() {
        // REQ: FR-CLI-007
        assert_eq!(
            render_session_name(DEFAULT_CLI_SESSION_NAME_PATTERN, &parts()),
            "capture_serial_COM7_20260519-153045"
        );
    }

    #[test]
    fn renders_server_unix_ns_pattern() {
        // REQ: FR-CLI-007
        assert_eq!(
            render_session_name(DEFAULT_SERVER_SESSION_NAME_PATTERN, &parts()),
            "wanlogger_serial_COM7_1768999111222333444"
        );
    }

    #[test]
    fn sanitises_path_separators_and_unicode() {
        assert_eq!(
            render_session_name("../{prefix}/{kind}:{iface}:\u{3042}", &parts()),
            "capture-serial-COM7"
        );
    }

    #[test]
    fn empty_pattern_falls_back_to_safe_name() {
        assert_eq!(render_session_name("///", &parts()), "session");
    }
}
