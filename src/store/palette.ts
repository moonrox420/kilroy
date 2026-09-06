/**
 * Command palette open/close state.
 *
 * Kept in its own tiny store so any component can toggle it without
 * threading callbacks through props (and the global Ctrl+Shift+P
 * binding in App.tsx can flip it without coordinating with React refs).
 */
import { create } from "zustand";

interface PaletteState {
  open: boolean;
  toggle: () => void;
  show: () => void;
  hide: () => void;
}

export const usePalette = create<PaletteState>((set) => ({
  open: false,
  toggle: () => set((s) => ({ open: !s.open })),
  show: () => set({ open: true }),
  hide: () => set({ open: false }),
}));
