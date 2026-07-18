import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/globals.css";

// ─── Local Monaco bootstrap ──────────────────────────────────────────────────
//
// Kilroy is offline-first; we cannot rely on Monaco being fetched from a CDN.
// We configure `@monaco-editor/react`'s loader to use the locally bundled
// `monaco-editor` package, and we register Web Workers for language services
// so things like TypeScript and JSON give real diagnostics.
//
// The `import * as monaco from "monaco-editor"` line imports the package's
// main entry — in current monaco-editor versions that IS `editor.main.js`,
// which side-effect-registers every basic-languages tokenizer (python, rust,
// go, csharp, php, sql, powershell, yaml, markdown, dockerfile, ...) plus
// the full TS/JS/JSON/CSS/HTML language services. One import, everything.
//
// `?worker` is a Vite-native suffix that imports a file as a Web Worker
// constructor — these are the workers that drive completions / diagnostics
// for the four languages Monaco ships first-class servers for.
// ─────────────────────────────────────────────────────────────────────────────
import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";

import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import JsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import CssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import HtmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import TsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

self.MonacoEnvironment = {
  getWorker(_workerId, label) {
    switch (label) {
      case "json":
        return new JsonWorker();
      case "css":
      case "scss":
      case "less":
        return new CssWorker();
      case "html":
      case "handlebars":
      case "razor":
        return new HtmlWorker();
      case "typescript":
      case "javascript":
        return new TsWorker();
      default:
        return new EditorWorker();
    }
  },
};

loader.config({ monaco });

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
