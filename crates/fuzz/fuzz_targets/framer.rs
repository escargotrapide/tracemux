#![no_main]

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use tracemux_core::framer::length_prefixed::{Endian, HeaderWidth, LengthPrefixedFramer};
use tracemux_core::framer::line::{Eol, LineFramer};
use tracemux_core::framer::passthrough::PassthroughFramer;
use tracemux_core::framer::Framer;

fn drain_framer(mut framer: impl Framer, data: &[u8]) {
    let mut buf = BytesMut::from(data);
    for _ in 0..1024 {
        match framer.poll_frame(&mut buf) {
            Ok(Some(frame)) => {
                let _ = std::hint::black_box(frame.len());
            }
            Ok(None) | Err(_) => break,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, frame)) = data.split_first() else {
        return;
    };
    match selector % 4 {
        0 => drain_framer(LineFramer::new(Eol::Auto, 4096), frame),
        1 => drain_framer(PassthroughFramer, frame),
        2 => drain_framer(
            LengthPrefixedFramer::new(HeaderWidth::U16, Endian::Big, false, 4096),
            frame,
        ),
        _ => drain_framer(
            LengthPrefixedFramer::new(HeaderWidth::U32, Endian::Little, true, 4096),
            frame,
        ),
    }
});
