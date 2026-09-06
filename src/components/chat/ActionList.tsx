/**
 * Action list — renders the pending+resolved actions for a given task as
 * a stack of ActionCards. Loads from the actions store on mount.
 */
import { useEffect, useMemo } from "react";
import { useActions } from "@/store/actions";
import { ActionCard } from "./ActionCard";

export function ActionList({ taskId }: { taskId: number }) {
  const loadForTask = useActions((s) => s.loadForTask);
  const byId = useActions((s) => s.byId);
  const byTask = useActions((s) => s.byTask);

  useEffect(() => {
    void loadForTask(taskId);
  }, [taskId, loadForTask]);

  const items = useMemo(() => {
    const ids = byTask[taskId] ?? [];
    return ids.map((id) => byId[id]).filter(Boolean);
  }, [byTask, byId, taskId]);

  if (items.length === 0) return null;

  return (
    <div className="flex flex-col gap-1.5">
      {items.map((a) => (
        <ActionCard key={a.id} action={a} />
      ))}
    </div>
  );
}
