/**
 * Command Palette — Ctrl+Shift+P.
 *
 * Fuzzy-filtered launcher for every menu action plus open tabs, recent
 * files, and terminal sessions. Keyboard-first: arrow keys to navigate,
 * Enter to run, Esc to close.
 *
 * Positioned near the top of the screen (15% from top) instead of
 * centered — that's where users expect a palette to live, and it keeps
 * the chat panel visible behind it.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { Search } from "lucide-react";
import { usePalette } from "@/store/palette";
import { rankPath, fuzzyScore } from "./fuzzy";
import {
  usePaletteCommands,
  type PaletteCommand,
  type PaletteContext,
} from "./usePaletteCommands";
import { cn } from "@/lib/utils";

interface RankedCommand extends PaletteCommand {
  matches: number[];
  detailMatches: number[];
  score: number;
}

export function CommandPalette(ctx: PaletteContext) {
  const open = usePalette((s) => s.open);
  const hide = usePalette((s) => s.hide);
  const commands = usePaletteCommands(ctx);
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  // Clear query whenever the palette reopens.
  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIdx(0);
    }
  }, [open]);

  const ranked: RankedCommand[] = useMemo(() => {
    if (!query.trim()) {
      // No query → keep canonical ordering (recents → tabs → commands)
      // by sorting on `weight`.
      return commands
        .filter((c) => !c.disabled)
        .sort((a, b) => (a.weight ?? 99) - (b.weight ?? 99))
        .map((c) => ({ ...c, matches: [], detailMatches: [], score: 0 }));
    }
    const q = query.trim();
    const hits: RankedCommand[] = [];
    for (const c of commands) {
      if (c.disabled) continue;
      // Try the label first, then the path-aware detail (file paths).
      const labelHit = fuzzyScore(q, c.label);
      const detailHit = c.detail ? rankPath(q, c.detail) : null;
      const best =
        labelHit && (!detailHit || labelHit.score >= detailHit.score)
          ? { score: labelHit.score, matches: labelHit.matches, detailMatches: [] }
          : detailHit
            ? { score: detailHit.score, matches: [], detailMatches: detailHit.matches }
            : null;
      if (best) {
        hits.push({ ...c, ...best });
      }
    }
    hits.sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return (a.weight ?? 99) - (b.weight ?? 99);
    });
    return hits.slice(0, 80);
  }, [query, commands]);

  // Keep the active row inside the viewport.
  useEffect(() => {
    const ul = listRef.current;
    if (!ul) return;
    const li = ul.children[activeIdx] as HTMLElement | undefined;
    if (li) li.scrollIntoView({ block: "nearest" });
  }, [activeIdx, ranked]);

  // Clamp activeIdx when the filter shrinks the list.
  useEffect(() => {
    if (activeIdx >= ranked.length) setActiveIdx(Math.max(0, ranked.length - 1));
  }, [ranked, activeIdx]);

  const run = (cmd: RankedCommand) => {
    hide();
    // Defer one tick so the dialog's exit animation can start before the
    // command (which often opens another modal) runs.
    queueMicrotask(() => {
      void cmd.run();
    });
  };

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIdx((i) => Math.min(ranked.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIdx((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const cmd = ranked[activeIdx];
      if (cmd) run(cmd);
    } else if (e.key === "Escape") {
      e.preventDefault();
      hide();
    }
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={(v) => !v && hide()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-bg-0/60 backdrop-blur-sm animate-fade-in" />
        <DialogPrimitive.Content
          className={cn(
            "fixed left-1/2 top-[15%] z-50 w-[min(720px,calc(100vw-3rem))] -translate-x-1/2",
            "overflow-hidden rounded-lg border border-line bg-bg-1 shadow-2xl animate-slide-up",
            "focus-visible:outline-none",
          )}
        >
          <DialogPrimitive.Title className="sr-only">
            Command Palette
          </DialogPrimitive.Title>
          <header className="flex items-center gap-2 border-b border-line bg-bg-1 px-3 py-2">
            <Search className="h-4 w-4 shrink-0 text-amber" />
            <input
              autoFocus
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setActiveIdx(0);
              }}
              onKeyDown={onKey}
              placeholder="Type a command, file, or session…"
              className="flex-1 bg-transparent text-[13px] text-ink outline-none placeholder:text-ink-subtle"
            />
            <kbd className="rounded bg-bg-2 px-1.5 py-[1px] font-mono text-[10px] text-ink-subtle">
              Esc
            </kbd>
          </header>

          <ul
            ref={listRef}
            className="max-h-[60vh] overflow-y-auto divide-y divide-line/40"
          >
            {ranked.length === 0 && (
              <li className="p-6 text-center text-[11px] text-ink-subtle">
                No matches. Try a different query.
              </li>
            )}
            {ranked.map((c, i) => (
              <Row
                key={c.id}
                cmd={c}
                active={i === activeIdx}
                onSelect={() => run(c)}
                onHover={() => setActiveIdx(i)}
              />
            ))}
          </ul>
          <footer className="flex items-center justify-between border-t border-line bg-bg-1 px-3 py-1.5 text-[10px] text-ink-subtle">
            <span>
              {ranked.length} match{ranked.length === 1 ? "" : "es"}
            </span>
            <span className="flex items-center gap-2">
              <Hint k="↑ ↓">navigate</Hint>
              <Hint k="↵">run</Hint>
              <Hint k="Esc">close</Hint>
            </span>
          </footer>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

function Row({
  cmd,
  active,
  onSelect,
  onHover,
}: {
  cmd: RankedCommand;
  active: boolean;
  onSelect: () => void;
  onHover: () => void;
}) {
  const Icon = cmd.icon;
  return (
    <li
      onMouseDown={(e) => {
        // Prevent the focused input from losing focus before click handles.
        e.preventDefault();
        onSelect();
      }}
      onMouseEnter={onHover}
      className={cn(
        "flex cursor-pointer items-center gap-3 px-3 py-1.5 text-[12px] transition-colors",
        active ? "bg-amber/10 text-ink" : "text-ink-muted hover:bg-bg-2",
      )}
    >
      <Icon
        className={cn(
          "h-3.5 w-3.5 shrink-0",
          active ? "text-amber" : "text-ink-subtle",
        )}
      />
      <div className="flex-1 min-w-0">
        <p className="truncate">
          <Highlight text={cmd.label} matches={cmd.matches} />
        </p>
        {cmd.detail && (
          <p className="truncate text-[10.5px] text-ink-subtle">
            <Highlight text={cmd.detail} matches={cmd.detailMatches} />
          </p>
        )}
      </div>
      <span className="shrink-0 rounded-sm border border-line px-1.5 py-[1px] text-[9px] uppercase tracking-wider text-ink-subtle">
        {cmd.category}
      </span>
      {cmd.shortcut && (
        <kbd className="shrink-0 rounded-sm bg-bg-2 px-1.5 py-[1px] font-mono text-[10px] text-ink-subtle">
          {cmd.shortcut}
        </kbd>
      )}
    </li>
  );
}

function Highlight({ text, matches }: { text: string; matches: number[] }) {
  if (matches.length === 0) return <>{text}</>;
  const matched = new Set(matches);
  return (
    <>
      {Array.from(text).map((c, i) => (
        <span
          key={i}
          className={matched.has(i) ? "text-amber font-semibold" : ""}
        >
          {c}
        </span>
      ))}
    </>
  );
}

function Hint({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <span className="flex items-center gap-1">
      <kbd className="rounded bg-bg-2 px-1 py-[1px] font-mono text-[9px] text-ink">
        {k}
      </kbd>
      <span>{children}</span>
    </span>
  );
}

