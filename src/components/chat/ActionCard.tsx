/**
 * Action card — pending actuator action awaiting user accept/reject.
 *
 * Renders inline in the chat under a task card. Behaviour by kind:
 *   * `file_write` — full-file replace. Shows the unified diff vs disk
 *     (via similar) as a single Accept/Reject choice.
 *   * `file_patch` — agent emitted a unified diff. We split it into
 *     hunks and let the user cherry-pick which ones land.
 *   * `shell`      — show the command + sandbox; Accept runs it.
 *
 * The Accept button always calls `actions.accept` with an optional
 * `override_diff` for file_patch.
 */
import { useMemo, useState } from "react";
import {
  Check,
  ChevronRight,
  Code2,
  FileCode2,
  Loader2,
  Terminal as TermIcon,
  X,
} from "lucide-react";
import type { ActionView } from "@/lib/tauri";
import { actions as actionsApi } from "@/lib/tauri";
import { useActions } from "@/store/actions";
import { Button } from "@/components/ui/button";
import { DiffView } from "@/components/diff/DiffView";
import {
  HunkedDiffView,
  parseUnifiedDiff,
  buildOverrideDiff,
} from "@/components/diff/HunkedDiffView";
import { SandboxBadge } from "./SandboxBadge";
import { notify } from "@/store/notifications";
import { cn } from "@/lib/utils";

export function ActionCard({ action }: { action: ActionView }) {
  const [expanded, setExpanded] = useState(action.status === "pending");
  const [busy, setBusy] = useState(false);
  const applyResolution = useActions((s) => s.applyResolution);
  const reject = useActions((s) => s.reject);

  // Per-hunk selection state for file_patch actions.
  const parsedDiff = useMemo(
    () => (action.kind === "file_patch" && action.diff ? parseUnifiedDiff(action.diff) : null),
    [action.kind, action.diff],
  );
  const [selectedHunks, setSelectedHunks] = useState<boolean[]>(() =>
    parsedDiff ? parsedDiff.hunks.map(() => true) : [],
  );
  const overrideDiff = useMemo(() => {
    if (!parsedDiff) return null;
    return buildOverrideDiff(parsedDiff, selectedHunks);
  }, [parsedDiff, selectedHunks]);

  const Icon =
    action.kind === "shell"
      ? TermIcon
      : action.kind === "file_patch"
        ? Code2
        : FileCode2;

  const onAccept = async () => {
    if (busy) return;
    setBusy(true);
    try {
      // Call the API directly so we can pass override_diff. The store's
      // accept() helper only takes an action_id.
      const r = await actionsApi.accept({
        action_id: action.id,
        override_diff: action.kind === "file_patch" ? overrideDiff : null,
      });
      applyResolution(r.action_id, r.status, r.error);
      if (r.status === "applied") {
        notify.success(
          `${action.kind.replace("_", " ")} applied`,
          action.target ?? undefined,
        );
      } else if (r.status === "failed") {
        notify.error(
          `${action.kind.replace("_", " ")} failed`,
          r.error ?? undefined,
        );
      }
    } catch (err) {
      notify.fromError(`accept_action[${action.id}]`, err);
      applyResolution(action.id, "failed", String(err));
    } finally {
      setBusy(false);
    }
  };

  const onReject = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await reject(action.id);
    } finally {
      setBusy(false);
    }
  };

  const acceptDisabled =
    busy || (action.kind === "file_patch" && !overrideDiff);

  return (
    <div
      className={cn(
        "rounded-md border text-[12px]",
        action.status === "applied"
          ? "border-ok/40 bg-ok/5"
          : action.status === "rejected"
            ? "border-line bg-bg-1 opacity-70"
            : action.status === "failed"
              ? "border-err/40 bg-err/5"
              : "border-amber/40 bg-amber/5",
      )}
    >
      <header className="flex items-center gap-2 px-2.5 py-1.5">
        <Icon
          className={cn(
            "h-3.5 w-3.5 shrink-0",
            action.status === "pending" ? "text-amber" : "text-ink-subtle",
          )}
        />
        <span className="text-[10px] uppercase tracking-wider text-ink-subtle">
          {action.kind.replace("_", " ")}
        </span>
        <span className="flex-1 truncate text-ink">
          {action.target ?? "(no target)"}
        </span>
        {action.kind === "shell" && action.payload?.sandbox && (
          <SandboxBadge sandbox={action.payload.sandbox} />
        )}
        <StatusBadge status={action.status} />
        <button
          onClick={() => setExpanded((v) => !v)}
          className="rounded-sm p-0.5 hover:bg-bg-2"
          aria-label="Expand"
        >
          <ChevronRight
            className={cn(
              "h-3 w-3 text-ink-subtle transition-transform",
              expanded && "rotate-90",
            )}
          />
        </button>
      </header>

      {expanded && (
        <div className="border-t border-line/60 p-2">
          {action.error && (
            <p className="mb-2 rounded-sm border border-err/40 bg-err/10 px-2 py-1 text-[11px] text-err">
              {action.error}
            </p>
          )}
          <ActionBody
            action={action}
            parsedDiff={parsedDiff}
            selectedHunks={selectedHunks}
            onToggleHunk={(i) =>
              setSelectedHunks((s) => s.map((v, idx) => (idx === i ? !v : v)))
            }
          />
        </div>
      )}

      {action.status === "pending" && (
        <footer className="flex items-center justify-end gap-2 border-t border-line/60 px-2 py-1.5">
          {action.kind === "file_patch" && parsedDiff && (
            <span className="mr-auto text-[10px] text-ink-subtle">
              {selectedHunks.filter(Boolean).length}/{parsedDiff.hunks.length} hunks selected
            </span>
          )}
          <Button variant="ghost" size="sm" onClick={onReject} disabled={busy}>
            <X className="h-3 w-3" />
            Reject
          </Button>
          <Button size="sm" onClick={onAccept} disabled={acceptDisabled}>
            {busy ? <Loader2 className="h-3 w-3 animate-spin" /> : <Check className="h-3 w-3" />}
            {busy ? "Applying…" : action.kind === "shell" ? "Run" : "Accept"}
          </Button>
        </footer>
      )}
    </div>
  );
}

function ActionBody({
  action,
  parsedDiff,
  selectedHunks,
  onToggleHunk,
}: {
  action: ActionView;
  parsedDiff: ReturnType<typeof parseUnifiedDiff> | null;
  selectedHunks: boolean[];
  onToggleHunk: (i: number) => void;
}) {
  if (action.kind === "shell") {
    const cmd: string = action.payload?.command ?? "(empty)";
    const sandbox: string = action.payload?.sandbox ?? "windows_sandbox";
    const sandboxLabel = sandbox === "windows_sandbox" ? "Windows Sandbox" : sandbox;
    return (
      <div>
        <p className="mb-1 text-[10px] uppercase tracking-wider text-ink-subtle">
          Sandbox: <span className="text-amber">{sandboxLabel}</span>
        </p>
        <pre className="overflow-x-auto rounded-sm border border-line bg-bg-0 px-2 py-1 font-mono text-[11px] text-ink whitespace-pre-wrap">
          {cmd}
        </pre>
      </div>
    );
  }
  if (action.kind === "file_patch" && parsedDiff) {
    return (
      <HunkedDiffView
        parsed={parsedDiff}
        selected={selectedHunks}
        onToggle={onToggleHunk}
      />
    );
  }
  // file_write
  if (action.diff) {
    return <DiffView diff={action.diff} />;
  }
  const content: string = action.payload?.content ?? "";
  return (
    <div>
      <p className="mb-1 text-[10px] uppercase tracking-wider text-ink-subtle">
        New file
      </p>
      <pre className="max-h-[260px] overflow-auto whitespace-pre-wrap rounded-sm border border-line bg-bg-0 px-2 py-1 font-mono text-[11px] text-ink">
        {content}
      </pre>
    </div>
  );
}

function StatusBadge({ status }: { status: ActionView["status"] }) {
  const map: Record<ActionView["status"], { label: string; cls: string }> = {
    pending: { label: "pending", cls: "bg-amber/15 text-amber" },
    accepted: { label: "applying", cls: "bg-amber/15 text-amber" },
    applied: { label: "applied", cls: "bg-ok/15 text-ok" },
    rejected: { label: "rejected", cls: "bg-line text-ink-subtle" },
    failed: { label: "failed", cls: "bg-err/15 text-err" },
  };
  const m = map[status];
  return (
    <span
      className={cn(
        "rounded-sm px-1.5 py-[1px] text-[9px] uppercase tracking-wider",
        m.cls,
      )}
    >
      {m.label}
    </span>
  );
}
