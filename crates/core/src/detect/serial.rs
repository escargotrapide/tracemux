//! Serial-port enumeration probe.
//!
//! Returns a list of likely serial-port device paths / names. When the
//! `serial` feature is enabled, this uses the same `serialport` backend
//! as the source implementation so Windows COM ports are discoverable.
//! Unix builds also scan `/dev` for common tty patterns as a fallback.

/// Enumerate likely serial-port endpoints.
#[must_use]
pub fn list() -> Vec<String> {
    sorted_unique(platform_candidates())
}

#[cfg(any(feature = "serial", unix))]
fn platform_candidates() -> Vec<String> {
    let mut out = Vec::new();
    #[cfg(feature = "serial")]
    {
        if let Ok(ports) = serialport::available_ports() {
            out.extend(ports.into_iter().map(|port| port.port_name));
        }
    }
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
    out
}

#[cfg(not(any(feature = "serial", unix)))]
fn platform_candidates() -> Vec<String> {
    Vec::new()
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
    fn list_returns_vec() {
        let _ = list();
    }

    #[test]
    fn sorted_unique_orders_and_dedups() {
        assert_eq!(
            sorted_unique(vec!["COM7".into(), "COM3".into(), "COM7".into()]),
            vec!["COM3", "COM7"]
        );
    }
}
