import { describe, it, expect } from "vitest";
import { inferMetricUnit, formatMetricValue } from "~/panels/metrics/formatMetric";

describe("inferMetricUnit", () => {
  it("detects bytes", () => {
    expect(inferMetricUnit("ingest.bytes_total")).toBe("bytes");
    expect(inferMetricUnit("ring.nbytes")).toBe("bytes");
    expect(inferMetricUnit("bytes_written")).toBe("bytes");
  });

  it("detects durations", () => {
    expect(inferMetricUnit("pipeline.lag_ms")).toBe("duration_ms");
    expect(inferMetricUnit("commit.latency")).toBe("duration_ms");
    expect(inferMetricUnit("clock.offset_ns")).toBe("duration_ns");
  });

  it("detects rates", () => {
    expect(inferMetricUnit("frames_per_sec")).toBe("rate");
    expect(inferMetricUnit("throughput_bps")).toBe("rate");
  });

  it("detects ratios", () => {
    expect(inferMetricUnit("buffer.fill_ratio")).toBe("ratio");
    expect(inferMetricUnit("drop_pct")).toBe("ratio");
  });

  it("detects counts", () => {
    expect(inferMetricUnit("framesTotal")).toBe("count");
    expect(inferMetricUnit("errors")).toBe("count");
  });

  it("falls back to none for unknown keys", () => {
    expect(inferMetricUnit("boot_id")).toBe("none");
  });
});

describe("formatMetricValue", () => {
  it("formats bytes with IEC units", () => {
    expect(formatMetricValue("bytes_total", 0)).toBe("0 B");
    expect(formatMetricValue("bytes_total", 1024)).toBe("1.00 KiB");
    expect(formatMetricValue("bytes_total", 1_572_864)).toBe("1.50 MiB");
  });

  it("formats durations in ms/s/min", () => {
    expect(formatMetricValue("lag_ms", 0)).toBe("0 ms");
    expect(formatMetricValue("lag_ms", 250)).toBe("250 ms");
    expect(formatMetricValue("lag_ms", 1500)).toBe("1.5 s");
    expect(formatMetricValue("lag_ms", 120000)).toBe("2 min");
  });

  it("converts nanoseconds to ms", () => {
    expect(formatMetricValue("offset_ns", 2_000_000)).toBe("2 ms");
  });

  it("formats rates with /s suffix", () => {
    expect(formatMetricValue("frames_per_sec", 1234)).toBe("1,234 /s");
  });

  it("formats ratios in [0,1] as percentages", () => {
    expect(formatMetricValue("fill_ratio", 0.5)).toBe("50 %");
    expect(formatMetricValue("fill_ratio", 0.125)).toBe("12.5 %");
  });

  it("groups thousands for counts", () => {
    expect(formatMetricValue("framesTotal", 1234567)).toBe("1,234,567");
  });

  it("passes non-numeric values through unchanged", () => {
    expect(formatMetricValue("boot_id", "abc-123")).toBe("abc-123");
    expect(formatMetricValue("node_id", null)).toBe("null");
  });
});
