#![no_main]

use libfuzzer_sys::fuzz_target;
use tracemux_core::log::index::IndexEntry;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for line in text.lines().take(64) {
        let _ = serde_json::from_str::<IndexEntry>(line);
    }
});
