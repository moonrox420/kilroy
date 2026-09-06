/**
 * Decision composer — modal form to log a new architectural decision.
 *
 * Decisions become part of the agent's retrieval pool: title + summary +
 * rationale are embedded together and queried via KNN whenever the user
 * sends a chat message. Use it to record "we chose SQLite because…",
 * "PTY is portable-pty, not winpty because…", etc.
 */
import { useState } from "react";
import { Lightbulb, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { memory } from "@/lib/tauri";
import { useMemoryPanel } from "@/store/memoryPanel";
import { useMemory } from "@/store/memory";

export function DecisionComposer() {
  const open = useMemoryPanel((s) => s.decisionComposerOpen);
  const close = useMemoryPanel((s) => s.closeDecisionComposer);
  const project = useMemory((s) => s.project);

  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");
  const [rationale, setRationale] = useState("");
  const [relatedRaw, setRelatedRaw] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reset = () => {
    setTitle("");
    setSummary("");
    setRationale("");
    setRelatedRaw("");
    setError(null);
  };

  const submit = async () => {
    if (!title.trim() || !summary.trim()) {
      setError("Title and summary are required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const related_files = relatedRaw
        .split(/[\n,]/)
        .map((s) => s.trim())
        .filter(Boolean);
      await memory.logDecision({
        title: title.trim(),
        summary: summary.trim(),
        rationale: rationale.trim() || undefined,
        related_files: related_files.length ? related_files : undefined,
      });
      reset();
      close();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v && !busy) {
          reset();
          close();
        }
      }}
    >
      <DialogContent className="w-[min(640px,calc(100vw-3rem))]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Lightbulb className="h-3.5 w-3.5 text-amber" />
            Log a Decision
          </DialogTitle>
          <DialogDescription>
            Embedded and surfaced to the agent on every chat turn.
          </DialogDescription>
        </DialogHeader>
        {!project ? (
          <div className="p-6 text-center text-[12px] text-ink-subtle">
            Open a project folder first.
          </div>
        ) : (
          <div className="flex flex-col gap-3 p-4">
            <Field
              label="Title"
              hint="One line. e.g. 'Use Tauri instead of Electron'."
            >
              <Input
                autoFocus
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="One-line title"
              />
            </Field>
            <Field
              label="Summary"
              hint="What we decided, in 1–2 sentences."
            >
              <Textarea
                value={summary}
                onChange={(e) => setSummary(e.target.value)}
                rows={3}
                placeholder="What we chose, briefly."
              />
            </Field>
            <Field
              label="Rationale"
              hint="Why. Constraints, alternatives considered, trade-offs."
            >
              <Textarea
                value={rationale}
                onChange={(e) => setRationale(e.target.value)}
                rows={5}
                placeholder="Optional but valuable. The agent will recall this."
              />
            </Field>
            <Field
              label="Related files"
              hint="Comma- or newline-separated paths. Optional."
            >
              <Textarea
                value={relatedRaw}
                onChange={(e) => setRelatedRaw(e.target.value)}
                rows={2}
                placeholder="src-tauri/src/lib.rs, README.md"
              />
            </Field>
            {error && (
              <p className="rounded-md border border-err/40 bg-err/5 px-2 py-1 text-[11px] text-err">
                {error}
              </p>
            )}
          </div>
        )}
        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => {
              if (busy) return;
              reset();
              close();
            }}
            disabled={busy}
          >
            Cancel
          </Button>
          <Button onClick={submit} disabled={busy || !project}>
            {busy ? (
              <>
                <Loader2 className="h-3 w-3 animate-spin" />
                Saving…
              </>
            ) : (
              "Save Decision"
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-[10.5px] text-ink-subtle">{hint}</p>}
    </div>
  );
}
