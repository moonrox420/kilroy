/**
 * UI store — panel collapse/expand state, layout sizes, and theme bits.
 *
 * Sizes persist to localStorage so layout survives reloads.
 */
import { create } from "zustand";
import { persist } from "zustand/middleware";

interface UIState {
  leftCollapsed: boolean;
  terminalCollapsed: boolean;
  rightSize: number;  // px width — chat panel stays exactly this wide
  terminalSize: number; // % of inner vertical space when open

  toggleLeft: () => void;
  toggleTerminal: () => void;
  setTerminalSize: (n: number) => void;
}

export const useUI = create<UIState>()(
  persist(
    (set) => ({
      leftCollapsed: false,
      terminalCollapsed: false,
      rightSize: 360,
      terminalSize: 32,

      toggleLeft: () => set((s) => ({ leftCollapsed: !s.leftCollapsed })),
      toggleTerminal: () =>
        set((s) => ({ terminalCollapsed: !s.terminalCollapsed })),
      setTerminalSize: (n) => set({ terminalSize: n }),
    }),
    {
      name: "kilroy-ui",
      version: 6,
      migrate: (persisted) => {
        const saved = { ...(persisted as Record<string, unknown>) };
        delete saved.leftSize;
        return saved as unknown as UIState;
      },
    },
  ),
);
