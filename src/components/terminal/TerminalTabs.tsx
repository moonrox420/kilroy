/**
 * Tab strip for the multi-tab terminal.
 *
 * Sits across the bottom edge of the terminal panel. Each tab shows
 * its label + an X to close. Active tab is highlighted with an amber
 * top border. Double-click a label to rename it inline.
 *
 * The trailing `+` is a dropdown — click to open a menu of installed
 * shells (cmd / powershell / pwsh / git bash / wsl distros). Choosing
 * one spawns a new tab with that shell.
 */
import { useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  Plus,
  Terminal as TermIcon,
  X,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useTerminals, type TermSession } from "@/store/terminals";
import { useWorkspace } from "@/store/workspace";
import { term, type ShellOption } from "@/lib/tauri";
import { notify } from "@/store/notifications";
import { cn } from "@/lib/utils";

export function TerminalTabs() {
  const sessions = useTerminals((s) => s.sessions);
  const activeId = useTerminals((s) => s.activeId);
  const setActive = useTerminals((s) => s.setActive);
  const close = useTerminals((s) => s.close);
  const add = useTerminals((s) => s.add);
  const rootPath = useWorkspace((s) => s.rootPath);

  const [shells, setShells] = useState<ShellOption[]>([]);

  // Refresh the shell list lazily — first time the user hovers the `+`,
  // or whenever the menu re-opens.
  const refreshShells = async () => {
    try {
      const list = await term.listShells();
      setShells(list);
    } catch (err) {
      notify.fromError("List shells", err);
    }
  };

  useEffect(() => {
    void refreshShells();
  }, []);

  const spawn = (shellId?: string) => {
    void add({ cwd: rootPath ?? undefined, shell: shellId });
  };

  const available = shells.filter((s) => s.available);

  return (
    <div className="flex h-7 shrink-0 items-center gap-0.5 overflow-x-auto border-t border-line bg-bg-1 px-1">
      {sessions.map((s) => (
        <TabButton
          key={s.id}
          session={s}
          active={s.id === activeId}
          onSelect={() => setActive(s.id)}
          onClose={() => void close(s.id)}
        />
      ))}

      {/* Right-side split button — clearly visible "+ New shell ▾"
          instead of the previous near-invisible chevron. The left half
          spawns the default shell, the right half opens the picker. */}
      <div className="ml-1 flex items-center">
        <button
          onClick={() => spawn()}
          title="New terminal (default shell)"
          className={cn(
            "flex h-5 items-center gap-1 rounded-l-sm border border-line bg-bg-0 px-2",
            "text-[10px] uppercase tracking-wider text-ink-subtle transition-colors",
            "hover:border-amber/50 hover:bg-amber/10 hover:text-amber",
          )}
        >
          <Plus className="h-3 w-3" />
          New
        </button>
        <DropdownMenu onOpenChange={(v) => v && void refreshShells()}>
          <DropdownMenuTrigger asChild>
            <button
              title="Pick a shell (powershell / pwsh / cmd / git bash / WSL)"
              className={cn(
                "flex h-5 items-center justify-center rounded-r-sm border border-l-0 border-line bg-bg-0 px-1",
                "text-ink-subtle transition-colors",
                "hover:border-amber/50 hover:bg-amber/10 hover:text-amber",
              )}
            >
              <ChevronDown className="h-3 w-3" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" sideOffset={6} className="w-[260px]">
            <div className="px-2 py-1 text-[10px] uppercase tracking-wider text-ink-subtle">
              Spawn in shell
            </div>
            {available.length === 0 ? (
              <DropdownMenuItem disabled>No shells detected</DropdownMenuItem>
            ) : (
              available.map((s) => (
                <DropdownMenuItem
                  key={s.id}
                  onSelect={() => spawn(s.id)}
                  className="flex flex-col items-start gap-0 py-1.5"
                >
                  <span className="text-[12px] text-ink">{s.label}</span>
                  {s.path && (
                    <span className="text-[10px] text-ink-subtle font-mono truncate max-w-[230px]">
                      {s.path}
                    </span>
                  )}
                </DropdownMenuItem>
              ))
            )}
            {shells.some((s) => !s.available) && (
              <>
                <div className="mx-2 my-1 h-px bg-line" />
                <div className="px-2 py-1 text-[10px] uppercase tracking-wider text-ink-subtle">
                  Not installed
                </div>
                {shells
                  .filter((s) => !s.available)
                  .map((s) => (
                    <DropdownMenuItem
                      key={s.id}
                      disabled
                      className="text-[11px] italic text-ink-subtle"
                    >
                      {s.label}
                    </DropdownMenuItem>
                  ))}
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}

function TabButton({
  session,
  active,
  onSelect,
  onClose,
}: {
  session: TermSession;
  active: boolean;
  onSelect: () => void;
  onClose: () => void;
}) {
  const rename = useTerminals((s) => s.rename);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(session.label);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing]);

  const commit = () => {
    rename(session.id, draft);
    setEditing(false);
  };

  return (
    <div
      onClick={onSelect}
      onDoubleClick={(e) => {
        e.stopPropagation();
        setDraft(session.label);
        setEditing(true);
      }}
      onAuxClick={(e) => {
        if (e.button === 1) {
          e.preventDefault();
          onClose();
        }
      }}
      className={cn(
        "group flex h-6 cursor-pointer items-center gap-1.5 rounded-sm border-t-2 px-2 text-[11px] transition-colors",
        active
          ? "border-t-amber bg-bg-2 text-ink"
          : "border-t-transparent text-ink-muted hover:bg-bg-2 hover:text-ink",
        session.exited && "opacity-60 italic",
      )}
      title={session.cwd ?? session.label}
    >
      <TermIcon
        className={cn(
          "h-3 w-3 shrink-0",
          active ? "text-amber" : "text-ink-subtle",
        )}
      />
      {editing ? (
        <input
          ref={inputRef}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              setEditing(false);
            }
          }}
          className="w-[110px] bg-transparent text-[11px] text-ink outline-none"
          onClick={(e) => e.stopPropagation()}
        />
      ) : (
        <span className="max-w-[140px] truncate">{session.label}</span>
      )}
      <button
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
        className="ml-1 flex h-3.5 w-3.5 items-center justify-center rounded-sm text-ink-subtle hover:bg-bg-3 hover:text-ink"
        aria-label="Close terminal"
        title="Close (Middle-click also closes)"
      >
        <X className="h-2.5 w-2.5" />
      </button>
    </div>
  );
}
