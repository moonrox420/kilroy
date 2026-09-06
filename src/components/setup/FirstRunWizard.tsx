/**
 * First-run setup wizard.
 *
 * Fires once on initial launch (when settings.first_run is true). Walks
 * the user through:
 *   1. Welcome — what Kilroy is, what it needs to run
 *   2. Ollama check — is the daemon reachable, is the chat model installed
 *   3. Project — pick a folder, or skip and use scratch mode
 *   4. Done
 *
 * On Finish, calls update_settings({ first_run: false }) so the wizard
 * doesn't show again. The user can re-open it from the Help menu.
 */
import { useEffect, useState } from "react";
import {
  ArrowRight,
  Brain,
  CircleCheck,
  FolderOpen,
  Loader2,
  Rocket,
  ShieldCheck,
  CircleX,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { KilroyMark } from "@/components/common/KilroyMark";
import { useSettings } from "@/store/settings";
import { useWorkspace } from "@/store/workspace";
import { notify } from "@/store/notifications";
import { settings as settingsApi } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type Step = "welcome" | "ollama" | "project" | "done";

export function FirstRunWizard() {
  const current = useSettings((s) => s.current);
  const checkOllama = useSettings((s) => s.checkOllama);
  const health = useSettings((s) => s.health);
  const openFolder = useWorkspace((s) => s.openFolder);
  const rootPath = useWorkspace((s) => s.rootPath);

  // Local control over open-state so we can hide on Finish without
  // having to wait for the settings round-trip.
  const [open, setOpen] = useState(false);
  const [step, setStep] = useState<Step>("welcome");
  const [verifying, setVerifying] = useState(false);

  useEffect(() => {
    // Open if and only if settings have loaded AND first_run is true.
    if (current && current.first_run) {
      setOpen(true);
    }
  }, [current?.first_run]);

  // Re-check Ollama status whenever the user lands on that step.
  useEffect(() => {
    if (step === "ollama") {
      void checkOllama();
    }
  }, [step, checkOllama]);

  const ollamaReady =
    health?.reachable === true && health.has_chat_model === true;

  const onVerify = async () => {
    setVerifying(true);
    try {
      await checkOllama();
    } finally {
      setVerifying(false);
    }
  };

  const onFinish = async () => {
    try {
      await settingsApi.update({ first_run: false });
      notify.success("Setup complete", "Kilroy is ready.");
      setOpen(false);
    } catch (err) {
      notify.fromError("Finish setup", err);
    }
  };

  if (!open) return null;

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onFinish()}>
      <DialogContent className="max-w-[640px] gap-0 p-0">
        <header className="flex items-center gap-3 border-b border-line px-5 py-3">
          <KilroyMark size={32} className="opacity-80" />
          <div className="min-w-0 flex-1">
            <DialogTitle className="text-[14px] font-semibold tracking-tight text-ink">
              Welcome to Kilroy
            </DialogTitle>
            <DialogDescription className="text-[11px] text-ink-muted">
              One-time setup — about 60 seconds.
            </DialogDescription>
          </div>
          <Stepper current={step} />
        </header>

        <div className="min-h-[260px] px-5 py-4">
          {step === "welcome" && <WelcomeStep />}
          {step === "ollama" && (
            <OllamaStep
              health={health}
              ollamaReady={ollamaReady}
              verifying={verifying}
              onVerify={onVerify}
              configuredModel={current?.chat_model ?? ""}
            />
          )}
          {step === "project" && (
            <ProjectStep
              rootPath={rootPath}
              onPick={() => void openFolder()}
            />
          )}
          {step === "done" && <DoneStep />}
        </div>

        <footer className="flex items-center justify-between gap-3 border-t border-line bg-bg-1 px-5 py-3">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setStep(prev(step))}
            disabled={step === "welcome"}
          >
            Back
          </Button>
          <div className="flex items-center gap-2">
            {step !== "welcome" && step !== "done" && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setStep(next(step))}
              >
                Skip
              </Button>
            )}
            {step === "done" ? (
              <Button size="sm" onClick={onFinish}>
                <Rocket className="h-3 w-3" />
                Launch Kilroy
              </Button>
            ) : (
              <Button
                size="sm"
                onClick={() => setStep(next(step))}
                disabled={step === "ollama" && !ollamaReady}
              >
                Continue
                <ArrowRight className="h-3 w-3" />
              </Button>
            )}
          </div>
        </footer>
      </DialogContent>
    </Dialog>
  );
}

// ─── Steps ──────────────────────────────────────────────────────────────────

function WelcomeStep() {
  return (
    <div className="flex flex-col gap-3 text-[12px] text-ink-muted">
      <p>
        Kilroy is a fully-local AI engineering platform. Everything runs on
        your machine — no API keys, no cloud calls, no usage limits.
      </p>
      <ul className="ml-2 list-disc space-y-1 pl-3 text-ink-muted">
        <li>
          <span className="text-ink">Ollama</span> serves the local LLM and
          embedding models. Kilroy talks to it on localhost.
        </li>
        <li>
          <span className="text-ink">Windows Sandbox</span> isolates any
          shell commands the agent proposes. Your host machine stays
          untouched until you explicitly Accept an action.
        </li>
        <li>
          <span className="text-ink">A project folder</span> gives the
          agent context — it indexes your code, remembers decisions, and
          carries chat history across sessions.
        </li>
      </ul>
      <p className="text-[11px] text-ink-subtle">
        The next two steps walk through each. About 60 seconds total.
      </p>
    </div>
  );
}

function OllamaStep({
  health,
  ollamaReady,
  verifying,
  onVerify,
  configuredModel,
}: {
  health: ReturnType<typeof useSettings.getState>["health"];
  ollamaReady: boolean;
  verifying: boolean;
  onVerify: () => void;
  configuredModel: string;
}) {
  const reachable = health?.reachable === true;
  const hasModel = health?.has_chat_model === true;

  return (
    <div className="flex flex-col gap-3 text-[12px] text-ink-muted">
      <p>
        Kilroy needs Ollama running locally and at least one chat model
        installed.
      </p>

      <div className="rounded-md border border-line bg-bg-0 p-3">
        <CheckLine
          ok={reachable}
          loading={verifying}
          label="Ollama daemon reachable"
          detail={
            reachable
              ? `${health?.models.length ?? 0} model(s) installed`
              : "not running on localhost:11434"
          }
        />
        <div className="my-1 h-px bg-line/60" />
        <CheckLine
          ok={hasModel}
          loading={verifying}
          label={`Chat model installed: ${configuredModel}`}
          detail={
            hasModel
              ? "ready"
              : reachable
                ? "not yet pulled"
                : "(blocked until Ollama is running)"
          }
        />
      </div>

      {!reachable && (
        <CmdHint
          title="Install Ollama"
          cmd="winget install Ollama.Ollama"
          note="After install, Ollama auto-starts as a Windows service."
        />
      )}
      {reachable && !hasModel && (
        <CmdHint
          title="Pull the chat model"
          cmd={`ollama pull ${configuredModel}`}
          note="This downloads ~5-8 GB. Run in PowerShell, then click Re-check."
        />
      )}

      <div className="flex items-center justify-between">
        <Button
          variant="ghost"
          size="sm"
          onClick={onVerify}
          disabled={verifying}
        >
          {verifying ? (
            <>
              <Loader2 className="h-3 w-3 animate-spin" />
              Checking…
            </>
          ) : (
            <>Re-check</>
          )}
        </Button>
        {ollamaReady && (
          <span className="flex items-center gap-1.5 text-[11px] text-ok">
            <ShieldCheck className="h-3.5 w-3.5" />
            All checks passed
          </span>
        )}
      </div>
    </div>
  );
}

function ProjectStep({
  rootPath,
  onPick,
}: {
  rootPath: string | null;
  onPick: () => void;
}) {
  return (
    <div className="flex flex-col gap-3 text-[12px] text-ink-muted">
      <p>
        Pick a folder to use as your first Kilroy project. The agent will
        index it, remember decisions you log, and carry chat history
        between sessions.
      </p>
      <p className="text-[11px] text-ink-subtle">
        You can change projects at any time via File → Open Folder. Skipping
        this is fine — Kilroy launches in scratch mode without a project.
      </p>
      <div className="flex items-center gap-2">
        <Button size="sm" onClick={onPick}>
          <FolderOpen className="h-3 w-3" />
          {rootPath ? "Change folder" : "Open folder"}
        </Button>
        {rootPath && (
          <span className="flex items-center gap-1.5 truncate text-[11px] text-ok">
            <CircleCheck className="h-3 w-3" />
            {rootPath}
          </span>
        )}
      </div>
    </div>
  );
}

function DoneStep() {
  return (
    <div className="flex flex-col items-center gap-3 py-6 text-center">
      <KilroyMark size={72} className="opacity-50" />
      <p className="text-[14px] font-semibold text-ink">You're set.</p>
      <p className="max-w-[420px] text-[12px] text-ink-muted">
        Open the agent chat on the right — SmartCoder is the default
        agent (writes, runs, self-corrects). Settings live behind Ctrl+,
        for tweaks later.
      </p>
    </div>
  );
}

// ─── Building blocks ────────────────────────────────────────────────────────

function CheckLine({
  ok,
  loading,
  label,
  detail,
}: {
  ok: boolean;
  loading: boolean;
  label: string;
  detail: string;
}) {
  return (
    <div className="flex items-center gap-2 py-0.5">
      {loading ? (
        <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-ink-subtle" />
      ) : ok ? (
        <CircleCheck className="h-3.5 w-3.5 shrink-0 text-ok" />
      ) : (
        <CircleX className="h-3.5 w-3.5 shrink-0 text-err" />
      )}
      <span className="flex-1 text-[12px] text-ink">{label}</span>
      <span className="text-[11px] text-ink-subtle">{detail}</span>
    </div>
  );
}

function CmdHint({
  title,
  cmd,
  note,
}: {
  title: string;
  cmd: string;
  note?: string;
}) {
  const onCopy = () => {
    void navigator.clipboard.writeText(cmd).then(
      () => notify.success("Copied", cmd),
      (err) => notify.fromError("Clipboard write", err),
    );
  };
  return (
    <div className="rounded-md border border-amber/40 bg-amber/5 p-2.5">
      <p className="mb-1 flex items-center gap-1.5 text-[11px] font-medium text-amber">
        <Brain className="h-3 w-3" />
        {title}
      </p>
      <button
        onClick={onCopy}
        className={cn(
          "w-full rounded-sm border border-amber/30 bg-bg-0 px-2 py-1 text-left font-mono text-[11px] text-ink",
          "transition-colors hover:bg-amber/10",
        )}
        title="Click to copy"
      >
        {cmd}
      </button>
      {note && <p className="mt-1 text-[10px] text-ink-subtle">{note}</p>}
    </div>
  );
}

function Stepper({ current }: { current: Step }) {
  const order: Step[] = ["welcome", "ollama", "project", "done"];
  return (
    <div className="flex items-center gap-1.5">
      {order.map((s) => (
        <span
          key={s}
          className={cn(
            "h-1.5 w-1.5 rounded-full",
            s === current
              ? "bg-amber"
              : order.indexOf(s) < order.indexOf(current)
                ? "bg-amber/40"
                : "bg-line",
          )}
        />
      ))}
    </div>
  );
}

function next(s: Step): Step {
  return s === "welcome"
    ? "ollama"
    : s === "ollama"
      ? "project"
      : s === "project"
        ? "done"
        : "done";
}

function prev(s: Step): Step {
  return s === "done"
    ? "project"
    : s === "project"
      ? "ollama"
      : s === "ollama"
        ? "welcome"
        : "welcome";
}
