/**
 * Shared Monaco editor command bridge.
 *
 * MonacoPane stores its editor instance here on mount. MenuBar reads it
 * to dispatch Edit/Selection commands via `editor.trigger()` instead of
 * the ineffective `document.execCommand()`.
 */
import type { editor as MonacoEditor } from "monaco-editor";

let _editor: MonacoEditor.IStandaloneCodeEditor | null = null;

export function setActiveEditor(e: MonacoEditor.IStandaloneCodeEditor | null) {
  _editor = e;
}

export function getActiveEditor(): MonacoEditor.IStandaloneCodeEditor | null {
  return _editor;
}

/**
 * Dispatch a Monaco editor action. Safe to call when no editor is mounted.
 */
export function editorAction(actionId: string): void {
  _editor?.trigger("menu", actionId, null);
}