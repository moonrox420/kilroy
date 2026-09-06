/**
 * Unified-diff renderer.
 *
 * Takes a `diff --unified` string (the format `similar` produces) and
 * renders it with +/- line prefixes coloured. No diff parsing — we just
 * split by line and look at the first character. Hunk headers are
 * subdued.
 */
import { cn } from "@/lib/utils";

interface Props {
  diff: string;
  /** Max height before internal scroll. */
  maxHeight?: number;
  className?: string;
}

export function DiffView({ diff, maxHeight = 320, className }: Props) {
  const lines = diff.split("\n");
  return (
    <pre
      className={cn(
        "overflow-auto rounded-md border border-line bg-bg-0 font-mono text-[11px] leading-snug",
        className,
      )}
      style={{ maxHeight }}
    >
      <code className="block">
        {lines.map((line, i) => (
          <Line key={i} text={line} />
        ))}
      </code>
    </pre>
  );
}

function Line({ text }: { text: string }) {
  let cls = "block px-2 py-[1px]";
  if (text.startsWith("+++") || text.startsWith("---")) {
    cls += " text-ink-subtle";
  } else if (text.startsWith("@@")) {
    cls += " bg-bg-2 text-amber";
  } else if (text.startsWith("+")) {
    cls += " bg-ok/10 text-ok";
  } else if (text.startsWith("-")) {
    cls += " bg-err/10 text-err";
  } else {
    cls += " text-ink-muted";
  }
  return <span className={cls}>{text || " "}</span>;
}
