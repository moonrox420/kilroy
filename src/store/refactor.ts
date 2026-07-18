/**
 * Background-refactor store — drives the Refactor panel UI.
 *
 * Owns:
 *   * `open` — dialog visibility
 *   * `candidates` — ranked list of files worth scanning (fetched on
 *     panel open, cheap)
 *   * `proposals` — inbox of pending refactor suggestions (fetched on
 *     panel open and refreshed after each scan or apply/dismiss)
 *   * `scanning` — the currently in-flight scan (one file at a time
 *     for now; future pass parallelises against multiple files via the
 *     scheduler)
 *   * `liveVoices` — buffered streaming output from the active scan so
 *     the panel can render the four-card swarm view while a scan runs
 *
 * Listeners are registered once at app boot via `initListeners()`. The
 * StrictMode-safe pattern (module-level `_attached` guard) matches the
 * runtime / council stores.
 */
import { create } from "zustand";
import {
  refactor,
  type RefactorCandidate,
  type RefactorProposal,
  type RefactorScanStats,
  type RefactorVoice,
} from "@/lib/tauri";

interface LiveVoiceState {
  voices: Record<RefactorVoice, string>;
  voicesDone: Record<RefactorVoice, boolean>;
  synthesis: string;
  filePath: string | null;
}

const EMPTY_LIVE: LiveVoiceState = {
  voices: {
    duplicate: "",
    complexity: "",
    error_handling: "",
    modernizer: "",
  },
  voicesDone: {
    duplicate: false,
    complexity: false,
    error_handling: false,
    modernizer: false,
  },
  synthesis: "",
  filePath: null,
};

interface RefactorState {
  open: boolean;
  candidates: RefactorCandidate[];
  proposals: RefactorProposal[];
  stats: RefactorScanStats | null;
  /** File path currently being scanned, or null when idle. */
  scanning: string | null;
  live: LiveVoiceState;

  openPanel: () => void;
  closePanel: () => void;
  refreshAll: () => Promise<void>;
  refreshProposals: () => Promise<void>;
  refreshCandidates: () => Promise<void>;
  startScan: (filePath: string) => Promise<void>;
  applyProposal: (id: number) => Promise<number | null>;
  dismissProposal: (id: number) => Promise<void>;
  initListeners: () => () => void;
}

let _attached = false;
const _disposers: Array<() => void> = [];

export const useRefactor = create<RefactorState>((set, get) => ({
  open: false,
  candidates: [],
  proposals: [],
  stats: null,
  scanning: null,
  live: { ...EMPTY_LIVE },

  openPanel: () => {
    set({ open: true });
    void get().refreshAll();
  },
  closePanel: () => set({ open: false }),

  async refreshAll() {
    await Promise.all([get().refreshCandidates(), get().refreshProposals()]);
    try {
      const stats = await refactor.stats();
      set({ stats });
    } catch (err) {
      console.error("refactor.stats:", err);
    }
  },

  async refreshProposals() {
    try {
      const proposals = await refactor.listProposals({ limit: 100 });
      set({ proposals });
    } catch (err) {
      console.error("refactor.listProposals:", err);
    }
  },

  async refreshCandidates() {
    try {
      const candidates = await refactor.scanCandidates(20);
      set({ candidates });
    } catch (err) {
      // No project open → empty list. Don't toast — this is the
      // common case on first launch.
      set({ candidates: [] });
    }
  },

  async startScan(filePath) {
    if (get().scanning) return; // one at a time for now
    set({
      scanning: filePath,
      live: { ...EMPTY_LIVE, filePath },
    });
    try {
      await refactor.analyzeFile({ file_path: filePath });
      // Listener will refresh proposals on scan_done.
    } catch (err) {
      console.error("refactor.analyzeFile:", err);
      set({ scanning: null });
    }
  },

  async applyProposal(id) {
    try {
      const actionId = await refactor.apply(id);
      await get().refreshProposals();
      return actionId;
    } catch (err) {
      console.error("refactor.apply:", err);
      return null;
    }
  },

  async dismissProposal(id) {
    try {
      await refactor.dismiss(id);
      await get().refreshProposals();
    } catch (err) {
      console.error("refactor.dismiss:", err);
    }
  },

  initListeners() {
    if (_attached) return () => {};
    _attached = true;

    void refactor
      .onVoiceChunk((c) => {
        set((s) => ({
          live: {
            ...s.live,
            voices: {
              ...s.live.voices,
              [c.voice]: s.live.voices[c.voice] + c.delta,
            },
          },
        }));
      })
      .then((u) => _disposers.push(u));

    void refactor
      .onVoiceDone((c) => {
        set((s) => ({
          live: {
            ...s.live,
            voices: { ...s.live.voices, [c.voice]: c.content },
            voicesDone: { ...s.live.voicesDone, [c.voice]: true },
          },
        }));
      })
      .then((u) => _disposers.push(u));

    void refactor
      .onSynthesis((c) => {
        set((s) => ({
          live: { ...s.live, synthesis: s.live.synthesis + c.delta },
        }));
      })
      .then((u) => _disposers.push(u));

    void refactor
      .onScanDone(() => {
        // Wrap up the live state, refresh the inbox.
        set({ scanning: null });
        void get().refreshProposals();
        void refactor.stats().then((stats) => set({ stats })).catch(() => {});
        // Leave `live` populated so the user sees the final state for
        // a beat. They can dismiss the panel or start another scan to
        // clear it.
      })
      .then((u) => _disposers.push(u));

    return () => {
      for (const u of _disposers) {
        try {
          u();
        } catch {}
      }
      _disposers.length = 0;
      _attached = false;
    };
  },
}));
