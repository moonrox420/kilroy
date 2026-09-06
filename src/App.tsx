/**
 * Root of the React tree.
 */
import { lazy, Suspense, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { createListenerScope } from "@/lib/listenerScope";
import { notify } from "@/store/notifications";
import { TooltipProvider } from "@/components/ui/tooltip";
import { TitleBar } from "@/components/layout/TitleBar";
import { MenuBar } from "@/components/layout/MenuBar";
import { StatusBar } from "@/components/layout/StatusBar";
import { IDELayout } from "@/components/layout/IDELayout";
import { MemoryPanel } from "@/components/memory/MemoryPanel";
import { DecisionComposer } from "@/components/memory/DecisionComposer";
import { SkillCreator } from "@/components/memory/SkillCreator";
import { DatasetsDialog } from "@/components/datasets/DatasetsDialog";
import { RefactorPanel } from "@/components/refactor/RefactorPanel";
import { PlanEditor } from "@/components/plan/PlanEditor";
import { CommandPalette } from "@/components/palette/CommandPalette";
import { Toaster } from "@/components/notifications/Toaster";
import { FirstRunWizard } from "@/components/setup/FirstRunWizard";
import { useRuntime } from "@/store/runtime";
import { useActions } from "@/store/actions";
import { useSettings } from "@/store/settings";
import { usePalette } from "@/store/palette";
import { useCouncil } from "@/store/council";
import { useRefactor } from "@/store/refactor";
import { usePlatform } from "@/store/platform";
import { useWorkspace } from "@/store/workspace";

const ActivityFeed = lazy(() => import("@/components/activity/ActivityFeed").then((module) => ({ default: module.ActivityFeed })));
const SettingsDialog = lazy(() => import("@/components/settings/SettingsDialog").then((module) => ({ default: module.SettingsDialog })));
const DiagnosticsPanel = lazy(() => import("@/components/diagnostics/DiagnosticsPanel").then((module) => ({ default: module.DiagnosticsPanel })));

interface AgentEditorOpenEvent {
  run_id: string;
  path: string;
  line?: number;
  reason: string;
}

interface AgentEditorPreviewEvent {
  run_id: string;
  action_id: number;
  path: string;
  contents: string;
  diff?: string;
}

interface ActionResolvedEvent {
  action_id: number;
  status: string;
  error?: string;
}

export default function App() {
  // Mount runtime + actuator + council + refactor listeners once.
  useEffect(() => {
    const offRuntime = useRuntime.getState().initListeners();
    const offActions = useActions.getState().initListeners();
    const offCouncil = useCouncil.getState().initListeners();
    const offRefactor = useRefactor.getState().initListeners();
    const agentEditorListeners = createListenerScope((error) => notify.fromError("Editor subscriptions", error));
    [
      listen<AgentEditorOpenEvent>("agent://editor/open", ({ payload }) => {
        void useWorkspace.getState().openFile(payload.path);
      }),
      listen<AgentEditorPreviewEvent>(
        "agent://editor/preview",
        ({ payload }) => {
          void useWorkspace.getState().showAgentPreview({
            actionId: payload.action_id,
            path: payload.path,
            contents: payload.contents,
            diff: payload.diff,
          });
        },
      ),
      listen<ActionResolvedEvent>("actuator://action_resolved", ({ payload }) => {
        void useWorkspace
          .getState()
          .resolveAgentPreview(payload.action_id, payload.status);
      }),
      listen<import("@/lib/tauri").CompilerCheckResult>(
        "agent://compiler/check",
        ({ payload }) => {
          if (payload.success) {
            notify.success(
              "Compiler Check Passed",
              `${payload.command} (${payload.duration_ms}ms)`,
            );
          } else {
            notify.warn(
              "Compiler Error Detected",
              `${payload.command} failed — Auto-Repair proposed`,
            );
          }
        },
      ),
    ].forEach((registration) => { void registration.then(agentEditorListeners.add).catch(agentEditorListeners.report); });

    // Detect the host OS once so the UI can render platform-appropriate
    // options (sandbox kinds, keyboard hints, shell labels).
    void usePlatform.getState().load();
    // Warm the settings cache + Ollama health so the status bar and other
    // passive consumers have something to render before the user clicks anywhere.
    void useSettings
      .getState()
      .load()
      .then(() => useSettings.getState().checkOllama());
    // Per-session PTY byte-cap toasts are wired inside the terminal bridge.
    return () => {
      agentEditorListeners.dispose();
      offRuntime();
      offActions();
      offCouncil();
      offRefactor();
    };
  }, []);

  const [activityOpen, setActivityOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);

  // Ctrl+, toggles Settings, Ctrl+Shift+P toggles the Command Palette,
  // Ctrl+Shift+D toggles the Diagnostics panel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const isCtrl = e.ctrlKey || e.metaKey;
      if (isCtrl && e.key === ",") {
        e.preventDefault();
        setSettingsOpen((v) => !v);
      } else if (isCtrl && e.shiftKey && e.key.toLowerCase() === "p") {
        e.preventDefault();
        usePalette.getState().toggle();
      } else if (isCtrl && e.shiftKey && e.key.toLowerCase() === "d") {
        e.preventDefault();
        setDiagnosticsOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <TooltipProvider delayDuration={400} skipDelayDuration={200}>
      <div className="flex h-screen w-screen flex-col bg-bg-0 text-ink">
        <TitleBar />
        <MenuBar
          onOpenActivity={() => setActivityOpen(true)}
          onOpenSettings={() => setSettingsOpen(true)}
        />
        <main className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <IDELayout />
        </main>
        <StatusBar
          onOpenActivity={() => setActivityOpen(true)}
          onOpenSettings={() => setSettingsOpen(true)}
          onOpenDiagnostics={() => setDiagnosticsOpen(true)}
        />
      </div>
      <MemoryPanel />
      <DecisionComposer />
      <SkillCreator />
      <DatasetsDialog />
      <RefactorPanel />
      <PlanEditor />
      <Suspense fallback={<div role="status" className="fixed bottom-10 right-4 rounded bg-bg-2 p-2 text-sm">Opening panel…</div>}>
      {activityOpen && <ActivityFeed open={activityOpen} onClose={() => setActivityOpen(false)} />}
      {settingsOpen && <SettingsDialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
      />}
      </Suspense>
      <CommandPalette
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenActivity={() => setActivityOpen(true)}
      />
      {/* Global toast surface — replaces console.error as the primary
          IPC-failure UX. Stays mounted always; renders nothing when the
          queue is empty. */}
      <Toaster />
      {/* First-run setup wizard — opens automatically when settings
          report first_run: true, walks the user through Ollama check
          and project picker, then flips first_run to false. */}
      <FirstRunWizard />
      {/* Diagnostics panel — Ctrl+Shift+D. Live Ollama health, PTY
          sessions, project DB status, recent IPC errors. */}
      <Suspense fallback={null}>
      {diagnosticsOpen && <DiagnosticsPanel
        open={diagnosticsOpen}
        onClose={() => setDiagnosticsOpen(false)}
      />}
      </Suspense>
    </TooltipProvider>
  );
}
