//! Integration tests for `wanlogger log` classification.

#![allow(clippy::missing_panics_doc)]

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_wanlogger"))
}

fn must_succeed(c: &mut Command) {
    let out = c.output().expect("spawn wanlogger");
    assert!(
        out.status.success(),
        "command failed: status={} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}

// REQ: FR-CLI-005
#[test]
fn log_classify_writes_tags_to_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("in.log"), b"INFO boot\nERROR motor stop\n").unwrap();

    must_succeed(
        Command::new(bin())
            .current_dir(dir.path())
            .arg("log")
            .arg("file:///in.log")
            .arg("--prefix")
            .arg("capture")
            .arg("--classify")
            .arg("ERROR=fault"),
    );

    let mut sessions = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|ty| ty.is_dir()))
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("capture_file_in.log_")
        })
        .collect::<Vec<_>>();
    sessions.sort_by_key(|entry| entry.file_name());
    assert_eq!(sessions.len(), 1, "expected one capture session-dir");

    let index = std::fs::read_to_string(sessions[0].path().join("index.jsonl")).unwrap();
    let rows = index
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["tags"], serde_json::json!(["fault"]));
}
