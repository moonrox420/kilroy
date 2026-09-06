import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

/**
 * Vite plugin that strips dangling sourceMappingURL references from monaco-editor
 * ESM modules (e.g. marked.esm.js.map, purify.es.mjs.map) to eliminate Vite sourcemap 404/ENOENT warnings.
 */
function ignoreMonacoMissingSourcemaps(): Plugin {
  return {
    name: 'ignore-monaco-missing-sourcemaps',
    enforce: 'pre',
    transform: {
      filter: {
        id: /node_modules[\\/](?:monaco-editor|marked|dompurify)[\\/]/,
      },
      handler(code: string) {
        return {
          code: code.replace(/\/\/[#@]\s*sourceMappingURL=\S+/g, ''),
          map: null,
        };
      },
    },
  };
}

export default defineConfig({
  plugins: [
    react(),
    ignoreMonacoMissingSourcemaps(),
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    target: ['es2022', 'chrome105'],
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    chunkSizeWarningLimit: 1200,
  },
});
