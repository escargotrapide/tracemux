// Sources panel: table of currently known sources with start/stop
// controls. Server is the source of truth.
//
// REQ: FR-UI-008
// REQ: FR-UI-014
// REQ: FR-UI-018

import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import {
  downloadSessionExport,
  type SessionExportFormat,
} from "~/adapters/sessionExport";
import {
  downloadSessionExportZip,
  type SessionExportZipEntry,
} from "~/adapters/sessionExportZip";
import {
  openNewTerminalChannel,
  openTerminalChannel,
  connState,
  sourcesStore,
  sourceErrorHistoryStore,
  sendCtl,
  pushToast,
  type SourceInfo,
} from "~/state";
import {
  BUILTIN_SOURCE_PRESETS,
  deleteUserSourcePreset,
  isValidPresetName,
  loadUserSourcePresets,
  saveUserSourcePreset,
} from "~/state/sourcePresets";
import {
  filterAndSortSources,
  type SourceSortKey,
  type SourceStatusFilter,
} from "~/state/sourceFilters";
import {
  detectSources,
  pcapSpecForInterface,
  serialSpecForPort,
  type PcapInterfaceInfo,
  type PcapPublishMode,
} from "~/state/sourceDiscovery";
import { MAX_SOURCE_ALIAS_LENGTH, sourceAliases, updateSourceAlias } from "~/state/sourceAliases";
import {
  channelEncodingKey,
  encodingForChannel,
  sourceEncodings,
  sourceEncodingKey,
  updateChannelEncoding,
  updateSourceEncoding,
} from "~/state/sourceEncodings";
import { exportSettings, updateExportSettings } from "~/state/exportSettings";
import {
  loadAndApplySourceAnnotations,
  syncSourceNoteToServer,
} from "~/state/annotationSync";
import { MAX_SOURCE_NOTE_LENGTH, sourceNotes, updateSourceNote } from "~/state/sourceNotes";
import {
  normalizeEncoding,
  sourceStartOptions,
  startCtlOptions,
  SUPPORTED_SOURCE_ENCODINGS,
  updateSourceStartOptions,
} from "~/state/sourceStartOptions";
import { parseSourceSpec } from "~/state/sourceSpec";
import { t } from "~/i18n";

function onAction(sid: string, action: "stop" | "restart" | "remove"): void {
  if (action === "remove" && !window.confirm(t("sources.action.remove_confirm"))) return;
  try {
    const sent = sendCtl(sid, action);
    pushToast({
      level: sent ? "info" : "error",
      message: sent ? `${action}: ${sid.slice(0, 8)}` : t("sources.action.send_failed"),
    });
  } catch (err) {
    pushToast({
      level: "error",
      message: (err as Error).message ?? "ctl failed",
    });
  }
}

function onRestartWithServerEncoding(sid: string): void {
  const encoding = sourceEncodings[sourceEncodingKey(sid)]?.encoding
    ?? sourceTextEncodingFallback(sid);
  try {
    const sent = sendCtl(sid, "restart", undefined, {
      ...(startCtlOptions() as Record<string, unknown>),
      encoding,
    });
    pushToast({
      level: sent ? "info" : "error",
      message: sent
        ? t("sources.detail.server_encoding_requested")
        : t("sources.action.send_failed"),
    });
  } catch (err) {
    pushToast({
      level: "error",
      message: (err as Error).message ?? "ctl failed",
    });
  }
}

function onRestartWithSuggestedEncoding(sid: string, encoding: string): void {
  try {
    updateSourceEncoding(sid, encoding);
    const sent = sendCtl(sid, "restart", undefined, {
      ...(startCtlOptions() as Record<string, unknown>),
      detection_mode: "configured",
      encoding,
    });
    pushToast({
      level: sent ? "info" : "error",
      message: sent
        ? t("sources.detail.detected_encoding_requested")
        : t("sources.action.send_failed"),
    });
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

// Per-source channel chosen for the next "open terminal" action. Defaults to
// the source's first channel; only surfaced in the UI when a source exposes
// more than one channel so single-channel sources stay one-click.
const [openChannelSelection, setOpenChannelSelection] = createSignal<Record<string, number>>({});

function selectedOpenChannel(sid: string, channels: number[]): number {
  const chosen = openChannelSelection()[sid];
  if (chosen !== undefined && channels.includes(chosen)) return chosen;
  return channels[0] ?? 0;
}

function setSelectedOpenChannel(sid: string, ch: number): void {
  setOpenChannelSelection((prev) => ({ ...prev, [sid]: ch }));
}

function onOpenTerminal(sid: string, ch: number): void {
  openTerminalChannel(sid, ch);
  pushToast({
    level: "info",
    message: `${t("sources.open_terminal.requested")} (ch ${ch})`,
  });
}

function onOpenNewTerminal(sid: string, ch: number): void {
  openNewTerminalChannel(sid, ch);
  pushToast({
    level: "info",
    message: `${t("sources.open_terminal.new_requested")} (ch ${ch})`,
  });
}

function sourceTextEncodingFallback(sid: string): string {
  return sourcesStore[sid]?.encoding ?? sourceStartOptions.encoding;
}

function updateSourceDisplayEncoding(sid: string, encoding: string): void {
  updateSourceEncoding(sid, encoding, undefined, Date.now(), sourceTextEncodingFallback(sid));
}

function updateChannelDisplayEncoding(sid: string, ch: number, encoding: string): void {
  const inheritedEncoding = sourceEncodings[sourceEncodingKey(sid)]?.encoding
    ?? sourceTextEncodingFallback(sid);
  updateChannelEncoding(sid, ch, encoding, undefined, Date.now(), inheritedEncoding);
}

function encodingOptions(currentEncoding: string): string[] {
  const options = [...SUPPORTED_SOURCE_ENCODINGS] as string[];
  const current = normalizeEncoding(currentEncoding);
  return options.includes(current) ? options : [current, ...options];
}

function sourceDisplayEncoding(sid: string): string {
  return sourceEncodings[sourceEncodingKey(sid)]?.encoding ?? sourceTextEncodingFallback(sid);
}

function channelDisplayEncoding(sid: string, ch: number): string {
  return encodingForChannel(sid, ch, sourceTextEncodingFallback(sid));
}

function suggestedEncodingForSource(source: SourceInfo): string | null {
  const candidate = source.detection?.encoding_candidates?.[0];
  if (!candidate || candidate.confidence < 80) return null;
  if (candidate.label === source.encoding) return null;
  return candidate.label;
}

const EXPORT_FORMATS: SessionExportFormat[] = ["text", "csv", "jsonl", "pcapng"];
const PCAP_PUBLISH_MODES: PcapPublishMode[] = ["stats-only", "sampled", "full"];
type AnnotationSyncStatus = "idle" | "loading" | "syncing" | "synced" | "error";
type SerialOpenResultStatus = "requested" | "failed";
type SerialOpenResult = {
  port: string;
  status: SerialOpenResultStatus;
  detail: string;
};

function annotationSyncLabel(status: AnnotationSyncStatus): string {
  return t(`annotations.sync.${status}`);
}

function pcapPublishModeLabel(mode: PcapPublishMode): string {
  return t(`sources.pcap.publish_mode.${mode}`);
}

function pcapPublishModeHint(mode: PcapPublishMode): string {
  return t(`sources.pcap.publish_hint.${mode}`);
}

function serialOpenStatusLabel(status: SerialOpenResultStatus): string {
  return t(`sources.serial.open_result.${status}`);
}

function isAbortError(err: unknown): boolean {
  return err instanceof Error && err.name === "AbortError";
}

export function SourcesPanel() {
  const [specInput, setSpecInput] = createSignal("mock://demo");
  const [presetName, setPresetName] = createSignal("");
  const [userPresets, setUserPresets] = createSignal(loadUserSourcePresets());
  const [query, setQuery] = createSignal("");
  const [statusFilter, setStatusFilter] = createSignal<SourceStatusFilter>("all");
  const [sortKey, setSortKey] = createSignal<SourceSortKey>("name");
  const [selectedSid, setSelectedSid] = createSignal<string | null>(null);
  const [serialCandidates, setSerialCandidates] = createSignal<string[]>([]);
  const [selectedSerialPorts, setSelectedSerialPorts] = createSignal<string[]>([]);
  const [serialOpenResults, setSerialOpenResults] = createSignal<SerialOpenResult[]>([]);
  const [serialDetecting, setSerialDetecting] = createSignal(false);
  const [serialBaud, setSerialBaud] = createSignal(115_200);
  const [pcapInterfaces, setPcapInterfaces] = createSignal<PcapInterfaceInfo[]>([]);
  const [selectedPcapDevice, setSelectedPcapDevice] = createSignal("");
  const [pcapSnaplen, setPcapSnaplen] = createSignal(65_535);
  const [pcapPromiscuous, setPcapPromiscuous] = createSignal(false);
  const [pcapFilter, setPcapFilter] = createSignal("");
  const [pcapPublishMode, setPcapPublishMode] = createSignal<PcapPublishMode>("stats-only");
  const [exportTimezone, setExportTimezone] = createSignal("");
  const [exporting, setExporting] = createSignal<string | null>(null);
  const [bulkExporting, setBulkExporting] = createSignal<SessionExportFormat | null>(null);
  const [bulkProgress, setBulkProgress] = createSignal<{ completed: number; total: number } | null>(null);
  const [bulkExportAbort, setBulkExportAbort] = createSignal<AbortController | null>(null);
  const [bulkExportCancelling, setBulkExportCancelling] = createSignal(false);
  const [pendingStarts, setPendingStarts] = createSignal<Array<{ id: number; label: string }>>([]);
  const [loadedNotesSid, setLoadedNotesSid] = createSignal<string | null>(null);
  const [sourceNoteSyncStatus, setSourceNoteSyncStatus] = createSignal<Record<string, AnnotationSyncStatus>>({});
  let pendingStartSeq = 1;
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
  const selectedSourceErrors = () => {
    const sid = selectedSid();
    return sid ? sourceErrorHistoryStore[sid] ?? [] : [];
  };
  const persistentSources = createMemo(() => Object.values(sourcesStore).filter((source) => source.persistent));
  const exportableSources = (format: SessionExportFormat) => {
    const sources = persistentSources();
    return format === "pcapng" ? sources.filter((source) => source.kind === "pcap") : sources;
  };
  const bulkProgressLabel = createMemo(() => {
    const progress = bulkProgress();
    if (!progress) return "";
    return `${progress.completed}/${progress.total}`;
  });
  const serialDetectProgressLabel = createMemo(() => (
    serialDetecting() ? t("sources.serial.detect_progress") : ""
  ));
  const bulkExportProgressLabel = createMemo(() => {
    const format = bulkExporting();
    if (!format) return "";
    if (bulkExportCancelling()) return t("sources.export_all.cancelling");
    return `${t("sources.export_all.progress")} ${format.toUpperCase()} ${bulkProgressLabel()}`.trim();
  });
  const singleExportProgressLabel = createMemo(() => {
    const format = exporting();
    if (!format) return "";
    return `${t("sources.export.progress")} ${format.toUpperCase()}`;
  });
  const presetNameInvalid = createMemo(() => {
    const name = presetName().trim();
    return name.length > 0 && !isValidPresetName(name);
  });

  function queuePendingStart(label: string): void {
    const id = pendingStartSeq++;
    setPendingStarts((prev) => [...prev.filter((item) => item.label !== label), { id, label }].slice(-8));
    window.setTimeout(() => {
      setPendingStarts((prev) => prev.filter((item) => item.id !== id));
    }, 8_000);
  }

  createEffect(() => {
    const sid = selectedSid();
    if (!sid || loadedNotesSid() === sid) return;
    if (connState().status !== "open") return;
    setLoadedNotesSid(sid);
    setSourceNoteStatus(sid, "loading");
    void loadAndApplySourceAnnotations(sid)
      .then(() => setSourceNoteStatus(sid, "synced"))
      .catch((err) => {
        setSourceNoteStatus(sid, "error");
        console.warn("E-UI-ANNOTATION-SYNC load source notes failed", err);
        pushToast({ level: "warn", message: t("sources.detail.notes_sync_failed") });
      });
  });

  function onStart(ev: SubmitEvent): void {
    ev.preventDefault();
    try {
      const spec = parseSourceSpec(specInput());
      const sent = sendCtl(undefined, "start", spec, startCtlOptions() as Record<string, unknown>);
      if (sent) queuePendingStart(specInput().trim());
      pushToast({
        level: sent ? "info" : "error",
        message: sent ? t("sources.start.requested") : t("sources.action.send_failed"),
      });
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
    if (!window.confirm(t("sources.preset.delete_confirm"))) return;
    const next = deleteUserSourcePreset(presetName());
    setUserPresets(next);
    pushToast({ level: "info", message: t("sources.preset.deleted") });
  }

  async function onDetectSerial(): Promise<void> {
    setSerialDetecting(true);
    try {
      const report = await detectSources();
      setSerialCandidates(report.serial_candidates);
      setSelectedSerialPorts(report.serial_candidates);
      setSerialOpenResults([]);
      setPcapInterfaces(report.pcap_interfaces);
      setSelectedPcapDevice((prev) => {
        if (report.pcap_interfaces.some((iface) => iface.device === prev)) return prev;
        return report.pcap_interfaces[0]?.device ?? "";
      });
      pushToast({
        level: report.serial_candidates.length > 0 ? "info" : "warn",
        message:
          report.serial_candidates.length > 0
            ? t("sources.serial.detected")
            : t("sources.serial.none"),
      });
    } catch (err) {
      pushToast({
        level: "error",
        message: (err as Error).message ?? t("sources.serial.detect_failed"),
      });
    } finally {
      setSerialDetecting(false);
    }
  }


  function selectedPcapInterface(): PcapInterfaceInfo | undefined {
    const device = selectedPcapDevice();
    return pcapInterfaces().find((iface) => iface.device === device);
  }

  function onUseSelectedPcap(): void {
    const iface = selectedPcapInterface();
    if (!iface) {
      pushToast({ level: "warn", message: t("sources.pcap.select_required") });
      return;
    }
    setSpecInput(pcapSpecForInterface(iface, {
      snaplen: pcapSnaplen(),
      promiscuous: pcapPromiscuous(),
      filter: pcapFilter(),
      publishMode: pcapPublishMode(),
    }));
    pushToast({ level: "info", message: t("sources.pcap.spec_applied") });
  }
  function toggleSerialPort(port: string, checked: boolean): void {
    setSelectedSerialPorts((prev) => {
      if (checked) return [...new Set([...prev, port])].sort();
      return prev.filter((item) => item !== port);
    });
  }

  function onOpenSelectedSerial(): void {
    const ports = selectedSerialPorts();
    if (ports.length === 0) {
      pushToast({ level: "warn", message: t("sources.serial.select_required") });
      return;
    }
    let requested = 0;
    const results: SerialOpenResult[] = [];
    for (const port of ports) {
      try {
        const spec = parseSourceSpec(serialSpecForPort(port, { baud: serialBaud() }));
        if (sendCtl(undefined, "start", spec, startCtlOptions() as Record<string, unknown>)) {
          requested += 1;
          results.push({
            port,
            status: "requested",
            detail: t("sources.serial.open_result.start_sent"),
          });
          queuePendingStart(`serial ${port}`);
        } else {
          const detail = t("sources.action.send_failed");
          results.push({ port, status: "failed", detail });
          pushToast({ level: "error", message: `${port}: ${detail}` });
        }
      } catch (err) {
        const detail = (err as Error).message ?? t("sources.start.invalid");
        results.push({ port, status: "failed", detail });
        pushToast({
          level: "error",
          message: `${port}: ${detail}`,
        });
      }
    }
    setSerialOpenResults(results);
    if (requested > 0) {
      pushToast({
        level: "info",
        message: `${t("sources.serial.open_requested")} (${requested}/${ports.length})`,
      });
    }
  }

  async function onDownloadExport(sid: string, format: SessionExportFormat): Promise<void> {
    setExporting(format);
    try {
      const source = sourcesStore[sid];
      const encoding = sourceEncodings[sourceEncodingKey(sid)]?.encoding;
      await downloadSessionExport(sid, {
        format,
        timezone: exportTimezone(),
        filenamePattern: exportSettings.filenamePattern,
        sourceName: sourceAliases[sid]?.label ?? source?.name ?? sid,
        ...(encoding !== undefined ? { encoding } : {}),
      });
      pushToast({ level: "info", message: t("sources.export.requested") });
    } catch (err) {
      pushToast({
        level: "error",
        message: (err as Error).message ?? t("sources.export.failed"),
      });
    } finally {
      setExporting(null);
    }
  }

  async function onDownloadAllExports(format: SessionExportFormat): Promise<void> {
    const sources = exportableSources(format);
    if (sources.length === 0) {
      pushToast({
        level: "warn",
        message: t(
          format === "pcapng"
            ? "sources.export_all.pcapng_unavailable"
            : "sources.export_all.unavailable",
        ),
      });
      return;
    }
    const entries: SessionExportZipEntry[] = sources.map((source) => ({
      sid: source.sid,
      sourceName: sourceAliases[source.sid]?.label ?? source.name ?? source.sid,
      encoding: sourceEncodings[sourceEncodingKey(source.sid)]?.encoding,
    }));
    const abortController = new AbortController();
    setBulkExportAbort(abortController);
    setBulkExportCancelling(false);
    setBulkExporting(format);
    setBulkProgress({ completed: 0, total: entries.length });
    try {
      await downloadSessionExportZip(entries, {
        format,
        timezone: exportTimezone(),
        filenamePattern: exportSettings.filenamePattern,
        signal: abortController.signal,
        onProgress: ({ completed, total }) => setBulkProgress({ completed, total }),
      });
      pushToast({
        level: "info",
        message: `${t("sources.export_all.requested")} (${entries.length})`,
      });
    } catch (err) {
      if (isAbortError(err)) {
        pushToast({ level: "info", message: t("sources.export_all.cancelled") });
        return;
      }
      pushToast({
        level: "error",
        message: (err as Error).message ?? t("sources.export_all.failed"),
      });
    } finally {
      setBulkExporting(null);
      setBulkProgress(null);
      setBulkExportAbort(null);
      setBulkExportCancelling(false);
    }
  }

  function onCancelBulkExport(): void {
    const controller = bulkExportAbort();
    if (!controller || controller.signal.aborted) return;
    setBulkExportCancelling(true);
    controller.abort();
  }

  function exportSettingsControls() {
    return (
      <div class="wl-export-settings-controls">
        <input
          type="text"
          value={exportTimezone()}
          onInput={(ev) => setExportTimezone(ev.currentTarget.value)}
          placeholder={t("sources.export.timezone_placeholder")}
          aria-label={t("sources.export.shared_timezone")}
        />
        <input
          type="text"
          value={exportSettings.filenamePattern}
          onInput={(ev) => updateExportSettings({ filenamePattern: ev.currentTarget.value })}
          placeholder={t("sources.export.filename_pattern_placeholder")}
          aria-label={t("sources.export.shared_filename_pattern")}
        />
        <span class="wl-export-settings-shared">{t("sources.export.shared_settings")}</span>
      </div>
    );
  }

  function onSourceNoteInput(sid: string, text: string): void {
    updateSourceNote(sid, text);
  }

  function onSourceNoteBlur(sid: string): void {
    syncSourceNoteNow(sid);
  }

  function setSourceNoteStatus(sid: string, status: AnnotationSyncStatus): void {
    setSourceNoteSyncStatus((prev) => ({ ...prev, [sid]: status }));
  }

  function sourceNoteStatus(sid: string): AnnotationSyncStatus {
    return sourceNoteSyncStatus()[sid] ?? "idle";
  }

  function syncSourceNoteNow(sid: string): void {
    const note = sourceNotes[sid];
    if (!note) return;
    setSourceNoteStatus(sid, "syncing");
    void syncSourceNoteToServer(note)
      .then(() => setSourceNoteStatus(sid, "synced"))
      .catch((err) => {
        setSourceNoteStatus(sid, "error");
        console.warn("E-UI-ANNOTATION-SYNC save source note failed", err);
        pushToast({ level: "warn", message: t("sources.detail.notes_sync_failed") });
      });
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
      <Show when={pendingStarts().length > 0}>
        <div class="wl-source-pending" role="status" aria-live="polite">
          <strong>{t("sources.pending.title")}</strong>
          <For each={pendingStarts()}>
            {(item) => <span class="wl-source-pending-item">{item.label}</span>}
          </For>
        </div>
      </Show>
      <div
        style={{
          display: "grid",
          "grid-template-columns": "repeat(auto-fit, minmax(220px, 1fr))",
          gap: "8px",
          "align-items": "end",
          "margin-bottom": "12px",
        }}
      >
        <label>
          {t("sources.start.encoding")} {" "}
          <select
            value={sourceStartOptions.encoding}
            onChange={(ev) => updateSourceStartOptions({ encoding: ev.currentTarget.value })}
            aria-label={t("sources.start.encoding")}
          >
            <For each={encodingOptions(sourceStartOptions.encoding)}>
              {(encoding) => <option value={encoding}>{encoding}</option>}
            </For>
          </select>
        </label>
        <label>
          {t("sources.start.session_pattern")} {" "}
          <input
            type="text"
            value={sourceStartOptions.sessionNamePattern}
            onInput={(ev) => updateSourceStartOptions({ sessionNamePattern: ev.currentTarget.value })}
            placeholder="{prefix}_{kind}_{iface}_{unix_ns}"
            aria-label={t("sources.start.session_pattern")}
          />
        </label>
        <label style={{ display: "flex", gap: "6px", "align-items": "center" }}>
          <input
            type="checkbox"
            checked={sourceStartOptions.sendClassificationRules}
            onChange={(ev) => updateSourceStartOptions({ sendClassificationRules: ev.currentTarget.checked })}
          />
          <span>{t("sources.start.send_rules")}</span>
        </label>
        <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
          {t("sources.start.options_help")}
        </div>
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
          pattern="[A-Za-z0-9_.-]+"
          title={t("sources.preset.name_help")}
          style={{ width: "160px" }}
        />
        <button type="button" onClick={onSavePreset} disabled={!presetName().trim() || presetNameInvalid()}>
          {t("sources.preset.save")}
        </button>
        <button type="button" onClick={onDeletePreset} disabled={!presetName().trim()}>
          {t("sources.preset.delete")}
        </button>
        <span style={{ color: presetNameInvalid() ? "var(--wl-error)" : "var(--wl-fg-muted)" }}>
          {presetNameInvalid() ? t("sources.preset.name_invalid") : t("sources.preset.help")}
        </span>
      </div>
      <div class="wl-serial-detect">
        <div class="wl-serial-detect-actions">
          <button
            type="button"
            onClick={() => void onDetectSerial()}
            disabled={serialDetecting()}
          >
            {serialDetecting() ? t("sources.serial.detecting") : t("sources.serial.detect")}
          </button>
          <label>
            {t("sources.serial.baud")} {" "}
            <input
              type="number"
              min="1"
              value={serialBaud()}
              onInput={(ev) => setSerialBaud(Number(ev.currentTarget.value))}
            />
          </label>
          <button
            type="button"
            onClick={onOpenSelectedSerial}
            disabled={selectedSerialPorts().length === 0}
          >
            {t("sources.serial.open_selected")}
          </button>
          <span style={{ color: "var(--wl-fg-muted)" }}>{t("sources.serial.help")}</span>
          <Show when={serialDetecting()}>
            <span class="wl-operation-progress" role="status" aria-live="polite">
              {serialDetectProgressLabel()}
            </span>
          </Show>
        </div>
        <Show when={serialCandidates().length > 0}>
          <div class="wl-serial-candidates" aria-label={t("sources.serial.candidates")}>
            <For each={serialCandidates()}>
              {(port) => (
                <label class="wl-serial-candidate">
                  <input
                    type="checkbox"
                    checked={selectedSerialPorts().includes(port)}
                    onChange={(ev) => toggleSerialPort(port, ev.currentTarget.checked)}
                  />
                  <code>{port}</code>
                </label>
              )}
            </For>
          </div>
        </Show>
        <Show when={serialOpenResults().length > 0}>
          <div
            class="wl-serial-open-results"
            role="status"
            aria-live="polite"
            aria-label={t("sources.serial.open_results")}
          >
            <strong>{t("sources.serial.open_results")}</strong>
            <div class="wl-serial-open-result-list">
              <For each={serialOpenResults()}>
                {(result) => (
                  <div class={`wl-serial-open-result wl-serial-open-result-${result.status}`}>
                    <code>{result.port}</code>
                    <span>{serialOpenStatusLabel(result.status)}</span>
                    <span class="wl-serial-open-result-detail">{result.detail}</span>
                  </div>
                )}
              </For>
            </div>
          </div>
        </Show>
        <Show when={pcapInterfaces().length > 0}>
          <div class="wl-pcap-candidates" aria-label={t("sources.pcap.candidates")}>
            <label>
              {t("sources.pcap.interface")} {" "}
              <select
                value={selectedPcapDevice()}
                onChange={(ev) => setSelectedPcapDevice(ev.currentTarget.value)}
              >
                <For each={pcapInterfaces()}>
                  {(iface) => (
                    <option value={iface.device}>
                      {iface.display_name ? `${iface.display_name} (${iface.device})` : iface.device}
                    </option>
                  )}
                </For>
              </select>
            </label>
            <label>
              {t("sources.pcap.snaplen")} {" "}
              <input
                type="number"
                min="1"
                value={pcapSnaplen()}
                onInput={(ev) => setPcapSnaplen(Number(ev.currentTarget.value))}
              />
            </label>
            <label style={{ display: "inline-flex", gap: "4px", "align-items": "center" }}>
              <input
                type="checkbox"
                checked={pcapPromiscuous()}
                onChange={(ev) => setPcapPromiscuous(ev.currentTarget.checked)}
              />
              <span>{t("sources.pcap.promisc")}</span>
            </label>
            <label>
              {t("sources.pcap.publish")} {" "}
              <select
                value={pcapPublishMode()}
                aria-describedby="pcap-publish-mode-hint"
                onChange={(ev) => setPcapPublishMode(ev.currentTarget.value as PcapPublishMode)}
              >
                <For each={PCAP_PUBLISH_MODES}>
                  {(mode) => <option value={mode}>{pcapPublishModeLabel(mode)}</option>}
                </For>
              </select>
            </label>
            <span id="pcap-publish-mode-hint" class="wl-pcap-publish-hint">
              {pcapPublishModeHint(pcapPublishMode())}
            </span>
            <input
              type="text"
              value={pcapFilter()}
              onInput={(ev) => setPcapFilter(ev.currentTarget.value)}
              placeholder={t("sources.pcap.filter_placeholder")}
              aria-label={t("sources.pcap.filter")}
            />
            <button type="button" onClick={onUseSelectedPcap}>
              {t("sources.pcap.use_spec")}
            </button>
            <span style={{ color: "var(--wl-fg-muted)" }}>{t("sources.pcap.help")}</span>
          </div>
        </Show>
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
      <div class="wl-source-bulk-export">
        <strong>{t("sources.export_all.title")}</strong>
        {exportSettingsControls()}
        <For each={EXPORT_FORMATS}>
          {(format) => (
            <button
              type="button"
              onClick={() => void onDownloadAllExports(format)}
              disabled={exportableSources(format).length === 0 || bulkExporting() !== null}
              title={
                exportableSources(format).length === 0
                  ? t(
                    format === "pcapng"
                      ? "sources.export_all.pcapng_unavailable"
                      : "sources.export_all.unavailable",
                  )
                  : t(`sources.export_all.${format}`)
              }
            >
              {bulkExporting() === format
                ? `${t("sources.export_all.downloading")} ${bulkProgressLabel()}`.trim()
                : t(`sources.export_all.${format}`)}
            </button>
          )}
        </For>
        <Show when={bulkExporting() !== null}>
          <span class="wl-operation-progress" role="status" aria-live="polite">
            {bulkExportProgressLabel()}
          </span>
        </Show>
        <Show when={bulkExporting() !== null}>
          <button
            type="button"
            onClick={onCancelBulkExport}
            disabled={bulkExportCancelling()}
          >
            {bulkExportCancelling()
              ? t("sources.export_all.cancelling")
              : t("sources.export_all.cancel")}
          </button>
        </Show>
        <span style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
          {persistentSources().length > 0
            ? `${t("sources.export_all.help")} (${persistentSources().length})`
            : t("sources.export_all.unavailable")}
        </span>
      </div>
      <Show
        when={rows().length > 0}
        fallback={
          <div style={{ color: "var(--wl-fg-muted)" }}>
            {t("sources.empty")}
          </div>
        }
      >
        <div class="wl-sources-table-wrap">
        <table class="wl-sources-table">
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
                    <div class="wl-source-actions">
                      <select
                        aria-label={`${s.name} ${t("sources.detail.encoding")}`}
                        title={t("sources.detail.encoding")}
                        value={sourceDisplayEncoding(s.sid)}
                        onChange={(ev) => updateSourceDisplayEncoding(s.sid, ev.currentTarget.value)}
                      >
                        <For each={encodingOptions(sourceDisplayEncoding(s.sid))}>
                          {(encoding) => <option value={encoding}>{encoding}</option>}
                        </For>
                      </select>
                    <button
                      type="button"
                      onClick={() => setSelectedSid(s.sid)}
                      title={t("sources.action.details")}
                    >
                      {t("sources.action.details")}
                    </button>{" "}
                    <Show when={s.channels.length > 1}>
                      <select
                        class="wl-source-channel-select"
                        aria-label={`${s.name} ${t("sources.action.select_channel")}`}
                        title={t("sources.action.select_channel")}
                        value={selectedOpenChannel(s.sid, s.channels)}
                        onChange={(ev) =>
                          setSelectedOpenChannel(s.sid, Number(ev.currentTarget.value))
                        }
                      >
                        <For each={s.channels}>
                          {(channel) => <option value={channel}>ch {channel}</option>}
                        </For>
                      </select>{" "}
                    </Show>
                    <button
                      type="button"
                      onClick={() => onOpenTerminal(s.sid, selectedOpenChannel(s.sid, s.channels))}
                      title={`${
                        s.channels.length > 1
                          ? t("sources.action.open_terminal_selected_channel")
                          : t("sources.action.open_terminal_first_channel")
                      } ch ${selectedOpenChannel(s.sid, s.channels)}`}
                    >
                      {t("sources.action.open_terminal")} ch {selectedOpenChannel(s.sid, s.channels)}
                    </button>{" "}
                    <button
                      type="button"
                      onClick={() =>
                        onOpenNewTerminal(s.sid, selectedOpenChannel(s.sid, s.channels))
                      }
                      title={`${
                        s.channels.length > 1
                          ? t("sources.action.open_new_terminal_selected_channel")
                          : t("sources.action.open_new_terminal_first_channel")
                      } ch ${selectedOpenChannel(s.sid, s.channels)}`}
                    >
                      {t("sources.action.open_new_terminal")} ch{" "}
                      {selectedOpenChannel(s.sid, s.channels)}
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
                    </div>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
        </div>
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
              <dt>{t("sources.detail.alias")}</dt>
              <dd>
                <input
                  type="text"
                  aria-label={t("sources.detail.alias")}
                  value={sourceAliases[source().sid]?.label ?? ""}
                  onInput={(ev) => updateSourceAlias(source().sid, ev.currentTarget.value)}
                  placeholder={t("sources.detail.alias_placeholder")}
                  maxLength={MAX_SOURCE_ALIAS_LENGTH}
                  style={{ width: "100%" }}
                />
                <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
                  {t("sources.detail.alias_help")} <span class="wl-source-count">{(sourceAliases[source().sid]?.label ?? "").length}/{MAX_SOURCE_ALIAS_LENGTH}</span>
                </div>
              </dd>
              <dt>{t("sources.detail.encoding")}</dt>
              <dd>
                <div style={{ display: "flex", gap: "6px", "align-items": "center" }}>
                  <select
                    aria-label={t("sources.detail.encoding")}
                    value={sourceDisplayEncoding(source().sid)}
                    onChange={(ev) => updateSourceDisplayEncoding(source().sid, ev.currentTarget.value)}
                    style={{ flex: 1 }}
                  >
                    <For each={encodingOptions(sourceDisplayEncoding(source().sid))}>
                      {(encoding) => <option value={encoding}>{encoding}</option>}
                    </For>
                  </select>
                  <button
                    type="button"
                    onClick={() => onRestartWithServerEncoding(source().sid)}
                    title={t("sources.detail.server_encoding_help")}
                  >
                    {t("sources.detail.server_encoding_apply")}
                  </button>
                </div>
                <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
                  {t("sources.detail.encoding_help")} {t("sources.detail.server_encoding_help")}
                </div>
              </dd>
              <dt>{t("sources.detail.detection")}</dt>
              <dd>
                <Show
                  when={source().detection}
                  fallback={<span style={{ color: "var(--wl-fg-muted)" }}>-</span>}
                >
                  {(detection) => (
                    <div style={{ display: "grid", gap: "6px" }}>
                      <div>
                        {t("sources.detail.detection_mode")}: {t(`settings.source_start.detection.${detection().mode}`)}
                      </div>
                      <Show when={suggestedEncodingForSource(source())}>
                        {(encoding) => (
                          <button
                            type="button"
                            onClick={() => onRestartWithSuggestedEncoding(source().sid, encoding())}
                          >
                            {t("sources.detail.detected_encoding_apply")}: {encoding()}
                          </button>
                        )}
                      </Show>
                      <Show when={(detection().encoding_candidates?.length ?? 0) > 0}>
                        <div style={{ display: "flex", gap: "6px", "flex-wrap": "wrap" }}>
                          <For each={detection().encoding_candidates?.slice(0, 3) ?? []}>
                            {(candidate) => (
                              <code>{candidate.label} {candidate.confidence}%</code>
                            )}
                          </For>
                        </div>
                      </Show>
                      <Show when={(detection().log_type_candidates?.length ?? 0) > 0}>
                        <div style={{ display: "flex", gap: "6px", "flex-wrap": "wrap" }}>
                          <For each={detection().log_type_candidates ?? []}>
                            {(candidate) => (
                              <span>{candidate.tag} ({candidate.kind}, {candidate.count})</span>
                            )}
                          </For>
                        </div>
                      </Show>
                    </div>
                  )}
                </Show>
              </dd>
              <dt>{t("sources.detail.channel_encodings")}</dt>
              <dd>
                <div style={{ display: "grid", gap: "6px" }}>
                  <For each={source().channels}>
                    {(channel) => (
                      <label style={{ display: "flex", gap: "6px", "align-items": "center" }}>
                        <span>ch {channel}</span>
                        <select
                          aria-label={`${t("sources.detail.channel_encoding")} ch ${channel}`}
                          value={channelDisplayEncoding(source().sid, channel)}
                          onChange={(ev) => updateChannelDisplayEncoding(source().sid, channel, ev.currentTarget.value)}
                          style={{ flex: 1 }}
                        >
                          <For each={encodingOptions(channelDisplayEncoding(source().sid, channel))}>
                            {(encoding) => <option value={encoding}>{encoding}</option>}
                          </For>
                        </select>
                        <Show when={sourceEncodings[channelEncodingKey(source().sid, channel)]?.encoding}>
                          <span style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>override</span>
                        </Show>
                      </label>
                    )}
                  </For>
                </div>
                <Show when={exporting() !== null}>
                  <div class="wl-operation-progress" role="status" aria-live="polite">
                    {singleExportProgressLabel()}
                  </div>
                </Show>
                <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
                  {t("sources.detail.channel_encoding_help")}
                </div>
              </dd>
              <dt>{t("sources.detail.kind")}</dt>
              <dd>{source().kind}</dd>
              <dt>{t("sources.detail.status")}</dt>
              <dd>{statusLabel(source().status)}</dd>
              <dt>{t("sources.detail.persistence")}</dt>
              <dd>
                {source().persistent
                  ? t("sources.detail.persistence_on")
                  : t("sources.detail.persistence_off")}
              </dd>
              <dt>{t("sources.detail.session_dir")}</dt>
              <dd>
                <Show
                  when={source().sessionDir}
                  fallback={<span style={{ color: "var(--wl-fg-muted)" }}>-</span>}
                >
                  {(dir) => <code title={dir()}>{dir()}</code>}
                </Show>
                <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
                  {t("sources.detail.session_dir_help")}
                </div>
              </dd>
              <dt>{t("sources.export.title")}</dt>
              <dd>
                <div style={{ display: "flex", gap: "6px", "flex-wrap": "wrap" }}>
                  {exportSettingsControls()}
                  <For each={EXPORT_FORMATS}>
                    {(format) => (
                      <button
                        type="button"
                        onClick={() => void onDownloadExport(source().sid, format)}
                        disabled={!source().persistent || exporting() !== null}
                      >
                        {exporting() === format
                          ? t("sources.export.downloading")
                          : t(`sources.export.${format}`)}
                      </button>
                    )}
                  </For>
                </div>
                <Show when={exporting() !== null}>
                  <div class="wl-operation-progress" role="status" aria-live="polite">
                    {singleExportProgressLabel()}
                  </div>
                </Show>
                <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
                  {source().persistent
                    ? `${t("sources.export.help")} ${t("sources.export.filename_pattern_help")}`
                    : t("sources.export.unavailable")}
                </div>
              </dd>
              <dt>{t("sources.detail.channels")}</dt>
              <dd>{source().channels.join(", ") || "-"}</dd>
              <dt>{t("sources.detail.bytes")}</dt>
              <dd>{source().bytesIn}</dd>
              <dt>{t("sources.detail.last")}</dt>
              <dd>{source().lastTsMs ? new Date(source().lastTsMs).toISOString() : "-"}</dd>
              <dt>{t("sources.detail.recent_errors")}</dt>
              <dd>
                <Show
                  when={selectedSourceErrors().length > 0}
                  fallback={<span style={{ color: "var(--wl-fg-muted)" }}>{t("sources.detail.no_recent_errors")}</span>}
                >
                  <div class="wl-source-error-history">
                    <For each={selectedSourceErrors()}>
                      {(entry) => (
                        <div class={`wl-source-error-history-item wl-source-error-history-${entry.level}`}>
                          <time dateTime={new Date(entry.ts).toISOString()}>
                            {new Date(entry.ts).toLocaleTimeString()}
                          </time>
                          <span class="wl-source-error-event">{entry.event}</span>
                          <span class="wl-source-error-message">{entry.message}</span>
                          <Show when={entry.errorId}>
                            {(errorId) => <code>{errorId()}</code>}
                          </Show>
                        </div>
                      )}
                    </For>
                  </div>
                </Show>
              </dd>
              <dt>{t("sources.detail.notes")}</dt>
              <dd>
                <textarea
                  aria-label={t("sources.detail.notes")}
                  value={sourceNotes[source().sid]?.text ?? ""}
                  onInput={(ev) => onSourceNoteInput(source().sid, ev.currentTarget.value)}
                  onBlur={() => onSourceNoteBlur(source().sid)}
                  placeholder={t("sources.detail.notes_placeholder")}
                  maxLength={MAX_SOURCE_NOTE_LENGTH}
                  style={{
                    width: "100%",
                    "min-height": "84px",
                    resize: "vertical",
                  }}
                />
                <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px", display: "flex", gap: "8px", "align-items": "center", "flex-wrap": "wrap" }}>
                  <span>{t("sources.detail.notes_help")}</span>
                  <span class="wl-source-count">{(sourceNotes[source().sid]?.text ?? "").length}/{MAX_SOURCE_NOTE_LENGTH}</span>
                  <button
                    type="button"
                    onClick={() => syncSourceNoteNow(source().sid)}
                    disabled={sourceNoteStatus(source().sid) === "loading" || sourceNoteStatus(source().sid) === "syncing"}
                  >
                    {t("annotations.sync.now")}
                  </button>
                  <span>{annotationSyncLabel(sourceNoteStatus(source().sid))}</span>
                </div>
              </dd>
            </dl>
          </aside>
        )}
      </Show>
    </div>
  );
}
