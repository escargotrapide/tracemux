// Human-friendly formatting for metric rows.
//
// Metric values arrive as raw numbers on flattened dotted keys (e.g.
// `ingest.bytes_total`, `pipeline.lag_ms`). We infer a unit from the key
// suffix and render the value with thousands separators plus a unit, so an
// operator can read a row without guessing whether `1500000` is bytes,
// nanoseconds, or a frame count.
//
// REQ: FR-UI-007

export type MetricUnit = "bytes" | "duration_ms" | "duration_ns" | "rate" | "ratio" | "count" | "none";

const BYTE_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"] as const;

/** Infer a unit from the (lowercased) metric key. Order matters: more specific suffixes win. */
export function inferMetricUnit(key: string): MetricUnit {
  const k = key.toLowerCase();
  if (/(^|[._])(bytes|byte_count|nbytes)([._]|$)|_bytes$|bytes_/.test(k)) return "bytes";
  if (/_ns$|_nanos$|nanos$/.test(k)) return "duration_ns";
  if (/_ms$|_millis$|latency|lag/.test(k)) return "duration_ms";
  if (/per_sec|_per_s$|_rate$|hz$|bps$|fps$/.test(k)) return "rate";
  if (/ratio$|_pct$|percent|fraction/.test(k)) return "ratio";
  if (/_total$|count$|_n$|frames|drops|errors|opens|closes|requests/.test(k)) return "count";
  return "none";
}

function groupThousands(n: number): string {
  if (!Number.isFinite(n)) return String(n);
  // Keep up to 3 fractional digits, then trim trailing zeros.
  const fixed = Number.isInteger(n) ? String(n) : n.toFixed(3).replace(/\.?0+$/, "");
  const [intPart = "0", fracPart] = fixed.split(".");
  const sign = intPart.startsWith("-") ? "-" : "";
  const digits = sign ? intPart.slice(1) : intPart;
  const grouped = digits.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return fracPart ? `${sign}${grouped}.${fracPart}` : `${sign}${grouped}`;
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes)) return String(bytes);
  const neg = bytes < 0;
  let v = Math.abs(bytes);
  let i = 0;
  while (v >= 1024 && i < BYTE_UNITS.length - 1) {
    v /= 1024;
    i += 1;
  }
  const value = i === 0 ? String(Math.round(v)) : v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2);
  return `${neg ? "-" : ""}${value} ${BYTE_UNITS[i]}`;
}

function formatDurationMs(ms: number): string {
  if (!Number.isFinite(ms)) return String(ms);
  if (ms === 0) return "0 ms";
  const abs = Math.abs(ms);
  if (abs < 1) return `${groupThousands(ms)} ms`;
  if (abs < 1000) return `${groupThousands(ms)} ms`;
  if (abs < 60_000) return `${groupThousands(ms / 1000)} s`;
  return `${groupThousands(ms / 60_000)} min`;
}

/**
 * Format a single metric value for display. Non-numeric values (strings,
 * already-stringified arrays/objects) are returned unchanged so we never lose
 * information we cannot interpret.
 */
export function formatMetricValue(key: string, raw: unknown): string {
  if (typeof raw !== "number" || !Number.isFinite(raw)) return String(raw);
  switch (inferMetricUnit(key)) {
    case "bytes":
      return formatBytes(raw);
    case "duration_ns":
      return formatDurationMs(raw / 1_000_000);
    case "duration_ms":
      return formatDurationMs(raw);
    case "rate":
      return `${groupThousands(raw)} /s`;
    case "ratio": {
      // Values in [0,1] render as a percentage; anything else stays as-is.
      if (raw >= 0 && raw <= 1) return `${groupThousands(raw * 100)} %`;
      return groupThousands(raw);
    }
    case "count":
    case "none":
    default:
      return groupThousands(raw);
  }
}
