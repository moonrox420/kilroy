/**
 * Terminal panel — header + active-session viewport + tab strip.
 *
 * Layout (top to bottom):
 *   1. panel-header — "Terminal" label, status dot, send-test button,
 *      clear / close panel buttons
 *   2. session viewport — all TerminalSessionViews stacked absolutely,
 *      only the active one visible
 *   3. tab strip — across the bottom edge of the panel, add / close /
 *      rename
 *
 * The first session auto-spawns when the panel mounts (or when the
 * user opens a folder). Subsequent sessions are created via the `+`
 * button in the tab strip.
 *
 * The header status dot is an at-a-glance health indicator: green if
 * the active session is alive, red if it exited, gray if there's no
 * session.
 */
import { useEffect } from "react";
import { CircleDot, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useUI } from "@/store/ui";
import { useTerminals } from "@/store/terminals";
import { useWorkspace } from "@/store/workspace";
import { term } from "@/lib/tauri";
import { notify } from "@/store/notifications";
import { TerminalSessionView } from "./TerminalSessionView";
import { TerminalTabs } from "./TerminalTabs";
import { cn } from "@/lib/utils";

export function TerminalPanel() {
  const toggleTerminal = useUI((s) => s.toggleTerminal);
  const rootPath = useWorkspace((s) => s.rootPath);
  const sessions = useTerminals((s) => s.sessions);
  const activeId = useTerminals((s) => s.activeId);
  const ensureSession = useTerminals((s) => s.ensureSession);

  const activeSession = sessions.find((s) => s.id === activeId) ?? null;

  // Auto-spawn an initial session once the panel exists. We don't tie
  // this to rootPath changes — switching projects doesn't kill existing
  // terminals; the user can spawn fresh tabs anchored to the new root
  // via the `+` button.
  useEffect(() => {
    void ensureSession(rootPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const clearActive = () => {
    // The xterm itself doesn't expose a "clear" via the store; we send
    // the ANSI clear sequence through the PTY which produces the same
    // visual outcome and keeps the shell happy.
    if (!activeId) return;
    void term.write(activeId, "\x1bc").catch((err) => {
      notify.fromError("Clear terminal", err);
    });
  };

  // Status dot: green alive, red exited, gray no session.
  const dotClass = !activeSession
    ? "text-ink-subtle"
    : activeSession.exited
      ? "text-err"
      : "text-ok";
  const dotTitle = !activeSession
    ? "No active terminal"
    : activeSession.exited
      ? `Session ${activeSession.id.slice(0, 8)} exited`
      : `Session ${activeSession.id.slice(0, 8)} alive — ${activeSession.label}`;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="panel-header">
        <span className="flex items-center gap-2.5">
          <span>Terminal</span>
          <CircleDot
            className={cn("h-3 w-3", dotClass)}
            aria-label={dotTitle}
          >
            <title>{dotTitle}</title>
          </CircleDot>
          <span className="text-[10px] normal-case tracking-normal text-ink-subtle">
            {sessions.length === 0
              ? "no sessions"
              : `${sessions.length} session${sessions.length === 1 ? "" : "s"}`}
          </span>
        </span>
        <div className="flex items-center gap-0.5">
          <Button
            variant="ghost"
            size="icon"
            title="Clear active terminal"
            onClick={clearActive}
            disabled={!activeId}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            title="Close panel"
            onClick={toggleTerminal}
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      <div className="relative min-h-0 flex-1">
        {sessions.length === 0 ? (
          <EmptyState
            onNew={() =>
              void useTerminals.getState().add({ cwd: rootPath ?? undefined })
            }
          />
        ) : (
          sessions.map((s) => (
            <TerminalSessionView
              key={s.id}
              session={s}
              isActive={s.id === activeId}
            />
          ))
        )}
      </div>

      <TerminalTabs />
    </div>
  );
}

function EmptyState({ onNew }: { onNew: () => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
      <p className="text-[12px] text-ink-muted">No terminals open.</p>
      <Button onClick={onNew} size="sm">
        New Terminal
      </Button>
    </div>
  );
}
