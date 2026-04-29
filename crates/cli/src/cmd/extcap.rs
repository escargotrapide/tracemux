//! `wanlogger extcap` ? Wireshark extcap protocol stub.
//!
//! Implements the minimum subset that lets Wireshark discover the
//! `wanlogger` interface. Capture is not implemented in v0.1 ?
//! `--capture` returns an error.
//!
//! See <https://www.wireshark.org/docs/man-pages/extcap.html>.

use anyhow::{bail, Result};

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
    /// `--capture --extcap-interface NAME --fifo PATH`
    Capture {
        /// Interface name.
        #[allow(dead_code)]
        interface: String,
        /// FIFO path.
        #[allow(dead_code)]
        fifo: String,
    },
}

/// Run the `extcap` subcommand.
///
/// # Errors
/// Returns an error in `Capture` mode (not yet implemented in v0.1).
pub fn run(mode: Mode) -> Result<()> {
    match mode {
        Mode::Interfaces => {
            println!("extcap {{version=0.1.0}}{{help=https://example.invalid/wanlogger}}");
            println!("interface {{value=wanlogger}}{{display=wanlogger universal logger}}");
        }
        Mode::Dlts { interface } => {
            if interface != "wanlogger" {
                bail!("unknown interface: {interface}");
            }
            // DLT 147 is USER0 in libpcap ? fine as a placeholder.
            println!("dlt {{number=147}}{{name=USER0}}{{display=wanlogger raw}}");
        }
        Mode::Config { interface } => {
            if interface != "wanlogger" {
                bail!("unknown interface: {interface}");
            }
            println!(
                "arg {{number=0}}{{call=--spec}}{{display=Channel spec}}{{type=string}}{{required=true}}"
            );
        }
        Mode::Capture { .. } => {
            bail!("extcap capture is not implemented in v0.1");
        }
    }
    Ok(())
}
