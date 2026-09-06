/**
 * Task stream — renders a live multi-task agent run as a stacked card
 * inside the chat panel. One card per run; one row per task. Click a
 * task to expand its streaming output.
 */
import { useState } from "react";
import {
  Bot,
  Check,
  ChevronRight,
  CircleDashed,
  Loader2,
  X,
} from "lucide-react";
import type { LiveRun, LiveTask } from "@/store/runtime";
import { cn } from "@/lib/utils";
import { ActionList } from "./ActionList";

export function TaskStream({ run }: { run: LiveRun }) {
  return (
    <div className="rounded-md border border-line bg-bg-1/70 text-[12px]">
      <header className="flex items-center justify-between border-b border-line px-2.5 py-1.5">
        <div className="flex items-center gap-2 truncate text-ink">
          <Bot className="h-3.5 w-3.5 text-amber" />
          <span className="text-[10px] uppercase tracking-wider text-ink-subtle">
            {run.mode.replace("_", " ")}
          </span>
          <span className="truncate">{run.overview || "Working…"}</span>
        </div>
        <span
          className={cn(
            "rounded-sm px-1.5 py-[1px] text-[9px] uppercase tracking-wider",
            run.completed
              ? run.success
                ? "bg-ok/15 text-ok"
                : "bg-err/15 text-err"
              : "bg-amber/15 text-amber animate-pulse-amber",
          )}
        >
          {run.completed ? (run.success ? "done" : "failed") : "running"}
        </span>
      </header>
      <ul className="divide-y divide-line">
        {run.tasks.length === 0 ? (
          <li className="flex items-center gap-2 px-2.5 py-2 text-ink-subtle">
            <Loader2 className="h-3 w-3 animate-spin text-amber" />
            Planning…
          </li>
        ) : (
          run.tasks.map((t) => <TaskRow key={t.task_id} task={t} />)
        )}
      </ul>
    </div>
  );
}

function TaskRow({ task }: { task: LiveTask }) {
  const [open, setOpen] = useState(task.status === "running");
  const Icon =
    task.status === "running"
      ? Loader2
      : task.status === "success"
        ? Check
        : task.status === "failed"
          ? X
          : CircleDashed;
  const color =
    task.status === "running"
      ? "text-amber animate-spin"
      : task.status === "success"
        ? "text-ok"
        : task.status === "failed"
          ? "text-err"
          : "text-ink-ghost";

  return (
    <li>
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left hover:bg-bg-2"
      >
        <Icon className={cn("h-3 w-3 shrink-0", color)} />
        <span className="text-[10px] uppercase tracking-wider text-ink-subtle">
          {task.agent}
        </span>
        <span className="flex-1 truncate text-ink">{task.title}</span>
        <ChevronRight
          className={cn(
            "h-3 w-3 shrink-0 text-ink-subtle transition-transform",
            open && "rotate-90",
          )}
        />
      </button>
      {open && (
        <div className="space-y-2 border-t border-line/60 bg-bg-0/60 px-2.5 py-1.5">
          {task.output && (
            <pre className="max-h-[260px] overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-snug text-ink">
              {task.output}
            </pre>
          )}
          <ActionList taskId={task.task_id} />
        </div>
      )}
    </li>
  );
}
