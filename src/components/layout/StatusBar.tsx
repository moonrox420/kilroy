/**
 * Status bar — bottom strip.
 *
 * Shows file language, dirty state, agent mode, current model, Ollama
 * health, indexing progress, and a live background-task indicator.
 * Hosts the explorer + terminal toggles so the panels above can extend
 * flush to the status bar.
 */
import {
  Activity,
  Brain,
  Cpu,
  Database,
  FileText,
  GitBranch,
  Hash,
  PanelLeft,
  Settings as SettingsIcon,
  Sparkles,
  TerminalSquare,
  Zap,
  ZapOff,
} from "lucide-react";
import { useWorkspace } from "@/store/workspace";
import { useAgent } from "@/store/agent";
import { useUI } from "@/store/ui";
import { useMemory } from "@/store/memory";
import { useSettings } from "@/store/settings";
import { cn } from "@/lib/utils";

export function StatusBar({
  onOpenActivity,
  onOpenSettings,
  onOpenDiagnostics,
}: {
  onOpenActivity: () => void;
  onOpenSettings: () => void;
  onOpenDiagnostics?: () => void;
}) {
  const tabs = useWorkspace((s) => s.openTabs);
  const activePath = useWorkspace((s) => s.activePath);
  const rootPath = useWorkspace((s) => s.rootPath);
  const active = tabs.find((t) => t.path === activePath) ?? null;

  const mode = useAgent((s) => s.mode);
  const isThinking = useAgent((s) => s.isThinking);

  const leftCollapsed = useUI((s) => s.leftCollapsed);
  const terminalCollapsed = useUI((s) => s.terminalCollapsed);
  const toggleLeft = useUI((s) => s.toggleLeft);
  const toggleTerminal = useUI((s) => s.toggleTerminal);

  // Prefer the rich health from settings (refreshed by Save / Test Connection).
  // Fall back to the OllamaStatus stashed on project open if we haven't checked yet.
  const settingsHealth = useSettings((s) => s.health);
  const projectOllama = useMemory((s) => s.ollama);
  const ollama = settingsHealth
    ? {
        reachable: settingsHealth.reachable,
        embedding_model: settingsHealth.embedding_model,
        has_embedding_model: settingsHealth.has_embedding_model,
      }
    : projectOllama;
  const settingsModel = useSettings((s) => s.current?.embedding_model);
  const chatModelLabel = useSettings((s) => s.current?.chat_model);
  const indexing = useMemory((s) => s.indexing);
  const indexProgress = useMemory((s) => s.indexProgress);
  const lastIndex = useMemory((s) => s.lastIndex);

  const indexingLabel = indexing
    ? indexProgress
      ? `indexing ${indexProgress.current}/${indexProgress.total}`
      : "indexing…"
    : lastIndex
      ? `${lastIndex.chunks_inserted} chunks`
      : "not indexed";

  return (
    <div
      className="flex items-center justify-between border-t border-line bg-bg-1 px-2 text-[11px] text-ink-muted select-none"
      style={{ height: "var(--statusbar-h)" }}
    >
      <div className="flex items-center gap-2">
        <Toggle
          active={!leftCollapsed}
          onClick={toggleLeft}
          title="Toggle Explorer (Ctrl+B)"
        >
          <PanelLeft className="h-3 w-3" />
        </Toggle>
        <Toggle
          active={!terminalCollapsed}
          onClick={toggleTerminal}
          title="Toggle Terminal (Ctrl+`)"
        >
          <TerminalSquare className="h-3 w-3" />
        </Toggle>
        <span className="mx-1 h-3 w-px bg-line" />
        <Item icon={<GitBranch className="h-3 w-3" />}>
          {rootPath ? abbreviate(rootPath) : "no folder"}
        </Item>
        <Item icon={<FileText className="h-3 w-3" />}>
          {active ? (active.dirty ? `${active.name} •` : active.name) : "no file"}
        </Item>
        <Item icon={<Hash className="h-3 w-3" />}>
          {active ? active.language : "—"}
        </Item>
      </div>
      <div className="flex items-center gap-3">
        <Item
          icon={
            ollama?.reachable ? (
              <Zap className={cn("h-3 w-3", ollama.has_embedding_model ? "text-ok" : "text-warn")} />
            ) : (
              <ZapOff className="h-3 w-3 text-err" />
            )
          }
          title={
            !ollama
              ? "Ollama status unknown"
              : ollama.reachable
                ? ollama.has_embedding_model
                  ? `Ollama up · ${ollama.embedding_model} installed`
                  : `Ollama up but ${ollama.embedding_model} missing — run: ollama pull ${ollama.embedding_model}`
                : "Ollama unreachable on localhost:11434"
          }
        >
          {!ollama
            ? "ollama ?"
            : ollama.reachable
              ? ollama.has_embedding_model
                ? "ollama"
                : "no model"
              : "no ollama"}
        </Item>
        <Item
          icon={<Database className={cn("h-3 w-3", indexing ? "text-amber" : "text-ink-subtle")} />}
          title={indexProgress?.message ?? "Memory index status"}
        >
          {indexingLabel}
        </Item>
        <Item icon={<Cpu className="h-3 w-3" />}>UTF-8</Item>
        <Item icon={<Cpu className="h-3 w-3" />}>LF</Item>
        <Item icon={<Brain className="h-3 w-3 text-amber" />}>
          <span className="text-ink">{labelForMode(mode)}</span>
        </Item>
        <Item
          icon={<Cpu className="h-3 w-3" />}
          title={`chat: ${chatModelLabel ?? "?"} · embed: ${settingsModel ?? "?"}`}
        >
          {chatModelLabel ?? settingsModel ?? "—"}
        </Item>
        {isThinking && (
          <span className="rounded-sm bg-amber px-1.5 py-[1px] text-[10px] font-semibold text-amber-ink animate-pulse-amber">
            WORKING
          </span>
        )}
        <button
          onClick={onOpenActivity}
          title="Activity feed"
          className={cn(
            "flex h-4 items-center gap-1 rounded-sm px-1 text-[10px] uppercase tracking-wider text-ink-subtle",
            "hover:bg-bg-2 hover:text-amber",
          )}
        >
          <Sparkles className="h-3 w-3" />
          activity
        </button>
        {onOpenDiagnostics && (
          <button
            onClick={onOpenDiagnostics}
            title="Diagnostics (Ctrl+Shift+D)"
            className={cn(
              "flex h-4 w-5 items-center justify-center rounded-sm text-ink-subtle",
              "hover:bg-bg-2 hover:text-amber",
            )}
          >
            <Activity className="h-3 w-3" />
          </button>
        )}
        <button
          onClick={onOpenSettings}
          title="Settings (Ctrl+,)"
          className={cn(
            "flex h-4 w-5 items-center justify-center rounded-sm text-ink-subtle",
            "hover:bg-bg-2 hover:text-amber",
          )}
        >
          <SettingsIcon className="h-3 w-3" />
        </button>
      </div>
    </div>
  );
}

function Item({
  icon,
  children,
  title,
}: {
  icon: React.ReactNode;
  children: React.ReactNode;
  title?: string;
}) {
  return (
    <span className={cn("flex items-center gap-1.5")} title={title}>
      {icon}
      <span>{children}</span>
    </span>
  );
}

function Toggle({
  active,
  onClick,
  title,
  children,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className={cn(
        "flex h-4 w-5 items-center justify-center rounded-sm transition-colors",
        active
          ? "text-amber hover:bg-bg-2"
          : "text-ink-subtle hover:bg-bg-2 hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}

function labelForMode(mode: string): string {
  switch (mode) {
    case "code_agent":
      return "Code";
    case "copilot":
      return "Chat";
    case "autonomous":
      return "Plan / Execute";
    case "multi_agent":
      return "Multi-Agent Org";
    case "governance":
      return "Governance";
    case "debug":
      return "Review / Debug";
    default:
      return mode;
  }
}

function abbreviate(p: string): string {
  const norm = p.replace(/\\/g, "/");
  const parts = norm.split("/").filter(Boolean);
  if (parts.length <= 3) return norm;
  return `…/${parts.slice(-2).join("/")}`;
}
