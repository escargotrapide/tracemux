//! Packet-capture interface discovery model.
//!
//! Native Npcap/libpcap enumeration is compiled only with the `pcap-capture`
//! feature. Default builds return an empty list so `/api/detect` can expose the
//! additive schema without requiring packet-capture drivers.

// REQ: FR-SRC-PCAP-DETECT
// REQ: NFR-SEC-PCAP

use serde::{Deserialize, Serialize};

/// Minimal packet-capture interface information safe for UI selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PcapInterfaceInfo {
    /// Backend device identifier used in `pcap://...` specs.
    pub device: String,
    /// Optional operator-friendly name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub display_name: Option<String>,
    /// Optional backend/interface description.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    /// Optional interface addresses. Kept empty until authenticated discovery is designed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addresses: Vec<String>,
    /// Optional backend flags such as `up` or `loopback`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

/// Enumerate packet-capture interfaces.
#[must_use]
pub fn list() -> Vec<PcapInterfaceInfo> {
    sorted_unique(platform_candidates())
}

#[cfg(feature = "pcap-capture")]
fn platform_candidates() -> Vec<PcapInterfaceInfo> {
    pcap::Device::list()
        .unwrap_or_default()
        .into_iter()
        .map(native_device_info)
        .collect()
}

#[cfg(feature = "pcap-capture")]
fn native_device_info(device: pcap::Device) -> PcapInterfaceInfo {
    let mut flags = Vec::new();
    if device.flags.is_loopback() {
        flags.push("loopback".to_string());
    }
    if device.flags.is_up() {
        flags.push("up".to_string());
    }
    if device.flags.is_running() {
        flags.push("running".to_string());
    }
    if device.flags.is_wireless() {
        flags.push("wireless".to_string());
    }

    PcapInterfaceInfo {
        device: device.name,
        display_name: device.desc.clone(),
        description: device.desc,
        // Keep addresses out of the public detect payload until authenticated
        // discovery policy is finalized.
        addresses: Vec::new(),
        flags,
    }
}

#[cfg(not(feature = "pcap-capture"))]
fn platform_candidates() -> Vec<PcapInterfaceInfo> {
    Vec::new()
}

fn sorted_unique(mut candidates: Vec<PcapInterfaceInfo>) -> Vec<PcapInterfaceInfo> {
    candidates.sort_by(|a, b| {
        a.display_name
            .as_deref()
            .unwrap_or(&a.device)
            .cmp(b.display_name.as_deref().unwrap_or(&b.device))
            .then_with(|| a.device.cmp(&b.device))
    });
    candidates.dedup_by(|a, b| a.device == b.device);
    candidates
}

#[cfg(test)]
mod tests {
    // REQ: FR-SRC-PCAP-DETECT

    use super::*;

    #[test]
    fn list_returns_vec() {
        let _ = list();
    }

    #[test]
    fn sorted_unique_orders_by_label_and_dedups_device() {
        let out = sorted_unique(vec![
            PcapInterfaceInfo {
                device: "eth1".to_string(),
                display_name: Some("Zeta".to_string()),
                description: None,
                addresses: vec![],
                flags: vec![],
            },
            PcapInterfaceInfo {
                device: "eth0".to_string(),
                display_name: Some("Alpha".to_string()),
                description: None,
                addresses: vec![],
                flags: vec![],
            },
            PcapInterfaceInfo {
                device: "eth0".to_string(),
                display_name: Some("Alpha duplicate".to_string()),
                description: None,
                addresses: vec![],
                flags: vec![],
            },
        ]);

        assert_eq!(
            out.iter()
                .map(|iface| iface.device.as_str())
                .collect::<Vec<_>>(),
            vec!["eth0", "eth1"]
        );
    }
}
