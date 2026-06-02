#![no_main]

use libfuzzer_sys::fuzz_target;
use tracemux_server::wire::{decode, encode};

fuzz_target!(|data: &[u8]| {
    if let Ok(envelope) = decode(data) {
        let _ = encode(&envelope);
    }
});
