// Display settings panel. These settings are local UI preferences for the
// first implementation slice; the server-backed settings store will reuse
// the same shape when the shared configuration API lands.
//
// REQ: FR-UI-014

import { t } from "~/i18n";
import { displaySettings, updateDisplaySettings } from "~/state/displaySettings";

function numberValue(value: string): number {
  return Number(value);
}

export function SettingsPanel() {
  return (
    <div class="wl-settings-panel">
      <section class="wl-settings-section">
        <h2>{t("settings.display.title")}</h2>
        <label class="wl-settings-row">
          <span>{t("settings.terminal_scrollback")}</span>
          <input
            type="number"
            min="100"
            max="1000000"
            value={displaySettings.terminalScrollback}
            onInput={(ev) => updateDisplaySettings({ terminalScrollback: numberValue(ev.currentTarget.value) })}
          />
        </label>
        <label class="wl-settings-row">
          <span>{t("settings.tile_scrollback")}</span>
          <input
            type="number"
            min="50"
            max="100000"
            value={displaySettings.tileScrollback}
            onInput={(ev) => updateDisplaySettings({ tileScrollback: numberValue(ev.currentTarget.value) })}
          />
        </label>
        <label class="wl-settings-row">
          <span>{t("settings.timezone")}</span>
          <select
            value={displaySettings.timezone}
            onChange={(ev) => updateDisplaySettings({ timezone: ev.currentTarget.value })}
          >
            <option value="local">{t("settings.timezone.local")}</option>
            <option value="UTC">UTC</option>
            <option value="Asia/Tokyo">Asia/Tokyo</option>
            <option value="GMT+09:00">GMT+09:00</option>
          </select>
        </label>
      </section>

      <section class="wl-settings-section">
        <h2>{t("settings.metadata.title")}</h2>
        <label class="wl-settings-check">
          <input
            type="checkbox"
            checked={displaySettings.showTimestamp}
            onChange={(ev) => updateDisplaySettings({ showTimestamp: ev.currentTarget.checked })}
          />
          <span>{t("settings.show_timestamp")}</span>
        </label>
        <label class="wl-settings-check">
          <input
            type="checkbox"
            checked={displaySettings.showKind}
            onChange={(ev) => updateDisplaySettings({ showKind: ev.currentTarget.checked })}
          />
          <span>{t("settings.show_kind")}</span>
        </label>
        <label class="wl-settings-check">
          <input
            type="checkbox"
            checked={displaySettings.showSource}
            onChange={(ev) => updateDisplaySettings({ showSource: ev.currentTarget.checked })}
          />
          <span>{t("settings.show_source")}</span>
        </label>
      </section>

      <section class="wl-settings-section">
        <h2>{t("settings.tiles.title")}</h2>
        <label class="wl-settings-row">
          <span>{t("settings.tile_min_width")}</span>
          <input
            type="number"
            min="120"
            max="1200"
            value={displaySettings.tileMinWidth}
            onInput={(ev) => updateDisplaySettings({ tileMinWidth: numberValue(ev.currentTarget.value) })}
          />
        </label>
        <label class="wl-settings-row">
          <span>{t("settings.tile_min_height")}</span>
          <input
            type="number"
            min="80"
            max="900"
            value={displaySettings.tileMinHeight}
            onInput={(ev) => updateDisplaySettings({ tileMinHeight: numberValue(ev.currentTarget.value) })}
          />
        </label>
      </section>
    </div>
  );
}
