// Metrics panel ? placeholder showing connection state. Real metrics
// will be driven by the `metrics` wire frame.

import { connState } from "~/state";
import { t } from "~/i18n";

export function MetricsPanel() {
  return (
    <div style={{ padding: "8px" }}>
      <pre style={{ margin: 0, "white-space": "pre-wrap" }}>
        {JSON.stringify(connState(), null, 2)}
      </pre>
      <p style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
        {t("panel.metrics")}
      </p>
    </div>
  );
}
