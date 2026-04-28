import { render } from "solid-js/web";
import "@xterm/xterm/css/xterm.css";
import "dockview-core/dist/styles/dockview.css";
import "./styles.css";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) {
  throw new Error("E-UI-0001: #root element missing");
}

render(() => <App />, root);
