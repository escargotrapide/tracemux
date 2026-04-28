//! `wanlogger ai-verify` — reads `target/ai-verify.json` produced by
//! `just ai-verify` and prints a concise human-readable summary.
//!
//! The aggregate gate itself runs as a `just` recipe. This subcommand
//! is the read-side: useful for CI, AI agents and `wanlogger`
//! shell-out integrations that want a structured exit code without
//! re-parsing raw `cargo` output.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

const DEFAULT_REPORT_PATH: &str = "target/ai-verify.json";

#[derive(Debug, Deserialize)]
struct Report {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct Step {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    detail: Option<String>,
}

/// Execute the `ai-verify` subcommand against the default report path.
///
/// # Errors
/// Returns an error if the report file is missing, cannot be read, or
/// is not valid JSON.
pub async fn run() -> anyhow::Result<()> {
    let path = PathBuf::from(DEFAULT_REPORT_PATH);
    run_at(&path).await
}

/// Execute against an arbitrary report path. Exposed for tests.
///
/// # Errors
/// See [`run`].
pub async fn run_at(path: &Path) -> anyhow::Result<()> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let report: Report = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {} as JSON", path.display()))?;

    let total = report.steps.len();
    let failed: Vec<&Step> = report
        .steps
        .iter()
        .filter(|s| !is_pass(&s.status))
        .collect();

    tracing::info!(
        path = %path.display(),
        schema = %report.schema,
        summary = %report.summary,
        steps = total,
        failed = failed.len(),
        "ai-verify report",
    );

    for step in &report.steps {
        tracing::info!(
            name = %step.name,
            status = %step.status,
            duration_ms = ?step.duration_ms,
            detail = ?step.detail,
            "step",
        );
    }

    if failed.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{} of {} ai-verify step(s) failed (see {})",
            failed.len(),
            total,
            DEFAULT_REPORT_PATH,
        );
    }
}

fn is_pass(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "pass" | "passed" | "ok" | "success" | "skip" | "skipped" | ""
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    struct TempJson(PathBuf);
    impl TempJson {
        fn new(contents: &str) -> Self {
            let mut p = std::env::temp_dir();
            let pid = std::process::id();
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            p.push(format!("wanlogger-aiverify-{pid}-{nonce}.json"));
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(contents.as_bytes()).unwrap();
            Self(p)
        }
    }
    impl Drop for TempJson {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[tokio::test]
    async fn ok_when_all_steps_pass() {
        let t = TempJson::new(
            r#"{"schema":"v1","summary":"green","steps":[
                 {"name":"fmt","status":"pass"},
                 {"name":"clippy","status":"ok"}
               ]}"#,
        );
        run_at(&t.0).await.unwrap();
    }

    #[tokio::test]
    async fn errors_when_any_step_fails() {
        let t = TempJson::new(
            r#"{"schema":"v1","summary":"red","steps":[
                 {"name":"clippy","status":"fail"}
               ]}"#,
        );
        let err = run_at(&t.0).await.unwrap_err();
        assert!(err.to_string().contains("ai-verify"));
    }

    #[tokio::test]
    async fn missing_file_is_error() {
        let err = run_at(Path::new("definitely-not-here.json"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("read"));
    }
}
