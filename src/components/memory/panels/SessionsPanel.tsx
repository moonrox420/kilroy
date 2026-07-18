/**
 * Sessions tab — list all conversations in the project.
 *
 * Click a session to load its messages into the chat. "New Session"
 * archives the current one and starts fresh. We always refresh the
 * list when the tab mounts so newly-created sessions show up.
 */
import { useEffect, useState } from "react";
import { Plus, MessageSquare } from "lucide-react";
import { memory, type Session } from "@/lib/tauri";
import { useMemory } from "@/store/memory";
import { useAgent } from "@/store/agent";
import { Button } from "@/components/ui/button";
import { EmptyState, Loading } from "../MemoryPanel";
import { useMemoryPanel } from "@/store/memoryPanel";

export function SessionsPanel() {
  const project = useMemory((s) => s.project);
  const currentSessionId = useMemory((s) => s.session?.id);
  const close = useMemoryPanel((s) => s.close);
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    if (!project) return;
    try {
      const list = await memory.listSessions(50);
      setSessions(list);
    } catch (err) {
      console.error("listSessions:", err);
      setSessions([]);
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project?.id]);

  const onNew = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const s = await memory.startSession();
      useMemory.getState().setSession(s);
      // Empty out the chat panel — the new session has no messages yet.
      useAgent.getState().loadHistory([]);
      close();
    } catch (err) {
      console.error("startSession:", err);
    } finally {
      setBusy(false);
    }
  };

  const onSwitch = async (sessionId: number) => {
    if (busy || sessionId === currentSessionId) return;
    setBusy(true);
    try {
      const switched = await memory.switchSession(sessionId);
      useMemory.getState().setSession(switched.session);
      useAgent.getState().loadHistory(switched.messages);
      close();
    } catch (err) {
      console.error("switchSession:", err);
    } finally {
      setBusy(false);
    }
  };

  if (!project) return <EmptyState title="No project" body="Open a folder first." />;
  if (sessions === null) return <Loading />;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <Toolbar>
        <Button variant="default" size="sm" onClick={onNew} disabled={busy}>
          <Plus className="h-3 w-3" />
          New Session
        </Button>
      </Toolbar>
      {sessions.length === 0 ? (
        <EmptyState
          title="No sessions yet"
          body="Send a message in chat to start your first session."
        />
      ) : (
        <ul className="flex-1 divide-y divide-line overflow-y-auto">
          {sessions.map((s) => (
            <li
              key={s.id}
              role="button"
              tabIndex={0}
              onClick={() => onSwitch(s.id)}
              onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onSwitch(s.id); }}
              className={`flex cursor-pointer items-center justify-between gap-3 px-3 py-2 text-[12px] ${
                s.id === currentSessionId ? "bg-amber/5" : "hover:bg-bg-2"
              }`}
            >
              <div className="flex min-w-0 items-center gap-2">
                <MessageSquare className="h-3 w-3 shrink-0 text-amber" />
                <span className="truncate text-ink">
                  {s.title ?? `Session #${s.id}`}
                </span>
                {s.id === currentSessionId && (
                  <span className="rounded-sm bg-amber/15 px-1 py-[1px] text-[9px] uppercase tracking-wider text-amber">
                    active
                  </span>
                )}
              </div>
              <div className="flex items-center gap-3 text-[10px] text-ink-subtle">
                <span className="rounded-sm border border-line px-1.5 py-[1px]">
                  {s.agent_mode.replace("_", " ")}
                </span>
                <span>{fmtTime(s.started_at)}</span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Toolbar({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2 border-b border-line px-3 py-2">
      {children}
    </div>
  );
}

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
