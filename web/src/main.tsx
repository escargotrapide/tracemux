import { render } from "solid-js/web";
import "@xterm/xterm/css/xterm.css";
import "dockview-core/dist/styles/dockview.css";
import "./styles.css";
import { App } from "./App";
import { __ingestFrameForTest, __setClientForTest, __setConnStateForTest } from "./state";

const root = document.getElementById("root");
if (!root) {
  throw new Error("E-UI-0001: #root element missing");
}

// Dev / e2e injection hook. Stripped in production builds.
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__wanloggerInject =
    __ingestFrameForTest;
  (window as unknown as Record<string, unknown>).__wanloggerSetClient =
    __setClientForTest;
  (window as unknown as Record<string, unknown>).__wanloggerSetConnState =
    __setConnStateForTest;
}

render(() => <App />, root);
