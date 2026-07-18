/**
 * Decisions tab — list and read architectural decisions.
 *
 * Plus a "+ New" button that opens the Decision composer.
 */
import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import { memory, type Decision } from "@/lib/tauri";
import { useMemory } from "@/store/memory";
import { useMemoryPanel } from "@/store/memoryPanel";
import { Button } from "@/components/ui/button";
import { EmptyState, Loading } from "../MemoryPanel";

export function DecisionsPanel() {
  const project = useMemory((s) => s.project);
  const openComposer = useMemoryPanel((s) => s.openDecisionComposer);
  const [decisions, setDecisions] = useState<Decision[] | null>(null);
  const [openId, setOpenId] = useState<number | null>(null);

  const refresh = async () => {
    if (!project) return;
    try {
      const list = await memory.listDecisions(100);
      setDecisions(list);
    } catch (err) {
      console.error("listDecisions:", err);
      setDecisions([]);
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project?.id]);

  if (!project) return <EmptyState title="No project" body="Open a folder first." />;
  if (decisions === null) return <Loading />;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between border-b border-line px-3 py-2">
        <span className="text-[11px] text-ink-subtle">
          {decisions.length} decision{decisions.length === 1 ? "" : "s"}
        </span>
        <Button variant="default" size="sm" onClick={openComposer}>
          <Plus className="h-3 w-3" />
          Log Decision
        </Button>
      </div>
      {decisions.length === 0 ? (
        <EmptyState
          title="No decisions yet"
          body="Log architectural decisions so the agent recalls them as it changes the codebase."
        />
      ) : (
        <ul className="flex-1 divide-y divide-line overflow-y-auto">
          {decisions.map((d) => {
            const expanded = openId === d.id;
            return (
              <li key={d.id}>
                <button
                  onClick={() => setOpenId(expanded ? null : d.id)}
                  className="flex w-full items-start justify-between gap-3 px-3 py-2 text-left hover:bg-bg-2"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[12px] font-medium text-ink">
                      #{d.id} · {d.title}
                    </p>
                    <p className="mt-0.5 line-clamp-2 text-[11px] text-ink-muted">
                      {d.summary}
                    </p>
                  </div>
                  <span className="shrink-0 text-[10px] text-ink-subtle">
                    {fmtTime(d.created_at)}
                  </span>
                </button>
                {expanded && d.rationale && (
                  <div className="border-t border-line/60 bg-bg-0/50 px-4 py-2 text-[11px] text-ink whitespace-pre-wrap">
                    {d.rationale}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}
