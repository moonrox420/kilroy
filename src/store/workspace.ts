/**
 * Workspace store — the single source of truth for which folder is open,
 * which files are open in tabs, and which file the editor is currently on.
 *
 * Opening a folder also triggers the memory side-effects:
 *   1. Open the per-project SQLite (memory.openProject).
 *   2. Load the existing chat history into the agent store.
 *   3. Surface Ollama health to the memory store so the UI can react.
 *
 * "Untitled" tabs have a synthetic path of `untitled:N` (the `:` makes it
 * invalid as a real Windows path so it can never collide with a real
 * file). The first time you Save one, we prompt for a real path via the
 * native save dialog and migrate the tab to point at the new path.
 */
import { create } from "zustand";
import { fs, memory } from "@/lib/tauri";
import { languageForPath, basename } from "@/lib/utils";
import { useAgent } from "./agent";
import { useMemory } from "./memory";
import { useRecents } from "./recents";
import { notify } from "./notifications";

export interface OpenFile {
  path: string;
  name: string;
  language: string;
  contents: string;
  dirty: boolean;
  /** Agent previews are immutable until their approval action is accepted. */
  readOnly?: boolean;
  /** Real project path represented by a synthetic preview tab. */
  sourcePath?: string;
  agentPreview?: boolean;
}

interface WorkspaceState {
  rootPath: string | null;
  openTabs: OpenFile[];
  activePath: string | null;
  /** Monotonic counter for default Untitled names. */
  untitledCounter: number;

  openFolder: (path?: string) => Promise<void>;
  openFile: (path: string) => Promise<void>;
  newUntitled: () => void;
  /** Open a new untitled tab pre-populated with contents (e.g. the
   *  "Open" button on a chat code block with no path hint). */
  newUntitledWith: (opts: { contents: string; language?: string }) => void;
  showAgentPreview: (opts: {
    actionId: number;
    path: string;
    contents: string;
  }) => void;
  resolveAgentPreview: (actionId: number, status: string) => Promise<void>;
  closeTab: (path: string) => void;
  setActive: (path: string) => void;
  updateContents: (path: string, contents: string) => void;
  saveActive: () => Promise<void>;
  saveActiveAs: () => Promise<void>;
  saveAll: () => Promise<void>;
  /** Write a code block (from chat) to a path on disk + hot-reload any
   *  open editor tab pointing at that path. */
  applyCodeBlock: (path: string, contents: string) => Promise<void>;
}

/** Untitled tabs use `untitled:N` as their synthetic path. */
function isUntitled(path: string): boolean {
  return path.startsWith("untitled:");
}

export const useWorkspace = create<WorkspaceState>((set, get) => ({
  rootPath: null,
  openTabs: [],
  activePath: null,
  untitledCounter: 0,

  async openFolder(path) {
    const chosen = path ?? (await fs.pickFolder());
    if (!chosen) return;
    set({ rootPath: chosen });

    // Hand off to the memory layer: open the project DB, load prior chat.
    try {
      const opened = await memory.openProject(chosen);
      useMemory.getState().setProject(opened.project);
      useMemory.getState().setSession(opened.session);
      useMemory.getState().setOllama(opened.ollama_status);
      useAgent.getState().loadHistory(opened.messages);

      // Auto-index only when the active project has no indexed chunks yet.
      // The status is persisted in the project-local .kilroy/memory.db, so
      // this won't re-trigger on each relaunch after the first successful index.
      const status = await memory.projectIndexStatus();
      if (status.is_indexed) {
        return;
      }
      if (status.files_indexed === 0 && status.chunks_indexed === 0) {
        // Fire-and-forget: the user can see progress via the status bar.
        void useMemory.getState().beginIndex();
      }
    } catch (err) {
      // Folder open failure is loud — user picked a folder and the
      // memory DB couldn't be created (disk full, permissions, etc.).
      // Roll back rootPath so the status bar doesn't claim a project
      // is open when there isn't.
      notify.fromError(`Open project ${chosen}`, err);
      set({ rootPath: null });
    }
  },

  async openFile(path) {
    const existing = get().openTabs.find((t) => t.path === path);
    if (existing) {
      set({ activePath: path });
      useRecents.getState().push(path);
      return;
    }
    try {
      const contents = await fs.readFile(path);
      const file: OpenFile = {
        path,
        name: basename(path),
        language: languageForPath(path),
        contents,
        dirty: false,
      };
      set((s) => ({ openTabs: [...s.openTabs, file], activePath: path }));
      useRecents.getState().push(path);
    } catch (err) {
      // Clicking a file in the explorer that fails to open silently
      // does nothing — confusing. Toast tells the user why (permission
      // denied, binary file, encoding issue).
      notify.fromError(`Open ${basename(path)}`, err);
    }
  },

  newUntitled() {
    set((s) => {
      const n = s.untitledCounter + 1;
      const path = `untitled:${n}`;
      const file: OpenFile = {
        path,
        name: `Untitled ${n}`,
        // Plain text by default. We re-derive on save once a real
        // extension exists.
        language: "plaintext",
        contents: "",
        // Untitled buffers are NOT considered dirty — they only become
        // dirty after the user types something. That avoids the "•" mark
        // appearing the moment the tab opens.
        dirty: false,
      };
      return {
        openTabs: [...s.openTabs, file],
        activePath: path,
        untitledCounter: n,
      };
    });
  },

  newUntitledWith({ contents, language }) {
    set((s) => {
      const n = s.untitledCounter + 1;
      const path = `untitled:${n}`;
      const file: OpenFile = {
        path,
        name: `Untitled ${n}`,
        language: language || "plaintext",
        contents,
        // Pre-populated untitled tabs ARE dirty — the user explicitly
        // asked to dump content in, so the "•" reminds them to save.
        dirty: true,
      };
      return {
        openTabs: [...s.openTabs, file],
        activePath: path,
        untitledCounter: n,
      };
    });
  },

  showAgentPreview({ actionId, path, contents }) {
    const previewPath = `agent-preview:${actionId}`;
    const file: OpenFile = {
      path: previewPath,
      name: `${basename(path)} (Agent Preview)`,
      language: languageForPath(path),
      contents,
      dirty: false,
      readOnly: true,
      sourcePath: path,
      agentPreview: true,
    };
    set((s) => {
      const exists = s.openTabs.some((tab) => tab.path === previewPath);
      return {
        openTabs: exists
          ? s.openTabs.map((tab) => (tab.path === previewPath ? file : tab))
          : [...s.openTabs, file],
        activePath: previewPath,
      };
    });
  },

  async resolveAgentPreview(actionId, status) {
    const previewPath = `agent-preview:${actionId}`;
    const preview = get().openTabs.find((tab) => tab.path === previewPath);
    if (!preview) return;
    const sourcePath = preview.sourcePath;
    get().closeTab(previewPath);
    if (status !== "applied" || !sourcePath) return;

    try {
      const contents = await fs.readFile(sourcePath);
      const existing = get().openTabs.find((tab) => tab.path === sourcePath);
      if (existing?.dirty) {
        notify.warn(
          `Applied ${basename(sourcePath)} on disk`,
          "The existing editor tab has unsaved changes, so its buffer was not overwritten.",
        );
        return;
      }
      const file: OpenFile = {
        path: sourcePath,
        name: basename(sourcePath),
        language: languageForPath(sourcePath),
        contents,
        dirty: false,
      };
      set((s) => ({
        openTabs: existing
          ? s.openTabs.map((tab) => (tab.path === sourcePath ? file : tab))
          : [...s.openTabs, file],
        activePath: sourcePath,
      }));
    } catch (err) {
      notify.fromError(`Refresh ${basename(sourcePath)}`, err);
    }
  },

  closeTab(path) {
    set((s) => {
      const remaining = s.openTabs.filter((t) => t.path !== path);
      let nextActive = s.activePath;
      if (s.activePath === path) {
        const idx = s.openTabs.findIndex((t) => t.path === path);
        nextActive = remaining[idx]?.path ?? remaining[idx - 1]?.path ?? null;
      }
      return { openTabs: remaining, activePath: nextActive };
    });
  },

  setActive(path) {
    set({ activePath: path });
  },

  updateContents(path, contents) {
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.path === path && !t.readOnly
          ? { ...t, contents, dirty: true }
          : t,
      ),
    }));
  },

  async saveActive() {
    const { activePath, openTabs } = get();
    if (!activePath) return;
    const tab = openTabs.find((t) => t.path === activePath);
    if (!tab) return;
    if (tab.readOnly) {
      notify.warn("Agent previews are read-only until the action is approved.");
      return;
    }

    // Untitled tabs need a real path before we can write — fall through
    // to Save As, which prompts the user.
    if (isUntitled(tab.path)) {
      await get().saveActiveAs();
      return;
    }

    try {
      await fs.writeFile(tab.path, tab.contents);
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.path === tab.path ? { ...t, dirty: false } : t,
        ),
      }));
    } catch (err) {
      notify.fromError(`Save ${tab.name}`, err);
    }
  },

  async saveActiveAs() {
    const { activePath, openTabs } = get();
    if (!activePath) return;
    const tab = openTabs.find((t) => t.path === activePath);
    if (!tab) return;
    if (tab.readOnly) {
      notify.warn("Agent previews are read-only until the action is approved.");
      return;
    }

    const suggested = isUntitled(tab.path) ? tab.name + ".txt" : tab.name;
    const chosen = await fs.pickSaveFile(suggested);
    if (!chosen) return; // user cancelled

    try {
      await fs.writeFile(chosen, tab.contents);

      // Migrate the tab to its new real path.
      const newName = basename(chosen);
      const newLang = languageForPath(chosen);
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.path === tab.path
            ? {
                ...t,
                path: chosen,
                name: newName,
                language: newLang,
                dirty: false,
              }
            : t,
        ),
        activePath: s.activePath === tab.path ? chosen : s.activePath,
      }));
      useRecents.getState().push(chosen);
    } catch (err) {
      notify.fromError(`Save ${tab.name} as`, err);
    }
  },

  async saveAll() {
    const dirty = get().openTabs.filter(
      (t) => t.dirty && !t.readOnly && !isUntitled(t.path),
    );
    const errors: string[] = [];
    for (const t of dirty) {
      try {
        await fs.writeFile(t.path, t.contents);
      } catch (err) {
        errors.push(`${t.name}: ${err}`);
      }
    }
    if (errors.length > 0) {
      notify.fromError("Save All", errors.join("\n"));
      // Still mark the successful ones as clean.
    }
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        isUntitled(t.path) || errors.some((e) => e.startsWith(t.name))
          ? t
          : { ...t, dirty: false },
      ),
    }));
    // Untitled tabs are left dirty because each needs its own Save As
    // dialog; the user can step through them individually.
  },

  async applyCodeBlock(path, contents) {
    try {
      await fs.writeFile(path, contents);
    } catch (err) {
      notify.fromError(`Apply code block to ${basename(path)}`, err);
      return;
    }
    // If the user has that file open in a tab, sync the editor buffer
    // with what's now on disk. Without this they'd keep seeing the old
    // content until they close-and-reopen, which is the worst possible
    // UX for "Apply".
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.path === path
          ? { ...t, contents, dirty: false }
          : t,
      ),
    }));
    // Track it as a recent file so the palette can find it later.
    useRecents.getState().push(path);
  },
}));
