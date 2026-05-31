import { resolveTraceMuxHttpUrl } from "~/adapters/wss";

export interface DetectReport {
  kinds: string[];
  serial_candidates: string[];
  pcap_interfaces: PcapInterfaceInfo[];
}

export interface PcapInterfaceInfo {
  device: string;
  display_name?: string;
  description?: string;
  addresses: string[];
  flags: string[];
}

export interface SerialSpecOptions {
  baud?: number;
  dataBits?: number;
  parity?: string;
  stopBits?: number;
  flow?: string;
}

export type PcapPublishMode = "stats-only" | "sampled" | "full";

export interface PcapSpecOptions {
  snaplen?: number;
  promiscuous?: boolean;
  filter?: string;
  publishMode?: PcapPublishMode;
  saveMode?: "session" | "pcapng" | "both";
}

type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function asPcapInterfaces(value: unknown): PcapInterfaceInfo[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  return value
    .flatMap((item): PcapInterfaceInfo[] => {
      if (!item || typeof item !== "object") return [];
      const input = item as Record<string, unknown>;
      const device = optionalString(input.device);
      if (!device || seen.has(device)) return [];
      seen.add(device);
      const info: PcapInterfaceInfo = {
        device,
        addresses: asStringArray(input.addresses).sort((a, b) => a.localeCompare(b)),
        flags: asStringArray(input.flags).sort((a, b) => a.localeCompare(b)),
      };
      const displayName = optionalString(input.display_name);
      if (displayName) info.display_name = displayName;
      const description = optionalString(input.description);
      if (description) info.description = description;
      return [info];
    })
    .sort((a, b) => {
      const labelA = a.display_name ?? a.device;
      const labelB = b.display_name ?? b.device;
      return labelA.localeCompare(labelB) || a.device.localeCompare(b.device);
    });
}

function normalizeDetectReport(value: unknown): DetectReport {
  const input = value && typeof value === "object" ? value as Partial<DetectReport> : {};
  return {
    kinds: asStringArray(input.kinds),
    serial_candidates: asStringArray(input.serial_candidates).sort((a, b) => a.localeCompare(b)),
    pcap_interfaces: asPcapInterfaces(input.pcap_interfaces),
  };
}

export async function detectSources(fetchImpl: FetchLike = fetch): Promise<DetectReport> {
  const response = await fetchImpl(resolveTraceMuxHttpUrl("/api/detect"));
  if (!response.ok) {
    throw new Error(`detect failed: HTTP ${response.status}`);
  }
  return normalizeDetectReport(await response.json() as unknown);
}

function encodeSerialPort(port: string): string {
  return encodeURIComponent(port).replace(/%2F/g, "/").replace(/%5C/g, "%5C");
}

function encodePcapDevice(device: string): string {
  return encodeURIComponent(device).replace(/%2F/g, "/").replace(/%5C/g, "%5C");
}

export function serialSpecForPort(port: string, options: SerialSpecOptions = {}): string {
  const baud = options.baud ?? 115_200;
  const dataBits = options.dataBits ?? 8;
  const parity = options.parity ?? "none";
  const stopBits = options.stopBits ?? 1;
  const flow = options.flow ?? "none";
  return `serial://${encodeSerialPort(port)}?baud=${baud}&data=${dataBits}&parity=${parity}&stop=${stopBits}&flow=${flow}`;
}

export function pcapSpecForInterface(
  iface: PcapInterfaceInfo | string,
  options: PcapSpecOptions = {},
): string {
  const device = typeof iface === "string" ? iface : iface.device;
  const displayName = typeof iface === "string" ? undefined : iface.display_name;
  const params = new URLSearchParams({
    snaplen: String(options.snaplen ?? 65_535),
    promisc: options.promiscuous ? "1" : "0",
    save: options.saveMode ?? "session",
    publish: options.publishMode ?? "stats-only",
  });
  if (displayName) params.set("display_name", displayName);
  if (options.filter?.trim()) params.set("filter", options.filter.trim());
  return `pcap://${encodePcapDevice(device)}?${params.toString()}`;
}
