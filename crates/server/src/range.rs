//! `/api/sessions/{sid}/range` — historical `raw.bin` streaming.
//!
//! v0.1 returns `501 Not Implemented` with a structured JSON error
//! body until the session-dir resolver is wired up. The route is
//! reserved so the wire surface stays stable.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

/// Structured error body for the range endpoint.
#[derive(Debug, Serialize)]
pub struct RangeError {
    /// Stable tracemux error id.
    pub error_id: &'static str,
    /// Human-readable message.
    pub message: String,
}

/// `GET /api/sessions/{sid}/range`.
pub async fn range_handler(Path(sid): Path<String>) -> (StatusCode, Json<RangeError>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(RangeError {
            error_id: "E-3201",
            message: format!(
                "range endpoint not implemented in v0.1; sid={sid} \
                 (will stream from session-dir/raw.bin once ingest is wired)"
            ),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_501_with_structured_body() {
        let (status, body) = range_handler(Path("abc".to_string())).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body.error_id, "E-3201");
        assert!(body.message.contains("abc"));
    }
}
