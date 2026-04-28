//! `wanlogger detect` ? list available transports.
//!
//! v0.1 lists statically known transport kinds and probes the local
//! filesystem for serial-port nodes (Linux/macOS). Full enumeration
//! lives in `wanlogger_core::detect` and is feature-gated.

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct DetectReport<'a> {
    kinds: &'a [&'a str],
    serial_candidates: Vec<String>,
}

/// Run the `detect` subcommand.
///
/// # Errors
/// Currently never fails (placeholder for transport probes that may
/// raise I/O errors in later versions).
pub fn run() -> Result<()> {
    let kinds: &[&str] = &[
        "file", "tcp", "udp", "serial", "process", "pipe", "mock",
    ];
    let serial_candidates = scan_serial_candidates();
    let report = DetectReport {
        kinds,
        serial_candidates,
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}

/// Best-effort scan for serial-style device nodes on Unix-likes.
/// Windows is left empty here; the full enumeration ships in
/// `wanlogger_core::detect::serial`.
fn scan_serial_candidates() -> Vec<String> {
    let mut out = Vec::new();
    #[cfg(unix)]
    {
        if let Ok(rd) = std::fs::read_dir("/dev") {
            for ent in rd.flatten() {
                if let Some(name) = ent.file_name().to_str() {
                    if name.starts_with("ttyUSB")
                        || name.starts_with("ttyACM")
                        || name.starts_with("ttyS")
                        || name.starts_with("cu.")
                        || name.starts_with("tty.")
                    {
                        out.push(format!("/dev/{name}"));
                    }
                }
            }
        }
    }
    out.sort();
    out
}
