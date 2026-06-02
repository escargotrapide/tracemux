# Authentication and TLS

TraceMux is secure by default for non-loopback use. Development on
`127.0.0.1` can use `--no-auth`; remote or shared servers should require
bearer authentication and TLS.

## Loopback development

`just dev-server` starts the server on loopback with `--no-auth`. The server
rejects no-auth mode for non-loopback peers.

## Token hashes

Generate an argon2id PHC hash:

```powershell
$env:TRACEMUX_TOKEN = '<paste-generated-token>'
tracemux token-hash > tokens.phc
Remove-Item Env:TRACEMUX_TOKEN
```

`tracemux token-hash --token <token>` is also available, but prefer the
environment variable form so the plaintext token is not recorded in shell
history.

Store the PHC output in a file such as `tokens.phc`, then reference that file
from config:

```toml
config_version = 1

[server]
bind = "127.0.0.1:9443"
session_root = "tracemux-sessions"
require_auth = true
token_phc_files = ["tokens.phc"]

[server.tls]
enabled = true
dir = "tls-state"
```

Do not store plaintext bearer tokens in TOML. For secrets used by source specs,
store only an indirection such as `secret://edge-token` and resolve the value
through the OS keyring.

## UI token passing

The web client sends the token as the `bearer.<token>` WebSocket subprotocol
alongside `tracemux.v1`. In development, pass the token through the existing
environment or UI launch path rather than hard-coding it in source specs:

```powershell
$env:TRACEMUX_TOKEN = '<paste-generated-token>'
pwsh scripts/dev-web.ps1 -Url wss://127.0.0.1:9443/ws -Token $env:TRACEMUX_TOKEN
```

## TLS and certificate trust

The server uses rustls. `--tls-dir <dir>` or `[server.tls]` generates
`server.crt` and `server.key` on first start when they do not already exist:

```powershell
tracemux serve --bind 127.0.0.1:9443 --require-auth --token-phc-file tokens.phc --tls-dir tls-state
```

For self-signed development certificates, import the generated `server.crt`
into the client trust store before using the browser UI or CLI against a
`wss://` endpoint. With `RUST_LOG=tracemux_server=info`, the server logs the
certificate SHA-256 fingerprint on startup. The codebase also includes TOFU
pin-store primitives for clients that wire explicit fingerprint pinning.

If a client reports `E-2103`, treat it as a possible endpoint or certificate
change. Verify the server fingerprint out of band before clearing a stored pin.
The runbook is `docs/errors/E-2103.md`.

## Recommended remote profile

- Bind the server to the intended interface explicitly.
- Set `require_auth = true`.
- Store token hashes in `token_phc_files`.
- Enable TLS for non-loopback use.
- Keep source specs free of plaintext credentials.