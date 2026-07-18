/**
 * Activity feed store — list + refresh of the activity table.
 */
import { create } from "zustand";
import { activity, type ActivityView } from "@/lib/tauri";

interface ActivityState {
  rows: ActivityView[];
  loading: boolean;
  load: (opts?: { session_only?: boolean; limit?: number }) => Promise<void>;
}

export const useActivity = create<ActivityState>((set) => ({
  rows: [],
  loading: false,
  async load(opts) {
    set({ loading: true });
    try {
      const rows = await activity.list(opts);
      set({ rows, loading: false });
    } catch (err) {
      console.error("list_activity:", err);
      set({ loading: false });
    }
  },
}));
