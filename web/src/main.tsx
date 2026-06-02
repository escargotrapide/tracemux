import { render } from "solid-js/web";
import "@xterm/xterm/css/xterm.css";
import "dockview-core/dist/styles/dockview.css";
import "./styles.css";
import { App } from "./App";
import {
  __ingestFrameForTest,
  __setClientForTest,
  __setConnStateForTest,
  connState,
  openTerminalChannel,
  sendCtl,
  sourcesStore,
  useChannel,
} from "./state";

const root = document.getElementById("root");
if (!root) {
  throw new Error("E-UI-0001: #root element missing");
}

// Dev / e2e injection hook. Stripped in production builds.
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__tracemuxInject =
    __ingestFrameForTest;
  (window as unknown as Record<string, unknown>).__tracemuxSetClient =
    __setClientForTest;
  (window as unknown as Record<string, unknown>).__tracemuxSetConnState =
    __setConnStateForTest;
  // Real-backend e2e helpers. Unlike the injection hooks above, these drive
  // the *live* WireClient against a real server (no fake/spy client), so the
  // GUI smoke suite can exercise the full browser -> WSS -> source path.
  (window as unknown as Record<string, unknown>).__tracemuxRealApi = {
    connStatus: () => connState().status,
    sources: () => Object.values(sourcesStore).map((s) => ({ ...s })),
    sendCtl,
    subscribe: useChannel,
    openTerminal: openTerminalChannel,
  };
}

render(() => <App />, root);
