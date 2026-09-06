/**
 * Right column — agent chat.
 *
 * Fully decoupled from the terminal collapse. Fixed full-height; the
 * width is locked by the IDE layout so it never resizes. Header /
 * banner (only when not indexed) / scrolling history / fixed input,
 * stacked vertically.
 */
import { useEffect, useState } from "react";
import { Bot, Database, Loader2, Trash2 } from "lucide-react";
import { ChatMessages } from "./ChatMessages";
import { ChatInput } from "./ChatInput";
import { ActionList } from "./ActionList";
import { Button } from "@/components/ui/button";
import { useAgent } from "@/store/agent";
import { useMemory } from "@/store/memory";
import { useWorkspace } from "@/store/workspace";
import { memory, type ProjectIndexStatus } from "@/lib/tauri";

export function AgentChat() {
  const clear = useAgent((s) => s.clear);
  const mode = useAgent((s) => s.mode);
  const project = useMemory((s) => s.project);
  const indexing = useMemory((s) => s.indexing);
  const lastIndex = useMemory((s) => s.lastIndex);
  const beginIndex = useMemory((s) => s.beginIndex);
  const rootPath = useWorkspace((s) => s.rootPath);

  const modeLabel: string =
    {
      copilot: "Chat",
      code_agent: "Code",
      autonomous: "Plan / Execute",
      multi_agent: "Multi-Agent Org",
      governance: "Governance",
      council: "Council",
      debug: "Review / Debug",
      test_first: "Test-First",
    }[mode] ?? mode;

  // Poll the index status on project open + after every indexing run.
  // Cheap query against the local SQLite — we don't even debounce.
  const [status, setStatus] = useState<ProjectIndexStatus | null>(null);
  useEffect(() => {
    let cancelled = false;
    const fetch = () => {
      if (!project) {
        setStatus(null);
        return;
      }
      void memory
        .projectIndexStatus()
        .then((s) => {
          if (!cancelled) setStatus(s);
        })
        .catch((err) => console.error("project_index_status:", err));
    };
    fetch();
    return () => {
      cancelled = true;
    };
  }, [project?.id, lastIndex, indexing]);

  const showBanner = !!project && status && !status.is_indexed && !indexing;

  return (
    <div className="flex h-full flex-col">
      <div className="panel-header">
        <span className="flex items-center gap-2">
          <Bot className="h-3.5 w-3.5 text-amber" />
          <span>Agent Chat</span>
          <span className="text-[10px] normal-case tracking-normal text-ink-subtle">
            {modeLabel}
          </span>
        </span>
        <Button variant="ghost" size="icon" title="Clear chat" onClick={clear}>
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
      {showBanner && (
        <div className="border-b border-amber/40 bg-amber/5 px-3 py-2 text-[11px]">
          <p className="mb-1 flex items-center gap-1.5 font-medium text-amber">
            <Database className="h-3.5 w-3.5" />
            Project not indexed
          </p>
          <p className="mb-2 text-ink-muted leading-snug">
            The agent can see file <em>names</em> in <span className="font-mono">{rootPath}</span>
            {" "}
            and Code mode can read their contents using native tools. Indexing adds semantic
            retrieval; it is not required for the agent to inspect source files.
          </p>
          <Button
            size="sm"
            onClick={() => void beginIndex()}
            disabled={indexing}
            className="gap-1.5"
          >
            {indexing ? (
              <>
                <Loader2 className="h-3 w-3 animate-spin" />
                Indexing…
              </>
            ) : (
              <>
                <Database className="h-3 w-3" />
                Index Project
              </>
            )}
          </Button>
        </div>
      )}
      <ChatMessages />
      {project && (
        <div className="max-h-[35%] shrink-0 overflow-y-auto px-2" aria-label="Code Agent approvals">
          <ActionList key={`${rootPath}:${project.id}`} taskId={0} />
        </div>
      )}
      <ChatInput />
    </div>
  );
}
