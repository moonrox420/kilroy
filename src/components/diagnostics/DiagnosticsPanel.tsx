/**
 * Live diagnostics panel.
 *
 * A floating modal accessible from the status bar (or Ctrl+Shift+D)
 * that shows runtime health information useful for debugging "why isn't
 * this working" without opening DevTools:
 *
 *   - Ollama health (URL, reachable, models installed, chat / embedding
 *     model presence)
 *   - PTY count + per-session metadata
 *   - Project DB connection (memory state)
 *   - Recent IPC errors (replays the last N toasts of kind=error)
 *   - App info (name, version, commit)
 *
 * Updates live: re-polls Ollama on mount and on a 5s timer while open.
 */
import { useEffect, useMemo, useState } from "react";
import {
  TriangleAlert,
  Box,
  Cpu,
  Database,
  RefreshCw,
  Terminal as TermIcon,
  X,
  Zap,
  ZapOff,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useSettings } from "@/store/settings";
import { useTerminals } from "@/store/terminals";
import { useMemory } from "@/store/memory";
import { useNotifications } from "@/store/notifications";
import { app as appApi, type AppInfo } from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function DiagnosticsPanel({ open, onClose }: Props) {
  const checkOllama = useSettings((s) => s.checkOllama);
  const health = useSettings((s) => s.health);
  const settingsCurrent = useSettings((s) => s.current);
  const sessions = useTerminals((s) => s.sessions);
  const activeId = useTerminals((s) => s.activeId);
  const project = useMemory((s) => s.project);
  const session = useMemory((s) => s.session);
  const toasts = useNotifications((s) => s.toasts);

  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    if (!open) return;
    void appApi.info().then(setAppInfo).catch(() => undefined);
    void checkOllama();
    const id = setInterval(() => void checkOllama(), 5000);
    return () => clearInterval(id);
  }, [open, checkOllama]);

  const refresh = async () => {
    setRefreshing(true);
    try {
      await checkOllama();
      setAppInfo(await appApi.info().catch(() => null));
    } finally {
      setRefreshing(false);
    }
  };

  const recentErrors = useMemo(
    () => toasts.filter((t) => t.kind === "error").slice(-5).reverse(),
    [toasts],
  );

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-[680px] gap-0 p-0">
        <header className="flex items-center gap-3 border-b border-line px-4 py-3">
          <Cpu className="h-4 w-4 text-amber" />
          <div className="min-w-0 flex-1">
            <DialogTitle className="text-[13px] font-semibold tracking-tight text-ink">
              Diagnostics
            </DialogTitle>
            <DialogDescription className="text-[11px] text-ink-muted">
              Live runtime health. Polls every 5s while open.
            </DialogDescription>
          </div>
          <Button variant="ghost" size="sm" onClick={refresh} disabled={refreshing}>
            <RefreshCw className={cn("h-3 w-3", refreshing && "animate-spin")} />
            Refresh
          </Button>
          <button
            onClick={onClose}
            className="rounded-sm p-0.5 text-ink-subtle hover:bg-bg-2 hover:text-ink"
            aria-label="Close"
          >
            <X className="h-3 w-3" />
          </button>
        </header>

        <div className="max-h-[60vh] overflow-y-auto px-4 py-3 text-[12px]">
          {/* Ollama */}
          <Section title="Ollama">
            <Row
              icon={
                health?.reachable ? (
                  <Zap className="h-3 w-3 text-ok" />
                ) : (
                  <ZapOff className="h-3 w-3 text-err" />
                )
              }
              label="Daemon"
              value={
                health
                  ? health.reachable
                    ? `reachable · ${health.models.length} model(s)`
                    : "unreachable"
                  : "checking…"
              }
              detail={settingsCurrent?.ollama_url ?? "?"}
            />
            <Row
              icon={
                health?.has_chat_model ? (
                  <Zap className="h-3 w-3 text-ok" />
                ) : (
                  <TriangleAlert className="h-3 w-3 text-warn" />
                )
              }
              label="Chat model"
              value={
                health
                  ? health.has_chat_model
                    ? "installed"
                    : "missing"
                  : "?"
              }
              detail={settingsCurrent?.chat_model ?? "?"}
            />
            <Row
              icon={
                health?.has_embedding_model ? (
                  <Zap className="h-3 w-3 text-ok" />
                ) : (
                  <TriangleAlert className="h-3 w-3 text-warn" />
                )
              }
              label="Embedding model"
              value={
                health
                  ? health.has_embedding_model
                    ? "installed"
                    : "missing"
                  : "?"
              }
              detail={settingsCurrent?.embedding_model ?? "?"}
            />
            {health?.models && health.models.length > 0 && (
              <details className="mt-1.5 rounded-sm border border-line bg-bg-0 px-2 py-1">
                <summary className="cursor-pointer text-[10px] uppercase tracking-wider text-ink-subtle">
                  All installed models ({health.models.length})
                </summary>
                <div className="mt-1 grid grid-cols-2 gap-x-3 gap-y-0.5 font-mono text-[11px] text-ink">
                  {health.models.map((m) => (
                    <span key={m} className="truncate">
                      {m}
                    </span>
                  ))}
                </div>
              </details>
            )}
            {health?.error && (
              <p className="mt-1 rounded-sm border border-err/40 bg-err/5 px-2 py-1 text-[11px] text-err">
                {health.error}
              </p>
            )}
          </Section>

          {/* PTY sessions */}
          <Section title="Terminal sessions">
            <Row
              icon={<TermIcon className="h-3 w-3 text-ink-subtle" />}
              label="Active"
              value={
                sessions.length === 0
                  ? "none"
                  : `${sessions.length} session(s)`
              }
              detail={activeId ? `active: ${activeId.slice(0, 8)}` : ""}
            />
            {sessions.length > 0 && (
              <div className="ml-5 mt-1 space-y-0.5 font-mono text-[11px] text-ink-muted">
                {sessions.map((s) => (
                  <div key={s.id} className="flex items-center gap-2">
                    <span
                      className={cn(
                        "h-1.5 w-1.5 rounded-full",
                        s.exited
                          ? "bg-err"
                          : s.id === activeId
                            ? "bg-amber"
                            : "bg-ok",
                      )}
                    />
                    <span className="truncate text-ink">{s.label}</span>
                    {s.exited && (
                      <span className="text-[10px] uppercase text-err">exited</span>
                    )}
                    <span className="ml-auto text-ink-subtle">
                      {s.id.slice(0, 8)}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </Section>

          {/* Memory / project */}
          <Section title="Project memory">
            <Row
              icon={<Database className="h-3 w-3 text-ink-subtle" />}
              label="Project"
              value={project ? project.name : "no project open"}
              detail={project ? project.root_path : "(open a folder)"}
            />
            <Row
              icon={<Database className="h-3 w-3 text-ink-subtle" />}
              label="Session"
              value={session ? `session #${session.id}` : "—"}
              detail={
                session
                  ? `started ${new Date(session.started_at * 1000).toLocaleString()}`
                  : ""
              }
            />
          </Section>

          {/* App info */}
          <Section title="App">
            <Row
              icon={<Box className="h-3 w-3 text-ink-subtle" />}
              label="Version"
              value={appInfo?.version ?? "?"}
              detail={appInfo?.commit ?? ""}
            />
            <Row
              icon={<Box className="h-3 w-3 text-ink-subtle" />}
              label="Name"
              value={appInfo?.name ?? "?"}
              detail=""
            />
          </Section>

          {/* Recent errors */}
          {recentErrors.length > 0 && (
            <Section title="Recent errors">
              <div className="space-y-1">
                {recentErrors.map((t) => (
                  <div
                    key={t.id}
                    className="rounded-sm border border-err/40 bg-err/5 px-2 py-1"
                  >
                    <p className="text-[12px] text-ink">{t.title}</p>
                    {t.detail && (
                      <p className="mt-0.5 break-words font-mono text-[10px] text-ink-muted">
                        {t.detail}
                      </p>
                    )}
                    <p className="mt-0.5 text-[10px] text-ink-subtle">
                      {new Date(t.createdAt).toLocaleTimeString()}
                    </p>
                  </div>
                ))}
              </div>
            </Section>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mb-3">
      <h3 className="mb-1 text-[10px] uppercase tracking-wider text-ink-subtle">
        {title}
      </h3>
      <div className="space-y-1">{children}</div>
    </div>
  );
}

function Row({
  icon,
  label,
  value,
  detail,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="flex h-3 w-3 shrink-0 items-center justify-center self-center">
        {icon}
      </span>
      <span className="w-[120px] shrink-0 text-ink">{label}</span>
      <span className="text-ink-muted">{value}</span>
      {detail && (
        <span className="ml-auto truncate font-mono text-[10px] text-ink-subtle">
          {detail}
        </span>
      )}
    </div>
  );
}
