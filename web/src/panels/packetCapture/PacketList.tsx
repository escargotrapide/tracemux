import { For, Show } from "solid-js";
import {
  formatPacketTimestamp,
  packetProtocolHint,
  type PacketCaptureEntry,
} from "~/state/packetCapture";
import { t } from "~/i18n";

export interface PacketListProps {
  packets: PacketCaptureEntry[];
  selectedId: number | undefined;
  onSelect: (packet: PacketCaptureEntry) => void;
}

export function PacketList(props: PacketListProps) {
  return (
    <Show
      when={props.packets.length > 0}
      fallback={<div class="wl-empty">{t("packetCapture.empty")}</div>}
    >
      <table class="wl-packet-list">
        <thead>
          <tr>
            <th>{t("packetCapture.column.seq")}</th>
            <th>{t("packetCapture.column.time")}</th>
            <th>{t("packetCapture.column.protocol")}</th>
            <th>{t("packetCapture.column.length")}</th>
            <th>{t("packetCapture.column.source")}</th>
          </tr>
        </thead>
        <tbody>
          <For each={props.packets}>
            {(packet) => (
              <tr
                class={packet.id === props.selectedId ? "selected" : ""}
                onClick={() => props.onSelect(packet)}
              >
                <td>{packet.id}</td>
                <td>{formatPacketTimestamp(packet.tsOrigin)}</td>
                <td>{packetProtocolHint(packet)}</td>
                <td>{packet.capturedLen}/{packet.originalLen}</td>
                <td>{packet.source ?? packet.sid.slice(0, 8)}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>
    </Show>
  );
}
