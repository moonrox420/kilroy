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
 * Each mount owns its subscriptions, including registrations resolving after cleanup.
 */
import { create } from "zustand";
import { runtime } from "@/lib/tauri";
import { createListenerScope } from "@/lib/listenerScope";
import { notify } from "./notifications";

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

export const useRuntime = create<RuntimeState>((set) => ({
  runs: {},
  streamingBuffer: "",
  clearStream: () => set({ streamingBuffer: "" }),

  initListeners() {
    const scope = createListenerScope((error) => notify.fromError("Runtime subscriptions", error));

    runtime
      .onStream((c) => set((s) => ({ streamingBuffer: s.streamingBuffer + c.delta })))
      .then(scope.add).catch(scope.report);

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
      .then(scope.add).catch(scope.report);

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
      .then(scope.add).catch(scope.report);

    runtime
      .onTaskStarted((e) => {
        set((s) => mutateTask(s, e.run_id, e.task_id, (t) => ({ ...t, status: "running" })));
      })
      .then(scope.add).catch(scope.report);

    runtime
      .onTaskChunk((e) => {
        set((s) => mutateTask(s, e.run_id, e.task_id, (t) => ({
          ...t,
          output: t.output + e.delta,
        })));
      })
      .then(scope.add).catch(scope.report);

    runtime
      .onTaskCompleted((e) => {
        set((s) => mutateTask(s, e.run_id, e.task_id, (t) => ({
          ...t,
          status: e.success ? "success" : "failed",
          output: e.success ? t.output : t.output + (t.output ? "\n" : "") + e.output_preview,
        })));
      })
      .then(scope.add).catch(scope.report);

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
      .then(scope.add).catch(scope.report);

    return scope.dispose;
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
