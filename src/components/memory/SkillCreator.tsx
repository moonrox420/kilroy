/**
 * Skill composer — "+ New Skill" dialog.
 *
 * The skill system was originally drop-a-file-in-a-folder ergonomics:
 * users had to know about <app config>/skills/ and .kilroy/skills/, hand-
 * author Markdown, and re-launch Kilroy to pick up the change. This
 * composer turns it into a 30-second flow: pick scope, type a slug, write
 * the body, save — the agent sees the new skill on the next chat turn.
 *
 * Output shape (composed for the user so `parse_metadata` in skills.rs
 * extracts the right title + summary):
 *
 *   # {title}
 *   {summary line}
 *
 *   {body}
 *
 * On save we call `skills.write({ name, scope, content })` which returns
 * the absolute on-disk path; we surface that in a success toast so the
 * user can find / edit the file later if they want.
 */
import { useState, useEffect, useMemo } from "react";
import { Sparkles, Loader2, Globe2, FolderKanban } from "lucide-react";
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
import { skills } from "@/lib/tauri";
import { useMemoryPanel } from "@/store/memoryPanel";
import { useMemory } from "@/store/memory";
import { notify } from "@/store/notifications";
import { cn } from "@/lib/utils";

type Scope = "global" | "project";

/** Slug-validation regex matching the Rust backend exactly. */
const SLUG_RE = /^[A-Za-z0-9_][A-Za-z0-9_-]{0,79}$/;

export function SkillCreator() {
  const open = useMemoryPanel((s) => s.skillCreatorOpen);
  const close = useMemoryPanel((s) => s.closeSkillCreator);
  const project = useMemory((s) => s.project);

  // Default scope follows the obvious affordance: if a project is open the
  // user almost always wants project-scoped skills (per-codebase
  // conventions, retrieval patterns, deploy quirks). If no project is open
  // we fall back to global since project scope would be impossible anyway.
  const [scope, setScope] = useState<Scope>(project ? "project" : "global");
  const [slug, setSlug] = useState("");
  const [title, setTitle] = useState("");
  const [summary, setSummary] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Re-default scope when the dialog reopens or project changes — the
  // user's last choice doesn't carry across opens, which avoids the trap
  // of authoring a project-conventions skill into the global folder by
  // accident after closing/reopening Kilroy on a new project.
  useEffect(() => {
    if (open) {
      setScope(project ? "project" : "global");
    }
  }, [open, project]);

  const slugValid = useMemo(() => SLUG_RE.test(slug), [slug]);
  const canSubmit =
    slugValid &&
    title.trim().length > 0 &&
    (scope === "global" || !!project);

  const reset = () => {
    setSlug("");
    setTitle("");
    setSummary("");
    setBody("");
    setError(null);
  };

  // Auto-derive a slug from the title on first type, so the user doesn't
  // have to think about both. Once they touch the slug field manually we
  // stop overriding it.
  const [slugTouched, setSlugTouched] = useState(false);
  const onTitleChange = (next: string) => {
    setTitle(next);
    if (!slugTouched) {
      const derived = next
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 60);
      setSlug(derived);
    }
  };

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      // Compose the markdown. We always emit a leading `# {title}` line
      // because parse_metadata in skills.rs uses that as the display
      // title; without it the fallback would be the slug itself which
      // looks ugly in the skills list ("react-hooks" instead of "React
      // hooks").
      const parts = [`# ${title.trim()}`];
      if (summary.trim()) parts.push(summary.trim());
      if (body.trim()) parts.push("", body.trimEnd());
      const content = parts.join("\n") + "\n";

      const path = await skills.write({
        name: slug,
        scope,
        content,
      });
      notify.success(
        "Skill saved",
        `${title.trim()} → ${path}`,
      );
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
      <DialogContent className="w-[min(680px,calc(100vw-3rem))]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="h-3.5 w-3.5 text-amber" />
            New Skill
          </DialogTitle>
          <DialogDescription>
            Markdown notes the agent carries into every chat turn. Use them
            for conventions, naming, library preferences — anything you'd
            otherwise repeat at the top of each session.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3 p-4">
          <Field
            label="Scope"
            hint={
              scope === "project"
                ? `Applies only when this project is open (${project?.name ?? "—"}).`
                : "Applies everywhere — picked up by every Kilroy session."
            }
          >
            <div className="flex gap-2">
              <ScopeButton
                active={scope === "global"}
                onClick={() => setScope("global")}
                icon={<Globe2 className="h-3 w-3" />}
                label="Global"
              />
              <ScopeButton
                active={scope === "project"}
                onClick={() => project && setScope("project")}
                disabled={!project}
                icon={<FolderKanban className="h-3 w-3" />}
                label={project ? "Project" : "Project (open a folder first)"}
              />
            </div>
          </Field>
          <Field
            label="Title"
            hint="Shown in the skills list. The first `# Heading` line of the file."
          >
            <Input
              autoFocus
              value={title}
              onChange={(e) => onTitleChange(e.target.value)}
              placeholder="e.g. React hooks conventions"
            />
          </Field>
          <Field
            label="Slug"
            hint={
              slugValid || !slug
                ? "Filename on disk: `<slug>.md`. Letters, digits, '-' and '_' only."
                : "Invalid — use letters, digits, '-' or '_' only (no dots, no slashes)."
            }
          >
            <Input
              value={slug}
              onChange={(e) => {
                setSlug(e.target.value);
                setSlugTouched(true);
              }}
              placeholder="react-hooks"
              className={cn(
                "font-mono text-[12px]",
                !slugValid && slug ? "border-err/60" : "",
              )}
            />
          </Field>
          <Field
            label="Summary"
            hint="One short line. Shown to the model upfront alongside the title. Optional."
          >
            <Input
              value={summary}
              onChange={(e) => setSummary(e.target.value)}
              placeholder="Prefer custom hooks for any logic touching two or more components."
            />
          </Field>
          <Field
            label="Body"
            hint="Free-form markdown. Examples, anti-patterns, links to internal docs."
          >
            <Textarea
              value={body}
              onChange={(e) => setBody(e.target.value)}
              rows={8}
              className="font-mono text-[12px]"
              placeholder={
                "## When to use\n" +
                "- multi-component shared state\n" +
                "- effect choreography\n\n" +
                "## Anti-patterns\n" +
                "- ad-hoc useEffect chains for derived values"
              }
            />
          </Field>
          {error && (
            <p className="rounded-md border border-err/40 bg-err/5 px-2 py-1 text-[11px] text-err">
              {error}
            </p>
          )}
        </div>
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
          <Button onClick={submit} disabled={busy || !canSubmit}>
            {busy ? (
              <>
                <Loader2 className="h-3 w-3 animate-spin" />
                Saving…
              </>
            ) : (
              "Save Skill"
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

function ScopeButton({
  active,
  onClick,
  icon,
  label,
  disabled,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "flex flex-1 items-center gap-2 rounded-md border px-3 py-2 text-[12px] transition-colors",
        active
          ? "border-amber/60 bg-amber/10 text-ink"
          : "border-line bg-bg-1 text-ink-subtle hover:text-ink hover:bg-bg-2",
        disabled && "cursor-not-allowed opacity-50 hover:bg-bg-1 hover:text-ink-subtle",
      )}
    >
      {icon}
      {label}
    </button>
  );
}
