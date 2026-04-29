//! `wanlogger extcap` ? Wireshark extcap protocol.
//!
//! Implements interface discovery (`--extcap-interfaces`,
//! `--extcap-dlts`, `--extcap-config`) and live capture
//! (`--capture --fifo PATH --spec URI`). In capture mode the CLI
//! opens a [`wanlogger_core::source::Source`] from `--spec`, writes
//! a libpcap global header (link-type 147 / USER0) to the FIFO, then
//! emits one pcap record per received frame.
//!
//! REQ: FR-CLI-002
//!
//! See <https://www.wireshark.org/docs/man-pages/extcap.html> and
//! <https://wiki.wireshark.org/Development/LibpcapFileFormat>.

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use wanlogger_core::source::Frame;

use super::spec;

/// libpcap link-type DLT_USER0 ? placeholder until a wanlogger-
/// specific DLT is registered.
pub const DLT_USER0: u32 = 147;

/// libpcap snapshot length advertised in the global header.
pub const SNAPLEN: u32 = 65_535;

/// Subcommand mode.
#[derive(Debug, Clone)]
pub enum Mode {
    /// `--extcap-interfaces`
    Interfaces,
    /// `--extcap-dlts --extcap-interface NAME`
    Dlts {
        /// Interface name.
        interface: String,
    },
    /// `--extcap-config --extcap-interface NAME`
    Config {
        /// Interface name.
        interface: String,
    },
    /// `--capture --extcap-interface NAME --fifo PATH --spec URI`
    Capture {
        /// Interface name.
        interface: String,
        /// FIFO path (named pipe on Windows, FIFO on Unix).
        fifo: String,
        /// Channel spec URI (see [`crate::cmd::spec`]).
        spec: String,
    },
}

/// Run the `extcap` subcommand.
///
/// # Errors
/// Returns an error if discovery sub-mode receives an unknown
/// interface, or if capture cannot open the spec / FIFO.
pub async fn run(mode: Mode) -> Result<()> {
    match mode {
        Mode::Interfaces => {
            println!("extcap {{version=0.1.0}}{{help=https://example.invalid/wanlogger}}");
            println!("interface {{value=wanlogger}}{{display=wanlogger universal logger}}");
        }
        Mode::Dlts { interface } => {
            if interface != "wanlogger" {
                bail!("unknown interface: {interface}");
            }
            println!("dlt {{number={DLT_USER0}}}{{name=USER0}}{{display=wanlogger raw}}");
        }
        Mode::Config { interface } => {
            if interface != "wanlogger" {
                bail!("unknown interface: {interface}");
            }
            println!(
                "arg {{number=0}}{{call=--spec}}{{display=Channel spec}}{{type=string}}{{required=true}}"
            );
        }
        Mode::Capture {
            interface,
            fifo,
            spec: spec_str,
        } => {
            if interface != "wanlogger" {
                bail!("unknown interface: {interface}");
            }
            run_capture(&fifo, &spec_str).await?;
        }
    }
    Ok(())
}

async fn run_capture(fifo: &str, spec_str: &str) -> Result<()> {
    let s = spec::parse(spec_str).context("parsing channel spec")?;
    let mut source = spec::open(&s).context("opening source")?;
    source.open().await.context("Source::open failed")?;

    let mut sink = OpenOptions::new()
        .write(true)
        .create(false)
        .open(fifo)
        .with_context(|| format!("opening fifo {fifo}"))?;
    sink.write_all(&pcap_global_header(DLT_USER0, SNAPLEN))
        .context("write pcap global header")?;
    sink.flush().ok();

    loop {
        let Some(frame) = source.recv().await? else {
            break;
        };
        let payload = frame_payload(&frame);
        if payload.is_empty() {
            continue;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let rec = pcap_record(
            u32::try_from(now.as_secs()).unwrap_or(u32::MAX),
            now.subsec_micros(),
            &payload,
            SNAPLEN,
        );
        if sink.write_all(&rec).is_err() {
            tracing::info!("extcap: fifo closed by peer");
            break;
        }
        sink.flush().ok();
    }
    source.close().await.ok();
    Ok(())
}

fn frame_payload(f: &Frame) -> Vec<u8> {
    match f {
        Frame::Bytes(b) => b.to_vec(),
        Frame::Datagram { data, .. }
        | Frame::Other { data, .. }
        | Frame::Ssh { data, .. }
        | Frame::Visa { data, .. } => data.to_vec(),
        _ => Vec::new(),
    }
}

/// Build a libpcap classic global header (little-endian, microsecond
/// resolution).
#[must_use]
pub fn pcap_global_header(linktype: u32, snaplen: u32) -> [u8; 24] {
    let mut h = [0u8; 24];
    h[0..4].copy_from_slice(&0xa1b2_c3d4u32.to_le_bytes()); // magic
    h[4..6].copy_from_slice(&2u16.to_le_bytes()); // major
    h[6..8].copy_from_slice(&4u16.to_le_bytes()); // minor
    h[8..12].copy_from_slice(&0u32.to_le_bytes()); // thiszone
    h[12..16].copy_from_slice(&0u32.to_le_bytes()); // sigfigs
    h[16..20].copy_from_slice(&snaplen.to_le_bytes());
    h[20..24].copy_from_slice(&linktype.to_le_bytes());
    h
}

/// Build a single libpcap record (header + payload).
#[must_use]
pub fn pcap_record(ts_sec: u32, ts_usec: u32, data: &[u8], snaplen: u32) -> Vec<u8> {
    let orig_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let incl_len = orig_len.min(snaplen);
    let truncated = &data[..incl_len as usize];
    let mut out = Vec::with_capacity(16 + truncated.len());
    out.extend_from_slice(&ts_sec.to_le_bytes());
    out.extend_from_slice(&ts_usec.to_le_bytes());
    out.extend_from_slice(&incl_len.to_le_bytes());
    out.extend_from_slice(&orig_len.to_le_bytes());
    out.extend_from_slice(truncated);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcap_header_is_well_formed() {
        // REQ: FR-CLI-002
        let h = pcap_global_header(DLT_USER0, SNAPLEN);
        assert_eq!(&h[0..4], &[0xd4, 0xc3, 0xb2, 0xa1]); // LE magic
        assert_eq!(u16::from_le_bytes([h[4], h[5]]), 2);
        assert_eq!(u16::from_le_bytes([h[6], h[7]]), 4);
        assert_eq!(u32::from_le_bytes([h[16], h[17], h[18], h[19]]), SNAPLEN);
        assert_eq!(u32::from_le_bytes([h[20], h[21], h[22], h[23]]), 147);
    }

    #[test]
    fn pcap_record_full_payload() {
        // REQ: FR-CLI-002
        let r = pcap_record(0x1122_3344, 0x5566, b"abc", SNAPLEN);
        assert_eq!(&r[0..4], &[0x44, 0x33, 0x22, 0x11]);
        assert_eq!(&r[4..8], &[0x66, 0x55, 0x00, 0x00]);
        assert_eq!(u32::from_le_bytes(r[8..12].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(r[12..16].try_into().unwrap()), 3);
        assert_eq!(&r[16..], b"abc");
    }

    #[test]
    fn pcap_record_truncates_to_snaplen() {
        // REQ: FR-CLI-002
        let big = vec![0xAB; 100];
        let r = pcap_record(1, 0, &big, 8);
        assert_eq!(u32::from_le_bytes(r[8..12].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(r[12..16].try_into().unwrap()), 100);
        assert_eq!(r.len(), 16 + 8);
        assert_eq!(&r[16..], &[0xAB; 8]);
    }

    #[test]
    fn frame_payload_extracts_bytes() {
        // REQ: FR-CLI-002
        let f = Frame::Bytes(bytes::Bytes::from_static(b"hi"));
        assert_eq!(frame_payload(&f), b"hi");
        let f = Frame::Datagram {
            src: None,
            data: bytes::Bytes::from_static(b"ho"),
        };
        assert_eq!(frame_payload(&f), b"ho");
    }
}
