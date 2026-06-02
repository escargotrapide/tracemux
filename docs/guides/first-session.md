# First session walkthrough

This walkthrough uses the mock source so no hardware or capture driver is
required.

## Start the server and UI

```powershell
just dev-server
```

In another terminal:

```powershell
just dev-web
```

## Add a mock source

In the Sources panel, enter:

```text
mock://demo
```

Click Add source. The source should appear in the table and the Terminal panel
should have a selectable target. The built-in mock source is finite and may
already show `stopped` with `0` bytes; that is expected for this hardware-free
walkthrough. The Metrics panel should show the connection and local UI counters;
server-side byte and record counters update when a selected source produces
frames.

## Send data when supported

Sources that expose a paired Sink can accept terminal input. The Terminal send
box is disabled while the server connection is closed or when no source is
selected. Source-only transports such as packet capture do not accept write-back.

## Add a note

Open source details and add a source note. Notes sync to the server when the
annotation API is reachable and remain browser-local as a fallback. Log bytes
are still persisted only by the server.

## Export the session

For persisted sources, use the per-source export buttons or the bulk export
controls in the Sources panel. Exports are produced by the server-side session
export API, not by browser-local log storage.

Useful formats:

- `text` for plain terminal-like review.
- `csv` for spreadsheets.
- `jsonl` for scripted analysis.
- `pcapng` for packet-shaped sessions.

## Stop or remove the source

Stop pauses a long-running live source. The mock source used here may already be
stopped, so Remove is the main cleanup action for this walkthrough. Remove
deletes the source from the server registry without deleting persisted
session-dir logs.