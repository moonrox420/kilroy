/**
 * Actuator actions store.
 *
 * Holds the set of pending actions surfaced by the executor. The chat
 * panel renders an ActionCard for each one. When the user accepts /
 * rejects, the backend updates the row and emits actuator://action_resolved;
 * we mirror that into local state so the card transitions smoothly.
 */
import { create } from "zustand";
import { actions, type ActionView } from "@/lib/tauri";

interface ActionsState {
  byId: Record<number, ActionView>;
  byTask: Record<number, number[]>;
  loadForTask: (task_id: number) => Promise<void>;
  upsert: (a: ActionView) => void;
  applyResolution: (action_id: number, status: ActionView["status"], error: string | null) => void;
  accept: (action_id: number) => Promise<void>;
  reject: (action_id: number) => Promise<void>;
  initListeners: () => () => void;
}

export const useActions = create<ActionsState>((set, get) => ({
  byId: {},
  byTask: {},

  async loadForTask(task_id) {
    try {
      const rows = await actions.listPendingForTask(task_id);
      set((s) => {
        const byId = { ...s.byId };
        for (const r of rows) byId[r.id] = r;
        const byTask = { ...s.byTask, [task_id]: rows.map((r) => r.id) };
        return { byId, byTask };
      });
    } catch (err) {
      console.error("loadForTask:", err);
    }
  },

  upsert(a) {
    set((s) => {
      const byId = { ...s.byId, [a.id]: a };
      const taskId = a.task_id;
      let byTask = s.byTask;
      if (taskId != null) {
        const prev = byTask[taskId] ?? [];
        if (!prev.includes(a.id)) {
          byTask = { ...byTask, [taskId]: [...prev, a.id] };
        }
      }
      return { byId, byTask };
    });
  },

  applyResolution(action_id, status, error) {
    set((s) => {
      const a = s.byId[action_id];
      if (!a) return {};
      return {
        byId: {
          ...s.byId,
          [action_id]: { ...a, status, error, resolved_at: Date.now() / 1000 },
        },
      };
    });
  },

  async accept(action_id) {
    try {
      const r = await actions.accept({ action_id });
      get().applyResolution(r.action_id, r.status, r.error);
    } catch (err) {
      console.error("accept_action:", err);
      get().applyResolution(action_id, "failed", String(err));
    }
  },

  async reject(action_id) {
    try {
      const r = await actions.reject(action_id);
      get().applyResolution(r.action_id, r.status, r.error);
    } catch (err) {
      console.error("reject_action:", err);
    }
  },

  initListeners() {
    const unlistens: Array<() => void> = [];

    actions
      .onProposed((e) => {
        // Pull the freshly-inserted action(s) for this task so the UI has
        // full payload / diff to render.
        void get().loadForTask(e.task_id);
      })
      .then((u) => unlistens.push(u));

    actions
      .onResolved((e) => {
        get().applyResolution(e.action_id, e.status, e.error);
      })
      .then((u) => unlistens.push(u));

    return () => {
      for (const u of unlistens) {
        try { u(); } catch { /* ok */ }
      }
    };
  },
}));
