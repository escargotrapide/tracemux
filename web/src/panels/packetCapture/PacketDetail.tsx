import { For, Show } from "solid-js";
import {
  DEFAULT_HEX_PREVIEW_BYTES,
  formatPacketTimestamp,
  hexPreview,
  packetProtocolHint,
  type PacketCaptureEntry,
} from "~/state/packetCapture";
import { t } from "~/i18n";

export interface PacketDetailProps {
  packet: PacketCaptureEntry | undefined;
}

export function PacketDetail(props: PacketDetailProps) {
  const rows = () => props.packet ? hexPreview(props.packet.bytes) : [];

  return (
    <Show
      when={props.packet}
      fallback={<div class="wl-empty">{t("packetCapture.detail.empty")}</div>}
    >
      {(packet) => (
        <div class="wl-packet-detail">
          <dl>
            <dt>{t("packetCapture.column.seq")}</dt>
            <dd>{packet().id}</dd>
            <dt>{t("packetCapture.column.time")}</dt>
            <dd>{formatPacketTimestamp(packet().tsOrigin)}</dd>
            <dt>{t("packetCapture.column.protocol")}</dt>
            <dd>{packetProtocolHint(packet())}</dd>
            <dt>{t("packetCapture.column.length")}</dt>
            <dd>{packet().capturedLen}/{packet().originalLen}</dd>
            <dt>{t("packetCapture.column.source")}</dt>
            <dd>{packet().source ?? packet().sid}</dd>
          </dl>
          <div class="wl-packet-hex-title">
            {t("packetCapture.hex.preview")} ({DEFAULT_HEX_PREVIEW_BYTES} bytes)
          </div>
          <pre class="wl-packet-hex">
            <For each={rows()}>
              {(row) => `${row.offset}  ${row.hex.padEnd(47, " ")}  ${row.ascii}\n`}
            </For>
          </pre>
        </div>
      )}
    </Show>
  );
}
