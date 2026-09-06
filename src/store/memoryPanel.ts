/**
 * UI state for the Memory dialog + Decision composer.
 *
 * Kept separate from the data-side memory store so these UI flags don't
 * fight for re-renders with the project / session / Ollama state.
 */
import { create } from "zustand";

export type MemoryTab = "sessions" | "decisions" | "files" | "tasks";

interface MemoryPanelState {
  open: boolean;
  tab: MemoryTab;
  decisionComposerOpen: boolean;
  /** Open state of the "+ New Skill" composer modal. Decoupled from the
   *  Memory panel itself so users can launch it from anywhere (palette,
   *  menu, future agent-suggested-skill flow). */
  skillCreatorOpen: boolean;
  openTab: (tab: MemoryTab) => void;
  close: () => void;
  setTab: (tab: MemoryTab) => void;
  openDecisionComposer: () => void;
  closeDecisionComposer: () => void;
  openSkillCreator: () => void;
  closeSkillCreator: () => void;
}

export const useMemoryPanel = create<MemoryPanelState>((set) => ({
  open: false,
  tab: "sessions",
  decisionComposerOpen: false,
  skillCreatorOpen: false,
  openTab: (tab) => set({ open: true, tab }),
  close: () => set({ open: false }),
  setTab: (tab) => set({ tab }),
  openDecisionComposer: () => set({ decisionComposerOpen: true }),
  closeDecisionComposer: () => set({ decisionComposerOpen: false }),
  openSkillCreator: () => set({ skillCreatorOpen: true }),
  closeSkillCreator: () => set({ skillCreatorOpen: false }),
}));
