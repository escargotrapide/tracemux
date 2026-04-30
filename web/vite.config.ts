import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { fileURLToPath } from "node:url";

// Vite config for the wanlogger web UI.
// Dev scripts point VITE_WANLOGGER_URL at the local loopback backend
// (default: ws://127.0.0.1:9000/ws).
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
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("@xterm")) return "vendor-xterm";
          if (id.includes("dockview")) return "vendor-dockview";
          if (id.includes("solid-js")) return "vendor-solid";
          if (id.includes("msgpackr")) return "vendor-msgpack";
          return "vendor";
        },
      },
    },
  },
  define: {
    __WANLOGGER_WIRE__: JSON.stringify("wanlogger.v1"),
  },
}));
