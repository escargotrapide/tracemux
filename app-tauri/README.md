# tracemux Tauri shell

Tauri 2 desktop wrapper around `web/`. In production the binary
ships the `tracemux serve` sidecar in `src-tauri/binaries/` and
launches it on startup. In dev, `just dev-tauri` starts the bundled
sidecar on `127.0.0.1:9000` by default after `just dev-prepare` has
copied the CLI binary.

## Dev

```bash
pnpm install
just dev-prepare
just dev-tauri
```

Use `scripts/dev-tauri.* --no-sidecar` / `-NoSidecar` when you want to
run `tracemux serve` manually. Override the backend with
`VITE_TRACEMUX_URL` or the script `--url` / `-Url` option.

## Closing while logging

Closing the Tauri window stops the bundled sidecar process. The server owns all
session data, and the file sink flushes raw, index, line, and frame files as
records are appended so recently ingested records are visible even before a
normal source EOF. For long captures where an orderly source shutdown matters,
stop the source in the UI first, or run `tracemux serve` manually and launch
Tauri with `--no-sidecar`.

Requires the Rust toolchain pinned in `rust-toolchain.toml` and the
`tauri-cli` (installed automatically as a dev dep).

## Notes

- The Tauri crate is intentionally kept *outside* the main Cargo
  workspace (it has its own `Cargo.toml` under `src-tauri/`) so that
  `cargo build --workspace` keeps working without a Tauri toolchain.
- Add app icons under `src-tauri/icons/` before bundling a release.
