// Sources panel ? table of currently known sources.

import { For, Show } from "solid-js";
import { sourcesStore } from "~/state";
import { t } from "~/i18n";

export function SourcesPanel() {
  const rows = () => Object.values(sourcesStore);

  return (
    <div style={{ padding: "8px", height: "100%", "overflow-y": "auto" }}>
      <Show
        when={rows().length > 0}
        fallback={
          <div style={{ color: "var(--wl-fg-muted)" }}>
            {t("sources.empty")}
          </div>
        }
      >
        <table style={{ width: "100%", "border-collapse": "collapse" }}>
          <thead>
            <tr style={{ "text-align": "left", color: "var(--wl-fg-muted)" }}>
              <th>{t("sources.column.name")}</th>
              <th>{t("sources.column.kind")}</th>
              <th>{t("sources.column.channels")}</th>
              <th>{t("sources.column.bytes")}</th>
              <th>{t("sources.column.last")}</th>
            </tr>
          </thead>
          <tbody>
            <For each={rows()}>
              {(s) => (
                <tr style={{ "border-top": "1px solid var(--wl-border)" }}>
                  <td>{s.name}</td>
                  <td>{s.kind}</td>
                  <td>{s.channels.join(", ")}</td>
                  <td>{s.bytesIn}</td>
                  <td>
                    {s.lastTsMs
                      ? new Date(s.lastTsMs).toISOString()
                      : "-"}
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </Show>
    </div>
  );
}
