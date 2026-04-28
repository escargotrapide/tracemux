import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { fileURLToPath } from "node:url";

// Vite config for the wanlogger web UI.
// Dev server proxies /ws to a locally-running `wanlogger serve`
// (default: wss://127.0.0.1:7443). Override with VITE_WANLOGGER_URL.
export default defineConfig(({ mode }) => ({
  plugins: [solid()],
  resolve: {
    alias: {
      "~": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    host: "127.0.0.1",
  },
  build: {
    target: "es2022",
    sourcemap: mode !== "production",
    outDir: "dist",
    emptyOutDir: true,
  },
  define: {
    __WANLOGGER_WIRE__: JSON.stringify("wanlogger.v1"),
  },
}));
