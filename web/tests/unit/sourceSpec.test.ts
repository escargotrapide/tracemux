import { describe, expect, it } from "vitest";
import { parseSourceSpec } from "../../src/state/sourceSpec";

describe("parseSourceSpec", () => {
  it("parses file specs with follow", () => {
    // REQ: FR-UI-008
    expect(parseSourceSpec("file:///C:/logs/app.log?follow=1")).toEqual({
      kind: "file",
      path: "C:/logs/app.log",
      follow: true,
    });
  });

  it("parses tcp, udp, and mock specs", () => {
    // REQ: FR-UI-008
    expect(parseSourceSpec("tcp://127.0.0.1:5555")).toEqual({
      kind: "tcp",
      addr: "127.0.0.1:5555",
    });
    expect(parseSourceSpec("udp://127.0.0.1:0")).toEqual({
      kind: "udp",
      bind: "127.0.0.1:0",
    });
    expect(parseSourceSpec("mock://demo%20source")).toEqual({
      kind: "mock",
      tag: "demo source",
    });
  });

  it("parses serial defaults and process argv", () => {
    // REQ: FR-UI-008
    expect(parseSourceSpec("serial://COM3")).toEqual({
      kind: "serial",
      port: "COM3",
      baud: 115200,
      data_bits: 8,
      parity: "none",
      stop_bits: 1,
      flow: "none",
    });
    expect(parseSourceSpec("process:///cmd?args=/C;echo%20hi")).toEqual({
      kind: "process",
      argv: ["cmd", "/C", "echo hi"],
    });
  });

  it("parses remote mirror specs", () => {
    // REQ: FR-REMOTE-001
    const remoteUrl = encodeURIComponent(
      "wss://edge.example.test:9000/ws?sid=00000000-0000-4000-8000-000000000001&ch=0&token_env=WANLOGGER_EDGE_TOKEN",
    );
    expect(parseSourceSpec(`remote://${remoteUrl}`)).toEqual({
      kind: "remote",
      url: "wss://edge.example.test:9000/ws?sid=00000000-0000-4000-8000-000000000001&ch=0&token_env=WANLOGGER_EDGE_TOKEN",
    });
  });

  it("rejects unsupported specs", () => {
    expect(() => parseSourceSpec("unknown://x")).toThrow(/unsupported/);
    expect(() => parseSourceSpec("not-a-uri")).toThrow(/missing scheme/);
  });
});
