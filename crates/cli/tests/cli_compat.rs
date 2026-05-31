//! CLI compatibility snapshots.
//!
//! These tests pin deterministic v0.1 CLI output that downstream tools
//! consume directly.

#![allow(clippy::missing_panics_doc)]

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tracemux"))
}

fn fixture_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/cli -> crates
    p.pop(); // crates -> workspace root
    p.join("tests").join("compat").join("cli").join("v1")
}

fn normalize_lf(bytes: &[u8]) -> Vec<u8> {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .into_bytes()
}

fn check_stdout(name: &str, args: &[&str]) {
    let out = Command::new(bin())
        .args(args)
        .output()
        .expect("spawn tracemux");
    assert!(
        out.status.success(),
        "command failed: status={} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr was not empty: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let path = fixture_dir().join(name);
    let stdout = normalize_lf(&out.stdout);
    if std::env::var_os("TRACEMUX_CLI_BLESS").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).expect("create fixture dir");
        std::fs::write(&path, &stdout).expect("write fixture");
        eprintln!("cli-compat: wrote {}", path.display());
    }

    let expected = std::fs::read(&path).expect("read fixture");
    assert_eq!(
        expected,
        stdout,
        "fixture {name} drifted. If intentional, update the cli-output compat notes and re-run with TRACEMUX_CLI_BLESS=1."
    );
}

// REQ: FR-CLI-002
#[test]
fn extcap_interfaces_snapshot_is_stable() {
    check_stdout("extcap_interfaces.txt", &["extcap", "--extcap-interfaces"]);
}

// REQ: FR-CLI-002
#[test]
fn extcap_dlts_snapshot_is_stable() {
    check_stdout(
        "extcap_dlts.txt",
        &["extcap", "--extcap-dlts", "--extcap-interface", "tracemux"],
    );
}

// REQ: FR-CLI-002
#[test]
fn extcap_config_snapshot_is_stable() {
    check_stdout(
        "extcap_config.txt",
        &[
            "extcap",
            "--extcap-config",
            "--extcap-interface",
            "tracemux",
        ],
    );
}
