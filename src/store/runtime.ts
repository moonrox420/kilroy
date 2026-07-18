/**
 * Runtime store — live state for autonomous agent runs.
 *
 * Each plan-and-execute run streams a sequence of events: started, plan
 * ready, task started, task chunks, task completed, run completed. We
 * hold the latest state per `run_id` and the chat panel renders a
 * task-stream card for each run inline below the user message that
 * triggered it.
 *
 * For Copilot streaming (single-shot), we keep a separate field that
 * appends deltas into a "live" message bubble.
 *
 * ⚠️ React StrictMode + async listeners: in dev mode StrictMode mounts the
 * App component twice. Each mount calls `initListeners()` which fires off
 * `listen()` Promises that resolve asynchronously. The first mount's
 * cleanup runs BEFORE those Promises resolve, so the unlistens array is
 * still empty — meaning nothing actually gets disposed. The second mount
 * then registers a second full set of listeners and every event handler
 * fires twice (visible to the user as doubled streaming text). The
 * module-level `_attached` flag below short-circuits the second call so
 * we only ever have ONE set of listeners live regardless of how many
 * times React decides to remount us.
 */
import { create } from "zustand";
import { runtime } from "@/lib/tauri";

export interface LiveTask {
  task_id: number;
  type: string;
  agent: string;
  title: string;
  status: "pending" | "running" | "success" | "failed";
  output: string;
}

export interface LiveRun {
  run_id: string;
  user_message: string;
  mode: string;
  overview: string;
  tasks: LiveTask[];
  completed: boolean;
  success: boolean;
}

interface RuntimeState {
  runs: Record<string, LiveRun>;
  /** Accumulating buffer for single-shot Copilot streams. */
  streamingBuffer: string;
  initListeners: () => () => void;
  clearStream: () => void;
}

// Module-level guards — survive StrictMode double-mount in a way that
// per-instance booleans on the store cannot. `_attached` ensures we only
// register listeners once; `_disposers` collects the unlisten callbacks
// from every Promise as they resolve so when the user actually closes the
// app we can dispose properly.
let _attached = false;
const _disposers: Array<() => void> = [];

export const useRuntime = create<RuntimeState>((set) => ({
  runs: {},
  streamingBuffer: "",
  clearStream: () => set({ streamingBuffer: "" }),

  initListeners() {
    // Idempotent: subsequent calls (e.g. StrictMode's second mount) are
    // no-ops. The returned cleanup function still works for the genuine
    // tear-down case but in practice the listeners stay alive for the
    // life of the renderer process.
    if (_attached) {
      return () => {
        // Intentionally a no-op for non-first callers. We don't want
        // StrictMode's first-mount cleanup to unhook everything just
        // because it ran before the second mount registered.
      };
    }
    _attached = true;

    const unlistens = _disposers;

    runtime
      .onStream((c) => set((s) => ({ streamingBuffer: s.streamingBuffer + c.delta })))
      .then((u) => unlistens.push(u));

    runtime
      .onRunStarted((e) => {
        set((s) => ({
          runs: {
            ...s.runs,
            [e.run_id]: {
              run_id: e.run_id,
              user_message: e.user_message,
              mode: e.mode,
              overview: "",
              tasks: [],
              completed: false,
              success: false,
            },
          },
        }));
      })
      .then((u) => unlistens.push(u));

    runtime
      .onPlanReady((e) => {
        set((s) => ({
          runs: {
            ...s.runs,
            [e.run_id]: {
              ...(s.runs[e.run_id] ?? {
                run_id: e.run_id,
                user_message: "",
                mode: "",
                overview: "",
                completed: false,
                success: false,
              }),
              run_id: e.run_id,
              tasks: e.tasks.map((t) => ({
                task_id: t.task_id,
                type: t.type,
                agent: t.agent,
                title: t.title,
                status: "pending",
                output: "",
              })),
            } as LiveRun,
          },
        }));
      })
      .then((u) => unlistens.push(u));

    runtime
      .onTaskStarted((e) => {
        set((s) => mutateTask(s, e.run_id, e.task_id, (t) => ({ ...t, status: "running" })));
      })
      .then((u) => unlistens.push(u));

    runtime
      .onTaskChunk((e) => {
        set((s) => mutateTask(s, e.run_id, e.task_id, (t) => ({
          ...t,
          output: t.output + e.delta,
        })));
      })
      .then((u) => unlistens.push(u));

    runtime
      .onTaskCompleted((e) => {
        set((s) => mutateTask(s, e.run_id, e.task_id, (t) => ({
          ...t,
          status: e.success ? "success" : "failed",
          output: e.success ? t.output : t.output + (t.output ? "\n" : "") + e.output_preview,
        })));
      })
      .then((u) => unlistens.push(u));

    runtime
      .onRunCompleted((e) => {
        set((s) => {
          const cur = s.runs[e.run_id];
          if (!cur) return s;
          return {
            runs: {
              ...s.runs,
              [e.run_id]: {
                ...cur,
                completed: true,
                success: e.success,
                overview: e.summary,
              },
            },
          };
        });
      })
      .then((u) => unlistens.push(u));

    return () => {
      // Genuine app-shutdown path. Drain the disposers and reset the
      // module flag so a hypothetical re-init (HMR, tests) can re-attach
      // cleanly.
      for (const u of unlistens) {
        try {
          u();
        } catch {
          // already disposed
        }
      }
      unlistens.length = 0;
      _attached = false;
    };
  },
}));

function mutateTask(
  state: RuntimeState,
  runId: string,
  taskId: number,
  f: (t: LiveTask) => LiveTask,
): Partial<RuntimeState> {
  const run = state.runs[runId];
  if (!run) return {};
  return {
    runs: {
      ...state.runs,
      [runId]: {
        ...run,
        tasks: run.tasks.map((t) => (t.task_id === taskId ? f(t) : t)),
      },
    },
  };
}
