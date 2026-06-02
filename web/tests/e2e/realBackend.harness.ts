// Shared harness for the GUI real-backend smoke suite.
//
// Spawns a *real* `tracemux serve` (plaintext WS, no-auth, loopback) plus a
// `tracemux-virt-peer` TCP listener, so Playwright can drive the live UI
// through the full browser -> WSS -> source -> session-dir path with no real
// hardware. The UI auto-connects to ws://127.0.0.1:9000/ws on the Vite host.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..", "..", "..");

export const SERVER_HOST = "127.0.0.1";
export const SERVER_PORT = 9000;
export const PEER_HOST = "127.0.0.1";
export const PEER_PORT = 9099;
export const PEER_SEND_TEXT = "virt-peer-e2e";

const STATE_FILE = path.join(HERE, "..", "..", "test-results", ".real-backend.json");

interface BackendState {
  pids: number[];
  sessionRoot: string;
}

function binPath(name: string): string {
  const targetDir = process.env.CARGO_TARGET_DIR
    ? path.resolve(process.env.CARGO_TARGET_DIR)
    : path.join(REPO_ROOT, "target");
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  return path.join(targetDir, "debug", exe);
}

function waitForPort(host: string, port: number, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const attempt = (): void => {
      const socket = createConnection({ host, port });
      socket.once("connect", () => {
        socket.destroy();
        resolve();
      });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() > deadline) {
          reject(new Error(`Timed out waiting for ${host}:${port}`));
        } else {
          setTimeout(attempt, 150);
        }
      });
    };
    attempt();
  });
}

function waitForStdout(
  child: ChildProcess,
  needle: string,
  label: string,
  timeoutMs: number,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`Timed out waiting for ${label} to print "${needle}"`));
    }, timeoutMs);
    let buffer = "";
    child.stdout?.on("data", (chunk: Buffer) => {
      buffer += chunk.toString("utf8");
      if (buffer.includes(needle)) {
        clearTimeout(timer);
        resolve();
      }
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`${label} exited early with code ${code ?? "null"}`));
    });
  });
}

/** Spawn the real server + virtual peer and record their PIDs for teardown. */
export async function startRealBackend(): Promise<void> {
  const cli = binPath("tracemux");
  const peer = binPath("tracemux-virt-peer");
  for (const bin of [cli, peer]) {
    if (!existsSync(bin)) {
      throw new Error(
        `Missing ${bin}. Build first: cargo build -p tracemux-cli -p tracemux-virt-peer`,
      );
    }
  }

  const sessionRoot = mkdtempSync(path.join(tmpdir(), "tmux-gui-smoke-"));

  // The peer listens and accepts exactly one connection, so we must not probe
  // its port; wait for its "listening" stdout line instead.
  const peerProc = spawn(
    peer,
    [
      "--log-filter",
      "warn",
      "tcp",
      "--mode",
      "listen",
      "--addr",
      `${PEER_HOST}:${PEER_PORT}`,
      "--send",
      PEER_SEND_TEXT,
      "--eol",
      "lf",
      "--repeat",
      "120",
      "--initial-delay-ms",
      "300",
      "--interval-ms",
      "1000",
    ],
    { stdio: ["ignore", "pipe", "ignore"] },
  );

  const serverProc = spawn(
    cli,
    [
      "serve",
      "--no-auth",
      "--bind",
      `${SERVER_HOST}:${SERVER_PORT}`,
      "--session-root",
      sessionRoot,
    ],
    { stdio: ["ignore", "ignore", "ignore"] },
  );

  try {
    await waitForStdout(peerProc, "listening", "tracemux-virt-peer", 15_000);
    await waitForPort(SERVER_HOST, SERVER_PORT, 30_000);
  } catch (err) {
    killPids([peerProc.pid, serverProc.pid]);
    rmSync(sessionRoot, { recursive: true, force: true });
    throw err;
  }

  const state: BackendState = {
    pids: [peerProc.pid, serverProc.pid].filter((p): p is number => typeof p === "number"),
    sessionRoot,
  };
  mkdirSync(path.dirname(STATE_FILE), { recursive: true });
  writeFileSync(STATE_FILE, JSON.stringify(state), "utf8");
}

function killPids(pids: Array<number | undefined>): void {
  for (const pid of pids) {
    if (typeof pid !== "number") continue;
    try {
      process.kill(pid);
    } catch {
      // already gone
    }
  }
}

/** Stop the processes started by {@link startRealBackend} and clean up. */
export function stopRealBackend(): void {
  if (!existsSync(STATE_FILE)) return;
  let state: BackendState;
  try {
    state = JSON.parse(readFileSync(STATE_FILE, "utf8")) as BackendState;
  } catch {
    rmSync(STATE_FILE, { force: true });
    return;
  }
  killPids(state.pids);
  rmSync(state.sessionRoot, { recursive: true, force: true });
  rmSync(STATE_FILE, { force: true });
}
