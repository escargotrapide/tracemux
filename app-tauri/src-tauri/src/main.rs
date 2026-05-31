// Tauri 2 desktop entry point. The actual app lives in the library
// so that mobile targets can reuse it.

fn main() {
    tracemux_tauri_lib::run();
}
