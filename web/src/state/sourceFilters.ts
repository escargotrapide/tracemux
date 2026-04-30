import type { SourceInfo } from "~/state";

export type SourceStatusFilter = "all" | SourceInfo["status"];
export type SourceSortKey = "name" | "kind" | "status" | "bytes";

function normalized(value: string): string {
  return value.trim().toLowerCase();
}

function matchesQuery(source: SourceInfo, query: string): boolean {
  const q = normalized(query);
  if (!q) return true;
  return [
    source.sid,
    source.name,
    source.kind,
    source.status,
    source.channels.join(","),
  ]
    .map((part) => part.toLowerCase())
    .some((part) => part.includes(q));
}

function compareSource(a: SourceInfo, b: SourceInfo, sortKey: SourceSortKey): number {
  switch (sortKey) {
    case "bytes":
      return b.bytesIn - a.bytesIn || a.name.localeCompare(b.name) || a.sid.localeCompare(b.sid);
    case "kind":
      return a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name) || a.sid.localeCompare(b.sid);
    case "status":
      return a.status.localeCompare(b.status) || a.name.localeCompare(b.name) || a.sid.localeCompare(b.sid);
    case "name":
    default:
      return a.name.localeCompare(b.name) || a.sid.localeCompare(b.sid);
  }
}

export function filterAndSortSources(
  sources: SourceInfo[],
  query: string,
  status: SourceStatusFilter,
  sortKey: SourceSortKey,
): SourceInfo[] {
  return sources
    .filter((source) => status === "all" || source.status === status)
    .filter((source) => matchesQuery(source, query))
    .sort((a, b) => compareSource(a, b, sortKey));
}
