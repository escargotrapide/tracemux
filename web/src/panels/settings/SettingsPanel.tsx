// Display settings panel. These settings are local UI preferences for the
// first implementation slice; the server-backed settings store will reuse
// the same shape when the shared configuration API lands.
//
// REQ: FR-UI-014

import { createSignal, For } from "solid-js";
import { t } from "~/i18n";
import {
  deleteClassificationRule,
  orderedClassificationRules,
  upsertClassificationRule,
} from "~/state/classificationRules";
import { displaySettings, updateDisplaySettings } from "~/state/displaySettings";
import {
  logTypeNotes,
  normalizeLogTypeKey,
  updateLogTypeNote,
} from "~/state/logTypeNotes";
import {
  sourceStartOptions,
  SUPPORTED_SOURCE_ENCODINGS,
  updateSourceStartOptions,
} from "~/state/sourceStartOptions";

function numberValue(value: string): number {
  return Number(value);
}

export function SettingsPanel() {
  const [logTypeKey, setLogTypeKey] = createSignal("bytes");
  const [ruleContains, setRuleContains] = createSignal("");
  const [ruleTag, setRuleTag] = createSignal("");
  const [ruleCaseSensitive, setRuleCaseSensitive] = createSignal(false);
  const selectedLogTypeNote = () => {
    const key = normalizeLogTypeKey(logTypeKey());
    return key ? logTypeNotes[key]?.text ?? "" : "";
  };

  function addRule(): void {
    upsertClassificationRule({
      contains: ruleContains(),
      tag: ruleTag(),
      caseSensitive: ruleCaseSensitive(),
      enabled: true,
    });
    setRuleContains("");
    setRuleTag("");
    setRuleCaseSensitive(false);
  }

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
          <span>{t("settings.terminal_max_records")}</span>
          <input
            type="number"
            min="100"
            max="1000000"
            value={displaySettings.terminalMaxRecords}
            onInput={(ev) => updateDisplaySettings({ terminalMaxRecords: numberValue(ev.currentTarget.value) })}
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
          <span>{t("settings.tile_max_records")}</span>
          <input
            type="number"
            min="50"
            max="100000"
            value={displaySettings.tileMaxRecords}
            onInput={(ev) => updateDisplaySettings({ tileMaxRecords: numberValue(ev.currentTarget.value) })}
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
        <h2>{t("settings.source_start.title")}</h2>
        <label class="wl-settings-row">
          <span>{t("settings.source_start.encoding")}</span>
          <input
            type="text"
            list="wl-settings-source-encodings"
            value={sourceStartOptions.encoding}
            onInput={(ev) => updateSourceStartOptions({ encoding: ev.currentTarget.value })}
          />
        </label>
        <datalist id="wl-settings-source-encodings">
          <For each={SUPPORTED_SOURCE_ENCODINGS}>
            {(encoding) => <option value={encoding} />}
          </For>
        </datalist>
        <label class="wl-settings-row">
          <span>{t("settings.source_start.session_pattern")}</span>
          <input
            type="text"
            value={sourceStartOptions.sessionNamePattern}
            placeholder="{prefix}_{kind}_{iface}_{unix_ns}"
            onInput={(ev) => updateSourceStartOptions({ sessionNamePattern: ev.currentTarget.value })}
          />
        </label>
        <label class="wl-settings-check">
          <input
            type="checkbox"
            checked={sourceStartOptions.sendClassificationRules}
            onChange={(ev) => updateSourceStartOptions({ sendClassificationRules: ev.currentTarget.checked })}
          />
          <span>{t("settings.source_start.send_rules")}</span>
        </label>
        <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px" }}>
          {t("settings.source_start.help")}
        </div>
      </section>

      <section class="wl-settings-section">
        <h2>{t("settings.classification.title")}</h2>
        <div class="wl-settings-row">
          <span>{t("settings.classification.contains")}</span>
          <input
            type="text"
            value={ruleContains()}
            onInput={(ev) => setRuleContains(ev.currentTarget.value)}
            placeholder={t("settings.classification.contains_placeholder")}
          />
        </div>
        <div class="wl-settings-row">
          <span>{t("settings.classification.tag")}</span>
          <input
            type="text"
            value={ruleTag()}
            onInput={(ev) => setRuleTag(ev.currentTarget.value)}
            placeholder={t("settings.classification.tag_placeholder")}
          />
        </div>
        <label class="wl-settings-check">
          <input
            type="checkbox"
            checked={ruleCaseSensitive()}
            onChange={(ev) => setRuleCaseSensitive(ev.currentTarget.checked)}
          />
          <span>{t("settings.classification.case_sensitive")}</span>
        </label>
        <button
          type="button"
          onClick={addRule}
          disabled={!ruleContains().trim() || !ruleTag().trim()}
        >
          {t("settings.classification.add")}
        </button>
        <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px", "margin-top": "4px" }}>
          {t("settings.classification.help")}
        </div>
        <table style={{ width: "100%", "border-collapse": "collapse", "margin-top": "8px" }}>
          <thead>
            <tr style={{ color: "var(--wl-fg-muted)", "text-align": "left" }}>
              <th>{t("settings.classification.enabled")}</th>
              <th>{t("settings.classification.contains")}</th>
              <th>{t("settings.classification.tag")}</th>
              <th>{t("settings.classification.case")}</th>
              <th>{t("settings.classification.actions")}</th>
            </tr>
          </thead>
          <tbody>
            <For each={orderedClassificationRules()}>
              {(rule) => (
                <tr style={{ "border-top": "1px solid var(--wl-border)" }}>
                  <td>
                    <input
                      type="checkbox"
                      checked={rule.enabled}
                      aria-label={`${t("settings.classification.enabled")}: ${rule.tag}`}
                      onChange={(ev) => upsertClassificationRule({ ...rule, enabled: ev.currentTarget.checked })}
                    />
                  </td>
                  <td><code>{rule.contains}</code></td>
                  <td>{rule.tag}</td>
                  <td>{rule.caseSensitive ? t("settings.classification.yes") : t("settings.classification.no")}</td>
                  <td>
                    <button type="button" onClick={() => deleteClassificationRule(rule.id)}>
                      {t("settings.classification.delete")}
                    </button>
                  </td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
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

      <section class="wl-settings-section">
        <h2>{t("settings.log_type_notes.title")}</h2>
        <label class="wl-settings-row">
          <span>{t("settings.log_type_notes.key")}</span>
          <input
            type="text"
            value={logTypeKey()}
            list="wl-log-type-note-keys"
            onInput={(ev) => setLogTypeKey(ev.currentTarget.value)}
            placeholder={t("settings.log_type_notes.key_placeholder")}
          />
        </label>
        <datalist id="wl-log-type-note-keys">
          <For each={Object.keys(logTypeNotes).sort()}>
            {(key) => <option value={key} />}
          </For>
        </datalist>
        <textarea
          value={selectedLogTypeNote()}
          onInput={(ev) => {
            const key = normalizeLogTypeKey(logTypeKey());
            if (key) updateLogTypeNote(key, ev.currentTarget.value);
          }}
          placeholder={t("settings.log_type_notes.placeholder")}
          style={{ width: "100%", "min-height": "86px", resize: "vertical" }}
        />
        <div style={{ color: "var(--wl-fg-muted)", "font-size": "12px", "margin-top": "4px" }}>
          {t("settings.log_type_notes.help")}
        </div>
      </section>
    </div>
  );
}
