// Browser-local terminal input settings (local echo + line ending) per source.
//
// Pipe-based process sources (cmd.exe, PowerShell) do not echo stdin and have
// no line discipline, so the Terminal panel can run a local cooked mode and
// translate the Enter key to a configurable line ending. These settings are
// client-side only and persisted locally; the server never sees them.
//
// REQ: FR-UI-002
// REQ: FR-UI-011

import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { browserStorage, safeGetItem, safeSetItem, type StorageLike } from "~/state/storage";

export const TERMINAL_INPUT_STORAGE_KEY = "tracemux.terminalInput.v1";

export type LocalEchoMode = "auto" | "on" | "off";
export type NewlineMode = "auto" | "cr" | "lf" | "crlf";

export const LOCAL_ECHO_MODES: readonly LocalEchoMode[] = ["auto", "on", "off"];
export const NEWLINE_MODES: readonly NewlineMode[] = ["auto", "cr", "lf", "crlf"];

export interface TerminalInputSetting {
  localEcho: LocalEchoMode;
  newline: NewlineMode;
  updatedAt: number;
}

export type TerminalInputSettings = Record<string, TerminalInputSetting>;

const DEFAULT_SETTING: TerminalInputSetting = {
  localEcho: "auto",
  newline: "auto",
  updatedAt: 0,
};

function isLocalEchoMode(value: unknown): value is LocalEchoMode {
  return typeof value === "string" && (LOCAL_ECHO_MODES as readonly string[]).includes(value);
}

function isNewlineMode(value: unknown): value is NewlineMode {
  return typeof value === "string" && (NEWLINE_MODES as readonly string[]).includes(value);
}

function sourceKey(sid: string): string {
  return sid.trim();
}

function normalizeRecord(value: unknown): TerminalInputSetting | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Partial<TerminalInputSetting>;
  const localEcho = isLocalEchoMode(input.localEcho) ? input.localEcho : "auto";
  const newline = isNewlineMode(input.newline) ? input.newline : "auto";
  const updatedAt =
    typeof input.updatedAt === "number" && Number.isFinite(input.updatedAt)
      ? Math.max(0, Math.trunc(input.updatedAt))
      : 0;
  if (localEcho === "auto" && newline === "auto" && updatedAt === 0) return null;
  return { localEcho, newline, updatedAt };
}

function loadInitial(storage: StorageLike | undefined): TerminalInputSettings {
  const raw = safeGetItem(TERMINAL_INPUT_STORAGE_KEY, storage);
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    const out: TerminalInputSettings = {};
    for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
      const sid = sourceKey(key);
      if (!sid) continue;
      const record = normalizeRecord(value);
      if (record) out[sid] = record;
    }
    return out;
  } catch {
    return {};
  }
}

const [terminalInput, setTerminalInputStore] = createStore<TerminalInputSettings>(
  loadInitial(browserStorage()),
);
const [terminalInputVersion, setTerminalInputVersion] = createSignal(0);

export { terminalInput, terminalInputVersion };

function persist(): void {
  safeSetItem(TERMINAL_INPUT_STORAGE_KEY, JSON.stringify(terminalInput));
}

/** Read the stored (possibly `auto`) setting for a source. */
export function terminalInputFor(sid: string): TerminalInputSetting {
  return terminalInput[sourceKey(sid)] ?? DEFAULT_SETTING;
}

/** Update one field of a source's terminal input setting. */
export function updateTerminalInput(
  sid: string,
  patch: Partial<Pick<TerminalInputSetting, "localEcho" | "newline">>,
): void {
  const key = sourceKey(sid);
  if (!key) return;
  const current = terminalInput[key] ?? DEFAULT_SETTING;
  const next: TerminalInputSetting = {
    localEcho: patch.localEcho ?? current.localEcho,
    newline: patch.newline ?? current.newline,
    updatedAt: Date.now(),
  };
  setTerminalInputStore(key, next);
  setTerminalInputVersion((v) => v + 1);
  persist();
}

/**
 * Heuristically infer whether a `process` source runs a Windows shell from its
 * display name (the server does not send argv to the browser).
 */
function looksLikeWindowsShell(name: string): boolean {
  return /(?:^|[\\/\s])(?:cmd|powershell|pwsh)(?:\.exe)?(?:$|[\s])|\.exe\b/i.test(name);
}

function looksLikePosixShell(name: string): boolean {
  return /(?:^|[\\/\s])(?:sh|bash|zsh|fish|dash)\b|\/bin\/|\/usr\/bin\//i.test(name);
}

/** Resolve the `auto` line-ending preset for a source kind/name. */
export function presetNewline(kind: string, name: string): NewlineMode {
  if (kind === "process") {
    if (looksLikePosixShell(name) && !looksLikeWindowsShell(name)) return "lf";
    return "crlf";
  }
  if (kind === "serial") return "cr";
  return "lf";
}

/** Resolve the `auto` local-echo preset for a source kind. */
export function presetLocalEcho(kind: string): LocalEchoMode {
  // Pipe-based process children do not echo stdin, so echo locally by default.
  return kind === "process" ? "on" : "off";
}

/** Resolve whether local echo is effectively enabled for a source. */
export function resolvedLocalEcho(sid: string, kind: string): boolean {
  const mode = terminalInputFor(sid).localEcho;
  const effective = mode === "auto" ? presetLocalEcho(kind) : mode;
  return effective === "on";
}

/** Resolve the bytes the Enter key should send for a source. */
export function resolvedNewlineBytes(sid: string, kind: string, name: string): string {
  const mode = terminalInputFor(sid).newline;
  const effective = mode === "auto" ? presetNewline(kind, name) : mode;
  switch (effective) {
    case "cr":
      return "\r";
    case "lf":
      return "\n";
    default:
      return "\r\n";
  }
}
