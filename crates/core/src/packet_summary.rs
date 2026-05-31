//! Lightweight packet summary parsing for packet-capture sessions.
//!
//! This module intentionally extracts only bounded L2-L4 metadata. It is not a
//! Wireshark-style dissector and does not perform TCP stream reassembly.

// REQ: FR-DEC-PACKET-SUMMARY

use std::fmt;

use etherparse::{EtherType, LinkExtSlice, LinkSlice, NetSlice, SlicedPacket, TransportSlice};
use serde::{Deserialize, Serialize};

/// libpcap/Npcap link type for Ethernet captures (`LINKTYPE_ETHERNET`).
pub const LINKTYPE_ETHERNET: u32 = 1;
/// libpcap/Npcap link type for raw IPv4/IPv6 captures (`LINKTYPE_RAW`).
pub const LINKTYPE_RAW: u32 = 101;
/// libpcap/Npcap link type for Linux cooked capture v1 (`LINKTYPE_LINUX_SLL`).
pub const LINKTYPE_LINUX_SLL: u32 = 113;

/// Compact summary of an L2-L4 packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketSummary {
    /// Link type used as parser entry point.
    pub link_type: PacketLinkType,
    /// Captured packet length in bytes.
    pub packet_len: usize,
    /// Human-readable protocol label (`tcp`, `udp`, `icmpv4`, ...).
    pub protocol: String,
    /// Ethernet source MAC address, if present.
    pub src_mac: Option<String>,
    /// Ethernet destination MAC address, if present.
    pub dst_mac: Option<String>,
    /// VLAN identifiers from outermost to innermost tag.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vlan_ids: Vec<u16>,
    /// Final Ethernet type after VLAN tags, if known.
    pub ethertype: Option<u16>,
    /// IP version (`4` or `6`), if present.
    pub ip_version: Option<u8>,
    /// IP source address, if present.
    pub src_ip: Option<String>,
    /// IP destination address, if present.
    pub dst_ip: Option<String>,
    /// IP protocol/next-header number, if present.
    pub ip_protocol: Option<u8>,
    /// IPv4 TTL or IPv6 hop limit, if present.
    pub hop_limit: Option<u8>,
    /// Whether the IP payload is fragmented.
    pub fragmented: bool,
    /// TCP/UDP source port, if present.
    pub src_port: Option<u16>,
    /// TCP/UDP destination port, if present.
    pub dst_port: Option<u16>,
    /// ICMP type, if present.
    pub icmp_type: Option<u8>,
    /// ICMP code, if present.
    pub icmp_code: Option<u8>,
    /// Offset where the application payload starts, if known.
    pub payload_offset: Option<usize>,
    /// Application payload length, if known.
    pub payload_len: Option<usize>,
}

/// Parser entry point derived from a pcap link type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PacketLinkType {
    /// Ethernet II (`LINKTYPE_ETHERNET`).
    Ethernet,
    /// Raw IPv4 or IPv6 packet (`LINKTYPE_RAW`).
    RawIp,
    /// Linux cooked capture v1 (`LINKTYPE_LINUX_SLL`).
    LinuxSll,
}

impl PacketLinkType {
    /// Convert a libpcap/Npcap numeric link type into a supported parser entry.
    pub const fn from_pcap_linktype(linktype: u32) -> Option<Self> {
        match linktype {
            LINKTYPE_ETHERNET => Some(Self::Ethernet),
            LINKTYPE_RAW => Some(Self::RawIp),
            LINKTYPE_LINUX_SLL => Some(Self::LinuxSll),
            _ => None,
        }
    }
}

/// Packet summary parse error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PacketSummaryError {
    /// The pcap link type is not supported by the lightweight parser.
    #[error("unsupported pcap link type {0}")]
    UnsupportedLinkType(u32),
    /// The packet is malformed or truncated for the selected link type.
    #[error("malformed or truncated packet: {0}")]
    Malformed(String),
}

/// Parse a packet using a libpcap/Npcap numeric link type.
pub fn summarize_pcap_packet(
    linktype: u32,
    packet: &[u8],
) -> Result<PacketSummary, PacketSummaryError> {
    let link_type = PacketLinkType::from_pcap_linktype(linktype)
        .ok_or(PacketSummaryError::UnsupportedLinkType(linktype))?;
    summarize_packet(link_type, packet)
}

/// Parse a packet using an explicit supported link type.
pub fn summarize_packet(
    link_type: PacketLinkType,
    packet: &[u8],
) -> Result<PacketSummary, PacketSummaryError> {
    let sliced = match link_type {
        PacketLinkType::Ethernet => SlicedPacket::from_ethernet(packet),
        PacketLinkType::RawIp => SlicedPacket::from_ip(packet),
        PacketLinkType::LinuxSll => SlicedPacket::from_linux_sll(packet),
    }
    .map_err(|err| PacketSummaryError::Malformed(err.to_string()))?;

    let mut summary = PacketSummary {
        link_type,
        packet_len: packet.len(),
        protocol: "unknown".to_string(),
        src_mac: None,
        dst_mac: None,
        vlan_ids: Vec::new(),
        ethertype: None,
        ip_version: None,
        src_ip: None,
        dst_ip: None,
        ip_protocol: None,
        hop_limit: None,
        fragmented: false,
        src_port: None,
        dst_port: None,
        icmp_type: None,
        icmp_code: None,
        payload_offset: None,
        payload_len: None,
    };

    let mut cursor = apply_link_summary(&sliced, &mut summary);
    apply_vlan_summary(&sliced, &mut summary, &mut cursor);
    apply_net_summary(&sliced, &mut summary, &mut cursor);
    apply_transport_summary(&sliced, &mut summary, &mut cursor);

    if summary.payload_len.is_none() {
        apply_fallback_payload_summary(&sliced, &mut summary, cursor);
    }

    Ok(summary)
}

fn apply_link_summary(sliced: &SlicedPacket<'_>, summary: &mut PacketSummary) -> usize {
    match sliced.link.as_ref() {
        Some(LinkSlice::Ethernet2(eth)) => {
            summary.src_mac = Some(format_mac(eth.source()));
            summary.dst_mac = Some(format_mac(eth.destination()));
            summary.ethertype = Some(eth.ether_type().0);
            eth.header_len()
        }
        Some(LinkSlice::LinuxSll(sll)) => {
            if let Some(ether_type) = linux_sll_ethertype(sll.protocol_type()) {
                summary.ethertype = Some(ether_type.0);
            }
            sll.header_len()
        }
        Some(LinkSlice::EtherPayload(payload)) => {
            summary.ethertype = Some(payload.ether_type.0);
            0
        }
        Some(LinkSlice::LinuxSllPayload(payload)) => {
            if let Some(ether_type) = linux_sll_ethertype(payload.protocol_type) {
                summary.ethertype = Some(ether_type.0);
            }
            0
        }
        None => 0,
    }
}

fn apply_vlan_summary(sliced: &SlicedPacket<'_>, summary: &mut PacketSummary, cursor: &mut usize) {
    for ext in &sliced.link_exts {
        match ext {
            LinkExtSlice::Vlan(vlan) => {
                summary.vlan_ids.push(vlan.vlan_identifier().value());
                summary.ethertype = Some(vlan.ether_type().0);
                *cursor += vlan.header_len();
            }
            LinkExtSlice::Macsec(_) => {
                *cursor += ext.header_len();
            }
        }
    }
}

fn apply_net_summary(sliced: &SlicedPacket<'_>, summary: &mut PacketSummary, cursor: &mut usize) {
    match sliced.net.as_ref() {
        Some(NetSlice::Ipv4(ipv4)) => {
            let header = ipv4.header();
            summary.protocol = "ipv4".to_string();
            summary.ip_version = Some(4);
            summary.src_ip = Some(header.source_addr().to_string());
            summary.dst_ip = Some(header.destination_addr().to_string());
            summary.ip_protocol = Some(ipv4.payload_ip_number().0);
            summary.hop_limit = Some(header.ttl());
            summary.fragmented = ipv4.is_payload_fragmented();
            *cursor += usize::from(header.total_len()).saturating_sub(ipv4.payload().payload.len());
        }
        Some(NetSlice::Ipv6(ipv6)) => {
            let header = ipv6.header();
            summary.protocol = "ipv6".to_string();
            summary.ip_version = Some(6);
            summary.src_ip = Some(header.source_addr().to_string());
            summary.dst_ip = Some(header.destination_addr().to_string());
            summary.ip_protocol = Some(ipv6.payload().ip_number.0);
            summary.hop_limit = Some(header.hop_limit());
            summary.fragmented = ipv6.is_payload_fragmented();
            *cursor += usize::from(header.payload_length())
                .saturating_sub(ipv6.payload().payload.len())
                + header.header_len();
        }
        Some(NetSlice::Arp(_)) => {
            summary.protocol = "arp".to_string();
        }
        None => {
            if let Some(ethertype) = summary.ethertype {
                summary.protocol = format!("ethertype:0x{ethertype:04x}");
            }
        }
    }
}

fn apply_transport_summary(
    sliced: &SlicedPacket<'_>,
    summary: &mut PacketSummary,
    cursor: &mut usize,
) {
    match sliced.transport.as_ref() {
        Some(TransportSlice::Tcp(tcp)) => {
            summary.protocol = "tcp".to_string();
            summary.src_port = Some(tcp.source_port());
            summary.dst_port = Some(tcp.destination_port());
            *cursor += tcp.header_len();
            summary.payload_offset = Some(*cursor);
            summary.payload_len = Some(tcp.payload().len());
        }
        Some(TransportSlice::Udp(udp)) => {
            summary.protocol = "udp".to_string();
            summary.src_port = Some(udp.source_port());
            summary.dst_port = Some(udp.destination_port());
            *cursor += udp.header_len();
            summary.payload_offset = Some(*cursor);
            summary.payload_len = Some(udp.payload().len());
        }
        Some(TransportSlice::Icmpv4(icmp)) => {
            summary.protocol = "icmpv4".to_string();
            summary.icmp_type = Some(icmp.type_u8());
            summary.icmp_code = Some(icmp.code_u8());
            *cursor += icmp.header_len();
            summary.payload_offset = Some(*cursor);
            summary.payload_len = Some(icmp.payload().len());
        }
        Some(TransportSlice::Icmpv6(icmp)) => {
            summary.protocol = "icmpv6".to_string();
            summary.icmp_type = Some(icmp.type_u8());
            summary.icmp_code = Some(icmp.code_u8());
            *cursor += icmp.header_len();
            summary.payload_offset = Some(*cursor);
            summary.payload_len = Some(icmp.payload().len());
        }
        None => {}
    }
}

fn apply_fallback_payload_summary(
    sliced: &SlicedPacket<'_>,
    summary: &mut PacketSummary,
    cursor: usize,
) {
    if let Some(payload) = sliced.ip_payload() {
        summary.payload_offset = Some(cursor);
        summary.payload_len = Some(payload.payload.len());
    } else if let Some(payload) = sliced.ether_payload() {
        summary.payload_offset = Some(cursor);
        summary.payload_len = Some(payload.payload.len());
    } else if summary.payload_len.is_none() && cursor <= summary.packet_len {
        summary.payload_offset = Some(cursor);
        summary.payload_len = Some(summary.packet_len - cursor);
    }
}

fn linux_sll_ethertype(protocol_type: etherparse::LinuxSllProtocolType) -> Option<EtherType> {
    match protocol_type {
        etherparse::LinuxSllProtocolType::EtherType(ether_type) => Some(ether_type),
        _ => None,
    }
}

fn format_mac(mac: [u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

impl fmt::Display for PacketLinkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Ethernet => "ethernet",
            Self::RawIp => "raw-ip",
            Self::LinuxSll => "linux-sll",
        };
        f.write_str(value)
    }
}

#[cfg(test)]
fn ethertype_packet(ethertype: EtherType, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(14 + payload.len());
    packet.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    packet.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
    packet.extend_from_slice(&ethertype.0.to_be_bytes());
    packet.extend_from_slice(payload);
    packet
}

#[cfg(test)]
mod tests {
    // REQ: FR-DEC-PACKET-SUMMARY

    use super::*;
    use etherparse::{ether_type, ip_number, PacketBuilder};

    fn write_packet<T>(builder: T, payload: &[u8]) -> Vec<u8>
    where
        T: PacketWrite,
    {
        builder.write_packet(payload)
    }

    trait PacketWrite {
        fn write_packet(self, payload: &[u8]) -> Vec<u8>;
    }

    macro_rules! impl_packet_write {
        ($last:ty) => {
            impl PacketWrite for etherparse::PacketBuilderStep<$last> {
                fn write_packet(self, payload: &[u8]) -> Vec<u8> {
                    let mut packet = Vec::new();
                    self.write(&mut packet, payload).expect("build packet");
                    packet
                }
            }
        };
    }

    impl_packet_write!(etherparse::TcpHeader);
    impl_packet_write!(etherparse::UdpHeader);
    impl_packet_write!(etherparse::Icmpv4Header);
    impl_packet_write!(etherparse::Icmpv6Header);

    #[test]
    fn summarizes_ethernet_ipv4_tcp() {
        let packet = write_packet(
            PacketBuilder::ethernet2(
                [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            )
            .ipv4([192, 168, 1, 10], [192, 168, 1, 20], 64)
            .tcp(12_345, 443, 1, 4096)
            .syn(),
            &[1, 2, 3, 4],
        );

        let summary = summarize_pcap_packet(LINKTYPE_ETHERNET, &packet).unwrap();

        assert_eq!(summary.link_type, PacketLinkType::Ethernet);
        assert_eq!(summary.protocol, "tcp");
        assert_eq!(summary.src_mac.as_deref(), Some("00:11:22:33:44:55"));
        assert_eq!(summary.dst_mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(summary.ethertype, Some(ether_type::IPV4.0));
        assert_eq!(summary.ip_version, Some(4));
        assert_eq!(summary.src_ip.as_deref(), Some("192.168.1.10"));
        assert_eq!(summary.dst_ip.as_deref(), Some("192.168.1.20"));
        assert_eq!(summary.ip_protocol, Some(ip_number::TCP.0));
        assert_eq!(summary.hop_limit, Some(64));
        assert_eq!(summary.src_port, Some(12_345));
        assert_eq!(summary.dst_port, Some(443));
        assert_eq!(summary.payload_offset, Some(14 + 20 + 20));
        assert_eq!(summary.payload_len, Some(4));
    }

    #[test]
    fn summarizes_vlan_ipv4_udp() {
        let packet = write_packet(
            PacketBuilder::ethernet2(
                [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            )
            .single_vlan(123.try_into().unwrap())
            .ipv4([10, 0, 0, 1], [10, 0, 0, 2], 32)
            .udp(5353, 53),
            &[9, 8, 7],
        );

        let summary = summarize_packet(PacketLinkType::Ethernet, &packet).unwrap();

        assert_eq!(summary.protocol, "udp");
        assert_eq!(summary.vlan_ids, vec![123]);
        assert_eq!(summary.ethertype, Some(ether_type::IPV4.0));
        assert_eq!(summary.src_ip.as_deref(), Some("10.0.0.1"));
        assert_eq!(summary.dst_ip.as_deref(), Some("10.0.0.2"));
        assert_eq!(summary.src_port, Some(5353));
        assert_eq!(summary.dst_port, Some(53));
        assert_eq!(summary.payload_offset, Some(14 + 4 + 20 + 8));
        assert_eq!(summary.payload_len, Some(3));
    }

    #[test]
    fn summarizes_ethernet_ipv6_icmpv6() {
        let src = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let dst = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
        let packet = write_packet(
            PacketBuilder::ethernet2(
                [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            )
            .ipv6(src, dst, 128)
            .icmpv6_echo_request(7, 8),
            &[1, 2, 3],
        );

        let summary = summarize_packet(PacketLinkType::Ethernet, &packet).unwrap();

        assert_eq!(summary.protocol, "icmpv6");
        assert_eq!(summary.ip_version, Some(6));
        assert_eq!(summary.src_ip.as_deref(), Some("2001:db8::1"));
        assert_eq!(summary.dst_ip.as_deref(), Some("2001:db8::2"));
        assert_eq!(summary.ip_protocol, Some(ip_number::IPV6_ICMP.0));
        assert_eq!(summary.hop_limit, Some(128));
        assert_eq!(summary.icmp_type, Some(128));
        assert_eq!(summary.icmp_code, Some(0));
        assert_eq!(summary.payload_offset, Some(14 + 40 + 8));
        assert_eq!(summary.payload_len, Some(3));
    }

    #[test]
    fn summarizes_ethernet_ipv4_icmpv4() {
        let packet = write_packet(
            PacketBuilder::ethernet2(
                [0x00, 0x11, 0x22, 0x33, 0x44, 0x55],
                [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            )
            .ipv4([192, 0, 2, 1], [192, 0, 2, 2], 255)
            .icmpv4_echo_request(7, 8),
            &[1, 2, 3],
        );

        let summary = summarize_packet(PacketLinkType::Ethernet, &packet).unwrap();

        assert_eq!(summary.protocol, "icmpv4");
        assert_eq!(summary.ip_version, Some(4));
        assert_eq!(summary.src_ip.as_deref(), Some("192.0.2.1"));
        assert_eq!(summary.dst_ip.as_deref(), Some("192.0.2.2"));
        assert_eq!(summary.ip_protocol, Some(ip_number::ICMP.0));
        assert_eq!(summary.hop_limit, Some(255));
        assert_eq!(summary.icmp_type, Some(8));
        assert_eq!(summary.icmp_code, Some(0));
        assert_eq!(summary.payload_offset, Some(14 + 20 + 8));
        assert_eq!(summary.payload_len, Some(3));
    }

    #[test]
    fn summarizes_raw_ipv4_udp() {
        let packet = write_packet(
            PacketBuilder::ipv4([172, 16, 0, 1], [172, 16, 0, 2], 16).udp(10, 20),
            &[1, 2],
        );

        let summary = summarize_pcap_packet(LINKTYPE_RAW, &packet).unwrap();

        assert_eq!(summary.link_type, PacketLinkType::RawIp);
        assert_eq!(summary.protocol, "udp");
        assert_eq!(summary.src_mac, None);
        assert_eq!(summary.ethertype, None);
        assert_eq!(summary.payload_offset, Some(20 + 8));
        assert_eq!(summary.payload_len, Some(2));
    }

    #[test]
    fn rejects_truncated_packet() {
        let err = summarize_packet(PacketLinkType::Ethernet, &[0u8; 8]).unwrap_err();

        assert!(matches!(err, PacketSummaryError::Malformed(_)));
    }

    #[test]
    fn reports_unsupported_ethertype_as_generic_protocol() {
        let packet = ethertype_packet(EtherType(0x88b5), &[1, 2, 3, 4]);

        let summary = summarize_packet(PacketLinkType::Ethernet, &packet).unwrap();

        assert_eq!(summary.protocol, "ethertype:0x88b5");
        assert_eq!(summary.ethertype, Some(0x88b5));
        assert_eq!(summary.payload_offset, Some(14));
        assert_eq!(summary.payload_len, Some(4));
    }

    #[test]
    fn rejects_unsupported_linktype() {
        let err = summarize_pcap_packet(999_999, &[]).unwrap_err();

        assert_eq!(err, PacketSummaryError::UnsupportedLinkType(999_999));
    }
}
