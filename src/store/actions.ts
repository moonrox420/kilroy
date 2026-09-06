/**
 * Actuator actions store.
 *
 * Holds the set of pending actions surfaced by the executor. The chat
 * panel renders an ActionCard for each one. When the user accepts /
 * rejects, the backend updates the row and emits actuator://action_resolved;
 * we mirror that into local state so the card transitions smoothly.
 */
import { create } from "zustand";
import { actions, type ActionView, type ActionResolved } from "@/lib/tauri";
import { notify } from "./notifications";
import { createListenerScope } from "@/lib/listenerScope";

interface ActionsState {
  byId: Record<number, ActionView>;
  byTask: Record<number, number[]>;
  generation: number;
  reset: () => void;
  loadForTask: (task_id: number) => Promise<void>;
  upsert: (a: ActionView) => void;
  applyResolution: (action_id: number, status: ActionView["status"], error: string | null) => void;
  accept: (action_id: number, override_diff?: string | null) => Promise<ActionResolved>;
  reject: (action_id: number) => Promise<void>;
  initListeners: () => () => void;
}

export const useActions = create<ActionsState>((set, get) => ({
  byId: {},
  byTask: {},
  generation: 0,
  reset: () => set((state) => ({ byId: {}, byTask: {}, generation: state.generation + 1 })),

  async loadForTask(task_id) {
    const generation = get().generation;
    try {
      const rows = task_id === 0
        ? (await actions.list(1000)).filter((action) => action.task_id == null)
        : await actions.listPendingForTask(task_id);
      set((s) => {
        if (s.generation !== generation) return {};
        const byId = { ...s.byId };
        for (const r of rows) byId[r.id] = r;
        const byTask = { ...s.byTask, [task_id]: rows.map((r) => r.id) };
        return { byId, byTask };
      });
    } catch (err) {
      if (get().generation === generation) notify.fromError("Load approval actions", err);
    }
  },

  upsert(a) {
    set((s) => {
      const byId = { ...s.byId, [a.id]: a };
      const taskId = a.task_id ?? 0;
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

  async accept(action_id, override_diff) {
    const generation = get().generation;
    try {
      const r = await actions.accept({ action_id, override_diff });
      if (get().generation === generation) {
        get().applyResolution(r.action_id, r.status, r.error);
        if (r.follow_up_action_ids?.length) await get().loadForTask(get().byId[action_id]?.task_id ?? 0);
      }
      return r;
    } catch (err) {
      notify.fromError("Accept action", err);
      throw err;
    }
  },

  async reject(action_id) {
    const generation = get().generation;
    try {
      const r = await actions.reject(action_id);
      if (get().generation === generation) get().applyResolution(r.action_id, r.status, r.error);
    } catch (err) {
      notify.fromError("Reject action", err);
      throw err;
    }
  },

  initListeners() {
    const scope = createListenerScope((error) => notify.fromError("Action subscriptions", error));

    actions
      .onProposed((e) => {
        // Pull the freshly-inserted action(s) for this task so the UI has
        // full payload / diff to render.
        void get().loadForTask(e.task_id);
      })
      .then(scope.add)
      .catch((error) => notify.fromError("Listen for proposed actions", error));

    actions
      .onResolved((e) => {
        get().applyResolution(e.action_id, e.status, e.error);
        if (e.follow_up_action_ids?.length) void get().loadForTask(get().byId[e.action_id]?.task_id ?? 0);
      })
      .then(scope.add)
      .catch((error) => notify.fromError("Listen for resolved actions", error));

    return scope.dispose;
  },
}));
