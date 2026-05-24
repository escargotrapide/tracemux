//! Packet capture source model and fake backend integration tests.

// REQ: FR-SRC-PCAP
// REQ: NFR-PORT-PCAP
// REQ: NFR-MAINT-PCAP

use bytes::Bytes;
use wanlogger_core::packet_summary::LINKTYPE_ETHERNET;
use wanlogger_core::source::pcap::{FakePcapBackend, PcapConfig, PcapPacket, PcapSource};
use wanlogger_core::source::{Frame, Source};
use wanlogger_core::ErrorId;

fn packet(seq: u64, body: &'static [u8]) -> PcapPacket {
    PcapPacket::new(
        seq,
        1_700_000_000_000_000_000 + seq as i64,
        64,
        LINKTYPE_ETHERNET,
        0,
        body,
    )
}

#[tokio::test]
async fn fake_backend_emits_deterministic_packets() {
    let backend = FakePcapBackend::new([packet(1, b"first"), packet(2, b"second")]);
    let mut source = PcapSource::with_backend(PcapConfig::new("fake0"), backend);

    source.open().await.unwrap();

    let first = source.recv_packet().await.unwrap().unwrap();
    assert_eq!(first.seq, 1);
    assert_eq!(first.captured_len, 5);
    assert_eq!(first.original_len, 64);
    assert_eq!(first.linktype, LINKTYPE_ETHERNET);
    assert_eq!(first.interface_id, 0);
    assert_eq!(first.data, Bytes::from_static(b"first"));

    let second = source.recv_packet().await.unwrap().unwrap();
    assert_eq!(second.seq, 2);
    assert_eq!(second.data, Bytes::from_static(b"second"));

    assert!(source.recv_packet().await.unwrap().is_none());
}

#[tokio::test]
async fn metadata_reports_pcap_kind_and_interface_label() {
    let mut config = PcapConfig::new("\\Device\\NPF_{fake}");
    config.display_name = Some("Fake Ethernet".into());
    config.promiscuous = true;
    config.filter = Some("tcp port 502".into());
    let source = PcapSource::with_backend(config, FakePcapBackend::default());

    let meta = source.metadata();

    assert_eq!(meta.kind, "pcap");
    assert_eq!(meta.iface, "Fake Ethernet");
    assert_eq!(
        meta.tags.get("interface").map(String::as_str),
        Some("\\Device\\NPF_{fake}")
    );
    assert_eq!(
        meta.tags.get("promiscuous").map(String::as_str),
        Some("true")
    );
    assert_eq!(
        meta.tags.get("filter").map(String::as_str),
        Some("tcp port 502")
    );
}

#[tokio::test]
async fn source_recv_maps_packets_to_datagrams() {
    let backend = FakePcapBackend::new([packet(1, b"payload")]);
    let mut source = PcapSource::with_backend(PcapConfig::new("fake0"), backend);

    source.open().await.unwrap();
    let frame = source.recv().await.unwrap().unwrap();

    match frame {
        Frame::Datagram { src, data } => {
            assert_eq!(src.as_deref(), Some("fake0"));
            assert_eq!(data, Bytes::from_static(b"payload"));
        }
        other => panic!("expected datagram, got {other:?}"),
    }
}

#[tokio::test]
async fn recv_packet_preserves_lengths_linktype_and_timestamp() {
    let packet = PcapPacket::new(
        42,
        123_456_789,
        128,
        LINKTYPE_ETHERNET,
        3,
        Bytes::from_static(b"captured"),
    );
    let backend = FakePcapBackend::new([packet]);
    let mut source = PcapSource::with_backend(PcapConfig::new("fake0"), backend);

    source.open().await.unwrap();
    let packet = source.recv_packet().await.unwrap().unwrap();

    assert_eq!(packet.seq, 42);
    assert_eq!(packet.ts_origin_ns, 123_456_789);
    assert_eq!(packet.captured_len, 8);
    assert_eq!(packet.original_len, 128);
    assert_eq!(packet.linktype, LINKTYPE_ETHERNET);
    assert_eq!(packet.interface_id, 3);
}

#[tokio::test]
async fn fake_backend_stats_advance() {
    let backend =
        FakePcapBackend::new([packet(1, b"abc"), packet(2, b"defg")]).with_kernel_drops(9);
    let mut source = PcapSource::with_backend(PcapConfig::new("fake0"), backend);

    source.open().await.unwrap();
    assert_eq!(source.stats().await.unwrap().capture_queue_depth, 2);

    source.recv_packet().await.unwrap().unwrap();
    let stats = source.stats().await.unwrap();

    assert_eq!(stats.packets_total, 1);
    assert_eq!(stats.bytes_total, 3);
    assert_eq!(stats.dropped_kernel_total, 9);
    assert_eq!(stats.capture_queue_depth, 1);
    assert_eq!(
        stats.last_packet_ts_origin_ns,
        Some(1_700_000_000_000_000_001)
    );
}

#[tokio::test]
async fn recv_packet_before_open_is_closed_error() {
    let backend = FakePcapBackend::new([packet(1, b"payload")]);
    let mut source = PcapSource::with_backend(PcapConfig::new("fake0"), backend);

    let err = source.recv_packet().await.unwrap_err();

    assert_eq!(err.id, ErrorId::E1102SourceClosed);
}
