import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

// Tauri exposes the host on TAURI_DEV_HOST for mobile / remote dev.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  // Force-pre-bundle a few deps that Vite's on-demand optimizer sometimes
  // mis-splits on first run. `react-resizable-panels` had an episode where
  // the bundled output lost its named exports until this was declared.
  //
  // NOTE: do NOT include `monaco-editor` or `@monaco-editor/react` here.
  // Including monaco-editor in optimizeDeps causes Vite to tree-shake the
  // language contribution side-effect imports in main.tsx (because they have
  // no named imports), which silently breaks syntax highlighting for every
  // language except TypeScript. Let Vite handle monaco-editor lazily — slower
  // first-page-load by ~100ms, syntax highlighting that actually works.
  optimizeDeps: {
    include: [
      "react",
      "react-dom",
      "react-dom/client",
      "react-resizable-panels",
      "@xterm/xterm",
      "@xterm/addon-fit",
      "@xterm/addon-search",
      "@xterm/addon-web-links",
    ],
    exclude: ["monaco-editor", "@monaco-editor/react"],
  },
  // Vite options tailored for Tauri development.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Tell Vite to ignore watching `src-tauri` so the dev server doesn't
      // restart on Rust changes (Tauri handles that itself).
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "esnext",
    sourcemap: false,
    chunkSizeWarningLimit: 4000,
    rollupOptions: {
      output: {
        manualChunks(id: string) {
          // Group third-party deps into deterministic vendor chunks.
          if (id.includes("node_modules")) {
            // Example id:
            //   /path/node_modules/react/index.js
            //   /path/node_modules/@tanstack/query-core/build/lib/index.js
            const parts = id.split("node_modules/")[1]?.split(/[\\/]/) ?? [];
            const pkg = parts[0];

            if (!pkg) return "vendor";

            // Normalize scoped packages: "@scope/pkg" -> "@scope-pkg"
            const normalized = pkg.replace(/^@/, "").replace(/\//g, "-");
            return `vendor-${normalized}`;
          }

          if (id.includes("/src/pages/")) return "pages";
          if (id.includes("/src/components/chat")) return "chat";
          if (id.includes("/src/components/editor")) return "editor";
          
          // Fallback: keep app code in a predictable chunk if Rollup would
          // otherwise decide to split it in surprising ways.
          return "app";
        }
      }
    }
  },
}));

