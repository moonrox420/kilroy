/**
 * Monaco editor pane.
 *
 * Renders the active tab. We register a custom "kilroy" theme tied to
 * our CSS tokens so the editor blends with the rest of the chrome.
 * When no file is open, we render the KilroyWatermark instead.
 */
import { useRef } from "react";
import Editor, {
  type Monaco,
  type OnMount,
  type BeforeMount,
} from "@monaco-editor/react";
import type { editor as MonacoEditor } from "monaco-editor";
import { useWorkspace } from "@/store/workspace";
import { KilroyWatermark } from "./KilroyWatermark";
import { EditorTabs } from "./EditorTabs";
import { setActiveEditor } from "@/lib/editorCommands";

// Lazy theme registration — runs once when Monaco is ready.
let themeRegistered = false;
function registerKilroyTheme(monacoNs: Monaco) {
  if (themeRegistered) return;
  themeRegistered = true;
  monacoNs.editor.defineTheme("kilroy", {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "comment", foreground: "5a6068", fontStyle: "italic" },
      { token: "keyword", foreground: "f59e0b" },
      { token: "string", foreground: "c8e1a0" },
      { token: "number", foreground: "e0b97c" },
      { token: "type", foreground: "8ab9f0" },
      { token: "function", foreground: "f5d77b" },
      { token: "variable", foreground: "d8dde2" },
    ],
    colors: {
      "editor.background": "#0d1014",
      "editor.foreground": "#d8dde2",
      "editorCursor.foreground": "#f59e0b",
      "editor.lineHighlightBackground": "#161a1f",
      "editor.lineHighlightBorder": "#161a1f",
      "editorLineNumber.foreground": "#3a4047",
      "editorLineNumber.activeForeground": "#f59e0b",
      "editorIndentGuide.background1": "#1a1f25",
      "editorIndentGuide.activeBackground1": "#2a3038",
      "editor.selectionBackground": "#f59e0b33",
      "editor.inactiveSelectionBackground": "#f59e0b1a",
      "editorWidget.background": "#14181d",
      "editorWidget.border": "#262b32",
      "scrollbarSlider.background": "#2a303890",
      "scrollbarSlider.hoverBackground": "#3a4047b0",
      "scrollbarSlider.activeBackground": "#4a5057c0",
    },
  });
}

export function MonacoPane() {
  const tabs = useWorkspace((s) => s.openTabs);
  const activePath = useWorkspace((s) => s.activePath);
  const updateContents = useWorkspace((s) => s.updateContents);

  const active = tabs.find((t) => t.path === activePath) ?? null;
  const editorRef = useRef<MonacoEditor.IStandaloneCodeEditor | null>(null);

  // Register the theme BEFORE the editor instantiates, so the
  // `theme="kilroy"` prop resolves to a defined theme on the very first
  // paint. Doing it in onMount (after creation) left a window where the
  // prop referenced an undefined theme and Monaco fell back to its
  // default — which reads as "highlighting/colors look wrong."
  const beforeMount: BeforeMount = (monacoNs) => {
    registerKilroyTheme(monacoNs);
  };

  const onMount: OnMount = (editor, monacoNs) => {
    editorRef.current = editor;
    setActiveEditor(editor);
    // Belt-and-suspenders: ensure the theme is active even if the prop
    // application raced.
    monacoNs.editor.setTheme("kilroy");
  };

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg-0">
      <EditorTabs />
      <div className="relative min-h-0 flex-1">
        {active ? (
          <Editor
            key={active.path}
            value={active.contents}
            language={active.language}
            theme="kilroy"
            beforeMount={beforeMount}
            onMount={onMount}
            onChange={(v) => updateContents(active.path, v ?? "")}
            options={{
              readOnly: active.readOnly === true,
              readOnlyMessage: {
                value: "This is an agent preview. Approve the action to write it to disk.",
              },
              fontFamily:
                "'JetBrains Mono', ui-monospace, Consolas, Cascadia Code, monospace",
              fontLigatures: true,
              fontSize: 13,
              lineHeight: 20,
              minimap: { enabled: true, renderCharacters: false, scale: 1 },
              smoothScrolling: true,
              cursorBlinking: "smooth",
              cursorSmoothCaretAnimation: "on",
              renderWhitespace: "selection",
              renderLineHighlight: "all",
              padding: { top: 12, bottom: 12 },
              scrollbar: {
                verticalScrollbarSize: 10,
                horizontalScrollbarSize: 10,
                useShadows: false,
              },
              guides: {
                indentation: true,
                bracketPairs: true,
              },
              bracketPairColorization: { enabled: true },
              automaticLayout: true,
              tabSize: 2,
              wordWrap: "off",
            }}
          />
        ) : (
          <KilroyWatermark />
        )}
      </div>
    </div>
  );
}
