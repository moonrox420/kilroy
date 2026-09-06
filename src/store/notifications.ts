/**
 * Toast / notification store.
 *
 * The global error surface — replaces "console.error in DevTools" as the
 * primary failure-reporting channel. When an IPC call fails, an unhandled
 * promise rejects, or anything else goes sideways, push a toast here and
 * the user sees a visible card stack in the corner.
 *
 * Toasts auto-dismiss after `durationMs` (default 6s for info, 10s for
 * error). The user can dismiss manually. Programmatic `dismiss(id)` and
 * `clear()` are available for cases where the originating event resolves.
 */
import { create } from "zustand";

export type ToastKind = "info" | "success" | "warn" | "error";

export interface Toast {
  id: string;
  kind: ToastKind;
  title: string;
  /** Optional secondary text — error.message, command output, etc. */
  detail?: string;
  /** Auto-dismiss timeout in ms. 0 = no auto-dismiss. */
  durationMs: number;
  createdAt: number;
}

interface NotificationsState {
  toasts: Toast[];
  push: (input: {
    kind: ToastKind;
    title: string;
    detail?: string;
    durationMs?: number;
  }) => string;
  dismiss: (id: string) => void;
  clear: () => void;
}

const DEFAULT_DURATIONS: Record<ToastKind, number> = {
  info: 5000,
  success: 4000,
  warn: 7000,
  error: 10000,
};

let counter = 0;
function nextId(): string {
  counter += 1;
  return `t${Date.now()}-${counter}`;
}

export const useNotifications = create<NotificationsState>((set, get) => ({
  toasts: [],
  push({ kind, title, detail, durationMs }) {
    const id = nextId();
    const t: Toast = {
      id,
      kind,
      title,
      detail,
      durationMs: durationMs ?? DEFAULT_DURATIONS[kind],
      createdAt: Date.now(),
    };
    set((s) => ({ toasts: [...s.toasts, t] }));
    if (t.durationMs > 0) {
      // Schedule auto-dismiss. We can't reach into setTimeout from
      // tests cleanly, but for the live app this is fine.
      setTimeout(() => get().dismiss(id), t.durationMs);
    }
    return id;
  },
  dismiss(id) {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
  clear() {
    set({ toasts: [] });
  },
}));

/** Convenience helpers — most callers don't need the full options object. */
export const notify = {
  info: (title: string, detail?: string) =>
    useNotifications.getState().push({ kind: "info", title, detail }),
  success: (title: string, detail?: string) =>
    useNotifications.getState().push({ kind: "success", title, detail }),
  warn: (title: string, detail?: string) =>
    useNotifications.getState().push({ kind: "warn", title, detail }),
  error: (title: string, detail?: string) =>
    useNotifications.getState().push({ kind: "error", title, detail }),
  /** Console-AND-toast — preserves the old DevTools surface while adding
   *  the visible toast. Use for IPC failures that you'd otherwise just
   *  `console.error()`. */
  fromError: (label: string, err: unknown) => {
    const msg = err instanceof Error ? err.message : String(err);
    console.error(`${label}:`, err);
    useNotifications.getState().push({
      kind: "error",
      title: label,
      detail: msg,
    });
  },
};
