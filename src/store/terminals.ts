/**
 * Terminal sessions store.
 *
 * Owns the list of live PTY sessions, the active tab id, label edits,
 * and the spawn/kill plumbing. The xterm/host wiring lives in
 * `TerminalSessionView` — one per session.
 *
 * Sessions survive panel collapse: react-resizable-panels keeps the
 * `TerminalPanel` mounted at 0 height when collapsed, so the
 * SessionView components don't unmount and their PTYs keep running.
 * They only die when the user explicitly closes the tab.
 */
import { create } from "zustand";
import { term } from "@/lib/tauri";
import { destroyTerminalBridge } from "@/lib/terminalBridge";
import { basename } from "@/lib/utils";
import { notify } from "./notifications";

export interface TermSession {
  id: string;
  label: string;
  cwd: string | null;
  spawnedAt: number;
  exited: boolean;
}

interface TerminalsState {
  sessions: TermSession[];
  activeId: string | null;
  /** Counter used when generating default "shell N" labels. */
  nextOrdinal: number;

  add: (opts?: { label?: string; cwd?: string; shell?: string }) =>
    Promise<TermSession | null>;
  close: (id: string) => Promise<void>;
  setActive: (id: string) => void;
  rename: (id: string, label: string) => void;
  markExited: (id: string) => void;

  /** Ensure at least one session exists; spawn one bound to `rootPath` if not. */
  ensureSession: (rootPath: string | null) => Promise<void>;
}

/**
 * Re-entrancy guard for `ensureSession`. Under React StrictMode the
 * TerminalPanel mount effect runs twice back-to-back; both invocations used
 * to pass the `sessions.length === 0` check (the first spawn hadn't resolved
 * yet) and TWO PTYs were spawned for one visible terminal. Module-level on
 * purpose: the guard must span concurrent calls, not store snapshots.
 */
let ensureInFlight = false;

export const useTerminals = create<TerminalsState>((set, get) => ({
  sessions: [],
  activeId: null,
  nextOrdinal: 1,

  async add(opts) {
    const cwd = opts?.cwd;
    const shell = opts?.shell;
    try {
      const spawned = await term.spawn({
        cwd: cwd ?? undefined,
        shell: shell ?? undefined,
      });
      const { sessions, nextOrdinal } = get();
      // Default label: "{shell_label} · {basename(cwd)}", e.g.
      //   "Windows PowerShell · kilroy"
      //   "Git Bash"                          (no cwd)
      //   "WSL · Ubuntu · kilroy"
      // The shell_label is FIRST so the user can always tell at a glance
      // which interpreter they're in. opts.label still wins if the caller
      // explicitly wants to override (e.g. user renamed the tab).
      const shellLabel = spawned.shell_label || `shell ${nextOrdinal}`;
      const cwdLabel = cwd ? basename(cwd) : "";
      const computedLabel =
        cwdLabel && cwdLabel !== shellLabel
          ? `${shellLabel} · ${cwdLabel}`
          : shellLabel;
      const label = opts?.label?.trim() || computedLabel;
      const session: TermSession = {
        id: spawned.id,
        label,
        cwd: cwd ?? null,
        spawnedAt: Date.now(),
        exited: false,
      };
      set({
        sessions: [...sessions, session],
        activeId: spawned.id,
        nextOrdinal: nextOrdinal + 1,
      });
      return session;
    } catch (err) {
      // Surface this loudly — the dev console is the only place the user
      // sees PTY spawn failures, and "I can't type in the terminal" is
      // almost always actually "the shell never spawned in the first place".
      notify.fromError(
        `Spawn terminal${shell ? ` (${shell})` : ""}`,
        err,
      );
      return null;
    }
  },

  async close(id) {
    destroyTerminalBridge(id);
    try {
      await term.kill(id);
    } catch (err) {
      // Already exited is fine.
      console.warn("terminal.kill:", err);
    }
    const { sessions, activeId } = get();
    const remaining = sessions.filter((s) => s.id !== id);
    let nextActive = activeId;
    if (activeId === id) {
      const idx = sessions.findIndex((s) => s.id === id);
      nextActive = remaining[idx]?.id ?? remaining[idx - 1]?.id ?? null;
    }
    set({ sessions: remaining, activeId: nextActive });
  },

  setActive(id) {
    set({ activeId: id });
  },

  rename(id, label) {
    const next = label.trim();
    if (!next) return;
    set((s) => ({
      sessions: s.sessions.map((sess) =>
        sess.id === id ? { ...sess, label: next } : sess,
      ),
    }));
  },

  markExited(id) {
    set((s) => ({
      sessions: s.sessions.map((sess) =>
        sess.id === id ? { ...sess, exited: true } : sess,
      ),
    }));
  },

  async ensureSession(rootPath) {
    if (get().sessions.length > 0 || ensureInFlight) return;
    ensureInFlight = true;
    try {
      // Re-check inside the guard: a session may have landed between the
      // first check and acquiring the flag.
      if (get().sessions.length === 0) {
        // Don't pass `label` — let add() compute it from shell_label + cwd so
        // the first tab clearly shows which shell is running (e.g.
        // "Windows PowerShell · kilroy" instead of just "kilroy").
        await get().add({ cwd: rootPath ?? undefined });
      }
    } finally {
      ensureInFlight = false;
    }
  },
}));
