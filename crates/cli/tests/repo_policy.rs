//! Repository policy checks that are cheaper than full release gates.

#![allow(clippy::missing_panics_doc)]

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/cli -> crates
    p.pop(); // crates -> workspace root
    p
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(workspace_root().join(rel)).unwrap_or_else(|err| {
        panic!("read {rel}: {err}");
    })
}

// REQ: FR-AI-001
#[test]
fn ai_verify_scripts_emit_required_report_contract() {
    let ps = read("scripts/ai-verify-summary.ps1");
    let sh = read("scripts/ai-verify-summary.sh");
    for text in [&ps, &sh] {
        assert!(text.contains("wanlogger/ai-verify/v1"));
        for step in ["encoding-check", "fmt-check", "clippy", "test", "rtm"] {
            assert!(text.contains(step), "missing ai-verify step {step}");
        }
    }
}

// REQ: FR-AI-002
#[test]
fn release_gate_validates_ai_verify_contract() {
    let ps = read("scripts/release-gate.ps1");
    let sh = read("scripts/release-gate.sh");
    for text in [&ps, &sh] {
        assert!(text.contains("wanlogger/ai-verify/v1"));
        assert!(text.contains("summary"));
        for step in ["encoding-check", "fmt-check", "clippy", "test", "rtm"] {
            assert!(
                text.contains(step),
                "release gate missing required step {step}"
            );
        }
    }
}

// REQ: NFR-REL-001
#[test]
fn ci_matrix_keeps_tier1_and_musl_targets() {
    let ci = read(".github/workflows/ci.yml");
    for expected in [
        "windows-latest",
        "ubuntu-latest",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "cargo test --workspace --all-features",
    ] {
        assert!(ci.contains(expected), "CI missing {expected}");
    }
}

// REQ: NFR-SEC-001
#[test]
fn workspace_policy_denies_unsafe_and_openssl() {
    let cargo = read("Cargo.toml");
    let deny = read("deny.toml");
    assert!(cargo.contains("unsafe_code = \"deny\""));
    assert!(deny.contains("openssl-sys"));
    assert!(deny.contains("openssl"));
}

// REQ: FR-UI-006
// REQ: NFR-PORT-001
#[test]
fn tauri_shell_uses_the_single_wanlogger_sidecar_binary() {
    let cli_manifest = read("crates/cli/Cargo.toml");
    let tauri_conf = read("app-tauri/src-tauri/tauri.conf.json");
    let tauri_lib = read("app-tauri/src-tauri/src/lib.rs");

    assert!(cli_manifest.contains("[[bin]]"));
    assert!(cli_manifest.contains("name = \"wanlogger\""));
    assert!(tauri_conf.contains("binaries/wanlogger"));
    assert!(tauri_lib.contains("sidecar(\"binaries/wanlogger\")"));
    assert!(tauri_lib.contains("serve"));
}
