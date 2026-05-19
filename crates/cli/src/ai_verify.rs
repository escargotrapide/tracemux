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
const EXPECTED_SCHEMA: &str = "wanlogger/ai-verify/v1";
const REQUIRED_STEPS: &[&str] = &["encoding-check", "fmt-check", "clippy", "test", "rtm"];

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
#[allow(clippy::unused_async)]
pub async fn run() -> anyhow::Result<()> {
    let path = PathBuf::from(DEFAULT_REPORT_PATH);
    run_at(&path).await
}

/// Execute against an arbitrary report path. Exposed for tests.
///
/// # Errors
/// See [`run`].
#[allow(clippy::unused_async)]
pub async fn run_at(path: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let report: Report = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {} as JSON", path.display()))?;
    validate_report(&report, path)?;

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

fn validate_report(report: &Report, path: &Path) -> anyhow::Result<()> {
    if report.schema != EXPECTED_SCHEMA {
        anyhow::bail!(
            "ai-verify report {} has schema `{}`; expected `{}`",
            path.display(),
            report.schema,
            EXPECTED_SCHEMA,
        );
    }
    if report.steps.is_empty() {
        anyhow::bail!(
            "ai-verify report {} has no steps; run `just ai-verify` to refresh it",
            path.display(),
        );
    }
    if report.summary != "green" {
        anyhow::bail!(
            "ai-verify report {} summary is `{}`; expected `green`",
            path.display(),
            report.summary,
        );
    }
    for required in REQUIRED_STEPS {
        match report.steps.iter().find(|s| s.name == *required) {
            Some(step) if is_required_pass(&step.status) => {}
            Some(step) => anyhow::bail!(
                "required ai-verify step `{required}` is `{}`; expected pass/ok/success",
                step.status,
            ),
            None => anyhow::bail!("required ai-verify step `{required}` is missing"),
        }
    }
    Ok(())
}

fn is_pass(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "pass" | "passed" | "ok" | "success" | "skip" | "skipped"
    )
}

fn is_required_pass(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "pass" | "passed" | "ok" | "success"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_JSON_SEQ: AtomicU64 = AtomicU64::new(0);

    struct TempJson(PathBuf);
    impl TempJson {
        fn new(contents: &str) -> Self {
            let mut p = std::env::temp_dir();
            let pid = std::process::id();
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = TEMP_JSON_SEQ.fetch_add(1, Ordering::Relaxed);
            p.push(format!("wanlogger-aiverify-{pid}-{nonce}-{seq}.json"));
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
            r#"{"schema":"wanlogger/ai-verify/v1","summary":"green","steps":[
                                 {"name":"encoding-check","status":"pass"},
                                 {"name":"fmt-check","status":"pass"},
                                 {"name":"clippy","status":"ok"},
                                 {"name":"test","status":"success"},
                                 {"name":"rtm","status":"passed"}
               ]}"#,
        );
        run_at(&t.0).await.unwrap();
    }

    #[tokio::test]
    async fn errors_when_any_step_fails() {
        let t = TempJson::new(
            r#"{"schema":"wanlogger/ai-verify/v1","summary":"1 failed","steps":[
                 {"name":"encoding-check","status":"pass"},
                 {"name":"fmt-check","status":"pass"},
                 {"name":"clippy","status":"fail"},
                 {"name":"test","status":"pass"},
                 {"name":"rtm","status":"pass"}
               ]}"#,
        );
        let err = run_at(&t.0).await.unwrap_err();
        assert!(err.to_string().contains("ai-verify"));
    }

    #[tokio::test]
    async fn empty_steps_are_rejected() {
        let t =
            TempJson::new(r#"{"schema":"wanlogger/ai-verify/v1","summary":"green","steps":[]}"#);
        let err = run_at(&t.0).await.unwrap_err();
        assert!(err.to_string().contains("no steps"));
    }

    #[tokio::test]
    async fn missing_required_step_is_rejected() {
        let t = TempJson::new(
            r#"{"schema":"wanlogger/ai-verify/v1","summary":"green","steps":[
                 {"name":"encoding-check","status":"pass"},
                 {"name":"fmt-check","status":"pass"},
                 {"name":"clippy","status":"pass"},
                 {"name":"test","status":"pass"}
               ]}"#,
        );
        let err = run_at(&t.0).await.unwrap_err();
        assert!(err.to_string().contains("rtm"));
    }

    #[tokio::test]
    async fn skipped_required_step_is_rejected() {
        let t = TempJson::new(
            r#"{"schema":"wanlogger/ai-verify/v1","summary":"green","steps":[
                 {"name":"encoding-check","status":"pass"},
                 {"name":"fmt-check","status":"pass"},
                 {"name":"clippy","status":"skipped"},
                 {"name":"test","status":"pass"},
                 {"name":"rtm","status":"pass"}
               ]}"#,
        );
        let err = run_at(&t.0).await.unwrap_err();
        assert!(err.to_string().contains("clippy"));
    }

    #[tokio::test]
    async fn missing_file_is_error() {
        let err = run_at(Path::new("definitely-not-here.json"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("read"));
    }
}
