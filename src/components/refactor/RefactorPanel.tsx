/**
 * Refactor panel — inbox + scan launcher for the background-refactor swarm.
 *
 * Two main surfaces stacked in the dialog:
 *
 *   1. Candidates list (top) — files ranked by heuristic, each with a
 *      one-click "Scan this file" button that triggers the 4-voice
 *      swarm against that file.
 *
 *   2. Proposals inbox (bottom) — GitHub-style cards for each pending
 *      refactor suggestion. Title + rationale + risk + diff preview +
 *      Apply / Dismiss buttons.
 *
 * While a scan is in flight, a live 4-card swarm view appears between
 * them so the user can watch the voices debate the file. When the
 * scan finishes, the live view stays for a beat and then the proposal
 * (if any) lands in the inbox below.
 */
import { useEffect } from "react";
import {
  GitBranch,
  Loader2,
  Sparkles,
  Trash2,
  Check,
  Play,
  FileCode2,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useRefactor } from "@/store/refactor";
import { useMemory } from "@/store/memory";
import { notify } from "@/store/notifications";
import type {
  RefactorCandidate,
  RefactorProposal,
  RefactorRisk,
  RefactorVoice,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

export function RefactorPanel() {
  const open = useRefactor((s) => s.open);
  const close = useRefactor((s) => s.closePanel);
  const candidates = useRefactor((s) => s.candidates);
  const proposals = useRefactor((s) => s.proposals);
  const stats = useRefactor((s) => s.stats);
  const scanning = useRefactor((s) => s.scanning);
  const live = useRefactor((s) => s.live);
  const startScan = useRefactor((s) => s.startScan);
  const applyProposal = useRefactor((s) => s.applyProposal);
  const dismissProposal = useRefactor((s) => s.dismissProposal);
  const project = useMemory((s) => s.project);

  useEffect(() => {
    if (open) {
      void useRefactor.getState().refreshAll();
    }
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={(v) => !v && close()}>
      <DialogContent className="w-[min(880px,calc(100vw-3rem))] max-h-[88vh] overflow-hidden">
        <DialogHeader>
          <div className="flex items-center justify-between gap-3">
            <DialogTitle className="flex items-center gap-2">
              <GitBranch className="h-3.5 w-3.5 text-amber" />
              Background Refactor
            </DialogTitle>
            <RefactorStatsRow stats={stats} />
          </div>
          <DialogDescription>
            {project
              ? `Scanning ${project.name}. Pick a candidate to run the 4-voice refactor swarm; review proposals below.`
              : "Open a project folder to scan for refactor opportunities."}
          </DialogDescription>
        </DialogHeader>

        <div className="flex max-h-[72vh] flex-col gap-4 overflow-y-auto p-4">
          {project ? (
            <>
              <Section
                title="Candidates"
                subtitle="Files ranked by heuristic priority — size, function density, TODO markers, unwrap density. Cheap to compute; no LLM here yet."
              >
                <CandidatesList
                  candidates={candidates}
                  scanning={scanning}
                  onScan={(c) => void startScan(c.path)}
                />
              </Section>

              {(scanning || live.synthesis || Object.values(live.voicesDone).some(Boolean)) && (
                <Section
                  title={
                    scanning
                      ? `🧠 Swarm scanning ${shortPath(scanning)}`
                      : "🧠 Last scan"
                  }
                  subtitle="Four voices analysing the file in parallel. The synthesiser picks the highest-impact / lowest-risk proposal once they all finish."
                >
                  <RefactorSwarmLive />
                </Section>
              )}

              <Section
                title={`Proposals (${proposals.length})`}
                subtitle="Pending refactor suggestions. Apply routes the diff through the actuator — same per-hunk Accept/Reject as chat-proposed edits."
              >
                <ProposalsInbox
                  proposals={proposals}
                  onApply={async (p) => {
                    const actionId = await applyProposal(p.id);
                    if (actionId !== null) {
                      notify.success(
                        "Proposal applied",
                        `Diff queued as action #${actionId}. Review it in the Actions panel.`,
                      );
                    }
                  }}
                  onDismiss={(p) => void dismissProposal(p.id)}
                />
              </Section>
            </>
          ) : (
            <p className="py-12 text-center text-[12px] text-ink-subtle">
              No project open. Use File → Open Folder… first.
            </p>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Section({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-2 rounded-md border border-line bg-bg-1 p-3">
      <div>
        <div className="text-[11px] uppercase tracking-wider text-ink-subtle">
          {title}
        </div>
        {subtitle && <div className="mt-0.5 text-[10.5px] text-ink-subtle/80">{subtitle}</div>}
      </div>
      {children}
    </div>
  );
}

function RefactorStatsRow({
  stats,
}: {
  stats: ReturnType<typeof useRefactor.getState>["stats"];
}) {
  if (!stats) return null;
  return (
    <div className="flex items-center gap-2 text-[10.5px] text-ink-subtle">
      <Chip color="amber">{stats.pending} pending</Chip>
      <Chip color="ok">{stats.applied} applied</Chip>
      <Chip color="subtle">{stats.dismissed} dismissed</Chip>
    </div>
  );
}

function Chip({
  color,
  children,
}: {
  color: "amber" | "ok" | "err" | "subtle";
  children: React.ReactNode;
}) {
  const cls =
    color === "amber"
      ? "border-amber/40 bg-amber/5 text-amber"
      : color === "ok"
        ? "border-ok/40 bg-ok/5 text-ok"
        : color === "err"
          ? "border-err/40 bg-err/5 text-err"
          : "border-line bg-bg-1 text-ink-subtle";
  return (
    <span className={cn("rounded-md border px-1.5 py-0.5", cls)}>{children}</span>
  );
}

function CandidatesList({
  candidates,
  scanning,
  onScan,
}: {
  candidates: RefactorCandidate[];
  scanning: string | null;
  onScan: (c: RefactorCandidate) => void;
}) {
  if (candidates.length === 0) {
    return (
      <p className="py-3 text-center text-[11px] text-ink-subtle">
        No candidates found (project may be empty or all files too small).
      </p>
    );
  }
  return (
    <div className="flex max-h-72 flex-col gap-1 overflow-y-auto pr-1">
      {candidates.map((c) => {
        const isThis = scanning === c.path;
        return (
          <div
            key={c.path}
            className={cn(
              "flex items-center justify-between gap-2 rounded-md border px-2 py-1.5",
              isThis
                ? "border-amber/60 bg-amber/5"
                : "border-line bg-bg-0 hover:bg-bg-2",
            )}
          >
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5 text-[11.5px] text-ink">
                <FileCode2 className="h-3 w-3 shrink-0 text-ink-subtle" />
                <span className="truncate font-mono">{c.rel_path}</span>
              </div>
              <div className="mt-0.5 text-[10px] text-ink-subtle">{c.reason}</div>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <span
                className={cn(
                  "rounded-md border px-1.5 py-0.5 text-[10px]",
                  c.score >= 70
                    ? "border-amber/40 bg-amber/5 text-amber"
                    : c.score >= 40
                      ? "border-line bg-bg-1 text-ink-muted"
                      : "border-line bg-bg-1 text-ink-subtle",
                )}
              >
                {c.score}
              </span>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onScan(c)}
                disabled={!!scanning}
              >
                {isThis ? (
                  <>
                    <Loader2 className="h-3 w-3 animate-spin" />
                    Scanning…
                  </>
                ) : (
                  <>
                    <Play className="h-3 w-3" />
                    Scan
                  </>
                )}
              </Button>
            </div>
          </div>
        );
      })}
    </div>
  );
}

const VOICE_META: Record<
  RefactorVoice,
  { label: string; emoji: string; accent: string }
> = {
  duplicate: { label: "Duplicate hunter", emoji: "🔁", accent: "bg-amber" },
  complexity: { label: "Complexity hunter", emoji: "📏", accent: "bg-info" },
  error_handling: { label: "Error-handling", emoji: "🛡️", accent: "bg-err" },
  modernizer: { label: "Pattern modernizer", emoji: "🔄", accent: "bg-ok" },
};

function RefactorSwarmLive() {
  const live = useRefactor((s) => s.live);
  const scanning = useRefactor((s) => s.scanning);

  return (
    <div className="flex flex-col gap-2">
      <div className="grid grid-cols-1 gap-2 lg:grid-cols-2">
        {(Object.keys(VOICE_META) as RefactorVoice[]).map((id) => (
          <VoiceCard
            key={id}
            meta={VOICE_META[id]}
            content={live.voices[id]}
            done={live.voicesDone[id]}
            running={!!scanning}
          />
        ))}
      </div>
      <SynthCard content={live.synthesis} scanning={!!scanning} />
    </div>
  );
}

function VoiceCard({
  meta,
  content,
  done,
  running,
}: {
  meta: { label: string; emoji: string; accent: string };
  content: string;
  done: boolean;
  running: boolean;
}) {
  const empty = content.length === 0;
  return (
    <div className="overflow-hidden rounded-md border border-line bg-bg-0">
      <div className="flex items-center gap-2 border-b border-line px-2 py-1">
        <span className={cn("h-2 w-2 rounded-full", meta.accent)} />
        <span className="text-[11px] font-medium text-ink">
          {meta.emoji} {meta.label}
        </span>
        <span className="ml-auto">
          {done ? (
            <Check className="h-3 w-3 text-ok" />
          ) : empty && running ? (
            <span className="text-[10px] text-ink-subtle">waiting…</span>
          ) : running ? (
            <Loader2 className="h-3 w-3 animate-spin text-ink-subtle" />
          ) : empty ? (
            <span className="text-[10px] text-ink-subtle">—</span>
          ) : null}
        </span>
      </div>
      <div className="max-h-36 overflow-y-auto px-2 py-1.5 text-[11px] text-ink whitespace-pre-wrap">
        {empty ? (
          <span className="text-ink-subtle italic">…</span>
        ) : (
          content
        )}
      </div>
    </div>
  );
}

function SynthCard({
  content,
  scanning,
}: {
  content: string;
  scanning: boolean;
}) {
  const empty = content.length === 0;
  return (
    <div
      className={cn(
        "overflow-hidden rounded-md border",
        empty ? "border-line bg-bg-0" : "border-amber/40 bg-amber/5",
      )}
    >
      <div className="flex items-center gap-2 border-b border-line px-2 py-1">
        <Sparkles className="h-3 w-3 text-amber" />
        <span className="text-[11px] font-medium text-ink">
          🧭 Refactor recommendation
        </span>
        <span className="ml-auto">
          {!empty && !scanning ? (
            <Check className="h-3 w-3 text-ok" />
          ) : scanning ? (
            <Loader2 className="h-3 w-3 animate-spin text-ink-subtle" />
          ) : null}
        </span>
      </div>
      <div className="max-h-52 overflow-y-auto px-2 py-1.5 text-[11.5px] text-ink whitespace-pre-wrap">
        {empty ? (
          <span className="text-ink-subtle italic">awaiting voices…</span>
        ) : (
          content
        )}
      </div>
    </div>
  );
}

function ProposalsInbox({
  proposals,
  onApply,
  onDismiss,
}: {
  proposals: RefactorProposal[];
  onApply: (p: RefactorProposal) => void;
  onDismiss: (p: RefactorProposal) => void;
}) {
  if (proposals.length === 0) {
    return (
      <p className="py-3 text-center text-[11px] text-ink-subtle">
        Inbox empty. Run a scan above to surface proposals.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      {proposals.map((p) => (
        <ProposalCard key={p.id} p={p} onApply={onApply} onDismiss={onDismiss} />
      ))}
    </div>
  );
}

function ProposalCard({
  p,
  onApply,
  onDismiss,
}: {
  p: RefactorProposal;
  onApply: (p: RefactorProposal) => void;
  onDismiss: (p: RefactorProposal) => void;
}) {
  return (
    <div className="overflow-hidden rounded-md border border-line bg-bg-0">
      <div className="flex items-start justify-between gap-2 border-b border-line bg-bg-1 px-2 py-1.5">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-[12px] font-medium text-ink">
            <Sparkles className="h-3 w-3 shrink-0 text-amber" />
            <span className="truncate">{p.title}</span>
          </div>
          <div className="mt-0.5 flex items-center gap-2 text-[10.5px] text-ink-subtle">
            <span className="truncate font-mono">{shortPath(p.file_path)}</span>
            <RiskBadge risk={p.risk} />
            <VerificationBadge status={p.verification_status} />
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button size="sm" variant="ghost" onClick={() => onDismiss(p)}>
            <Trash2 className="h-3 w-3" />
            Dismiss
          </Button>
          <Button size="sm" onClick={() => onApply(p)}>
            <Check className="h-3 w-3" />
            Apply
          </Button>
        </div>
      </div>
      <div className="px-2 py-1.5 text-[11.5px] text-ink whitespace-pre-wrap">
        {p.rationale}
      </div>
      <details className="border-t border-line">
        <summary className="cursor-pointer px-2 py-1 text-[10.5px] text-ink-subtle hover:text-ink">
          Show diff
        </summary>
        <pre className="max-h-60 overflow-auto bg-bg-0 px-2 py-1 font-mono text-[10.5px] text-ink-muted">
          {p.diff}
        </pre>
      </details>
    </div>
  );
}

function RiskBadge({ risk }: { risk: RefactorRisk }) {
  const cls =
    risk === "low"
      ? "border-ok/40 bg-ok/5 text-ok"
      : risk === "high"
        ? "border-err/40 bg-err/5 text-err"
        : "border-amber/40 bg-amber/5 text-amber";
  return (
    <span className={cn("rounded-md border px-1 py-0.5 text-[9.5px] uppercase tracking-wider", cls)}>
      {risk} risk
    </span>
  );
}

function VerificationBadge({
  status,
}: {
  status: RefactorProposal["verification_status"];
}) {
  if (status === "untested") {
    return (
      <span className="rounded-md border border-line bg-bg-1 px-1 py-0.5 text-[9.5px] uppercase tracking-wider text-ink-subtle">
        untested
      </span>
    );
  }
  if (status === "verified_pass") {
    return (
      <span className="rounded-md border border-ok/40 bg-ok/5 px-1 py-0.5 text-[9.5px] uppercase tracking-wider text-ok">
        tests pass
      </span>
    );
  }
  return (
    <span className="rounded-md border border-err/40 bg-err/5 px-1 py-0.5 text-[9.5px] uppercase tracking-wider text-err">
      tests failed
    </span>
  );
}

function shortPath(p: string): string {
  // Collapse leading project-root prefix to just the last 2–3 components.
  const parts = p.split(/[\\/]/);
  if (parts.length <= 3) return p;
  return ".../" + parts.slice(-3).join("/");
}
