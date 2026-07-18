/**
 * Datasets dialog — load a training-data file, inspect it, optionally
 * turn it into a custom Ollama model.
 *
 * Three rough stages visible in the UI:
 *
 *   1. **Pick a file.** Native file picker accepts .json, .jsonl, .ndjson.
 *      (.arrow is stubbed — surfaces a "needs pyarrow" hint.)
 *
 *   2. **Inspect.** Backend reads the file, auto-detects format
 *      (Alpaca / ShareGPT / OpenAI / prompt-completion), counts records,
 *      samples a few rows. UI renders stats + a sample preview.
 *
 *   3. **Create custom model** (optional). Pick a base model + new tag.
 *      Backend renders a Modelfile (SYSTEM directive derived from
 *      dataset samples) and POSTs to `ollama /api/create`, streaming
 *      status events back. Result: a new model that's selectable in
 *      Settings → Chat model.
 *
 * Training-environment status sits at the bottom as a "ready for LoRA
 * fine-tuning?" panel. Today this is informational only — the actual
 * subprocess invocation is scaffolded but not wired to a button yet.
 */
import { useEffect, useState } from "react";
import {
  Database,
  FileJson,
  FileQuestion,
  Loader2,
  Sparkles,
  Upload,
} from "lucide-react";
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
import { datasets, fs, type CreateProgress } from "@/lib/tauri";
import { useDatasets } from "@/store/datasets";
import { useSettings } from "@/store/settings";
import { notify } from "@/store/notifications";
import { cn } from "@/lib/utils";

export function DatasetsDialog() {
  const open = useDatasets((s) => s.open);
  const close = useDatasets((s) => s.closeDialog);
  const inspect = useDatasets((s) => s.inspect);
  const inspecting = useDatasets((s) => s.inspecting);
  const error = useDatasets((s) => s.error);
  const creating = useDatasets((s) => s.creating);
  const createProgress = useDatasets((s) => s.createProgress);
  const lastBuild = useDatasets((s) => s.lastBuild);
  const trainingEnv = useDatasets((s) => s.trainingEnv);
  const setInspecting = useDatasets((s) => s.setInspecting);
  const setInspect = useDatasets((s) => s.setInspect);
  const setError = useDatasets((s) => s.setError);
  const setCreating = useDatasets((s) => s.setCreating);
  const setCreateProgress = useDatasets((s) => s.setCreateProgress);
  const setLastBuild = useDatasets((s) => s.setLastBuild);
  const setTrainingEnv = useDatasets((s) => s.setTrainingEnv);
  const reset = useDatasets((s) => s.reset);

  const settings = useSettings((s) => s.current);
  const refreshHealth = useSettings((s) => s.checkOllama);

  // Form state for the "create custom model" sub-section. We default the
  // base model to whatever the user has configured for chat — that's
  // usually what they want to extend.
  const [newName, setNewName] = useState("");
  const [base, setBase] = useState("");
  const [extraSystem, setExtraSystem] = useState("");
  const [temperature, setTemperature] = useState(0.4);

  useEffect(() => {
    if (open && settings?.chat_model && !base) {
      setBase(settings.chat_model);
    }
  }, [open, settings?.chat_model, base]);

  // Probe the training environment once when the dialog opens — cheap
  // (a couple of subprocess `--version` calls). Lets us show a Ready /
  // Not Ready chip at the bottom.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void datasets.trainingEnvStatus().then((env) => {
      if (!cancelled) setTrainingEnv(env);
    });
    return () => {
      cancelled = true;
    };
  }, [open, setTrainingEnv]);

  // Deep-link: when a caller (e.g. CorpusBanner) opened us with a
  // specific path, auto-inspect it so the user lands directly on the
  // stats view instead of having to find the file in the picker.
  const consumePendingPath = useDatasets((s) => s.consumePendingPath);
  useEffect(() => {
    if (!open) return;
    const p = consumePendingPath();
    if (p) {
      void runInspect(p);
    }
    // runInspect is stable enough — depending on it would cause a
    // re-run whenever we changed unrelated dialog state. Lint disabled
    // intentionally below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // Subscribe to model-create progress only while the dialog is open.
  // The dialog component stays mounted for the app's lifetime (it's in
  // App.tsx), so without the `open` guard this listener would keep
  // firing toasts and mutating state after the user closed the dialog.
  useEffect(() => {
    if (!open) return;
    let off: (() => void) | null = null;
    let disposed = false;
    void datasets.onCreateProgress((p: CreateProgress) => {
      setCreateProgress(p);
      if (p.done) {
        setCreating(false);
        if (p.status === "success") {
          notify.success("Custom model created", `${p.name} is ready in Ollama.`);
          // Refresh health so the new model lands in dropdowns.
          void refreshHealth();
        } else if (p.status === "error") {
          notify.error("Model creation failed", p.error ?? "(unknown)");
        }
      }
    }).then((u) => {
      // If the effect already cleaned up before the listen() promise
      // resolved, dispose immediately to avoid a leak.
      if (disposed) u();
      else off = u;
    });
    return () => {
      disposed = true;
      if (off) off();
    };
  }, [open, setCreateProgress, setCreating, refreshHealth]);

  const onClose = () => {
    if (inspecting || creating) return;
    reset();
    setNewName("");
    setExtraSystem("");
    close();
  };

  const pickFile = async () => {
    try {
      const p = await fs.pickOpenFile([
        {
          name: "Training datasets",
          extensions: ["json", "jsonl", "ndjson", "arrow", "feather", "parquet"],
        },
        { name: "All files", extensions: ["*"] },
      ]);
      if (p) {
        await runInspect(p);
      }
    } catch (err) {
      notify.error("Pick file failed", String(err));
    }
  };

  const runInspect = async (path: string) => {
    setInspecting(true);
    setError(null);
    try {
      const out = await datasets.inspect(path);
      setInspect(out);
      // Auto-suggest a model name slugged from the file's basename.
      // (`basename`, not `base`, to avoid shadowing the `base` model-name
      // state declared above.)
      if (!newName) {
        const basename = path.split(/[\\/]/).pop() ?? "";
        const slug = basename
          .replace(/\.(jsonl?|ndjson|arrow|feather|parquet)$/i, "")
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, "-")
          .replace(/^-+|-+$/g, "")
          .slice(0, 40);
        if (slug) setNewName(`kilroy-${slug}`);
      }
    } catch (err) {
      setError(String(err));
      setInspect(null);
    } finally {
      setInspecting(false);
    }
  };

  const canCreate =
    !!inspect &&
    newName.trim().length > 0 &&
    base.trim().length > 0 &&
    !creating;

  const startCreate = async () => {
    if (!inspect || !canCreate) return;
    setCreating(true);
    setCreateProgress(null);
    setLastBuild(null);
    try {
      const built = await datasets.createModelfile({
        name: newName.trim(),
        base: base.trim(),
        dataset_path: inspect.path,
        extra_system: extraSystem.trim() || undefined,
        temperature,
      });
      setLastBuild(built);
    } catch (err) {
      // The progress listener will have already toasted; we just clear
      // the spinner if the Promise rejects without a `done` event.
      setCreating(false);
      notify.error("Create failed", String(err));
    }
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="w-[min(820px,calc(100vw-3rem))] max-h-[88vh] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Database className="h-3.5 w-3.5 text-amber" />
            Datasets &amp; custom models
          </DialogTitle>
          <DialogDescription>
            Load a .json / .jsonl dataset (Alpaca, ShareGPT, OpenAI chat, or
            prompt/completion) and turn it into a custom Ollama model
            without leaving Kilroy. The new model becomes selectable in
            Settings → Chat model.
          </DialogDescription>
        </DialogHeader>

        <div className="flex max-h-[68vh] flex-col gap-4 overflow-y-auto p-4">
          {/* Stage 1: pick a file ───────────────────────────────────── */}
          <Section title="1 · Load a dataset" icon={<Upload className="h-3 w-3" />}>
            <div className="flex flex-col gap-2">
              <Button
                variant="ghost"
                onClick={pickFile}
                disabled={inspecting}
              >
                {inspecting ? (
                  <>
                    <Loader2 className="h-3 w-3 animate-spin" />
                    Inspecting…
                  </>
                ) : (
                  <>
                    <FileJson className="h-3 w-3" />
                    Choose file…
                  </>
                )}
              </Button>
              <p className="text-[10.5px] text-ink-subtle">
                Accepted: <span className="font-mono">.json</span>,{" "}
                <span className="font-mono">.jsonl</span>,{" "}
                <span className="font-mono">.ndjson</span>. Arrow / Parquet
                support requires{" "}
                <span className="font-mono">pip install pyarrow</span> (coming
                next pass).
              </p>
              {error && (
                <p className="rounded-md border border-err/40 bg-err/5 px-2 py-1 text-[11px] text-err">
                  {error}
                </p>
              )}
            </div>
          </Section>

          {/* Stage 2: inspection result ─────────────────────────────── */}
          {inspect && (
            <Section
              title="2 · Inspection"
              icon={<FileQuestion className="h-3 w-3" />}
            >
              <InspectView />
            </Section>
          )}

          {/* Stage 3: create model ──────────────────────────────────── */}
          {inspect && (
            <Section
              title="3 · Create a custom model from this dataset"
              icon={<Sparkles className="h-3 w-3" />}
            >
              <div className="flex flex-col gap-2">
                <Field
                  label="New model tag"
                  hint="Lowercase letters / digits / `-` / `_` / `:` / `.`. Will be available as a chat model after creation."
                >
                  <Input
                    value={newName}
                    onChange={(e) => setNewName(e.target.value.toLowerCase())}
                    placeholder="kilroy-myproject-conventions"
                    className="font-mono text-[12px]"
                    disabled={creating}
                  />
                </Field>
                <Field
                  label="Base model"
                  hint="The Ollama model to extend. Defaults to your current chat model."
                >
                  <Input
                    value={base}
                    onChange={(e) => setBase(e.target.value)}
                    placeholder="qwen2.5-coder:14b-instruct-q8_0"
                    className="font-mono text-[12px]"
                    disabled={creating}
                  />
                </Field>
                <Field
                  label="Extra system instructions (optional)"
                  hint="Prepended to the dataset-derived system prompt. Use this to steer the persona / scope beyond what the data alone implies."
                >
                  <Textarea
                    value={extraSystem}
                    onChange={(e) => setExtraSystem(e.target.value)}
                    rows={3}
                    placeholder="You are a senior engineer focused on Rust + Tauri. Be terse."
                    disabled={creating}
                  />
                </Field>
                <Field
                  label={`Temperature: ${temperature.toFixed(2)}`}
                  hint="Lower = more faithful to the dataset style. 0.4 is a good default for code/convention emulation."
                >
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    value={temperature}
                    onChange={(e) => setTemperature(Number(e.target.value))}
                    className="w-full accent-amber"
                    disabled={creating}
                  />
                </Field>

                {createProgress && (
                  <CreateProgressStrip progress={createProgress} />
                )}
                {lastBuild && (
                  <div className="rounded-md border border-ok/40 bg-ok/5 p-2 text-[11px] text-ok">
                    <p className="font-medium">Created · {lastBuild.name}</p>
                    <p className="mt-1 text-ink-muted">
                      Modelfile saved to{" "}
                      <span className="font-mono break-all">
                        {lastBuild.modelfile_path}
                      </span>
                    </p>
                    <p className="mt-1 text-ink-muted">
                      Open Settings → Chat model to switch to it.
                    </p>
                  </div>
                )}

                <Button
                  onClick={startCreate}
                  disabled={!canCreate}
                  className="self-start"
                >
                  {creating ? (
                    <>
                      <Loader2 className="h-3 w-3 animate-spin" />
                      Creating model…
                    </>
                  ) : (
                    <>
                      <Sparkles className="h-3 w-3" />
                      Create model
                    </>
                  )}
                </Button>
              </div>
            </Section>
          )}

          {/* Stage 4 (info only): LoRA training env probe ───────────── */}
          <Section
            title="Optional · LoRA fine-tuning environment"
            icon={<Database className="h-3 w-3" />}
          >
            <TrainingEnvCard env={trainingEnv} />
          </Section>
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={onClose} disabled={inspecting || creating}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Section({
  title,
  icon,
  children,
}: {
  title: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-2 rounded-md border border-line bg-bg-1 p-3">
      <div className="flex items-center gap-2 text-[11px] uppercase tracking-wider text-ink-subtle">
        {icon}
        {title}
      </div>
      {children}
    </div>
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

function InspectView() {
  const inspect = useDatasets((s) => s.inspect)!;
  return (
    <div className="flex flex-col gap-2 text-[12px]">
      <div className="grid grid-cols-2 gap-2">
        <Stat label="Format" value={inspect.format} />
        <Stat label="Records" value={inspect.record_count.toLocaleString()} />
        <Stat
          label="Size on disk"
          value={`${(inspect.size_bytes / 1024 / 1024).toFixed(2)} MB`}
        />
        <Stat
          label="Avg input"
          value={inspect.avg_input_chars > 0 ? `${inspect.avg_input_chars} chars` : "—"}
        />
        <Stat
          label="Avg output"
          value={inspect.avg_output_chars > 0 ? `${inspect.avg_output_chars} chars` : "—"}
        />
        <Stat label="Container" value={inspect.container} />
      </div>
      {inspect.notes.length > 0 && (
        <ul className="rounded-md border border-warn/40 bg-warn/5 p-2 text-[11px] text-warn">
          {inspect.notes.map((n, i) => (
            <li key={i}>· {n}</li>
          ))}
        </ul>
      )}
      <details className="rounded-md border border-line bg-bg-0">
        <summary className="cursor-pointer px-2 py-1 text-[11px] text-ink-muted hover:text-ink">
          Sample records ({inspect.samples.length})
        </summary>
        <div className="flex flex-col gap-2 p-2">
          {inspect.samples.map((s, i) => (
            <pre
              key={i}
              className="max-h-48 overflow-auto rounded-md bg-bg-1 p-2 font-mono text-[10.5px] text-ink-muted"
            >
              {s}
            </pre>
          ))}
        </div>
      </details>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-line bg-bg-0 px-2 py-1.5">
      <div className="text-[10px] uppercase tracking-wider text-ink-subtle">{label}</div>
      <div className="font-mono text-[12px] text-ink">{value}</div>
    </div>
  );
}

function CreateProgressStrip({ progress }: { progress: CreateProgress }) {
  const isErr = progress.status === "error";
  const isOk = progress.done && progress.status === "success";
  return (
    <div
      className={cn(
        "rounded-md border px-2 py-1.5 text-[11px]",
        isErr
          ? "border-err/40 bg-err/5 text-err"
          : isOk
            ? "border-ok/40 bg-ok/5 text-ok"
            : "border-line bg-bg-1 text-ink-muted",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate font-mono">{progress.name}</span>
        <span className="shrink-0 uppercase tracking-wider text-[9.5px]">
          {progress.status}
        </span>
      </div>
      {progress.error && (
        <div className="mt-1 text-[10.5px] text-err">{progress.error}</div>
      )}
    </div>
  );
}

function TrainingEnvCard({ env }: { env: ReturnType<typeof useDatasets.getState>["trainingEnv"] }) {
  if (!env) {
    return (
      <p className="text-[11px] text-ink-subtle">Probing training environment…</p>
    );
  }
  const checks: Array<{ label: string; ok: boolean; detail?: string }> = [
    {
      label: "Python",
      ok: env.python_available,
      detail: env.python_version ?? undefined,
    },
    { label: "transformers", ok: env.transformers_installed },
    { label: "unsloth", ok: env.unsloth_installed },
    { label: "NVIDIA GPU visible", ok: env.gpu_visible },
  ];
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap gap-1.5">
        {checks.map((c) => (
          <span
            key={c.label}
            className={cn(
              "rounded-md border px-1.5 py-0.5 text-[10.5px]",
              c.ok
                ? "border-ok/40 bg-ok/5 text-ok"
                : "border-line bg-bg-1 text-ink-subtle",
            )}
          >
            {c.ok ? "✓" : "·"} {c.label}
            {c.detail ? ` · ${c.detail}` : ""}
          </span>
        ))}
      </div>
      <p className="text-[10.5px] text-ink-subtle">{env.hint}</p>
      <p className="text-[10.5px] text-ink-subtle">
        LoRA training itself is scaffolded but not yet wired to a button —
        for now use the Modelfile composition above (works on every Ollama
        install, no Python needed).
      </p>
    </div>
  );
}
