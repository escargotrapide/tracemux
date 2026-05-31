# Security policy

`tracemux` ships secure-by-default. This file is a **critical path**
(see `AGENTS.md` §5). Changes require human review.

## Reporting a vulnerability

Please open a private security advisory on GitHub. Do not file a public
issue. Acknowledgement within 7 days; coordinated disclosure within
90 days for confirmed issues.

## Threat model (v0.1)

- Adversary on the local network can reach the WSS server.
- Adversary may attempt to exfiltrate logs via the wire protocol.
- Adversary may attempt to inject malformed frames / sources.
- A compromised UI **does not** imply server compromise.

## Defaults

- TLS via `rustls` + `rcgen` self-signed cert; **TOFU fingerprint pin**
  on first connect.
- Bearer tokens; `argon2id` (m=64 MB, t=3, p=1) on the server.
- `--no-auth` is gated to `127.0.0.1` / `::1`.
- WSS limits: at most 32 connections, at most 1 MiB per frame, 1 KiB/s sustained per
  connection without backpressure ack.
- File permissions: `0600` on POSIX, Owner-only ACL on Windows.
- Tauri capabilities are minimal (no `shell.open`).
- Secrets indirection via OS keyring (`keyring` crate); TOML stores
  only `secret://name`.
- `unsafe_code = "deny"` workspace-wide.
- `cargo deny` bans `openssl-sys` / `openssl`.
- `cargo audit` + `cargo deny` + `pnpm audit` in CI.
- Releases cosign-signed; no auto-update.

## Crypto choices

- KDF: argon2id (parameters above).
- Symmetric: rustls TLS 1.3 cipher suites only.
- Random: `rand::rngs::OsRng`.

## Logging hygiene

- Authentication tokens and key material **must never** appear in
  logs, traces, or `clientlog` frames. The `secret::Redact` newtype is
  the canonical way to wrap them.
