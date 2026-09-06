/**
 * Pending plan store — tracks the latest plan awaiting execution.
 *
 * The chat panel surfaces "Edit" and "Execute" buttons for whichever
 * plan is currently pending. The PlanEditor reads/edits from here.
 * Closing the editor or executing clears the pending plan.
 */
import { create } from "zustand";
import { plan, type TaskRow } from "@/lib/tauri";

export interface DraftTask {
  /** Existing pending task id, or null for a newly-added local-only entry. */
  task_id: number | null;
  type: string;
  agent: string;
  title: string;
  input: string;
  /** True while a single-task save is in-flight. */
  saving?: boolean;
}

interface PlanState {
  open: boolean;
  run_id: string | null;
  draft: DraftTask[];
  /** Task ids present when the plan was opened. Snapshotted here (not
   *  re-derived from `draft`) so `saveAll` can tell which originally-pending
   *  tasks the user DELETED and cancel them — `removeLocal` drops them from
   *  `draft`, so the draft alone can't reveal what's gone. */
  originalTaskIds: number[];

  openWith: (run_id: string, tasks: TaskRow[]) => void;
  close: () => void;
  updateLocal: (idx: number, patch: Partial<DraftTask>) => void;
  addEmpty: () => void;
  removeLocal: (idx: number) => void;
  saveAll: () => Promise<{ run_id: string; task_ids: number[] } | null>;
}

export const usePlan = create<PlanState>((set, get) => ({
  open: false,
  run_id: null,
  draft: [],
  originalTaskIds: [],

  openWith(run_id, tasks) {
    set({
      open: true,
      run_id,
      originalTaskIds: tasks.map((t) => t.id),
      draft: tasks.map((t) => ({
        task_id: t.id,
        type: t.type,
        agent: t.agent,
        title: t.title,
        input: t.input,
      })),
    });
  },

  close() {
    set({ open: false });
  },

  updateLocal(idx, patch) {
    set((s) => ({
      draft: s.draft.map((d, i) => (i === idx ? { ...d, ...patch } : d)),
    }));
  },

  addEmpty() {
    set((s) => ({
      draft: [
        ...s.draft,
        {
          task_id: null,
          type: "code",
          agent: "developer",
          title: "New task",
          input: "",
        },
      ],
    }));
  },

  removeLocal(idx) {
    set((s) => ({ draft: s.draft.filter((_, i) => i !== idx) }));
  },

  async saveAll() {
    const { draft, run_id, originalTaskIds } = get();
    if (!run_id) return null;

    // Sync forward: update each surviving existing task, insert each
    // newly-added one, then cancel any task that WAS in the plan when it
    // opened but is no longer in the draft (the user deleted it). The set
    // of executed ids is exactly the kept + inserted ids.
    const keptIds: number[] = [];

    for (const d of draft) {
      if (d.task_id != null) {
        await plan.updateTask({
          task_id: d.task_id,
          title: d.title,
          input: d.input,
        });
        keptIds.push(d.task_id);
      } else {
        const id = await plan.insertTask({
          type: d.type,
          agent: d.agent,
          title: d.title,
          input: d.input,
        });
        keptIds.push(id);
      }
    }

    // Cancel originally-pending tasks the user deleted. Sourced from the
    // open-time snapshot, NOT from `draft` (deleted rows are already gone
    // from `draft`, which is exactly why the old draft-derived check could
    // never find them and left zombie 'pending' rows in the DB).
    const removed = originalTaskIds.filter((id) => !keptIds.includes(id));
    if (removed.length) {
      await plan.cancelPlan(removed);
    }

    set({ open: false });
    return { run_id, task_ids: keptIds };
  },
}));
