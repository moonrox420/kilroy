/**
 * Distillation corpus store — caches the per-project corpus stats and
 * tracks which agent messages have been blessed during the current
 * session.
 *
 * Why a separate store: the corpus is fetched lazily on chat-panel
 * mount + after every successful append, so we want a single shared
 * cache that the banner, the FeedbackButton, and the DatasetsDialog can
 * all read without re-issuing the same `corpus_stats` IPC.
 *
 * Why per-session "saved set": agent message IDs aren't durably linked
 * to corpus rows on disk (we append plain JSONL by content, not by
 * message id). To prevent double-clicks adding the same exchange twice
 * within a single session, we remember which message IDs have been
 * thumbs-up'd. Re-launching the app clears the set — which is fine,
 * the corpus file itself stays intact.
 */
import { create } from "zustand";
import { corpus, type CorpusStats } from "@/lib/tauri";

interface CorpusState {
  stats: CorpusStats | null;
  /** Message ids the user has saved this session. Used to disable the
   *  thumbs-up button on a message that's already in the corpus. */
  savedIds: Set<string>;

  refresh: () => Promise<void>;
  markSaved: (messageId: string, stats: CorpusStats) => void;
}

export const useCorpus = create<CorpusState>((set) => ({
  stats: null,
  savedIds: new Set<string>(),

  async refresh() {
    try {
      const s = await corpus.stats();
      set({ stats: s });
    } catch (err) {
      // Corpus is per-project — if no project is open, the backend
      // returns a zero-state, not an error. A real error here is
      // unusual; log but don't toast (chat panel polls passively).
      console.error("corpus.stats:", err);
    }
  },

  markSaved(messageId, stats) {
    set((s) => {
      const next = new Set(s.savedIds);
      next.add(messageId);
      return { savedIds: next, stats };
    });
  },
}));
