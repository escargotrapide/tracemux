// Metrics panel: shows the latest `metrics` wire frame plus the
// connection state. The server is the source of truth.
//
// REQ: FR-UI-007

import { Show, For } from "solid-js";
import { connState, metricsState, uiPerfState } from "~/state";
import { t } from "~/i18n";
import { formatMetricValue } from "./formatMetric";

function flatten(o: unknown, prefix = ""): Array<[string, unknown]> {
  if (o === null || o === undefined) return [];
  if (typeof o !== "object") {
    return [[prefix || "value", o]];
  }
  const out: Array<[string, unknown]> = [];
  for (const [k, v] of Object.entries(o as Record<string, unknown>)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      out.push(...flatten(v, key));
    } else {
      out.push([key, Array.isArray(v) ? JSON.stringify(v) : v]);
    }
  }
  return out;
}

function MetricsTable(props: { rows: () => Array<[string, unknown]> }) {
  return (
    <table style={{ width: "100%", "border-collapse": "collapse" }}>
      <thead>
        <tr style={{ "text-align": "left", color: "var(--wl-fg-muted)" }}>
          <th scope="col">{t("metrics.column.metric")}</th>
          <th scope="col">{t("metrics.column.value")}</th>
        </tr>
      </thead>
      <tbody>
        <For each={props.rows()}>
          {([k, v]) => (
            <tr style={{ "border-top": "1px solid var(--wl-border)" }}>
              <th
                scope="row"
                style={{ "font-family": "monospace", "font-weight": "normal", "text-align": "left" }}
              >
                {k}
              </th>
              <td style={{ "font-family": "monospace" }}>{formatMetricValue(k, v)}</td>
            </tr>
          )}
        </For>
      </tbody>
    </table>
  );
}

export function MetricsPanel() {
  const rows = () => flatten(metricsState());
  const localRows = () => flatten(uiPerfState(), "ui");

  return (
    <div style={{ padding: "8px", height: "100%", "overflow-y": "auto" }}>
      <p style={{ margin: "0 0 8px 0", color: "var(--wl-fg-muted)" }}>
        {t("metrics.connection")}: {connState().status}
      </p>
      <p
        class="wl-metrics-legend"
        style={{ margin: "0 0 8px 0", color: "var(--wl-fg-muted)", "font-size": "0.85em" }}
      >
        {t("metrics.legend")}
      </p>
      <Show
        when={rows().length > 0}
        fallback={
          <div style={{ color: "var(--wl-fg-muted)" }}>
            {t("metrics.empty")}
          </div>
        }
      >
        <MetricsTable rows={rows} />
      </Show>
      <h3 style={{ "margin-top": "12px" }}>{t("metrics.local_ui")}</h3>
      <MetricsTable rows={localRows} />
    </div>
  );
}
