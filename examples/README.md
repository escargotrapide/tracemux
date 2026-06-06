# TraceMux example configs

These examples are starting points for `tracemux serve --config <file>`.
They intentionally avoid plaintext secrets. Adjust paths, ports, and auth
settings for your environment.

- `mock.toml`: hardware-free first run.
- `serial.toml`: one COM port at 115200 baud.
- `tcp-listener.toml`: local TCP source.
- `multi-source.toml`: mock plus TCP plus UDP.
- `packet-capture.toml`: packet capture with bounded UI publishing.

For walkthroughs, see [`docs/guides/`](../docs/guides/).