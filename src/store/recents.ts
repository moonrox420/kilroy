/**
 * Recently-opened files, persisted to localStorage.
 *
 * Pushed to by `useWorkspace.openFile`. The Command Palette reads it
 * to surface recent files at the top of the list when no query is
 * typed. Max 30 entries, most-recent first, dedupe-on-push.
 */
import { create } from "zustand";
import { persist } from "zustand/middleware";

interface RecentsState {
  files: string[];
  push: (path: string) => void;
  remove: (path: string) => void;
  clear: () => void;
}

export const useRecents = create<RecentsState>()(
  persist(
    (set) => ({
      files: [],
      push: (path) =>
        set((s) => ({
          files: [path, ...s.files.filter((f) => f !== path)].slice(0, 30),
        })),
      remove: (path) =>
        set((s) => ({ files: s.files.filter((f) => f !== path) })),
      clear: () => set({ files: [] }),
    }),
    { name: "kilroy-recents" },
  ),
);
