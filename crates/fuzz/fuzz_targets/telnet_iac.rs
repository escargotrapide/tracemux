#![no_main]

use libfuzzer_sys::fuzz_target;

const IAC: u8 = 0xff;

fuzz_target!(|data: &[u8]| {
    let mut commands = 0usize;
    let mut index = 0usize;
    while index < data.len() {
        if data[index] == IAC {
            commands = commands.saturating_add(1);
            index = index.saturating_add(2);
        } else {
            index += 1;
        }
    }
    let _ = std::hint::black_box(commands);
});
