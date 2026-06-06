# TraceMux fuzz targets

This standalone `cargo-fuzz` crate is intentionally outside the main workspace
so normal `cargo test --workspace` and `just ai-verify` do not require nightly
or libFuzzer.

Run smoke fuzzing from the repository root:

```powershell
just fuzz-smoke
```

Run one target directly:

```powershell
cargo +nightly fuzz run --manifest-path crates/fuzz/Cargo.toml framer
```

Targets should stay focused on parser and decoder boundaries that accept
untrusted bytes.