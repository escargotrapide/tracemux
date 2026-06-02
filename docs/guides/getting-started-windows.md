# Getting started on Windows

This guide gets a Windows developer from a fresh checkout to a working
TraceMux server and web UI.

## Prerequisites

- Rust via rustup, matching `rust-toolchain.toml`.
- Microsoft C++ build tools or Visual Studio Build Tools.
- Node.js LTS.
- `just` installed with `cargo install just`.
- PowerShell 7 or Windows PowerShell with script execution allowed for this repo.

Install web dependencies once:

```powershell
pwsh scripts/pnpm.ps1 install
```

If antivirus software blocks Cargo build scripts with access denied errors,
add exclusions for the Cargo home, the workspace target directory, and
`cargo.exe`. See `AGENTS.md` for the longer local-machine note.

## Verify the checkout

```powershell
just build
just test
pwsh scripts/check-encoding.ps1
```

For a full pre-review gate, run:

```powershell
just ai-verify
```

## Start the development UI

Use two terminals:

```powershell
just dev-server
```

```powershell
just dev-web
```

Open the Vite URL printed by `just dev-web`. The web UI talks to the
loopback server at `127.0.0.1:9000` with `--no-auth`, which is allowed
only on loopback.

## Start the desktop shell

Prepare the sidecar and generated placeholder icons once:

```powershell
just dev-prepare
```

Then run:

```powershell
just dev-tauri
```

Re-run `just dev-prepare` after rebuilding the CLI binary. Production
desktop builds must replace the generated placeholder icons.