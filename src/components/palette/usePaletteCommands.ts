/**
 * Command-palette command registry.
 *
 * Returns the full list of `PaletteCommand`s the palette can run.
 * Built fresh on each render — the cost is trivial and it means we
 * always close over the latest store handles.
 *
 * Categories surface as chips on each row in the palette UI.
 */
import { useMemo } from "react";
import type { ComponentType } from "react";
import {
  Bot,
  Brain,
  Cpu,
  Database,
  FileCode2,
  FileText,
  FolderOpen,
  Lightbulb,
  ListChecks,
  PanelLeft,
  Save,
  ScrollText,
  Sparkles,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-react";
import { useWorkspace } from "@/store/workspace";
import { useUI } from "@/store/ui";
import { useAgent } from "@/store/agent";
import { useMemory } from "@/store/memory";
import { useMemoryPanel } from "@/store/memoryPanel";
import { useTerminals } from "@/store/terminals";
import { useRecents } from "@/store/recents";

export interface PaletteCommand {
  id: string;
  label: string;
  /** Optional sub-line for context (file path, agent mode blurb, etc.). */
  detail?: string;
  category: string;
  shortcut?: string;
  icon: ComponentType<{ className?: string }>;
  /** Stable rank used to pre-order tied scores. Lower = preferred. */
  weight?: number;
  run: () => void | Promise<void>;
  disabled?: boolean;
}

export interface PaletteContext {
  onOpenSettings: () => void;
  onOpenActivity: () => void;
}

export function usePaletteCommands(ctx: PaletteContext): PaletteCommand[] {
  const ws = useWorkspace();
  const ui = useUI();
  const ag = useAgent();
  const mem = useMemory();
  const memPanel = useMemoryPanel();
  const terms = useTerminals();
  const recents = useRecents();

  const fileTabs = ws.openTabs;
  const sessions = terms.sessions;

  return useMemo(() => {
    const out: PaletteCommand[] = [];

    // ─── File ─────────────────────────────────────────────
    out.push(
      {
        id: "file.new",
        label: "New File",
        category: "File",
        shortcut: "Ctrl+N",
        icon: FileText,
        weight: 5,
        run: () => ws.newUntitled(),
      },
      {
        id: "file.open-folder",
        label: "Open Folder…",
        category: "File",
        shortcut: "Ctrl+O",
        icon: FolderOpen,
        weight: 10,
        run: () => ws.openFolder(),
      },
      {
        id: "file.save",
        label: "Save",
        category: "File",
        shortcut: "Ctrl+S",
        icon: Save,
        disabled: !ws.activePath,
        weight: 20,
        run: () => ws.saveActive(),
      },
      {
        id: "file.save-as",
        label: "Save As…",
        category: "File",
        icon: Save,
        disabled: !ws.activePath,
        weight: 22,
        run: () => ws.saveActiveAs(),
      },
      {
        id: "file.save-all",
        label: "Save All",
        category: "File",
        shortcut: "Ctrl+Shift+S",
        icon: Save,
        weight: 21,
        run: () => ws.saveAll(),
      },
      {
        id: "file.close-tab",
        label: "Close Tab",
        category: "File",
        shortcut: "Ctrl+W",
        icon: X,
        disabled: !ws.activePath,
        weight: 22,
        run: () => {
          if (ws.activePath) ws.closeTab(ws.activePath);
        },
      },
      {
        id: "file.settings",
        label: "Settings…",
        category: "File",
        shortcut: "Ctrl+,",
        icon: Cpu,
        weight: 25,
        run: () => ctx.onOpenSettings(),
      },
    );

    // ─── View ─────────────────────────────────────────────
    out.push(
      {
        id: "view.toggle-explorer",
        label: ui.leftCollapsed ? "Show Explorer" : "Hide Explorer",
        category: "View",
        shortcut: "Ctrl+B",
        icon: PanelLeft,
        weight: 30,
        run: () => ui.toggleLeft(),
      },
      {
        id: "view.toggle-terminal",
        label: ui.terminalCollapsed ? "Show Terminal Panel" : "Hide Terminal Panel",
        category: "View",
        shortcut: "Ctrl+`",
        icon: TerminalSquare,
        weight: 31,
        run: () => ui.toggleTerminal(),
      },
      {
        id: "view.activity",
        label: "Activity Feed…",
        category: "View",
        icon: Sparkles,
        weight: 32,
        disabled: !mem.project,
        run: () => ctx.onOpenActivity(),
      },
    );

    // ─── Terminal ─────────────────────────────────────────
    out.push(
      {
        id: "terminal.new",
        label: "New Terminal",
        category: "Terminal",
        shortcut: "Ctrl+Shift+`",
        icon: TerminalSquare,
        weight: 40,
        run: () => {
          void terms.add({ cwd: ws.rootPath ?? undefined });
        },
      },
      {
        id: "terminal.close-active",
        label: "Close Active Terminal",
        category: "Terminal",
        icon: X,
        weight: 41,
        disabled: !terms.activeId,
        run: () => {
          if (!terms.activeId) return;
          void terms.close(terms.activeId);
        },
      },
    );
    for (const s of sessions) {
      out.push({
        id: `terminal.switch.${s.id}`,
        label: `Switch to Terminal: ${s.label}`,
        detail: s.cwd ?? undefined,
        category: "Terminal",
        icon: TerminalSquare,
        weight: 45,
        run: () => terms.setActive(s.id),
      });
    }

    // ─── Agent ────────────────────────────────────────────
    out.push(
      {
        id: "agent.mode.code",
        label: "Agent Mode: Code",
        detail: "Typed investigation; writes and shell require approval.",
        category: "Agent",
        icon: Bot,
        weight: 49,
        run: () => ag.setMode("code_agent"),
      },
      {
        id: "agent.mode.copilot",
        label: "Agent Mode: Chat",
        detail: "Ollama conversation with project context; no execution.",
        category: "Agent",
        icon: Bot,
        weight: 50,
        run: () => ag.setMode("copilot"),
      },
      {
        id: "agent.mode.autonomous",
        label: "Agent Mode: Plan / Execute",
        detail: "Typed task DAG with approval-gated execution.",
        category: "Agent",
        icon: Brain,
        weight: 51,
        run: () => ag.setMode("autonomous"),
      },
      {
        id: "agent.mode.debug",
        label: "Agent Mode: Review / Debug",
        detail: "Evidence-based analysis of diffs, diagnostics, logs, and tests.",
        category: "Agent",
        icon: Brain,
        weight: 52,
        run: () => ag.setMode("debug"),
      },
      {
        id: "agent.clear-chat",
        label: "Clear Chat",
        category: "Agent",
        icon: Trash2,
        weight: 53,
        run: () => ag.clear(),
      },
    );

    // ─── Memory ───────────────────────────────────────────
    out.push(
      {
        id: "memory.index-project",
        label: mem.indexing ? "Indexing in progress…" : "Index Project",
        category: "Memory",
        shortcut: "Ctrl+Shift+I",
        icon: Database,
        weight: 60,
        disabled: !mem.project || mem.indexing,
        run: () => {
          void mem.beginIndex();
        },
      },
      {
        id: "memory.log-decision",
        label: "Log Decision…",
        category: "Memory",
        icon: Lightbulb,
        weight: 61,
        disabled: !mem.project,
        run: () => memPanel.openDecisionComposer(),
      },
      {
        id: "memory.browse-sessions",
        label: "Browse Sessions…",
        category: "Memory",
        icon: ScrollText,
        weight: 62,
        disabled: !mem.project,
        run: () => memPanel.openTab("sessions"),
      },
      {
        id: "memory.browse-decisions",
        label: "Browse Decisions…",
        category: "Memory",
        icon: ListChecks,
        weight: 63,
        disabled: !mem.project,
        run: () => memPanel.openTab("decisions"),
      },
      {
        id: "memory.search-code-index",
        label: "Search Code Index…",
        category: "Memory",
        icon: FileCode2,
        weight: 64,
        disabled: !mem.project,
        run: () => memPanel.openTab("files"),
      },
      {
        id: "memory.task-history",
        label: "Task History…",
        category: "Memory",
        icon: ListChecks,
        weight: 65,
        disabled: !mem.project,
        run: () => memPanel.openTab("tasks"),
      },
    );

    // ─── Open file tabs (jump to) ────────────────────────
    for (const t of fileTabs) {
      out.push({
        id: `tab.switch.${t.path}`,
        label: t.name + (t.dirty ? " •" : ""),
        detail: t.path,
        category: "Open Tab",
        icon: FileText,
        weight: 70,
        run: () => ws.setActive(t.path),
      });
    }

    // ─── Recent files ─────────────────────────────────────
    // Skip recents that are already open as a tab to avoid duplication.
    const openPaths = new Set(fileTabs.map((t) => t.path));
    for (const path of recents.files) {
      if (openPaths.has(path)) continue;
      out.push({
        id: `recent.${path}`,
        label: basename(path),
        detail: path,
        category: "Recent",
        icon: FileText,
        weight: 80,
        run: () => ws.openFile(path),
      });
    }

    return out;
  }, [ws, ui, ag, mem, memPanel, terms, recents, fileTabs, sessions, ctx]);
}

function basename(path: string): string {
  const norm = path.replace(/\\/g, "/");
  const i = norm.lastIndexOf("/");
  return i === -1 ? norm : norm.slice(i + 1);
}
