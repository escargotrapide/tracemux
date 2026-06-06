#![no_main]

use libfuzzer_sys::fuzz_target;
use tracemux_core::codec::{decode, split_lines, Eol};

fuzz_target!(|data: &[u8]| {
    let eol = Eol::detect(data);
    for line in split_lines(data, eol).take(256) {
        let _ = decode(line, "utf-8");
        let _ = std::hint::black_box(line.contains(&0x1b));
    }
});
