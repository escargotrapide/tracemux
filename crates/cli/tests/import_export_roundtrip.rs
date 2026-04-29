//! Integration: `wanlogger import text` -> `wanlogger export text` round-trip.
//!
//! Drives the compiled `wanlogger` binary via `Command` and asserts
//! that each line of the source text appears in the exported text.

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

// REQ: FR-CLI-001
// REQ: FR-IMP-001
// REQ: FR-EXP-001
#[test]
fn text_roundtrip_preserves_lines_in_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("in.txt");
    std::fs::write(&src, b"alpha\nbeta\ngamma\n").unwrap();

    let session = dir.path().join("session");
    must_succeed(
        Command::new(bin())
            .arg("import")
            .arg("text")
            .arg(&src)
            .arg(&session),
    );

    let out = dir.path().join("out.txt");
    must_succeed(
        Command::new(bin())
            .arg("export")
            .arg("text")
            .arg(&session)
            .arg(&out),
    );

    let body = std::fs::read_to_string(&out).expect("read export");
    let last_cols: Vec<&str> = body
        .lines()
        .map(|l| l.rsplit_once('\t').map_or(l, |(_, t)| t))
        .collect();
    assert_eq!(last_cols, vec!["alpha", "beta", "gamma"]);
}

// REQ: FR-IMP-001
#[test]
fn import_refuses_overwriting_nonempty_dst() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("in.txt");
    std::fs::write(&src, b"x\n").unwrap();

    let session = dir.path().join("session");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(session.join("dummy"), b"existing").unwrap();

    let out = Command::new(bin())
        .arg("import")
        .arg("text")
        .arg(&src)
        .arg(&session)
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("non-empty"),
        "stderr did not mention non-empty: {err}"
    );
}

// REQ: FR-EXP-001
#[test]
fn export_rejects_non_session_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bogus = dir.path().join("not-a-session");
    std::fs::create_dir_all(&bogus).unwrap();

    let out = Command::new(bin())
        .arg("export")
        .arg("text")
        .arg(&bogus)
        .arg(dir.path().join("out.txt"))
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("missing index.jsonl"),
        "stderr did not mention missing index.jsonl: {err}"
    );
}
