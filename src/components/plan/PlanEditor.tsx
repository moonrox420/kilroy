/**
 * Plan editor — modal form for refining a pending task plan.
 *
 * Lets the user rename / re-input / delete / append tasks before
 * execution starts. Save & Execute persists edits via plan.update_task
 * / plan.delete_plan_task / plan.insert_plan_task and then kicks off
 * the executor through plan.execute_plan.
 */
import { Plus, Trash2, Play, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { usePlan } from "@/store/plan";
import { useAgent } from "@/store/agent";
import { plan as planApi } from "@/lib/tauri";
import { useState } from "react";

const TYPES = ["plan", "review", "code", "refactor", "test", "analysis", "doc"];
const AGENTS = ["planner", "architect", "developer", "qa", "reviewer", "orchestrator"];

export function PlanEditor() {
  const open = usePlan((s) => s.open);
  const close = usePlan((s) => s.close);
  const draft = usePlan((s) => s.draft);
  const update = usePlan((s) => s.updateLocal);
  const addEmpty = usePlan((s) => s.addEmpty);
  const remove = usePlan((s) => s.removeLocal);
  const saveAll = usePlan((s) => s.saveAll);
  const run_id = usePlan((s) => s.run_id);
  const markPlanExecuted = useAgent((s) => s.markPlanExecuted);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onSaveAndExecute = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await saveAll();
      if (!result) return;
      await planApi.executePlan(result.run_id, result.task_ids);
      // Clear the pending state on the originating chat message so its
      // inline Edit/Execute controls disappear — matching PlanControls'
      // own Execute path. Without this the user could re-run the plan.
      markPlanExecuted(result.run_id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !busy) close();
      }}
    >
      <DialogContent className="w-[min(820px,calc(100vw-3rem))]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Play className="h-3.5 w-3.5 text-amber" />
            Review Plan
          </DialogTitle>
          <DialogDescription>
            Rename, edit, delete, or append tasks. The agent executes them top to bottom.
          </DialogDescription>
        </DialogHeader>

        <div className="flex max-h-[60vh] flex-col gap-3 overflow-y-auto p-4">
          {draft.length === 0 ? (
            <p className="text-center text-[12px] text-ink-subtle">
              No tasks. Add one below.
            </p>
          ) : (
            draft.map((d, idx) => (
              <div
                key={`${d.task_id ?? "new"}-${idx}`}
                className="flex flex-col gap-2 rounded-md border border-line bg-bg-2 p-3"
              >
                <div className="flex items-center gap-2">
                  <span className="rounded-sm bg-bg-3 px-1.5 py-[1px] text-[10px] uppercase tracking-wider text-ink-subtle">
                    #{idx + 1}
                  </span>
                  <select
                    value={d.type}
                    onChange={(e) => update(idx, { type: e.target.value })}
                    className="rounded-md border border-line bg-bg-1 px-1.5 py-0.5 text-[11px] text-ink"
                  >
                    {TYPES.map((t) => (
                      <option key={t} value={t}>
                        {t}
                      </option>
                    ))}
                  </select>
                  <select
                    value={d.agent}
                    onChange={(e) => update(idx, { agent: e.target.value })}
                    className="rounded-md border border-line bg-bg-1 px-1.5 py-0.5 text-[11px] text-ink"
                  >
                    {AGENTS.map((a) => (
                      <option key={a} value={a}>
                        {a}
                      </option>
                    ))}
                  </select>
                  <div className="flex-1" />
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => remove(idx)}
                    disabled={busy}
                    title="Remove"
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
                <Field label="Title">
                  <Input
                    value={d.title}
                    onChange={(e) => update(idx, { title: e.target.value })}
                    placeholder="What this task is doing"
                  />
                </Field>
                <Field label="Instruction">
                  <Textarea
                    value={d.input}
                    onChange={(e) => update(idx, { input: e.target.value })}
                    rows={3}
                    placeholder="Concrete prompt the agent will work from"
                  />
                </Field>
              </div>
            ))
          )}
          <Button variant="ghost" size="sm" onClick={addEmpty} disabled={busy}>
            <Plus className="h-3 w-3" />
            Add task
          </Button>
          {error && (
            <p className="rounded-md border border-err/40 bg-err/5 px-2 py-1 text-[11px] text-err">
              {error}
            </p>
          )}
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => !busy && close()} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={onSaveAndExecute} disabled={busy || !run_id || draft.length === 0}>
            {busy ? (
              <>
                <Loader2 className="h-3 w-3 animate-spin" />
                Saving…
              </>
            ) : (
              <>
                <Play className="h-3 w-3" />
                Save &amp; Execute
              </>
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <Label>{label}</Label>
      {children}
    </div>
  );
}
