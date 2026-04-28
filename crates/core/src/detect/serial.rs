//! Serial-port enumeration probe.
//!
//! Returns a list of likely serial-port device paths / names. Linux
//! and macOS scan `/dev` for known patterns; Windows enumeration
//! requires the `serialport` crate (test-only in v0.1) or a registry
//! probe and is left as an empty list — the CLI's `wanlogger detect`
//! falls back to that.

/// Enumerate likely serial-port endpoints.
#[must_use]
pub fn list() -> Vec<String> {
    #[cfg(unix)]
    {
        let mut out = Vec::new();
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
        out.sort();
        out
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_returns_vec() {
        let _ = list();
    }
}
