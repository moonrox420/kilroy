/**
 * Plan controls — sits under an agent reply that carries a pending plan.
 *
 * Two buttons: Edit (opens the PlanEditor) and Execute (skips editing,
 * runs the plan as-is).
 */
import { Play, Pencil, Loader2 } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { usePlan } from "@/store/plan";
import { plan as planApi, type TaskRow } from "@/lib/tauri";
import { useAgent } from "@/store/agent";

interface Props {
  run_id: string;
  tasks: TaskRow[];
}

export function PlanControls({ run_id, tasks }: Props) {
  const openWith = usePlan((s) => s.openWith);
  const markPlanExecuted = useAgent((s) => s.markPlanExecuted);
  const [busy, setBusy] = useState(false);

  const onEdit = () => openWith(run_id, tasks);

  const onExecute = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await planApi.executePlan(
        run_id,
        tasks.map((t) => t.id),
      );
      markPlanExecuted(run_id);
    } catch (err) {
      console.error("execute_plan:", err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex items-center gap-2 rounded-md border border-amber/40 bg-amber/5 px-2 py-1.5">
      <span className="flex-1 text-[11px] text-ink-muted">
        Plan ready · {tasks.length} task{tasks.length === 1 ? "" : "s"}
      </span>
      <Button variant="ghost" size="sm" onClick={onEdit} disabled={busy}>
        <Pencil className="h-3 w-3" />
        Edit
      </Button>
      <Button size="sm" onClick={onExecute} disabled={busy}>
        {busy ? <Loader2 className="h-3 w-3 animate-spin" /> : <Play className="h-3 w-3" />}
        Execute
      </Button>
    </div>
  );
}
