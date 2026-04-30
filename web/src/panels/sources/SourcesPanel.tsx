// Sources panel: table of currently known sources with start/stop
// controls. Server is the source of truth.
//
// REQ: FR-UI-008

import { createSignal, For, Show } from "solid-js";
import { openTerminalChannel, sourcesStore, sendCtl, pushToast } from "~/state";
import {
  BUILTIN_SOURCE_PRESETS,
  deleteUserSourcePreset,
  loadUserSourcePresets,
  saveUserSourcePreset,
} from "~/state/sourcePresets";
import {
  filterAndSortSources,
  type SourceSortKey,
  type SourceStatusFilter,
} from "~/state/sourceFilters";
import { parseSourceSpec } from "~/state/sourceSpec";
import { t } from "~/i18n";

function onAction(sid: string, action: "stop" | "restart" | "remove"): void {
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

function statusLabel(status: string): string {
  return t(`sources.status.${status}`);
}

function onOpenTerminal(sid: string, channels: number[]): void {
  const ch = channels[0] ?? 0;
  openTerminalChannel(sid, ch);
  pushToast({ level: "info", message: t("sources.open_terminal.requested") });
}

export function SourcesPanel() {
  const [specInput, setSpecInput] = createSignal("mock://demo");
  const [presetName, setPresetName] = createSignal("");
  const [userPresets, setUserPresets] = createSignal(loadUserSourcePresets());
  const [query, setQuery] = createSignal("");
  const [statusFilter, setStatusFilter] = createSignal<SourceStatusFilter>("all");
  const [sortKey, setSortKey] = createSignal<SourceSortKey>("name");
  const [selectedSid, setSelectedSid] = createSignal<string | null>(null);
  const rows = () =>
    filterAndSortSources(
      Object.values(sourcesStore),
      query(),
      statusFilter(),
      sortKey(),
    );
  const selectedSource = () => {
    const sid = selectedSid();
    return sid ? sourcesStore[sid] : undefined;
  };

  function onStart(ev: SubmitEvent): void {
    ev.preventDefault();
    try {
      const spec = parseSourceSpec(specInput());
      sendCtl(undefined, "start", spec);
      pushToast({ level: "info", message: t("sources.start.requested") });
    } catch (err) {
      pushToast({
        level: "error",
        message: (err as Error).message ?? t("sources.start.invalid"),
      });
    }
  }

  function onSavePreset(): void {
    try {
      const next = saveUserSourcePreset(presetName(), specInput());
      setUserPresets(next);
      pushToast({ level: "info", message: t("sources.preset.saved") });
    } catch (err) {
      pushToast({
        level: "error",
        message: (err as Error).message ?? t("sources.preset.invalid"),
      });
    }
  }

  function onDeletePreset(): void {
    const next = deleteUserSourcePreset(presetName());
    setUserPresets(next);
    pushToast({ level: "info", message: t("sources.preset.deleted") });
  }

  return (
    <div style={{ padding: "8px", height: "100%", "overflow-y": "auto" }}>
      <form
        onSubmit={onStart}
        style={{
          display: "flex",
          "align-items": "center",
          gap: "8px",
          "margin-bottom": "8px",
        }}
      >
        <input
          type="text"
          value={specInput()}
          onInput={(ev) => setSpecInput(ev.currentTarget.value)}
          placeholder={t("sources.spec.placeholder")}
          aria-label={t("sources.spec.label")}
          style={{ flex: 1, "min-width": "220px" }}
        />
        <button type="submit" disabled={specInput().trim().length === 0}>
          {t("action.add_source")}
        </button>
        <span style={{ color: "var(--wl-fg-muted)" }}>
          {t("sources.spec.help")}
        </span>
      </form>
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: "8px",
          "margin-bottom": "12px",
          "flex-wrap": "wrap",
        }}
      >
        <select
          aria-label={t("sources.preset.label")}
          onChange={(ev) => {
            if (ev.currentTarget.value) setSpecInput(ev.currentTarget.value);
          }}
          style={{ "min-width": "220px" }}
        >
          <option value="">{t("sources.preset.placeholder")}</option>
          <optgroup label={t("sources.preset.builtin")}>
            <For each={BUILTIN_SOURCE_PRESETS}>
              {(p) => <option value={p.spec}>{p.name}</option>}
            </For>
          </optgroup>
          <Show when={userPresets().length > 0}>
            <optgroup label={t("sources.preset.user")}>
              <For each={userPresets()}>
                {(p) => <option value={p.spec}>{p.name}</option>}
              </For>
            </optgroup>
          </Show>
        </select>
        <input
          type="text"
          value={presetName()}
          onInput={(ev) => setPresetName(ev.currentTarget.value)}
          placeholder={t("sources.preset.name_placeholder")}
          aria-label={t("sources.preset.name_label")}
          style={{ width: "160px" }}
        />
        <button type="button" onClick={onSavePreset} disabled={!presetName().trim()}>
          {t("sources.preset.save")}
        </button>
        <button type="button" onClick={onDeletePreset} disabled={!presetName().trim()}>
          {t("sources.preset.delete")}
        </button>
        <span style={{ color: "var(--wl-fg-muted)" }}>{t("sources.preset.help")}</span>
      </div>
      <div
        style={{
          display: "flex",
          "align-items": "center",
          gap: "8px",
          "margin-bottom": "12px",
          "flex-wrap": "wrap",
        }}
      >
        <input
          type="search"
          value={query()}
          onInput={(ev) => setQuery(ev.currentTarget.value)}
          placeholder={t("sources.filter.search_placeholder")}
          aria-label={t("sources.filter.search_label")}
          style={{ "min-width": "220px" }}
        />
        <label>
          {t("sources.filter.status_label")}{" "}
          <select
            value={statusFilter()}
            onChange={(ev) => setStatusFilter(ev.currentTarget.value as SourceStatusFilter)}
          >
            <option value="all">{t("sources.filter.status_all")}</option>
            <option value="running">{t("sources.status.running")}</option>
            <option value="stopped">{t("sources.status.stopped")}</option>
            <option value="unknown">{t("sources.status.unknown")}</option>
          </select>
        </label>
        <label>
          {t("sources.filter.sort_label")}{" "}
          <select
            value={sortKey()}
            onChange={(ev) => setSortKey(ev.currentTarget.value as SourceSortKey)}
          >
            <option value="name">{t("sources.filter.sort_name")}</option>
            <option value="kind">{t("sources.filter.sort_kind")}</option>
            <option value="status">{t("sources.filter.sort_status")}</option>
            <option value="bytes">{t("sources.filter.sort_bytes")}</option>
          </select>
        </label>
      </div>
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
              <th>{t("sources.column.status")}</th>
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
                  <td>
                    <span
                      style={{
                        color:
                          s.status === "running"
                            ? "var(--wl-accent)"
                            : "var(--wl-fg-muted)",
                      }}
                    >
                      {statusLabel(s.status)}
                    </span>
                  </td>
                  <td>{s.channels.join(", ")}</td>
                  <td>{s.bytesIn}</td>
                  <td>
                    {s.lastTsMs ? new Date(s.lastTsMs).toISOString() : "-"}
                  </td>
                  <td>
                    <button
                      type="button"
                      onClick={() => setSelectedSid(s.sid)}
                      title={t("sources.action.details")}
                    >
                      {t("sources.action.details")}
                    </button>{" "}
                    <button
                      type="button"
                      onClick={() => onOpenTerminal(s.sid, s.channels)}
                      title={t("sources.action.open_terminal")}
                    >
                      {t("sources.action.open_terminal")}
                    </button>{" "}
                    <button
                      type="button"
                      onClick={() => onAction(s.sid, "restart")}
                      title={t("sources.action.restart")}
                    >
                      {t("sources.action.restart")}
                    </button>{" "}
                    <button
                      type="button"
                      onClick={() => onAction(s.sid, "stop")}
                      title={t("sources.action.stop")}
                      disabled={s.status === "stopped"}
                    >
                      {t("sources.action.stop")}
                    </button>{" "}
                    <button
                      type="button"
                      onClick={() => onAction(s.sid, "remove")}
                      title={t("sources.action.remove")}
                    >
                      {t("sources.action.remove")}
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </Show>
      <Show when={selectedSource()}>
        {(source) => (
          <aside
            style={{
              margin: "12px 0 0",
              padding: "10px",
              border: "1px solid var(--wl-border)",
              "border-radius": "8px",
              background: "var(--wl-bg-elev)",
            }}
          >
            <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
              <strong>{t("sources.detail.title")}</strong>
              <button
                type="button"
                onClick={() => setSelectedSid(null)}
                style={{ "margin-left": "auto" }}
              >
                {t("sources.detail.close")}
              </button>
            </div>
            <dl
              style={{
                display: "grid",
                "grid-template-columns": "max-content 1fr",
                gap: "6px 10px",
                margin: "10px 0 0",
              }}
            >
              <dt>{t("sources.detail.sid")}</dt>
              <dd><code>{source().sid}</code></dd>
              <dt>{t("sources.detail.name")}</dt>
              <dd>{source().name}</dd>
              <dt>{t("sources.detail.kind")}</dt>
              <dd>{source().kind}</dd>
              <dt>{t("sources.detail.status")}</dt>
              <dd>{statusLabel(source().status)}</dd>
              <dt>{t("sources.detail.channels")}</dt>
              <dd>{source().channels.join(", ") || "-"}</dd>
              <dt>{t("sources.detail.bytes")}</dt>
              <dd>{source().bytesIn}</dd>
              <dt>{t("sources.detail.last")}</dt>
              <dd>{source().lastTsMs ? new Date(source().lastTsMs).toISOString() : "-"}</dd>
            </dl>
          </aside>
        )}
      </Show>
    </div>
  );
}
