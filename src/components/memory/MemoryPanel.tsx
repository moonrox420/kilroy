/**
 * Memory panel — a tabbed dialog over the project memory DB.
 *
 * Triggered from the Memory menu. Tabs:
 *   * Sessions  — list conversations, start a fresh session
 *   * Decisions — list logged architectural decisions
 *   * Files     — list indexed files, jump to one
 *   * Tasks     — recent task graph runs
 */
import { Bookmark, Database, FileCode2, ListChecks, Plus } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useMemoryPanel, type MemoryTab } from "@/store/memoryPanel";
import { SessionsPanel } from "./panels/SessionsPanel";
import { DecisionsPanel } from "./panels/DecisionsPanel";
import { FilesPanel } from "./panels/FilesPanel";
import { TasksPanel } from "./panels/TasksPanel";
import { useMemory } from "@/store/memory";

export function MemoryPanel() {
  const open = useMemoryPanel((s) => s.open);
  const close = useMemoryPanel((s) => s.close);
  const tab = useMemoryPanel((s) => s.tab);
  const setTab = useMemoryPanel((s) => s.setTab);
  const openSkillCreator = useMemoryPanel((s) => s.openSkillCreator);
  const project = useMemory((s) => s.project);

  return (
    <Dialog open={open} onOpenChange={(v) => !v && close()}>
      <DialogContent>
        <DialogHeader>
          <div className="flex items-center justify-between gap-3">
            <DialogTitle className="flex items-center gap-2">
              <Database className="h-3.5 w-3.5 text-amber" />
              Project Memory
            </DialogTitle>
            {/* The "+ New Skill" button lives here (not as a tab) because
                skills are user-authored knowledge that applies across all
                four memory views — surfacing it at the top makes it the
                primary "add to my agent's brain" action regardless of
                which tab the user happens to be on. */}
            <button
              type="button"
              onClick={openSkillCreator}
              className="flex h-6 items-center gap-1 rounded-md border border-line bg-bg-1 px-2 text-[11px] text-ink hover:bg-bg-2 hover:text-ink"
              title="Create a new skill — Markdown notes the agent carries into every chat turn"
            >
              <Plus className="h-3 w-3" />
              New Skill
            </button>
          </div>
          <DialogDescription>
            {project
              ? `${project.name} · ${project.root_path}`
              : "No project open — open a folder first."}
          </DialogDescription>
        </DialogHeader>
        <Tabs
          value={tab}
          onValueChange={(v) => setTab(v as MemoryTab)}
          className="flex h-[520px] flex-col"
        >
          <TabsList>
            <TabsTrigger value="sessions">
              <Bookmark className="h-3 w-3" />
              Sessions
            </TabsTrigger>
            <TabsTrigger value="decisions">
              <ListChecks className="h-3 w-3" />
              Decisions
            </TabsTrigger>
            <TabsTrigger value="files">
              <FileCode2 className="h-3 w-3" />
              Files
            </TabsTrigger>
            <TabsTrigger value="tasks">
              <Database className="h-3 w-3" />
              Tasks
            </TabsTrigger>
          </TabsList>
          <TabsContent value="sessions" className="flex min-h-0 flex-1 flex-col p-0">
            <SessionsPanel />
          </TabsContent>
          <TabsContent value="decisions" className="flex min-h-0 flex-1 flex-col p-0">
            <DecisionsPanel />
          </TabsContent>
          <TabsContent value="files" className="flex min-h-0 flex-1 flex-col p-0">
            <FilesPanel />
          </TabsContent>
          <TabsContent value="tasks" className="flex min-h-0 flex-1 flex-col p-0">
            <TasksPanel />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

/** Empty-state row used by every tab when there's nothing to show. */
export function EmptyState({
  title,
  body,
}: {
  title: string;
  body: string;
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-1 p-8 text-center">
      <p className="text-[12px] font-medium text-ink">{title}</p>
      <p className="max-w-[320px] text-[11px] text-ink-subtle">{body}</p>
    </div>
  );
}

/** Loading placeholder. */
export function Loading() {
  return (
    <div className="flex flex-1 items-center justify-center text-[11px] text-ink-subtle">
      loading…
    </div>
  );
}
