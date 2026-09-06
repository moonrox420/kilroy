/**
 * Top menu bar — File / Edit / Selection / View / Go / Terminal / Agent / Memory / Help.
 *
 * Items dispatch into the workspace, UI, agent, or memory stores.
 * Keyboard shortcuts are mounted in a single window-level listener.
 */
import { useEffect } from "react";
import { editorAction } from "@/lib/editorCommands";
import { skills } from "@/lib/tauri";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useWorkspace } from "@/store/workspace";
import { useUI } from "@/store/ui";
import { useAgent } from "@/store/agent";
import { useMemory } from "@/store/memory";
import { useMemoryPanel } from "@/store/memoryPanel";
import { useTerminals } from "@/store/terminals";
import { usePalette } from "@/store/palette";
import { useDatasets } from "@/store/datasets";
import { useRefactor } from "@/store/refactor";
import { cn } from "@/lib/utils";

const ITEM = "text-ink-muted hover:text-ink";

export function MenuBar({
  onOpenActivity,
  onOpenSettings,
}: {
  onOpenActivity: () => void;
  onOpenSettings: () => void;
}) {
  const openFolder = useWorkspace((s) => s.openFolder);
  const newUntitled = useWorkspace((s) => s.newUntitled);
  const saveActive = useWorkspace((s) => s.saveActive);
  const saveActiveAs = useWorkspace((s) => s.saveActiveAs);
  const saveAll = useWorkspace((s) => s.saveAll);
  const closeTab = useWorkspace((s) => s.closeTab);
  const activePath = useWorkspace((s) => s.activePath);

  const toggleLeft = useUI((s) => s.toggleLeft);
  const toggleTerminal = useUI((s) => s.toggleTerminal);

  const setMode = useAgent((s) => s.setMode);
  const clearChat = useAgent((s) => s.clear);

  const beginIndex = useMemory((s) => s.beginIndex);
  const project = useMemory((s) => s.project);
  const indexing = useMemory((s) => s.indexing);

  const openTab = useMemoryPanel((s) => s.openTab);
  const openDecisionComposer = useMemoryPanel((s) => s.openDecisionComposer);
  const openSkillCreator = useMemoryPanel((s) => s.openSkillCreator);
  const openDatasetsDialog = useDatasets((s) => s.openDialog);
  const openRefactorPanel = useRefactor((s) => s.openPanel);

  const rootPath = useWorkspace((s) => s.rootPath);
  const addTerminal = useTerminals((s) => s.add);
  const sessions = useTerminals((s) => s.sessions);
  const activeTermId = useTerminals((s) => s.activeId);
  const closeTerminal = useTerminals((s) => s.close);

  // Global shortcuts.
  useEffect(() => {
    const onKey = async (e: KeyboardEvent) => {
      const isCtrl = e.ctrlKey || e.metaKey;
      if (isCtrl && e.key.toLowerCase() === "n" && !e.shiftKey) {
        e.preventDefault();
        newUntitled();
      } else if (isCtrl && e.key.toLowerCase() === "s" && !e.shiftKey) {
        e.preventDefault();
        await saveActive();
      } else if (isCtrl && e.shiftKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        await saveAll();
      } else if (isCtrl && e.key.toLowerCase() === "o") {
        e.preventDefault();
        await openFolder();
      } else if (isCtrl && e.key.toLowerCase() === "w") {
        e.preventDefault();
        if (activePath) closeTab(activePath);
      } else if (isCtrl && e.key.toLowerCase() === "b") {
        e.preventDefault();
        toggleLeft();
      } else if (isCtrl && e.shiftKey && e.key === "`") {
        e.preventDefault();
        await addTerminal({ cwd: rootPath ?? undefined });
      } else if (isCtrl && e.key === "`") {
        e.preventDefault();
        toggleTerminal();
      } else if (isCtrl && e.shiftKey && e.key.toLowerCase() === "i") {
        e.preventDefault();
        if (project) await beginIndex();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    openFolder,
    newUntitled,
    saveActive,
    saveAll,
    closeTab,
    activePath,
    toggleLeft,
    toggleTerminal,
    beginIndex,
    project,
    addTerminal,
    rootPath,
  ]);

  return (
    <div
      className="no-drag flex items-center border-b border-line bg-bg-0 px-2 select-none"
      style={{ height: "var(--menubar-h)" }}
    >
      <Menu label="File">
        <DropdownMenuItem onSelect={() => newUntitled()} className={ITEM}>
          New File
          <DropdownMenuShortcut>Ctrl+N</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => openFolder()} className={ITEM}>
          Open Folder…
          <DropdownMenuShortcut>Ctrl+O</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => saveActive()}
          className={ITEM}
          disabled={!activePath}
        >
          Save
          <DropdownMenuShortcut>Ctrl+S</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => saveActiveAs()}
          className={ITEM}
          disabled={!activePath}
        >
          Save As…
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => saveAll()} className={ITEM}>
          Save All
          <DropdownMenuShortcut>Ctrl+Shift+S</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => activePath && closeTab(activePath)}
          className={ITEM}
          disabled={!activePath}
        >
          Close Tab
          <DropdownMenuShortcut>Ctrl+W</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={onOpenSettings} className={ITEM}>
          Settings…
          <DropdownMenuShortcut>Ctrl+,</DropdownMenuShortcut>
        </DropdownMenuItem>
      </Menu>

      <Menu label="Edit">
        <DropdownMenuItem onSelect={() => editorAction("undo")} className={ITEM}>
          Undo<DropdownMenuShortcut>Ctrl+Z</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => editorAction("redo")} className={ITEM}>
          Redo<DropdownMenuShortcut>Ctrl+Y</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => editorAction("editor.action.clipboardCutAction")} className={ITEM}>
          Cut<DropdownMenuShortcut>Ctrl+X</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => editorAction("editor.action.clipboardCopyAction")} className={ITEM}>
          Copy<DropdownMenuShortcut>Ctrl+C</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => editorAction("editor.action.clipboardPasteAction")} className={ITEM}>
          Paste<DropdownMenuShortcut>Ctrl+V</DropdownMenuShortcut>
        </DropdownMenuItem>
      </Menu>

      <Menu label="Selection">
        <DropdownMenuItem onSelect={() => editorAction("editor.action.selectAll")} className={ITEM}>
          Select All<DropdownMenuShortcut>Ctrl+A</DropdownMenuShortcut>
        </DropdownMenuItem>
      </Menu>

      <Menu label="View">
        <DropdownMenuItem onSelect={toggleLeft} className={ITEM}>
          Toggle Explorer<DropdownMenuShortcut>Ctrl+B</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={toggleTerminal} className={ITEM}>
          Toggle Terminal<DropdownMenuShortcut>Ctrl+`</DropdownMenuShortcut>
        </DropdownMenuItem>
      </Menu>

      <Menu label="Go">
        <DropdownMenuItem className={ITEM} disabled>Go to File…<DropdownMenuShortcut>Ctrl+P</DropdownMenuShortcut></DropdownMenuItem>
        <DropdownMenuItem className={ITEM} disabled>Go to Symbol…<DropdownMenuShortcut>Ctrl+Shift+O</DropdownMenuShortcut></DropdownMenuItem>
      </Menu>

      <Menu label="Terminal">
        <DropdownMenuItem
          onSelect={() => void addTerminal({ cwd: rootPath ?? undefined })}
          className={ITEM}
        >
          New Terminal
          <DropdownMenuShortcut>Ctrl+Shift+`</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => activeTermId && void closeTerminal(activeTermId)}
          className={ITEM}
          disabled={!activeTermId}
        >
          Close Active Terminal
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={toggleTerminal} className={ITEM}>
          Toggle Panel
          <DropdownMenuShortcut>Ctrl+`</DropdownMenuShortcut>
        </DropdownMenuItem>
        {sessions.length > 0 && <DropdownMenuSeparator />}
        {sessions.map((s, i) => (
          <DropdownMenuItem
            key={s.id}
            onSelect={() => useTerminals.getState().setActive(s.id)}
            className={ITEM}
          >
            {i + 1}. {s.label}
            {s.id === activeTermId && (
              <DropdownMenuShortcut>active</DropdownMenuShortcut>
            )}
          </DropdownMenuItem>
        ))}
      </Menu>

      <Menu label="Agent">
        <DropdownMenuItem onSelect={() => setMode("code_agent")} className={ITEM}>
          Mode · Code (default)
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => setMode("copilot")} className={ITEM}>
          Mode · Chat (no execution)
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => setMode("autonomous")} className={ITEM}>
          Mode · Plan / Execute
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => setMode("debug")} className={ITEM}>
          Mode · Review / Debug
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={openSkillCreator} className={ITEM}>
          New Skill…
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={openDatasetsDialog} className={ITEM}>
          Train Custom Model from Dataset…
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={openRefactorPanel} className={ITEM}>
          Background Refactor Swarm…
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={clearChat} className={ITEM}>
          Clear Chat
        </DropdownMenuItem>
      </Menu>

      <Menu label="Memory">
        <DropdownMenuItem
          onSelect={() => project && beginIndex()}
          className={ITEM}
          disabled={!project || indexing}
        >
          {indexing ? "Indexing…" : "Index Project"}
          <DropdownMenuShortcut>Ctrl+Shift+I</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={openDecisionComposer}
          className={ITEM}
          disabled={!project}
        >
          Log Decision…
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => openTab("sessions")}
          className={ITEM}
          disabled={!project}
        >
          Browse Sessions…
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => openTab("decisions")}
          className={ITEM}
          disabled={!project}
        >
          Browse Decisions…
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => openTab("files")}
          className={ITEM}
          disabled={!project}
        >
          Search Code Index…
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => openTab("tasks")}
          className={ITEM}
          disabled={!project}
        >
          Task History…
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() =>
            void skills.openFolder("global").catch((err) =>
              console.error("open_skills_folder:", err),
            )
          }
          className={ITEM}
        >
          Open Global Skills Folder…
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() =>
            void skills.openFolder("project").catch(
              (err) => console.error("open_skills_folder:", err),
            )
          }
          className={ITEM}
          disabled={!project}
        >
          Open Project Skills Folder…
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={onOpenActivity}
          className={ITEM}
          disabled={!project}
        >
          Activity Feed…
        </DropdownMenuItem>
      </Menu>

      <Menu label="Help">
        <DropdownMenuItem
          onSelect={() => usePalette.getState().show()}
          className={ITEM}
        >
          Command Palette…
          <DropdownMenuShortcut>Ctrl+Shift+P</DropdownMenuShortcut>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem className={ITEM} disabled>About Kilroy</DropdownMenuItem>
        <DropdownMenuItem className={ITEM} disabled>Keyboard Shortcuts</DropdownMenuItem>
        <DropdownMenuItem className={ITEM} disabled>Open Logs Folder</DropdownMenuItem>
      </Menu>
    </div>
  );
}

function Menu({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          className={cn(
            "h-[22px] rounded-sm px-2 text-[12px] text-ink-muted hover:bg-bg-2 hover:text-ink",
            "data-[state=open]:bg-bg-2 data-[state=open]:text-ink",
          )}
        >
          {label}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" sideOffset={2}>
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
