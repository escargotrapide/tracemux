export type SourceSpec = Record<string, unknown>;

function decodePart(value: string): string {
  try {
    return decodeURIComponent(value.replace(/\+/g, " "));
  } catch {
    return value;
  }
}

function parseQuery(query: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const pair of query.split("&")) {
    if (!pair) continue;
    const [rawKey, rawValue = ""] = pair.split("=");
    if (!rawKey) continue;
    const key = decodePart(rawKey);
    out.set(key, decodePart(rawValue));
  }
  return out;
}

function parseBoolean(query: Map<string, string>, key: string): boolean {
  const value = query.get(key)?.toLowerCase();
  return value === "1" || value === "true" || value === "yes";
}

function parseNumber(
  query: Map<string, string>,
  key: string,
  defaultValue: number,
): number {
  const value = query.get(key);
  if (value === undefined || value.length === 0) return defaultValue;
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0) {
    throw new Error(`query parameter ${key} must be a positive integer`);
  }
  return parsed;
}

function stripLeadingSlash(value: string): string {
  return value.replace(/^\/+/, "");
}

function parseProcessArgv(body: string, query: Map<string, string>): string[] {
  const argv: string[] = [];
  const program = stripLeadingSlash(body);
  if (program) argv.push(decodePart(program));
  const rest = query.get("args");
  if (rest) {
    argv.push(...rest.split(";").filter(Boolean));
  }
  if (argv.length === 0) throw new Error("process spec requires a program path");
  return argv;
}

/** Parse the same URI-style source spec accepted by the CLI subset. */
export function parseSourceSpec(input: string): SourceSpec {
  const trimmed = input.trim();
  if (!trimmed) throw new Error("source spec is required");
  const sep = trimmed.indexOf("://");
  if (sep < 0) throw new Error("missing scheme; expected kind://...");

  const scheme = trimmed.slice(0, sep);
  const rest = trimmed.slice(sep + 3);
  const queryStart = rest.indexOf("?");
  const body = queryStart >= 0 ? rest.slice(0, queryStart) : rest;
  const query = parseQuery(queryStart >= 0 ? rest.slice(queryStart + 1) : "");

  switch (scheme) {
    case "file":
      return {
        kind: "file",
        path: decodePart(stripLeadingSlash(body)),
        follow: parseBoolean(query, "follow"),
      };
    case "tcp":
      return { kind: "tcp", addr: decodePart(body) };
    case "udp":
      return { kind: "udp", bind: decodePart(body) };
    case "pipe":
      return { kind: "pipe", path: decodePart(stripLeadingSlash(body)) };
    case "process":
      return { kind: "process", argv: parseProcessArgv(body, query) };
    case "mock":
      return { kind: "mock", tag: decodePart(body) };
    case "serial":
      return {
        kind: "serial",
        port: decodePart(body),
        baud: parseNumber(query, "baud", 115_200),
        data_bits: parseNumber(query, "data", 8),
        parity: query.get("parity") ?? "none",
        stop_bits: parseNumber(query, "stop", 1),
        flow: query.get("flow") ?? "none",
      };
    default:
      throw new Error(`unsupported source kind: ${scheme}`);
  }
}
