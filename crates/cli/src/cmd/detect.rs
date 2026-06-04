//! `tracemux detect` -- list available transports.
//!
//! v0.1 lists statically known transport kinds and probes the host for
//! serial-port candidates. On Windows this uses the serial-port backend
//! so virtual COM pairs appear when their driver exposes them normally.

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
    let kinds: &[&str] = &["file", "tcp", "udp", "serial", "process", "pipe", "mock"];
    let serial_candidates = scan_serial_candidates();
    let report = DetectReport {
        kinds,
        serial_candidates,
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}

/// Best-effort scan for serial-style device nodes.
fn scan_serial_candidates() -> Vec<String> {
    let out: Vec<String> = serialport::available_ports()
        .map(|ports| ports.into_iter().map(|port| port.port_name).collect())
        .unwrap_or_default();
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
    sorted_unique(out)
}

fn sorted_unique(mut candidates: Vec<String>) -> Vec<String> {
    candidates.sort();
    candidates.dedup();
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_candidates_are_sorted_and_unique() {
        assert_eq!(
            sorted_unique(vec!["COM7".into(), "COM3".into(), "COM7".into()]),
            vec!["COM3", "COM7"]
        );
    }
}
