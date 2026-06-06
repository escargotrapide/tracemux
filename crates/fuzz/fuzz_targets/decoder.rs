#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;
use tracemux_core::decoder::json_lines::JsonLinesDecoder;
use tracemux_core::decoder::passthrough::PassthroughDecoder;
use tracemux_core::decoder::utf8_text::Utf8TextDecoder;
use tracemux_core::decoder::Decoder;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, frame)) = data.split_first() else {
        return;
    };
    match selector % 3 {
        0 => {
            let mut decoder = PassthroughDecoder::new();
            let _ = decoder.decode(Bytes::copy_from_slice(frame));
        }
        1 => {
            let mut decoder = Utf8TextDecoder::new("shift_jis");
            let _ = decoder.decode(Bytes::copy_from_slice(frame));
        }
        _ => {
            let mut decoder = JsonLinesDecoder::new();
            let _ = decoder.decode(Bytes::copy_from_slice(frame));
        }
    }
});
