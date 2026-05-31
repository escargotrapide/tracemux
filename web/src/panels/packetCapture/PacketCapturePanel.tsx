import { createEffect, createMemo, createSignal, For, onCleanup, Show } from "solid-js";
import { sourcesStore, useChannel } from "~/state";
import {
  appendPacketRing,
  packetFromDataPayload,
  type PacketCaptureEntry,
} from "~/state/packetCapture";
import { t } from "~/i18n";
import { PacketDetail } from "./PacketDetail";
import { PacketList } from "./PacketList";

export function PacketCapturePanel() {
  const pcapSources = createMemo(() =>
    Object.values(sourcesStore).filter((source) => source.kind === "pcap"),
  );
  const [selectedSid, setSelectedSid] = createSignal("");
  const [paused, setPaused] = createSignal(false);
  const [follow, setFollow] = createSignal(true);
  const [packets, setPackets] = createSignal<PacketCaptureEntry[]>([]);
  const [selectedId, setSelectedId] = createSignal<number | undefined>(undefined);
  let nextPacketId = 1;
  let unsubscribe: (() => void) | undefined;

  createEffect(() => {
    const sources = pcapSources();
    const current = selectedSid();
    if (current && sources.some((source) => source.sid === current)) return;
    setSelectedSid(sources[0]?.sid ?? "");
  });

  createEffect(() => {
    unsubscribe?.();
    unsubscribe = undefined;
    setPackets([]);
    setSelectedId(undefined);
    nextPacketId = 1;
    const sid = selectedSid();
    if (!sid) return;
    unsubscribe = useChannel(sid, 0, (payload) => {
      if (paused()) return;
      const packet = packetFromDataPayload(payload, nextPacketId++);
      if (!packet) return;
      setPackets((prev) => {
        const next = appendPacketRing(prev, packet);
        if (follow()) setSelectedId(packet.id);
        return next;
      });
    });
  });

  onCleanup(() => unsubscribe?.());

  const selectedPacket = () => packets().find((packet) => packet.id === selectedId());

  function selectPacket(packet: PacketCaptureEntry): void {
    setFollow(false);
    setSelectedId(packet.id);
  }

  function clearPackets(): void {
    setPackets([]);
    setSelectedId(undefined);
    nextPacketId = 1;
  }

  return (
    <div class="wl-packet-capture-panel">
      <div class="wl-packet-toolbar">
        <label>
          {t("packetCapture.source")} {" "}
          <select
            value={selectedSid()}
            onChange={(ev) => setSelectedSid(ev.currentTarget.value)}
          >
            <option value="">{t("packetCapture.source.none")}</option>
            <For each={pcapSources()}>
              {(source) => <option value={source.sid}>{source.name}</option>}
            </For>
          </select>
        </label>
        <button type="button" onClick={() => setPaused((value) => !value)} disabled={!selectedSid()}>
          {paused() ? t("packetCapture.resume") : t("packetCapture.pause")}
        </button>
        <label style={{ display: "inline-flex", gap: "4px", "align-items": "center" }}>
          <input
            type="checkbox"
            checked={follow()}
            onChange={(ev) => setFollow(ev.currentTarget.checked)}
          />
          <span>{t("packetCapture.follow")}</span>
        </label>
        <button type="button" onClick={clearPackets} disabled={packets().length === 0}>
          {t("packetCapture.clear")}
        </button>
        <span style={{ color: "var(--wl-fg-muted)" }}>
          {t("packetCapture.count")}: {packets().length}
        </span>
      </div>
      <Show
        when={selectedSid()}
        fallback={<div class="wl-empty">{t("packetCapture.no_source")}</div>}
      >
        <div class="wl-packet-grid">
          <PacketList packets={packets()} selectedId={selectedId()} onSelect={selectPacket} />
          <PacketDetail packet={selectedPacket()} />
        </div>
      </Show>
    </div>
  );
}
