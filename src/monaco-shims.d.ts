// Belt-and-suspenders type shim for monaco-editor deep subpath imports.
//
// monaco-editor publishes its ESM modules under `monaco-editor/esm/...`
// but its package.json's `types` / `typesVersions` map does NOT cover those
// subpaths. So `import * as monaco from "monaco-editor"` gets full TS types
// (resolves to esm/vs/editor/editor.api.d.ts), but
// `import "monaco-editor/esm/vs/language/json/json.worker?worker"` has no
// .d.ts and TypeScript rejects it with TS2882.
//
// This wildcard ambient declaration tells TS that any `monaco-editor/esm/...`
// path is a valid side-effect module. The `?worker` suffix is a Vite plugin
// concern, not a TS concern — Vite handles it at build time. This shim only
// silences the type-checker so tsc -b passes.
//
// If a future audit wants stricter types per subpath, replace the wildcard
// with per-path declarations.
declare module "monaco-editor/esm/*";
declare module "monaco-editor/esm/*?worker";
