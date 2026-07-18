/**
 * Agent store — chat history + current mode.
 *
 * Messages now arrive in two flavours:
 *   * Live messages composed in this app session (user + agent), with
 *     optional `context` block describing what memory was retrieved.
 *   * Historical messages loaded from the project's SQLite memory DB.
 *
 * Both share the same `ChatMessage` shape so the UI never has to branch.
 */
import { create } from "zustand";
import {
  agent,
  memory,
  type AgentContext,
  type AgentMode,
  type StoredMessage,
  type TaskRow,
} from "@/lib/tauri";
import { notify } from "./notifications";

export interface ChatMessage {
  id: string;
  role: "user" | "agent" | "system";
  content: string;
  timestamp: number;
  /** Retrieved code chunks + decisions used to compose this reply (agent only). */
  context?: AgentContext;
  /** Set when this agent message was produced by an autonomous run. */
  run_id?: string | null;
  /** Final task summary for an autonomous run. */
  tasks?: TaskRow[];
  /** True until the user accepts/edits and executes the plan. */
  plan_pending?: boolean;
  /** Number of images attached to a user turn. We don't keep the
   *  base64 around past the send (memory bloat) — the count is enough
   *  for the UI to render a "📎 N images" badge below the bubble. */
  attached_images_count?: number;
}

interface AgentState {
  mode: AgentMode;
  messages: ChatMessage[];
  isThinking: boolean;

  setMode: (mode: AgentMode) => Promise<void>;
  /** `images` are raw base64 strings (no `data:` URL prefix). Passed
   *  verbatim to Ollama's `/api/chat`. Optional — text-only turns omit
   *  the field. */
  send: (content: string, images?: string[]) => Promise<void>;
  clear: () => void;
  loadHistory: (msgs: StoredMessage[]) => void;
  markPlanExecuted: (run_id: string) => void;
}

const BOOT_MSG: ChatMessage = {
  id: "boot",
  role: "system",
  content: "Kilroy is ready. Open a folder to load project memory.",
  timestamp: Date.now(),
};

export const useAgent = create<AgentState>((set, get) => ({
  mode: "code_agent",
  messages: [BOOT_MSG],
  isThinking: false,

  async setMode(mode) {
    const prev = get().mode;
    set({ mode });
    try {
      await agent.setMode(mode);
    } catch (err) {
      // Roll back UI state on IPC failure
      set({ mode: prev });
      notify.fromError("Set agent mode", err);
    }
  },

  async send(content, images) {
    const trimmed = content.trim();
    // Allow image-only turns ("here, look at this") — only bail if BOTH
    // text and images are empty.
    if (!trimmed && (!images || images.length === 0)) return;
    const userMsg: ChatMessage = {
      id: crypto.randomUUID(),
      role: "user",
      content: trimmed,
      timestamp: Date.now(),
      attached_images_count: images?.length ?? 0,
    };
    set((s) => ({ messages: [...s.messages, userMsg], isThinking: true }));

    try {
      const reply = await agent.send(trimmed, images);
      set((s) => ({
        messages: [
          ...s.messages,
          {
            id: reply.id,
            role: "agent",
            content: reply.content,
            timestamp: Date.now(),
            context: reply.context,
            run_id: reply.run_id,
            tasks: reply.tasks,
            plan_pending: reply.plan_pending,
          },
        ],
        isThinking: false,
      }));
    } catch (err) {
      set((s) => ({
        messages: [
          ...s.messages,
          {
            id: crypto.randomUUID(),
            role: "system",
            content: `Agent call failed: ${String(err)}`,
            timestamp: Date.now(),
          },
        ],
        isThinking: false,
      }));
    }
  },

  clear() {
    set({
      messages: [
        {
          id: crypto.randomUUID(),
          role: "system",
          content: "Chat cleared.",
          timestamp: Date.now(),
        },
      ],
    });
  },

  markPlanExecuted(run_id) {
    set((s) => ({
      messages: s.messages.map((m) =>
        m.run_id === run_id ? { ...m, plan_pending: false } : m,
      ),
    }));
  },

  loadHistory(msgs) {
    if (!msgs.length) {
      set({ messages: [BOOT_MSG] });
      return;
    }
    // Run linkages discovered while mapping: each is an agent message that
    // was produced by an autonomous run, paired with the task ids that run
    // created. We re-attach the task rows asynchronously below so the
    // TaskStream card rehydrates on reload.
    const linkages: { messageId: string; taskIds: number[] }[] = [];

    const mapped: ChatMessage[] = msgs.map((m) => {
      let context: AgentContext | undefined;
      let runId: string | null = null;
      if (m.metadata && m.role === "agent") {
        try {
          // Metadata is a superset of AgentContext: the backend injects
          // `run_id` + `run_task_ids` for autonomous-run replies. Extra keys
          // are harmless to the context popover, which only reads its own.
          const parsed = JSON.parse(m.metadata) as AgentContext & {
            run_id?: string | null;
            run_task_ids?: number[];
          };
          context = parsed;
          if (typeof parsed.run_id === "string" && parsed.run_id) {
            runId = parsed.run_id;
            if (Array.isArray(parsed.run_task_ids) && parsed.run_task_ids.length) {
              linkages.push({
                messageId: String(m.id),
                taskIds: parsed.run_task_ids,
              });
            }
          }
        } catch {
          context = undefined;
        }
      }
      return {
        id: String(m.id),
        role:
          m.role === "tool"
            ? "system"
            : (m.role as "user" | "agent" | "system"),
        content: m.content,
        timestamp: m.created_at * 1000,
        context,
        run_id: runId,
      };
    });
    set({ messages: mapped });
    if (linkages.length > 0) {
      void attachPersistedRuns(linkages);
    }
  },
}));

/**
 * Re-attach persisted task rows to history messages so an autonomous run's
 * TaskStream card survives an app restart. Tasks live in the session-scoped
 * `tasks` table; the message metadata told us which task ids belong to each
 * run, so we fetch the session's tasks once and slot them back onto the
 * right messages. Best-effort: a fetch failure just leaves the plain summary
 * text (which always persists) untouched.
 */
async function attachPersistedRuns(
  linkages: { messageId: string; taskIds: number[] }[],
): Promise<void> {
  try {
    const all = await memory.listTasks(1000);
    const byId = new Map(all.map((t) => [t.id, t]));
    useAgent.setState((s) => ({
      messages: s.messages.map((m) => {
        const link = linkages.find((l) => l.messageId === m.id);
        if (!link) return m;
        const tasks: TaskRow[] = [];
        for (const id of link.taskIds) {
          const rec = byId.get(id);
          if (!rec) continue;
          let title = "(task)";
          let inner = "";
          try {
            const v = JSON.parse(rec.input) as { title?: string; input?: string };
            if (typeof v.title === "string") title = v.title;
            if (typeof v.input === "string") inner = v.input;
          } catch {
            /* malformed input json — keep defaults */
          }
          tasks.push({
            id: rec.id,
            type: rec.type,
            agent: rec.agent,
            title,
            input: inner,
            status: rec.status,
            output_preview: rec.output ?? "",
          });
        }
        if (tasks.length === 0) return m;
        return { ...m, tasks };
      }),
    }));
  } catch (err) {
    console.error("attachPersistedRuns:", err);
  }
}
