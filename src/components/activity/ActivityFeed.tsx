/**
 * Activity feed — modal timeline of every meaningful agent action.
 *
 * Filterable to the current session. Each row renders a compact icon +
 * relative time + a one-line summary specialised by activity kind.
 */
import { useEffect, useState } from "react";
import {
  Bot,
  Brain,
  Database,
  FileCode2,
  Lightbulb,
  ListChecks,
  MessageSquare,
  Sparkles,
  X,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useActivity } from "@/store/activity";
import type { ActivityView } from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function ActivityFeed({ open, onClose }: Props) {
  const rows = useActivity((s) => s.rows);
  const loading = useActivity((s) => s.loading);
  const load = useActivity((s) => s.load);
  const [sessionOnly, setSessionOnly] = useState(true);

  useEffect(() => {
    if (!open) return;
    void load({ session_only: sessionOnly, limit: 200 });
  }, [open, sessionOnly, load]);

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="w-[min(720px,calc(100vw-3rem))]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="h-3.5 w-3.5 text-amber" />
            Activity
          </DialogTitle>
          <DialogDescription>
            Append-only timeline of every action the agent and you have taken.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center justify-between gap-2 border-b border-line px-3 py-2">
          <div className="flex items-center gap-2 text-[11px] text-ink-subtle">
            <button
              onClick={() => setSessionOnly(true)}
              className={cn(
                "rounded-sm px-1.5 py-[1px]",
                sessionOnly ? "bg-amber text-amber-ink" : "hover:bg-bg-2",
              )}
            >
              This session
            </button>
            <button
              onClick={() => setSessionOnly(false)}
              className={cn(
                "rounded-sm px-1.5 py-[1px]",
                !sessionOnly ? "bg-amber text-amber-ink" : "hover:bg-bg-2",
              )}
            >
              All sessions
            </button>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => load({ session_only: sessionOnly, limit: 200 })}
            disabled={loading}
          >
            Refresh
          </Button>
        </div>
        <div className="max-h-[60vh] overflow-y-auto">
          {loading && rows.length === 0 && (
            <p className="p-6 text-center text-[11px] text-ink-subtle">loading…</p>
          )}
          {!loading && rows.length === 0 && (
            <p className="p-6 text-center text-[11px] text-ink-subtle">
              Nothing here yet. Open a folder, send a message, or log a decision.
            </p>
          )}
          {rows.length > 0 && (
            <ul className="divide-y divide-line/60">
              {rows.map((r) => (
                <ActivityRow key={r.id} row={r} />
              ))}
            </ul>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ActivityRow({ row }: { row: ActivityView }) {
  const meta = describe(row);
  const Icon = meta.icon;
  return (
    <li className="flex items-start gap-3 px-3 py-2">
      <Icon className={cn("h-3.5 w-3.5 shrink-0 mt-0.5", meta.color)} />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[12px] text-ink">{meta.title}</p>
        {meta.detail && (
          <p className="truncate text-[10.5px] text-ink-subtle">{meta.detail}</p>
        )}
      </div>
      <span className="shrink-0 text-[10px] text-ink-subtle">
        {fmtTime(row.created_at)}
      </span>
    </li>
  );
}

function describe(row: ActivityView): {
  icon: React.ComponentType<{ className?: string }>;
  color: string;
  title: string;
  detail?: string;
} {
  const p = row.payload ?? {};
  switch (row.kind) {
    case "project_opened":
      return {
        icon: Database,
        color: "text-amber",
        title: `Opened project ${p.name ?? ""}`,
        detail: p.root ?? "",
      };
    case "message_sent":
      return {
        icon: MessageSquare,
        color: "text-amber",
        title: "You sent a message",
        detail: p.preview ?? "",
      };
    case "message_received":
      return {
        icon: Bot,
        color: "text-ok",
        title: "Agent replied",
        detail: p.preview ?? "",
      };
    case "plan_ready":
      return {
        icon: ListChecks,
        color: "text-amber",
        title: "Plan ready",
        detail: Array.isArray(p.tasks) ? p.tasks.join(" · ") : "",
      };
    case "run_started":
      return {
        icon: Brain,
        color: "text-amber",
        title: "Run started",
        detail: `${p.task_count ?? "?"} tasks · ${p.run_id ?? ""}`,
      };
    case "run_completed":
      return {
        icon: Brain,
        color: p.success ? "text-ok" : "text-err",
        title: p.success ? "Run completed" : "Run failed",
        detail: `${p.task_count ?? "?"} tasks`,
      };
    case "decision_logged":
      return {
        icon: Lightbulb,
        color: "text-amber",
        title: `Decision logged: ${p.title ?? ""}`,
      };
    case "index_completed":
      return {
        icon: Database,
        color: "text-ok",
        title: "Indexing complete",
        detail: `${p.files_indexed ?? 0} files · ${p.chunks_inserted ?? 0} chunks · ${
          p.duration_ms ?? 0
        }ms`,
      };
    case "action_applied":
      return {
        icon: FileCode2,
        color: "text-ok",
        title: `Applied ${p.kind ?? "action"}: ${p.target ?? ""}`,
      };
    case "action_failed":
      return {
        icon: X,
        color: "text-err",
        title: `Action failed: ${p.target ?? ""}`,
        detail: p.error ?? "",
      };
    case "action_rejected":
      return {
        icon: X,
        color: "text-ink-subtle",
        title: "Action rejected",
      };
    default:
      return {
        icon: Sparkles,
        color: "text-ink-subtle",
        title: row.kind,
        detail: JSON.stringify(row.payload).slice(0, 80),
      };
  }
}

function fmtTime(unix: number): string {
  const d = new Date(unix * 1000);
  const now = Date.now();
  const delta = (now - d.getTime()) / 1000;
  if (delta < 60) return "just now";
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}
