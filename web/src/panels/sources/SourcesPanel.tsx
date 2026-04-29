// Sources panel ? table of currently known sources with start/stop
// controls. Server is the source of truth.
//
// REQ: FR-UI-008

import { For, Show } from "solid-js";
import { sourcesStore, sendCtl, pushToast } from "~/state";
import { t } from "~/i18n";

function onAction(sid: string, action: "start" | "stop" | "remove"): void {
  try {
    sendCtl(sid, action);
    pushToast({ level: "info", message: `${action}: ${sid.slice(0, 8)}` });
  } catch (err) {
    pushToast({
      level: "error",
      message: (err as Error).message ?? "ctl failed",
    });
  }
}

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
              <th>{t("sources.column.actions")}</th>
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
                    {s.lastTsMs ? new Date(s.lastTsMs).toISOString() : "-"}
                  </td>
                  <td>
                    <button
                      type="button"
                      onClick={() => onAction(s.sid, "start")}
                      title={t("sources.action.start")}
                    >
                      ?
                    </button>{" "}
                    <button
                      type="button"
                      onClick={() => onAction(s.sid, "stop")}
                      title={t("sources.action.stop")}
                    >
                      ?
                    </button>{" "}
                    <button
                      type="button"
                      onClick={() => onAction(s.sid, "remove")}
                      title={t("sources.action.remove")}
                    >
                      ?
                    </button>
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
