import { resolveWanloggerHttpUrl } from "~/adapters/wss";

export interface DetectReport {
  kinds: string[];
  serial_candidates: string[];
}

export interface SerialSpecOptions {
  baud?: number;
  dataBits?: number;
  parity?: string;
  stopBits?: number;
  flow?: string;
}

type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function normalizeDetectReport(value: unknown): DetectReport {
  const input = value && typeof value === "object" ? value as Partial<DetectReport> : {};
  return {
    kinds: asStringArray(input.kinds),
    serial_candidates: asStringArray(input.serial_candidates).sort((a, b) => a.localeCompare(b)),
  };
}

export async function detectSources(fetchImpl: FetchLike = fetch): Promise<DetectReport> {
  const response = await fetchImpl(resolveWanloggerHttpUrl("/api/detect"));
  if (!response.ok) {
    throw new Error(`detect failed: HTTP ${response.status}`);
  }
  return normalizeDetectReport(await response.json() as unknown);
}

function encodeSerialPort(port: string): string {
  return encodeURIComponent(port).replace(/%2F/g, "/").replace(/%5C/g, "%5C");
}

export function serialSpecForPort(port: string, options: SerialSpecOptions = {}): string {
  const baud = options.baud ?? 115_200;
  const dataBits = options.dataBits ?? 8;
  const parity = options.parity ?? "none";
  const stopBits = options.stopBits ?? 1;
  const flow = options.flow ?? "none";
  return `serial://${encodeSerialPort(port)}?baud=${baud}&data=${dataBits}&parity=${parity}&stop=${stopBits}&flow=${flow}`;
}
