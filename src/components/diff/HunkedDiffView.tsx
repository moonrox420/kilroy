/**
 * Unified-diff renderer with per-hunk checkboxes (state lives in the parent).
 *
 * The diff is split into a header (`---`/`+++` lines) and N hunks
 * (everything starting at `@@`). The parent owns the boolean[]
 * selection — that's how the ActionCard's Accept handler can read the
 * current state without coordinating refs.
 *
 * Helpers (`parseUnifiedDiff`, `buildOverrideDiff`) are exported so the
 * ActionCard can compute the final diff to send.
 */
import { Check, Square } from "lucide-react";
import { cn } from "@/lib/utils";

export interface ParsedDiff {
  header: string;
  hunks: string[];
}

interface Props {
  parsed: ParsedDiff;
  selected: boolean[];
  onToggle: (idx: number) => void;
  maxHeight?: number;
  className?: string;
}

export function HunkedDiffView({
  parsed,
  selected,
  onToggle,
  maxHeight = 360,
  className,
}: Props) {
  if (parsed.hunks.length === 0) {
    return (
      <pre
        className={cn(
          "overflow-auto rounded-md border border-line bg-bg-0 px-2 py-1 font-mono text-[11px] leading-snug text-ink-muted",
          className,
        )}
        style={{ maxHeight }}
      >
        {parsed.header || "(empty diff)"}
      </pre>
    );
  }

  return (
    <div
      className={cn("overflow-hidden rounded-md border border-line", className)}
      style={{ maxHeight }}
    >
      <div className="max-h-full overflow-auto">
        {parsed.header && (
          <pre className="bg-bg-0 px-2 py-1 font-mono text-[10.5px] text-ink-subtle">
            {parsed.header}
          </pre>
        )}
        {parsed.hunks.map((hunk, i) => {
          const isSelected = selected[i];
          const stats = hunkStats(hunk);
          return (
            <section
              key={i}
              className={cn(
                "border-t border-line bg-bg-0 font-mono text-[11px]",
                !isSelected && "opacity-50",
              )}
            >
              <header className="sticky top-0 flex items-center gap-2 border-b border-line/60 bg-bg-1 px-2 py-1">
                <button
                  onClick={() => onToggle(i)}
                  className={cn(
                    "flex h-4 w-4 items-center justify-center rounded-sm border transition-colors",
                    isSelected
                      ? "border-amber bg-amber text-amber-ink"
                      : "border-line bg-bg-2 text-transparent hover:border-amber",
                  )}
                  aria-label={isSelected ? "Deselect hunk" : "Select hunk"}
                  title={
                    isSelected
                      ? "Click to exclude this hunk"
                      : "Click to include this hunk"
                  }
                >
                  {isSelected ? (
                    <Check className="h-3 w-3" />
                  ) : (
                    <Square className="h-3 w-3" />
                  )}
                </button>
                <span className="text-[10px] uppercase tracking-wider text-ink-subtle">
                  hunk {i + 1}
                </span>
                <span className="ml-auto flex items-center gap-2 text-[10px] text-ink-subtle">
                  <span className="text-ok">+{stats.added}</span>
                  <span className="text-err">−{stats.removed}</span>
                </span>
              </header>
              <pre>
                {hunk.split("\n").map((line, lineIdx) => (
                  <DiffLine key={lineIdx} text={line} />
                ))}
              </pre>
            </section>
          );
        })}
      </div>
    </div>
  );
}

function DiffLine({ text }: { text: string }) {
  let cls = "block px-2";
  if (text.startsWith("@@")) {
    cls += " bg-bg-2 text-amber py-[1px]";
  } else if (text.startsWith("+")) {
    cls += " bg-ok/10 text-ok";
  } else if (text.startsWith("-")) {
    cls += " bg-err/10 text-err";
  } else {
    cls += " text-ink-muted";
  }
  return <span className={cls}>{text || " "}</span>;
}

export function parseUnifiedDiff(diff: string): ParsedDiff {
  const lines = diff.split("\n");
  const header: string[] = [];
  const hunks: string[][] = [];
  let inHunk = false;

  for (const line of lines) {
    if (line.startsWith("@@")) {
      inHunk = true;
      hunks.push([line]);
    } else if (inHunk) {
      hunks[hunks.length - 1].push(line);
    } else {
      header.push(line);
    }
  }
  return {
    header: header.join("\n").replace(/\n+$/, ""),
    hunks: hunks.map((h) => h.join("\n").replace(/\n+$/, "")),
  };
}

/**
 * Reassemble a unified diff with only the selected hunks.
 * Returns `null` when no hunks are selected (caller should disable Accept).
 */
export function buildOverrideDiff(parsed: ParsedDiff, selected: boolean[]): string | null {
  const keep = parsed.hunks.filter((_, i) => selected[i]);
  if (keep.length === 0) return null;
  const sections: string[] = [];
  if (parsed.header) sections.push(parsed.header);
  for (const h of keep) sections.push(h);
  return sections.join("\n") + "\n";
}

function hunkStats(hunk: string): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const line of hunk.split("\n")) {
    if (line.startsWith("+") && !line.startsWith("+++")) added++;
    else if (line.startsWith("-") && !line.startsWith("---")) removed++;
  }
  return { added, removed };
}
