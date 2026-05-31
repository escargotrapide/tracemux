// Top-level app shell. Hosts a Dockview grid with the built-in panels.

import { Show, createEffect, onCleanup, onMount } from "solid-js";
import { render } from "solid-js/web";
import {
  DefaultTab,
  createDockview,
  type DockviewApi,
  type GroupPanelPartInitParameters,
  type IContentRenderer,
  type ITabRenderer,
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
import { PacketCapturePanel } from "~/panels/packetCapture/PacketCapturePanel";
import { TerminalPanel } from "~/panels/terminal/TerminalPanel";
import { TileGridPanel } from "~/panels/tiles/TileGridPanel";
import { SettingsPanel } from "~/panels/settings/SettingsPanel";
import { Toasts } from "~/panels/Toasts";

interface PanelParams {
  sid?: string;
  ch?: number;
  followSelection?: boolean;
  panelKind?: PanelKind;
}

type PanelKind = "sources" | "metrics" | "packet" | "terminal" | "tiles" | "settings";

function panelKindClass(kind: PanelKind | undefined): string {
  return kind ? `wl-panel-kind-${kind}` : "";
}

function panelKindForComponent(component: string | undefined): PanelKind | undefined {
  if (component === "sources") return "sources";
  if (component === "metrics") return "metrics";
  if (component === "packetCapture") return "packet";
  if (component === "terminal") return "terminal";
  if (component === "tiles") return "tiles";
  if (component === "settings") return "settings";
  return undefined;
}

function panelKindForId(id: string | undefined): PanelKind | undefined {
  if (id === "sources") return "sources";
  if (id === "metrics") return "metrics";
  if (id === "packetCapture") return "packet";
  if (id === "terminal" || id?.startsWith("terminal-")) return "terminal";
  if (id === "tiles") return "tiles";
  if (id === "settings") return "settings";
  return undefined;
}

function panelKindFromInit(parameters: GroupPanelPartInitParameters): PanelKind | undefined {
  const params = parameters.api.getParameters<PanelParams>();
  return params.panelKind
    ?? panelKindForComponent(parameters.api.component)
    ?? panelKindForId(parameters.api.id);
}

class SolidPanel implements IContentRenderer {
  readonly element: HTMLElement;
  private _cleanup?: () => void;
  constructor(private factory: (params: PanelParams) => unknown) {
    this.element = document.createElement("div");
    this.element.className = "wl-panel-content";
    this.element.style.width = "100%";
    this.element.style.height = "100%";
  }
  init(parameters: GroupPanelPartInitParameters): void {
    const kind = panelKindFromInit(parameters);
    if (kind) {
      this.element.dataset.panelKind = kind;
      this.element.classList.add(panelKindClass(kind));
    }
    this._cleanup = render(
      () => this.factory(parameters.params as PanelParams) as never,
      this.element,
    );
  }
  dispose(): void {
    this._cleanup?.();
  }
}

class PanelTab implements ITabRenderer {
  private readonly tab = new DefaultTab();
  readonly element = this.tab.element;

  init(parameters: GroupPanelPartInitParameters): void {
    const kind = panelKindFromInit(parameters);
    this.element.classList.add("wl-dock-tab");
    if (kind) {
      this.element.dataset.panelKind = kind;
      this.element.classList.add(panelKindClass(kind));
    }
    this.tab.init(parameters);
  }

  dispose(): void {
    this.tab.dispose();
  }
}

const components: Record<string, () => IContentRenderer> = {
  sources: () => new SolidPanel(() => SourcesPanel()),
  metrics: () => new SolidPanel(() => MetricsPanel()),
  packetCapture: () => new SolidPanel(() => PacketCapturePanel()),
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
      params: { sid, ch, followSelection: false, panelKind: "terminal" },
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
      defaultTabComponent: "wl-default-tab",
      createComponent: (options) => {
        const factory = components[options.name];
        if (!factory) {
          throw new Error(`E-UI-0002: unknown panel '${options.name}'`);
        }
        return factory();
      },
      createTabComponent: () => new PanelTab(),
    });

    api.addPanel({
      id: "sources",
      component: "sources",
      title: t("panel.sources"),
      params: { panelKind: "sources" },
    });
    api.addPanel({
      id: "metrics",
      component: "metrics",
      title: t("panel.metrics"),
      params: { panelKind: "metrics" },
      position: { referencePanel: "sources", direction: "right" },
    });
    api.addPanel({
      id: "packetCapture",
      component: "packetCapture",
      title: t("panel.packetCapture"),
      params: { panelKind: "packet" },
      position: { referencePanel: "metrics", direction: "below" },
    });
    api.addPanel({
      id: "terminal",
      component: "terminal",
      title: t("panel.terminal"),
      params: { sid: "", ch: 0, panelKind: "terminal" },
      position: { referencePanel: "sources", direction: "below" },
    });
    api.addPanel({
      id: "tiles",
      component: "tiles",
      title: t("panel.tiles"),
      params: { panelKind: "tiles" },
      position: { referencePanel: "terminal", direction: "right" },
    });
    api.addPanel({
      id: "settings",
      component: "settings",
      title: t("panel.settings"),
      params: { panelKind: "settings" },
      position: { referencePanel: "packetCapture", direction: "below" },
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

  const connectionNotice = () => {
    const s = connState();
    if (s.status === "connecting") return t("status.connecting_detail");
    if (s.status === "closed") return t("status.closed_detail");
    if (s.status === "error") return s.message || t("status.error_detail");
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
      <Show when={connectionNotice()}>
        {(message) => (
          <div class={`wl-connection-banner ${statusClass()}`} role="status" aria-live="polite">
            {message()}
          </div>
        )}
      </Show>
      <main class="wl-dock" ref={dockHost!} />
      <footer class="wl-statusbar">
        <span class={`wl-status-dot ${statusClass()}`} />
        <span>{t(`status.${connState().status}`)}</span>
        <Toasts />
      </footer>
    </div>
  );
}
