//! Auto-detection probes (serial / TCP / UDP / pcap).
//!
//! See `add-source` skill for adding probes.

pub mod pcap;
pub mod serial;
pub mod tcp;
pub mod udp;
