/**
 * Inline code block rendered inside an agent chat message.
 *
 *   ┌─ rust src/foo.rs ─────────────────────[ Copy ] [ Apply ]─┐
 *   │ fn foo() {                                               │
 *   │     // long line that overflows but scrolls horizontally │
 *   │ }                                                        │
 *   └──────────────────────────────────────────────────────────┘
 *
 *  • horizontal scroll, never word-wraps (preserves indentation)
 *  • monospace, faithful whitespace
 *  • Copy button → clipboard
 *  • Apply button — visible only when the fence info string supplies a path
 *    — writes the block to that path and hot-reloads the open editor tab if
 *    one matches. The agent gets to "act like an agent" in Copilot mode too.
 */
import { useState } from "react";
import { Check, ClipboardCopy, FileCode2, FilePlus2, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useWorkspace } from "@/store/workspace";

interface Props {
  language?: string;
  path?: string;
  code: string;
}

export function ChatCodeBlock({ language, path, code }: Props) {
  const rootPath = useWorkspace((s) => s.rootPath);
  const openTabs = useWorkspace((s) => s.openTabs);
  const applyBlock = useWorkspace((s) => s.applyCodeBlock);
  const newUntitledWith = useWorkspace((s) => s.newUntitledWith);

  const [copied, setCopied] = useState(false);
  const [applying, setApplying] = useState(false);
  const [applied, setApplied] = useState<"ok" | "err" | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const absolutePath = path ? resolveAgainst(rootPath, path) : null;
  const tabIsOpen = !!(absolutePath && openTabs.find((t) => t.path === absolutePath));

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch (err) {
      console.warn("clipboard.writeText:", err);
    }
  };

  const onApply = async () => {
    if (!absolutePath) return;
    setApplying(true);
    setErrorMsg(null);
    try {
      await applyBlock(absolutePath, code);
      setApplied("ok");
      setTimeout(() => setApplied(null), 1800);
    } catch (err) {
      setApplied("err");
      setErrorMsg(String(err));
    } finally {
      setApplying(false);
    }
  };

  const onOpenAsUntitled = () => {
    newUntitledWith({ contents: code, language });
  };

  return (
    <div className="my-2 overflow-hidden rounded-md border border-line bg-bg-0">
      <header className="flex items-center gap-2 border-b border-line bg-bg-1 px-2 py-1 text-[10px]">
        {language && (
          <span className="rounded-sm bg-bg-2 px-1.5 py-[1px] font-mono uppercase tracking-wider text-ink-subtle">
            {language}
          </span>
        )}
        {path && (
          <span className="flex min-w-0 items-center gap-1 truncate font-mono text-ink">
            <FileCode2 className="h-3 w-3 shrink-0 text-amber" />
            <span className="truncate" title={path}>{path}</span>
            {tabIsOpen && (
              <span className="shrink-0 rounded-sm bg-ok/15 px-1 py-[1px] text-[9px] uppercase text-ok">
                open
              </span>
            )}
          </span>
        )}
        <span className="flex-1" />

        <button
          onClick={onCopy}
          title="Copy code to clipboard"
          className={cn(
            "flex items-center gap-1 rounded-sm px-1.5 py-[2px] text-[10px] transition-colors",
            "text-ink-muted hover:bg-bg-2 hover:text-ink",
            copied && "text-ok hover:text-ok",
          )}
        >
          {copied ? <Check className="h-3 w-3" /> : <ClipboardCopy className="h-3 w-3" />}
          {copied ? "Copied" : "Copy"}
        </button>

        {path ? (
          <button
            onClick={onApply}
            disabled={applying}
            title={`Write this block to ${path}` + (tabIsOpen ? " (open tab will refresh)" : "")}
            className={cn(
              "flex items-center gap-1 rounded-sm px-1.5 py-[2px] text-[10px] transition-colors",
              applied === "ok"
                ? "bg-ok/15 text-ok"
                : applied === "err"
                  ? "bg-err/15 text-err"
                  : "text-amber hover:bg-amber/15",
              applying && "opacity-60",
            )}
          >
            {applying ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : applied === "ok" ? (
              <Check className="h-3 w-3" />
            ) : (
              <FileCode2 className="h-3 w-3" />
            )}
            {applying ? "Applying…" : applied === "ok" ? "Applied" : applied === "err" ? "Failed" : "Apply"}
          </button>
        ) : (
          <button
            onClick={onOpenAsUntitled}
            title="Open this block as a new editor tab"
            className="flex items-center gap-1 rounded-sm px-1.5 py-[2px] text-[10px] text-ink-muted transition-colors hover:bg-bg-2 hover:text-ink"
          >
            <FilePlus2 className="h-3 w-3" />
            Open
          </button>
        )}
      </header>

      <pre
        className="overflow-x-auto p-3 font-mono text-[11.5px] leading-relaxed text-ink"
        style={{
          // The CRITICAL bits — preserve whitespace + indentation but never wrap.
          whiteSpace: "pre",
          wordBreak: "normal",
          overflowWrap: "normal",
          tabSize: 2,
        }}
      >
        {code}
      </pre>

      {errorMsg && (
        <div className="border-t border-err/40 bg-err/5 px-2 py-1 text-[10.5px] text-err">
          {errorMsg}
        </div>
      )}
    </div>
  );
}

function resolveAgainst(root: string | null, path: string): string {
  if (/^[a-zA-Z]:[\\/]/.test(path) || path.startsWith("/")) return path; // absolute
  if (!root) return path;
  const sep = root.includes("\\") ? "\\" : "/";
  return (root.endsWith(sep) ? root : root + sep) + path.replace(/[\\/]/g, sep);
}
