//! `/api/ai/*` endpoints.
//!
//! v0.1 ships read-only access to the local `target/ai-verify.json`
//! produced by `just ai-verify`. The endpoint exists so the Tauri /
//! web UI can render the same gate result that CI consumes, without
//! shelling out to cargo.
//!
//! Future endpoints (requirements / RTM / skills) live behind the
//! `add-ui-panel` skill.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::Value;

/// `GET /api/ai/verify` ? returns the contents of
/// `target/ai-verify.json` if present.
pub async fn verify() -> Result<Json<Value>, (StatusCode, String)> {
    match tokio::fs::read("target/ai-verify.json").await {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) => Ok(Json(v)),
            Err(e) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("ai-verify.json parse: {e}"),
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err((
            StatusCode::NOT_FOUND,
            "target/ai-verify.json not found; run `just ai-verify` first".to_string(),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("ai-verify.json read: {e}"),
        )),
    }
}

/// Helper for callers that want the raw response without the JSON
/// wrapper (e.g. tests that round-trip through axum).
pub async fn verify_raw() -> impl IntoResponse {
    match verify().await {
        Ok(j) => j.into_response(),
        Err((s, m)) => (s, m).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_404_when_missing() {
        // Run from a tmpdir so target/ai-verify.json is missing.
        let prev = std::env::current_dir().unwrap();
        let tmp = std::env::temp_dir().join(format!("wlg-ai-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_current_dir(&tmp).unwrap();
        let r = verify().await;
        std::env::set_current_dir(prev).unwrap();
        assert!(matches!(r, Err((StatusCode::NOT_FOUND, _))));
    }
}
