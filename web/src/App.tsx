// Top-level app shell. Hosts a Dockview grid with the built-in panels.

import { createEffect, onCleanup, onMount } from "solid-js";
import { render } from "solid-js/web";
import {
  createDockview,
  type DockviewApi,
  type DockviewPanelApi,
  type IContentRenderer,
} from "dockview-core";
import {
  connState,
  getClient,
  terminalFocusRequest,
  terminalOpenRequest,
} from "~/state";
import { t, locale, setLocale } from "~/i18n";
import { SourcesPanel } from "~/panels/sources/SourcesPanel";
import { MetricsPanel } from "./panels/metrics/MetricsPanel";
import { TerminalPanel } from "~/panels/terminal/TerminalPanel";
import { TileGridPanel } from "~/panels/tiles/TileGridPanel";
import { SettingsPanel } from "~/panels/settings/SettingsPanel";
import { Toasts } from "~/panels/Toasts";

interface PanelParams {
  sid?: string;
  ch?: number;
  followSelection?: boolean;
}

class SolidPanel implements IContentRenderer {
  readonly element: HTMLElement;
  private _cleanup?: () => void;
  constructor(private factory: (params: PanelParams) => unknown) {
    this.element = document.createElement("div");
    this.element.style.width = "100%";
    this.element.style.height = "100%";
  }
  init(parameters: { params: PanelParams; api: DockviewPanelApi }): void {
    this._cleanup = render(
      () => this.factory(parameters.params) as never,
      this.element,
    );
  }
  dispose(): void {
    this._cleanup?.();
  }
}

const components: Record<string, () => IContentRenderer> = {
  sources: () => new SolidPanel(() => SourcesPanel()),
  metrics: () => new SolidPanel(() => MetricsPanel()),
  tiles: () => new SolidPanel(() => TileGridPanel()),
  settings: () => new SolidPanel(() => SettingsPanel()),
  terminal: () =>
    new SolidPanel((p) =>
      TerminalPanel({
        sid: p.sid ?? "",
        ch: p.ch ?? 0,
        followSelection: p.followSelection ?? true,
      }),
    ),
};

export function App() {
  let dockHost!: HTMLDivElement;
  let api: DockviewApi | null = null;
  let terminalSeq = 2;

  function focusTerminalPanel(): void {
    const panel = api?.getPanel("terminal");
    panel?.api.setActive();
    panel?.focus();
  }

  function addTerminalPanel(sid = "", ch = 0): void {
    if (!api) return;
    const id = `terminal-${terminalSeq}`;
    const panel = api.addPanel({
      id,
      component: "terminal",
      title: `${t("panel.terminal")} ${terminalSeq}`,
      params: { sid, ch, followSelection: false },
      position: { referencePanel: "terminal", direction: "right" },
    });
    terminalSeq += 1;
    panel.api.setActive();
    panel.focus();
  }

  createEffect(() => {
    const request = terminalFocusRequest();
    if (!request) return;
    focusTerminalPanel();
  });

  createEffect(() => {
    const request = terminalOpenRequest();
    if (!request) return;
    addTerminalPanel(request.sid, request.ch);
  });

  onMount(() => {
    // Eagerly start the WSS client.
    getClient();

    api = createDockview(dockHost, {
      createComponent: (options) => {
        const factory = components[options.name];
        if (!factory) {
          throw new Error(`E-UI-0002: unknown panel '${options.name}'`);
        }
        return factory();
      },
    });

    api.addPanel({
      id: "sources",
      component: "sources",
      title: t("panel.sources"),
    });
    api.addPanel({
      id: "metrics",
      component: "metrics",
      title: t("panel.metrics"),
      position: { referencePanel: "sources", direction: "right" },
    });
    api.addPanel({
      id: "terminal",
      component: "terminal",
      title: t("panel.terminal"),
      params: { sid: "", ch: 0 },
      position: { referencePanel: "sources", direction: "below" },
    });
    api.addPanel({
      id: "tiles",
      component: "tiles",
      title: t("panel.tiles"),
      position: { referencePanel: "terminal", direction: "right" },
    });
    api.addPanel({
      id: "settings",
      component: "settings",
      title: t("panel.settings"),
      position: { referencePanel: "metrics", direction: "below" },
    });
  });

  onCleanup(() => {
    api?.dispose();
  });

  const statusClass = () => {
    const s = connState();
    if (s.status === "open") return "ok";
    if (s.status === "error" || s.status === "closed") return "err";
    if (s.status === "connecting") return "warn";
    return "";
  };

  return (
    <div class="wl-app">
      <header class="wl-topbar">
        <strong>{t("app.title")}</strong>
        <span style={{ color: "var(--wl-fg-muted)" }}>
          {t("app.subtitle")}
        </span>
        <span style={{ "margin-left": "auto" }}>
          <button
            type="button"
            onClick={() => addTerminalPanel()}
            title={t("terminal.new")}
          >
            {t("terminal.new_short")}
          </button>{" "}
          <button
            type="button"
            onClick={() => setLocale(locale() === "ja" ? "en" : "ja")}
            title="Toggle language"
          >
            {locale().toUpperCase()}
          </button>
        </span>
      </header>
      <main class="wl-dock" ref={dockHost!} />
      <footer class="wl-statusbar">
        <span class={`wl-status-dot ${statusClass()}`} />
        <span>{t(`status.${connState().status}`)}</span>
      </footer>
      <Toasts />
    </div>
  );
}
