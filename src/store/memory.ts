/**
 * Memory store — the agent's persistent brain status.
 *
 * Holds: which project is open, which session is active, Ollama
 * reachability, indexing progress. Lives alongside `workspace` (files
 * and tabs) and `agent` (chat history) without overlapping them.
 */
import { create } from "zustand";
import {
  memory,
  type ClearIndexResult,
  type IndexProgress,
  type IndexResult,
  type OllamaStatus,
  type Project,
  type Session,
} from "@/lib/tauri";
import { notify } from "./notifications";

interface MemoryState {
  project: Project | null;
  session: Session | null;
  ollama: OllamaStatus | null;

  indexing: boolean;
  indexProgress: IndexProgress | null;
  lastIndex: IndexResult | null;

  setProject: (p: Project) => void;
  setSession: (s: Session) => void;
  setOllama: (s: OllamaStatus) => void;
  setIndexProgress: (p: IndexProgress | null) => void;
  setIndexResult: (r: IndexResult) => void;

  beginIndex: () => Promise<IndexResult | null>;
  clearIndex: () => Promise<ClearIndexResult | null>;
}

export const useMemory = create<MemoryState>((set) => ({
  project: null,
  session: null,
  ollama: null,

  indexing: false,
  indexProgress: null,
  lastIndex: null,

  setProject: (p) => set({ project: p }),
  setSession: (s) => set({ session: s }),
  setOllama: (s) => set({ ollama: s }),
  setIndexProgress: (p) => set({ indexProgress: p }),
  setIndexResult: (r) => set({ lastIndex: r, indexing: false, indexProgress: null }),

  async beginIndex() {
    set({ indexing: true, indexProgress: null });
    try {
      const result = await memory.indexProject();
      set({ lastIndex: result, indexing: false, indexProgress: null });

      // FULL breakdown so a "0 files indexed" result tells us WHY:
      //   files_seen      = how many file candidates the walker found
      //   files_indexed   = how many got embedded into chunks this run
      //   skipped_too_large = >1MB by default, would be too noisy
      //   skipped_binary  = looked binary by content sniff (NUL bytes etc.)
      //   errors          = read errors, embedding failures, DB errors
      //
      // Common patterns:
      //   files_seen=0                       → walker hit nothing (empty folder,
      //                                        wrong root, or everything filtered
      //                                        by is_ignored). Open file explorer
      //                                        to verify what's in the project.
      //   files_seen>0 + indexed=0 + skipped_binary=large
      //                                      → files are being misclassified as
      //                                        binary; usually whitespace/encoding.
      //   files_seen>0 + indexed=0 + errors>0
      //                                      → Ollama embedding failing; check
      //                                        the Ollama dot in the status bar.
      //   files_seen>0 + indexed=0 + others=0
      //                                      → all already indexed (same hash);
      //                                        no work needed.
      const r = result;
      const headline = r.files_indexed > 0
        ? `${r.chunks_inserted} chunks across ${r.files_indexed} files`
        : r.files_seen === 0
          ? "walker found 0 candidate files — project root empty or everything filtered"
          : r.errors > 0
            ? `${r.errors} files errored — check Ollama is reachable`
            : r.files_seen === r.skipped_binary
              ? `all ${r.files_seen} candidates looked binary (NUL bytes detected)`
              : `${r.files_seen} candidates, 0 indexed (likely all already up-to-date or filtered)`;
      const breakdown = `seen=${r.files_seen} indexed=${r.files_indexed} skipped_large=${r.skipped_too_large} skipped_binary=${r.skipped_binary} errors=${r.errors}`;
      const ok = r.files_indexed > 0;
      if (ok) {
        notify.success("Project indexed", `${headline} · ${breakdown}`);
      } else {
        notify.warn("Project indexed — 0 files", `${headline} · ${breakdown}`);
      }
      return result;
    } catch (err) {
      // Indexing failure is high-impact — the agent's retrieval relies
      // on it. Surface the error AND the partial state so the user can
      // re-trigger after fixing whatever's wrong (usually Ollama or
      // disk space).
      notify.fromError("Index project", err);
      set({ indexing: false, indexProgress: null });
      return null;
    }
  },

  async clearIndex() {
    try {
      const result = await memory.clearProjectIndex();
      set({ lastIndex: null });
      if (result.files_removed > 0 || result.chunks_removed > 0) {
        notify.success(
          "Project index cleared",
          `Removed ${result.chunks_removed} chunks across ${result.files_removed} files. Chat history kept — use Index Project when ready.`,
        );
      } else {
        notify.info("Project index", "Nothing indexed for this folder.");
      }
      return result;
    } catch (err) {
      notify.fromError("Clear project index", err);
      return null;
    }
  },
}));

// Listener — keep store in sync with progress events from the Rust indexer.
void memory
  .onIndexProgress((p) => {
    useMemory.getState().setIndexProgress(p);
  })
  .catch((err) => console.error("indexProgress listener:", err));
