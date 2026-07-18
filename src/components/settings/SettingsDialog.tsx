/**
 * Settings dialog — Models, Sandbox, Memory tabs.
 *
 * Drives real backend behaviour: every change is persisted to
 * `<app config dir>/settings.json` and picked up by the next call into
 * the embedder, chat client, or sandbox dispatcher.
 *
 * The "Test connection" button on the Models tab hits the typed
 * `ollama_health` endpoint and surfaces which configured models are
 * actually installed.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import {
  CircleCheck,
  Cpu,
  Database,
  Download,
  Loader2,
  RefreshCw,
  Shield,
  CircleX,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSettings } from "@/store/settings";
import {
  models,
  type PullProgress,
  type SandboxDefault,
  type SettingsPatch,
  type SettingsView,
} from "@/lib/tauri";
import { notify } from "@/store/notifications";
import { usePlatform, useAvailableSandboxes } from "@/store/platform";
import { cn } from "@/lib/utils";
import { ProjectIndexSection } from "./ProjectIndexSection";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SettingsDialog({ open, onClose }: Props) {
  const current = useSettings((s) => s.current);
  const loading = useSettings((s) => s.loading);
  const saving = useSettings((s) => s.saving);
  const health = useSettings((s) => s.health);
  const load = useSettings((s) => s.load);
  const save = useSettings((s) => s.save);
  const checkOllama = useSettings((s) => s.checkOllama);

  const [draft, setDraft] = useState<SettingsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Reload settings + warm the health check whenever the dialog opens.
  useEffect(() => {
    if (!open) return;
    void load().then(() => {
      void checkOllama();
    });
  }, [open, load, checkOllama]);

  useEffect(() => {
    if (current) setDraft({ ...current });
  }, [current]);

  const dirty = useMemo(() => {
    if (!current || !draft) return false;
    return JSON.stringify(current) !== JSON.stringify(draft);
  }, [current, draft]);

  const patch: SettingsPatch | null = useMemo(() => {
    if (!current || !draft || !dirty) return null;
    const out: SettingsPatch = {};
    for (const key of Object.keys(draft) as Array<keyof SettingsView>) {
      if ((current as any)[key] !== (draft as any)[key]) {
        (out as any)[key] = (draft as any)[key];
      }
    }
    return out;
  }, [current, draft, dirty]);

  const onSave = async () => {
    if (!patch) return;
    setError(null);
    const result = await save(patch);
    if (!result) {
      setError("Save failed. Check the console for details.");
      return;
    }
    // Re-warm health since the new config might point at a different model.
    void checkOllama();
  };

  const set = <K extends keyof SettingsView>(key: K, value: SettingsView[K]) => {
    setDraft((d) => (d ? { ...d, [key]: value } : d));
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && !saving && onClose()}>
      <DialogContent className="w-[min(820px,calc(100vw-3rem))]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Cpu className="h-3.5 w-3.5 text-amber" />
            Settings
          </DialogTitle>
          <DialogDescription>
            Persisted to <span className="font-mono">settings.json</span> under your app config
            dir. Changes take effect immediately — no restart needed.
          </DialogDescription>
        </DialogHeader>

        {loading || !draft ? (
          <div className="flex items-center justify-center p-12 text-[12px] text-ink-subtle">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            loading…
          </div>
        ) : (
          <Tabs defaultValue="models" className="flex h-120 flex-col">
            <TabsList>
              <TabsTrigger value="models">
                <Cpu className="h-3 w-3" />
                Models
              </TabsTrigger>
              <TabsTrigger value="sandbox">
                <Shield className="h-3 w-3" />
                Sandbox
              </TabsTrigger>
              <TabsTrigger value="memory">
                <Database className="h-3 w-3" />
                Memory
              </TabsTrigger>
            </TabsList>

            <TabsContent value="models" className="overflow-y-auto p-4">
              <Section title="Ollama">
                <Field label="Endpoint URL" hint="Defaults to localhost:11434.">
                  <Input
                    value={draft.ollama_url}
                    onChange={(e) => set("ollama_url", e.target.value)}
                    placeholder="http://localhost:11434"
                  />
                </Field>
                <div className="flex items-center gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => void checkOllama()}
                  >
                    <RefreshCw className="h-3 w-3" />
                    Test connection
                  </Button>
                  <HealthPill health={health} />
                </div>
              </Section>

              <Section title="Chat model" subtitle="Used for replies and the planner JSON-mode call. Any Ollama-compatible tag works — Kilroy is model-agnostic.">
                <ModelSelector
                  kind="chat"
                  value={draft.chat_model}
                  installed={health?.models ?? []}
                  onChange={(v) => set("chat_model", v)}
                />
              </Section>

              <Section
                title="Embedding model"
                subtitle="Used for semantic search over your codebase + decisions. Dimension is locked at 768 to match the vec0 table."
              >
                <ModelSelector
                  kind="embedding"
                  value={draft.embedding_model}
                  installed={health?.models ?? []}
                  onChange={(v) => set("embedding_model", v)}
                />
              </Section>
            </TabsContent>

            <TabsContent value="sandbox" className="overflow-y-auto p-4">
              <Section
                title="Default sandbox"
                subtitle="Newly-proposed shell actions will be tagged with this. You can change a single action's sandbox before accepting."
              >
                <SandboxRadio
                  value={draft.default_sandbox}
                  onChange={(v) => set("default_sandbox", v)}
                />
              </Section>
              <Section
                title="Windows Sandbox timeout"
                subtitle="Maximum seconds to wait for the disposable VM to finish a command before giving up."
              >
                <Field label={`${draft.sandbox_timeout_secs} seconds`}>
                  <input
                    type="range"
                    min={30}
                    max={1800}
                    step={15}
                    value={draft.sandbox_timeout_secs}
                    onChange={(e) =>
                      set("sandbox_timeout_secs", Number(e.target.value))
                    }
                    className="w-full accent-amber"
                  />
                </Field>
              </Section>
            </TabsContent>

            <TabsContent value="memory" className="overflow-y-auto p-4">
              <Section
                title="Retrieval"
                subtitle="How many neighbours to pull from the vector index on each chat turn."
              >
                <NumberField
                  label="Code chunks (k)"
                  min={0}
                  max={20}
                  value={draft.retrieval_chunks_k}
                  onChange={(v) => set("retrieval_chunks_k", v)}
                />
                <NumberField
                  label="Decisions (k)"
                  min={0}
                  max={20}
                  value={draft.retrieval_decisions_k}
                  onChange={(v) => set("retrieval_decisions_k", v)}
                />
              </Section>
              <Section
                title="Chunking"
                subtitle="How files are split before embedding. Smaller windows = more precise retrieval but bigger index."
              >
                <NumberField
                  label="Window (lines)"
                  min={8}
                  max={200}
                  value={draft.chunk_window}
                  onChange={(v) => set("chunk_window", v)}
                />
                <NumberField
                  label="Stride (lines)"
                  min={4}
                  max={draft.chunk_window}
                  value={draft.chunk_stride}
                  onChange={(v) => set("chunk_stride", v)}
                />
                <p className="text-[10.5px] text-ink-subtle">
                  Overlap is <span className="text-ink">{Math.max(0, draft.chunk_window - draft.chunk_stride)}</span> lines.
                  Re-run <span className="text-amber">Index Project</span> after changing these to apply.
                </p>
              </Section>
              <ProjectIndexSection settingsOpen={open} />
            </TabsContent>
          </Tabs>
        )}

        <DialogFooter>
          {/* One left-aligned status line: an error takes precedence over
              the dirty/clean indicator so the footer never shows two
              conflicting messages at once. */}
          {error ? (
            <span className="mr-auto text-[11px] text-err">{error}</span>
          ) : (
            <span className="mr-auto text-[11px] text-ink-subtle">
              {dirty ? "Unsaved changes" : "Up to date"}
            </span>
          )}
          <Button variant="ghost" onClick={() => !saving && onClose()} disabled={saving}>
            Close
          </Button>
          <Button onClick={onSave} disabled={!dirty || saving}>
            {saving ? (
              <>
                <Loader2 className="h-3 w-3 animate-spin" />
                Saving…
              </>
            ) : (
              "Save"
            )}
          </Button>
        </DialogFooter>
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
    <section className="mb-5 flex flex-col gap-2">
      <div>
        <h3 className="text-[12px] font-semibold text-ink">{title}</h3>
        {subtitle && (
          <p className="text-[11px] text-ink-subtle">{subtitle}</p>
        )}
      </div>
      {children}
    </section>
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

function NumberField({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (n: number) => void;
}) {
  return (
    <Field label={label}>
      <div className="flex items-center gap-2">
        <Input
          type="number"
          value={value}
          min={min}
          max={max}
          onChange={(e) => {
            const n = Number(e.target.value);
            if (Number.isFinite(n)) onChange(Math.max(min, Math.min(max, Math.round(n))));
          }}
          className="w-24"
        />
        <input
          type="range"
          min={min}
          max={max}
          value={value}
          onChange={(e) => onChange(Number(e.target.value))}
          className="flex-1 accent-amber"
        />
      </div>
    </Field>
  );
}

/** Examples surfaced as one-click suggestions. NOT a closed list — the
 *  free-text field accepts any Ollama tag. We just give people a starting
 *  point so they don't have to memorise tag names. */
const CHAT_SUGGESTIONS = [
  "qwen2.5-coder:14b-instruct-q8_0",
  "deepseek-coder-v2:16b-lite-instruct-q5_K_M",
  "llama3.1:8b-instruct-q5_K_M",
  "codestral:22b",
  "mixtral:8x7b-instruct-q4_K_M",
  "phi3:14b-medium-128k-instruct-q5_K_M",
];

const EMBEDDING_SUGGESTIONS = [
  "nomic-embed-text",
  "mxbai-embed-large",
  "snowflake-arctic-embed:33m",
  "all-minilm:l6-v2",
];

function ModelSelector({
  kind,
  value,
  installed,
  onChange,
}: {
  kind: "chat" | "embedding";
  value: string;
  installed: string[];
  onChange: (v: string) => void;
}) {
  const isCustom = value !== "" && !installed.includes(value);
  const suggestions = kind === "chat" ? CHAT_SUGGESTIONS : EMBEDDING_SUGGESTIONS;
  const placeholder =
    kind === "chat"
      ? "any Ollama tag — e.g. llama3.1:8b, deepseek-coder-v2:16b, qwen2.5-coder:7b"
      : "any Ollama embedding tag — e.g. nomic-embed-text, mxbai-embed-large";

  // Per-selector pull state. Each ModelSelector instance manages its
  // own pull lifecycle (chat and embedding can pull in parallel).
  const [pulling, setPulling] = useState(false);
  const [progress, setProgress] = useState<PullProgress | null>(null);
  // The Ollama /api/pull stream emits the same `digest` across many
  // chunks; we track the latest seen digest so the progress bar shows
  // the active layer, not whichever digest came last in race conditions.
  const checkOllama = useSettings((s) => s.checkOllama);

  // Listener registration is per-mount; subscribe once on first render.
  // We filter inside the callback by the tag being pulled so chat and
  // embedding selectors never cross-update each other's progress UI.
  const pullingTagRef = useRef<string | null>(null);
  useEffect(() => {
    let off: (() => void) | null = null;
    void models
      .onPullProgress((p) => {
        if (p.tag !== pullingTagRef.current) return;
        setProgress(p);
        if (p.done) {
          setPulling(false);
          if (p.status === "success" || p.status === "complete") {
            notify.success("Model pulled", `${p.tag} is ready.`);
            // Refresh installed-models list so the new tag appears in
            // the dropdown without making the user re-open Settings.
            void checkOllama();
          } else if (p.status === "error") {
            notify.error("Pull failed", p.error ?? "(no error message)");
          }
        }
      })
      .then((u) => {
        off = u;
      });
    return () => {
      if (off) off();
    };
  }, [checkOllama]);

  const startPull = async () => {
    if (!value.trim()) {
      notify.warn("No model specified", "Type a tag (e.g. llama3.1:8b) first.");
      return;
    }
    pullingTagRef.current = value.trim();
    setPulling(true);
    setProgress({
      tag: value.trim(),
      status: "starting",
      completed: 0,
      total: 0,
      digest: null,
      error: null,
      done: false,
    });
    try {
      await models.pull(value.trim());
    } catch (err) {
      // The progress listener will have already fired the error toast;
      // we just need to make sure the spinner stops if the Promise
      // rejects without a `done` event for some reason.
      setPulling(false);
      console.error("pull_model rejected:", err);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      {installed.length > 0 && (
        <select
          value={installed.includes(value) ? value : ""}
          onChange={(e) => e.target.value && onChange(e.target.value)}
          className="rounded-md border border-line bg-bg-2 px-2 py-1.5 text-[12px] text-ink"
        >
          <option value="">— pick an installed model —</option>
          {installed.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      )}
      <div className="flex items-center gap-2">
        <Input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          className="flex-1"
          disabled={pulling}
        />
        {/* Pull button — fires `ollama pull <tag>` via the Tauri command
            and shows progress in the strip below. Disabled when the tag
            is already installed (no point pulling again) or when an
            equivalent pull is already in flight. */}
        <Button
          variant="ghost"
          size="sm"
          onClick={startPull}
          disabled={pulling || installed.includes(value) || !value.trim()}
          title={
            installed.includes(value)
              ? "Already installed"
              : "Run `ollama pull` for the typed tag and stream progress below"
          }
        >
          {pulling ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <Download className="h-3 w-3" />
          )}
          {pulling ? "Pulling…" : "Pull"}
        </Button>
      </div>
      {/* Progress strip — visible during pull AND for a moment after
          completion so the success state is observable. */}
      {progress && (
        <PullProgressStrip progress={progress} />
      )}
      {/* Suggestion chips. Click to fill the input — pull is still a
          separate explicit action, no surprise downloads. */}
      <div className="flex flex-wrap gap-1">
        {suggestions
          .filter((s) => !installed.includes(s) && s !== value)
          .slice(0, 6)
          .map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => onChange(s)}
              className="rounded-md border border-line bg-bg-1 px-1.5 py-0.5 text-[10.5px] text-ink-subtle hover:bg-bg-2 hover:text-ink"
              title={`Use ${s} — then click Pull to download`}
            >
              {s}
            </button>
          ))}
      </div>
      {isCustom && installed.length > 0 && !pulling && (
        <p className="text-[10.5px] text-warn">
          <span className="font-mono">{value}</span> isn't installed yet. Click{" "}
          <span className="font-mono">Pull</span> to download it.
        </p>
      )}
    </div>
  );
}

/** Live progress strip for an in-flight `ollama pull`. Renders a
 *  filled bar for the current layer plus the human-readable status. */
function PullProgressStrip({ progress }: { progress: PullProgress }) {
  const pct =
    progress.total > 0
      ? Math.min(100, Math.round((progress.completed / progress.total) * 100))
      : 0;
  const isErr = progress.status === "error";
  const isDone =
    progress.done && (progress.status === "success" || progress.status === "complete");
  return (
    <div
      className={cn(
        "rounded-md border px-2 py-1.5 text-[11px]",
        isErr
          ? "border-err/40 bg-err/5 text-err"
          : isDone
            ? "border-ok/40 bg-ok/5 text-ok"
            : "border-line bg-bg-1 text-ink-muted",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate font-mono">{progress.tag}</span>
        <span className="shrink-0 uppercase tracking-wider text-[9.5px]">
          {progress.status}
        </span>
      </div>
      {progress.total > 0 && (
        <div className="mt-1 h-1 overflow-hidden rounded-full bg-bg-3">
          <div
            className={cn(
              "h-full transition-[width] duration-200",
              isErr ? "bg-err" : isDone ? "bg-ok" : "bg-amber",
            )}
            style={{ width: `${pct}%` }}
          />
        </div>
      )}
      {progress.total > 0 && (
        <div className="mt-1 flex justify-between text-[10px] text-ink-subtle">
          <span>{formatBytes(progress.completed)} / {formatBytes(progress.total)}</span>
          <span>{pct}%</span>
        </div>
      )}
      {progress.error && (
        <div className="mt-1 text-[10.5px] text-err">{progress.error}</div>
      )}
    </div>
  );
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function SandboxRadio({
  value,
  onChange,
}: {
  value: SandboxDefault;
  onChange: (v: SandboxDefault) => void;
}) {
  // Only show sandbox kinds the host actually supports. Windows Sandbox
  // is filtered out on macOS/Linux by the OS detector, so the user never
  // sees a choice that would error on every command.
  const available = useAvailableSandboxes();
  const platformInfo = usePlatform((s) => s.info);
  const allOptions: {
    id: SandboxDefault;
    title: string;
    body: string;
    badge?: string;
  }[] = [
    {
      id: "windows_sandbox",
      title: "Windows Sandbox",
      body: "Disposable VM. Requires the Containers-DisposableClientVM feature (Windows only).",
      badge: platformInfo?.default_sandbox === "windows_sandbox" ? "default" : undefined,
    },
    {
      id: "host",
      title: "Host",
      body:
        platformInfo && !platformInfo.is_windows
          ? "Run in your real shell. Fast but no isolation."
          : "Run in your real PowerShell. Fast but no isolation.",
      badge: platformInfo?.default_sandbox === "host" ? "default" : undefined,
    },
    {
      id: "docker",
      title: "Docker",
      body: "Disposable container with the project mounted at /work. Requires Docker installed and running. Override the image via KILROY_DOCKER_IMAGE (default: debian:stable-slim).",
    },
  ];
  const options = allOptions.filter((o) => available.includes(o.id));
  return (
    <div className="flex flex-col gap-2">
      {options.map((o) => {
        const selected = value === o.id;
        return (
          <button
            key={o.id}
            onClick={() => onChange(o.id)}
            className={cn(
              "flex items-start gap-3 rounded-md border bg-bg-2 px-3 py-2 text-left transition-colors",
              selected
                ? "border-amber bg-amber/5"
                : "border-line hover:border-line-strong",
            )}
          >
            <div
              className={cn(
                "mt-0.5 h-3.5 w-3.5 shrink-0 rounded-full border",
                selected ? "border-amber bg-amber" : "border-line bg-bg-1",
              )}
            />
            <div className="flex-1">
              <p className="flex items-center gap-2 text-[12px] font-medium text-ink">
                {o.title}
                {o.badge && (
                  <span className="rounded-sm bg-line px-1 py-px text-[9px] uppercase tracking-wider text-ink-subtle">
                    {o.badge}
                  </span>
                )}
              </p>
              <p className="text-[11px] text-ink-subtle">{o.body}</p>
            </div>
          </button>
        );
      })}
    </div>
  );
}

function HealthPill({ health }: { health: ReturnType<typeof useSettings.getState>["health"] }) {
  if (!health) return null;
  if (!health.reachable) {
    return (
      <span className="flex items-center gap-1.5 text-[11px] text-err">
        <CircleX className="h-3 w-3" />
        <span>Unreachable</span>
        {health.error && (
          <span className="ml-1 truncate text-ink-subtle max-w-70">
            {health.error}
          </span>
        )}
      </span>
    );
  }
  const chatOk = health.has_chat_model;
  const embOk = health.has_embedding_model;
  const allGood = chatOk && embOk;
  return (
    <span
      className={cn(
        "flex items-center gap-1.5 text-[11px]",
        allGood ? "text-ok" : "text-warn",
      )}
    >
      {allGood ? (
        <CircleCheck className="h-3 w-3" />
      ) : (
        <CircleX className="h-3 w-3" />
      )}
      <span>{health.models.length} models installed</span>
      <span className="text-ink-subtle">·</span>
      <span className={chatOk ? "text-ok" : "text-warn"}>chat {chatOk ? "✓" : "missing"}</span>
      <span className="text-ink-subtle">·</span>
      <span className={embOk ? "text-ok" : "text-warn"}>embed {embOk ? "✓" : "missing"}</span>
    </span>
  );
}
