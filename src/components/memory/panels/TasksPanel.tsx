/**
 * Tasks tab — recent task graph runs for the current session.
 *
 * Each row shows status, type, agent, and a short input snippet. Click
 * to expand for the full input + output.
 */
import { useEffect, useState } from "react";
import { Check, CircleDashed, Loader2, X } from "lucide-react";
import { memory, type TaskRecord } from "@/lib/tauri";
import { useMemory } from "@/store/memory";
import { cn } from "@/lib/utils";
import { EmptyState, Loading } from "../MemoryPanel";

export function TasksPanel() {
  const project = useMemory((s) => s.project);
  const session = useMemory((s) => s.session);
  const [tasks, setTasks] = useState<TaskRecord[] | null>(null);
  const [openId, setOpenId] = useState<number | null>(null);

  useEffect(() => {
    if (!session) {
      setTasks([]);
      return;
    }
    let cancelled = false;
    memory
      .listTasks(100)
      .then((res) => {
        if (!cancelled) setTasks(res);
      })
      .catch((err) => {
        console.error("listTasks:", err);
        if (!cancelled) setTasks([]);
      });
    return () => {
      cancelled = true;
    };
  }, [session?.id]);

  if (!project) return <EmptyState title="No project" body="Open a folder first." />;
  if (tasks === null) return <Loading />;
  if (tasks.length === 0)
    return (
      <EmptyState
        title="No tasks yet"
        body="Switch to Autonomous mode and send a message to kick off your first task graph run."
      />
    );

  return (
    <ul className="flex-1 divide-y divide-line overflow-y-auto">
      {tasks.map((t) => {
        const open = openId === t.id;
        return (
          <li key={t.id}>
            <button
              onClick={() => setOpenId(open ? null : t.id)}
              className="flex w-full items-start gap-2 px-3 py-2 text-left hover:bg-bg-2"
            >
              <StatusIcon status={t.status} />
              <div className="min-w-0 flex-1">
                <p className="flex items-center gap-2 text-[12px] text-ink">
                  <span className="rounded-sm border border-line px-1 py-[1px] text-[9px] uppercase tracking-wider text-ink-subtle">
                    {t.type}
                  </span>
                  <span className="rounded-sm border border-line px-1 py-[1px] text-[9px] uppercase tracking-wider text-ink-subtle">
                    {t.agent}
                  </span>
                  <span className="truncate">{summarize(t.input)}</span>
                </p>
              </div>
              <span className="shrink-0 text-[10px] text-ink-subtle">
                {t.created_at ? fmtTime(t.created_at) : ""}
              </span>
            </button>
            {open && (
              <div className="space-y-2 border-t border-line/60 bg-bg-0/50 px-3 py-2 text-[11px]">
                <Field label="Input">
                  <pre className="whitespace-pre-wrap text-ink">{t.input}</pre>
                </Field>
                {t.output && (
                  <Field label="Output">
                    <pre className="whitespace-pre-wrap text-ink-muted">
                      {t.output}
                    </pre>
                  </Field>
                )}
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

function StatusIcon({ status }: { status: TaskRecord["status"] }) {
  const cls = "h-3 w-3 shrink-0 mt-1";
  switch (status) {
    case "success":
      return <Check className={cn(cls, "text-ok")} />;
    case "running":
      return <Loader2 className={cn(cls, "text-amber animate-spin")} />;
    case "failed":
      return <X className={cn(cls, "text-err")} />;
    default:
      return <CircleDashed className={cn(cls, "text-ink-ghost")} />;
  }
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-0.5 text-[10px] uppercase tracking-wider text-ink-subtle">
        {label}
      </p>
      <div className="rounded-sm bg-bg-1 p-2 font-mono text-[10.5px]">{children}</div>
    </div>
  );
}

function summarize(raw: string): string {
  // Tasks store input as JSON like `{"title": "...", "input": "..."}`.
  try {
    const obj = JSON.parse(raw);
    return obj.title || obj.input || raw;
  } catch {
    return raw;
  }
}

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}
