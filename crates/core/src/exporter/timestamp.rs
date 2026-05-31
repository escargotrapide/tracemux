//! Timestamp formatting helpers for exporters.

use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

use crate::error_id::{ErrorId, TraceMuxError};
use crate::Result;

/// Parse a user-facing timezone label into a fixed UTC offset.
///
/// v0.1 intentionally avoids a timezone database dependency. Named zones
/// are limited to common aliases; offset forms such as `GMT+9`,
/// `GMT+09:00`, `UTC`, `+0900`, and `-05:30` are supported.
pub fn parse_timezone_offset(label: &str) -> Result<UtcOffset> {
    let raw = label.trim();
    if raw.is_empty() {
        return Err(parse_err("timezone label is empty"));
    }
    let lower = raw.to_ascii_lowercase();
    match lower.as_str() {
        "utc" | "z" | "gmt" => return Ok(UtcOffset::UTC),
        "jst" | "asia/tokyo" => return offset(9, 0),
        _ => {}
    }

    let offset_text = lower
        .strip_prefix("gmt")
        .or_else(|| lower.strip_prefix("utc"))
        .unwrap_or(lower.as_str());
    parse_numeric_offset(offset_text).ok_or_else(|| {
        parse_err(format!(
            "unsupported timezone `{label}`; use UTC, Asia/Tokyo, GMT+9, GMT+09:00, +0900, or -05:30"
        ))
    })
}

/// Convert an RFC3339 timestamp string to the requested fixed offset.
pub fn format_rfc3339_in_timezone(ts: &str, offset: Option<UtcOffset>) -> Result<String> {
    let Some(offset) = offset else {
        return Ok(ts.to_string());
    };
    let parsed = OffsetDateTime::parse(ts, &Rfc3339).map_err(|e| {
        TraceMuxError::new(ErrorId::E1001PipelineGeneric, "export timestamp parse").with_source(e)
    })?;
    parsed.to_offset(offset).format(&Rfc3339).map_err(|e| {
        TraceMuxError::new(ErrorId::E1001PipelineGeneric, "export timestamp format").with_source(e)
    })
}

fn parse_numeric_offset(value: &str) -> Option<UtcOffset> {
    let (sign, rest) = match value.as_bytes().first().copied() {
        Some(b'+') => (1, &value[1..]),
        Some(b'-') => (-1, &value[1..]),
        _ => return None,
    };
    if rest.is_empty() {
        return None;
    }

    let (hour_text, minute_text) = if let Some((h, m)) = rest.split_once(':') {
        (h, m)
    } else if rest.len() > 2 {
        rest.split_at(rest.len() - 2)
    } else {
        (rest, "0")
    };
    let hours = hour_text.parse::<i8>().ok()?;
    let minutes = minute_text.parse::<i8>().ok()?;
    if !(0..=23).contains(&hours) || !(0..=59).contains(&minutes) {
        return None;
    }
    offset(sign * hours, sign * minutes).ok()
}

fn offset(hours: i8, minutes: i8) -> Result<UtcOffset> {
    UtcOffset::from_hms(hours, minutes, 0).map_err(|e| {
        TraceMuxError::new(ErrorId::E1001PipelineGeneric, "invalid timezone offset").with_source(e)
    })
}

fn parse_err(message: impl Into<String>) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1001PipelineGeneric, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_timezone_labels() {
        // REQ: FR-EXP-001
        assert_eq!(parse_timezone_offset("UTC").unwrap(), UtcOffset::UTC);
        assert_eq!(
            parse_timezone_offset("Asia/Tokyo").unwrap(),
            offset(9, 0).unwrap()
        );
        assert_eq!(
            parse_timezone_offset("GMT+9").unwrap(),
            offset(9, 0).unwrap()
        );
        assert_eq!(
            parse_timezone_offset("GMT+09:30").unwrap(),
            offset(9, 30).unwrap()
        );
        assert_eq!(
            parse_timezone_offset("-0530").unwrap(),
            offset(-5, -30).unwrap()
        );
    }

    #[test]
    fn formats_rfc3339_in_offset() {
        let out = format_rfc3339_in_timezone(
            "2024-01-01T00:00:00Z",
            Some(parse_timezone_offset("GMT+9").unwrap()),
        )
        .unwrap();
        assert_eq!(out, "2024-01-01T09:00:00+09:00");
    }
}
