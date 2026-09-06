/**
 * Context block — shows the user what memory the agent pulled in for a reply.
 *
 * Renders below an agent message bubble. Collapsed by default to keep the
 * chat dense; click to expand the full chunk list and decisions list.
 * This is what makes "the agent has memory" feel real — every reply is
 * audited.
 */
import { useState } from "react";
import { ChevronRight, Files, Hash, Lightbulb, Library } from "lucide-react";
import type { AgentContext } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useWorkspace } from "@/store/workspace";

interface Props {
  context: AgentContext;
}

export function ContextBlock({ context }: Props) {
  const [open, setOpen] = useState(false);
  const openFile = useWorkspace((s) => s.openFile);
  const rootPath = useWorkspace((s) => s.rootPath);

  const total = context.chunks.length + context.decisions.length;
  if (total === 0 && context.recent_messages === 0 && !context.note) {
    return null;
  }

  // Summary pill — always visible.
  const summary = (
    <button
      onClick={() => setOpen((v) => !v)}
      className={cn(
        "group inline-flex items-center gap-1.5 rounded-full border border-line/70 bg-bg-1/60 px-2 py-0.5 text-[10px] text-ink-subtle transition-colors",
        "hover:border-amber/40 hover:text-ink",
      )}
    >
      <ChevronRight
        className={cn(
          "h-3 w-3 shrink-0 transition-transform",
          open && "rotate-90",
        )}
      />
      <Library className="h-3 w-3 text-amber" />
      <span>
        {context.chunks.length} chunks · {context.decisions.length} decisions ·{" "}
        {context.recent_messages} recent
      </span>
      {context.ollama_used && (
        <span className="ml-1 rounded-sm bg-amber/15 px-1 py-[1px] text-[9px] uppercase tracking-wider text-amber">
          live
        </span>
      )}
    </button>
  );

  if (!open) return <div className="mt-1">{summary}</div>;

  return (
    <div className="mt-1 flex flex-col gap-2">
      {summary}
      <div className="rounded-md border border-line bg-bg-1/50 p-2 text-[11px]">
        {context.note && (
          <p className="mb-2 text-ink-subtle">{context.note}</p>
        )}
        {context.chunks.length > 0 && (
          <section>
            <h4 className="mb-1 flex items-center gap-1 text-[10px] uppercase tracking-wider text-ink-subtle">
              <Files className="h-3 w-3" />
              Code
            </h4>
            <ul className="flex flex-col gap-0.5">
              {context.chunks.map((c) => {
                const abs = rootPath ? joinPath(rootPath, c.file_path) : c.file_path;
                return (
                  <li key={c.chunk_id}>
                    <button
                      onClick={() => openFile(abs)}
                      title={c.symbol ?? ""}
                      className="flex w-full items-center gap-2 rounded-sm px-1 py-0.5 text-left hover:bg-bg-2"
                    >
                      <Hash className="h-3 w-3 shrink-0 text-amber/70" />
                      <span className="truncate text-ink">
                        {c.file_path}
                        <span className="text-ink-subtle">
                          :{c.start_line}–{c.end_line}
                        </span>
                      </span>
                      <span className="ml-auto shrink-0 font-mono text-[10px] text-ink-ghost">
                        d={c.distance.toFixed(3)}
                      </span>
                    </button>
                    {c.symbol && (
                      <p className="pl-5 text-[10px] text-ink-subtle truncate">
                        {c.symbol}
                      </p>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        )}
        {context.decisions.length > 0 && (
          <section className={context.chunks.length ? "mt-2" : ""}>
            <h4 className="mb-1 flex items-center gap-1 text-[10px] uppercase tracking-wider text-ink-subtle">
              <Lightbulb className="h-3 w-3" />
              Decisions
            </h4>
            <ul className="flex flex-col gap-0.5">
              {context.decisions.map((d) => (
                <li key={d.decision_id} className="flex items-center gap-2 px-1">
                  <span className="text-ink-ghost">#{d.decision_id}</span>
                  <span className="truncate text-ink">{d.title}</span>
                  <span className="ml-auto shrink-0 font-mono text-[10px] text-ink-ghost">
                    d={d.distance.toFixed(3)}
                  </span>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}

function joinPath(root: string, rel: string): string {
  const sep = root.includes("\\") ? "\\" : "/";
  if (root.endsWith(sep)) return root + rel.replace(/[\\/]/g, sep);
  return root + sep + rel.replace(/[\\/]/g, sep);
}
