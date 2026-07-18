/**
 * Agent Mode Selector.
 *
 * Lives in the left column below the File Explorer. Switches the global
 * agent operating mode and shows a one-line description of what each
 * mode does so the user always knows what they're authorizing.
 */
import { Bot, Cpu, ShieldCheck, Sparkles, ChevronDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAgent } from "@/store/agent";
import type { AgentMode } from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface ModeMeta {
  id: AgentMode;
  label: string;
  blurb: string;
  icon: React.ComponentType<{ className?: string }>;
}

const MODES: ModeMeta[] = [
  {
    id: "code_agent",
    label: "Code",
    blurb: "Investigates with typed tools; changes require approval.",
    icon: Sparkles,
  },
  {
    id: "copilot",
    label: "Chat",
    blurb: "Conversation with retrieved project context; no execution.",
    icon: Bot,
  },
  {
    id: "autonomous",
    label: "Plan / Execute",
    blurb: "Creates a task DAG, then waits for approval to execute.",
    icon: Cpu,
  },
  {
    id: "debug",
    label: "Review / Debug",
    blurb: "Reasons from diffs, diagnostics, logs, and test evidence.",
    icon: ShieldCheck,
  },
];

export function AgentModeSelector() {
  const mode = useAgent((s) => s.mode);
  const setMode = useAgent((s) => s.setMode);
  const current = MODES.find((m) => m.id === mode) ?? MODES[0];
  const Icon = current.icon;

  return (
    <div className="flex flex-col border-t border-line">
      <div className="panel-header">
        <span>Agent Mode</span>
      </div>
      <div className="p-2">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              className={cn(
                "group flex w-full items-center justify-between gap-2 rounded-md border border-line bg-bg-2 px-2 py-1.5 text-left transition-colors",
                "hover:border-line-strong",
                "data-[state=open]:border-amber data-[state=open]:ring-amber-glow",
              )}
            >
              <span className="flex items-center gap-2 truncate">
                <span className="flex h-6 w-6 items-center justify-center rounded-sm bg-amber/10 text-amber">
                  <Icon className="h-3.5 w-3.5" />
                </span>
                <span className="flex flex-col truncate">
                  <span className="text-[12px] font-medium text-ink">
                    {current.label}
                  </span>
                  <span className="truncate text-[10.5px] text-ink-subtle">
                    {current.blurb}
                  </span>
                </span>
              </span>
              <ChevronDown className="h-3.5 w-3.5 shrink-0 text-ink-subtle" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-[260px]" align="start">
            {MODES.map((m) => {
              const Mi = m.icon;
              const active = m.id === mode;
              return (
                <DropdownMenuItem
                  key={m.id}
                  onSelect={() => setMode(m.id)}
                  className="flex flex-col items-start gap-0.5 py-2"
                >
                  <span className="flex items-center gap-2">
                    <Mi
                      className={cn(
                        "h-3.5 w-3.5",
                        active ? "text-amber" : "text-ink-subtle",
                      )}
                    />
                    <span
                      className={cn(
                        "text-[12px] font-medium",
                        active ? "text-amber" : "text-ink",
                      )}
                    >
                      {m.label}
                    </span>
                  </span>
                  <span className="pl-5 text-[11px] text-ink-subtle">
                    {m.blurb}
                  </span>
                </DropdownMenuItem>
              );
            })}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}
