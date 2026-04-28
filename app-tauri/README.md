# wanlogger Tauri shell

Tauri 2 desktop wrapper around `web/`. In production the binary
ships the `wanlogger serve` sidecar in `src-tauri/binaries/` and
launches it on startup. In dev, run a backend manually and point
`VITE_WANLOGGER_URL` at it.

## Dev

```bash
pnpm install
pnpm --filter ./app-tauri dev   # runs `tauri dev`
```

Requires the Rust toolchain pinned in `rust-toolchain.toml` and the
`tauri-cli` (installed automatically as a dev dep).

## Notes

- The Tauri crate is intentionally kept *outside* the main Cargo
  workspace (it has its own `Cargo.toml` under `src-tauri/`) so that
  `cargo build --workspace` keeps working without a Tauri toolchain.
- Add app icons under `src-tauri/icons/` before bundling a release.
