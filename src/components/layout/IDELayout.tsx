/**
 * IDE shell layout — the nested split structure.
 *
 * Outer (PanelGroup horizontal):
 *   [LEFT_PLUS_CENTER, grows]  [RIGHT_CHAT, resizable]
 *
 * Inner vertical (PanelGroup):
 *   [TOP_ROW]
 *   [TERMINAL, collapsible]
 *
 * TOP_ROW (PanelGroup horizontal):
 *   [FILE_EXPLORER + AGENT_MODE]  [EDITOR]
 *
 * Arrangement is fixed — explorer left, editor center, terminal bottom,
 * chat right. Explorer, editor, terminal, and chat are all resizable.
 *
 * When the terminal collapses, the TOP_ROW panel claims the full inner
 * height — which naturally extends the File Explorer to the window
 * bottom.
 */
import { useEffect, useLayoutEffect, useRef } from "react";
import {
  Group,
  Panel,
  Separator,
  usePanelRef,
} from "react-resizable-panels";
import { FileExplorer } from "@/components/explorer/FileExplorer";
import { AgentModeSelector } from "@/components/explorer/AgentModeSelector";
import { MonacoPane } from "@/components/editor/MonacoPane";
import { TerminalPanel } from "@/components/terminal/TerminalPanel";
import { AgentHub } from "@/components/chat/AgentHub";
import { useUI } from "@/store/ui";

export function IDELayout() {
  const leftCollapsed = useUI((s) => s.leftCollapsed);
  const terminalCollapsed = useUI((s) => s.terminalCollapsed);
  const setTerminalSize = useUI((s) => s.setTerminalSize);
  const terminalSize = useUI((s) => s.terminalSize);

  // Imperative handles let us drive collapse from the UI store rather than
  // relying on react-resizable-panels' internal collapse semantics, which
  // are awkward for our "claim full vertical space" requirement.
  const leftPanelRef = usePanelRef();
  const terminalPanelRef = usePanelRef();
  const chatPanelRef = usePanelRef();
  const startupWidthSyncedRef = useRef(false);

  // Preserve the existing nested layout. Once both horizontal groups have
  // completed their initial layout, resize only Explorer to Chat's rendered
  // pixel width. The panels remain independent after this one-time sync.
  useLayoutEffect(() => {
    if (leftCollapsed || startupWidthSyncedRef.current) return;

    const frame = window.requestAnimationFrame(() => {
      const explorer = leftPanelRef.current;
      const chat = chatPanelRef.current;
      if (!explorer || !chat) return;

      explorer.resize(`${chat.getSize().inPixels}px`);
      startupWidthSyncedRef.current = true;
    });

    return () => window.cancelAnimationFrame(frame);
  }, [leftCollapsed]);

  useEffect(() => {
    const p = leftPanelRef?.current;
    if (!p) return;
    if (leftCollapsed && !p.isCollapsed()) p.collapse();
    if (!leftCollapsed && p.isCollapsed()) p.expand();
  }, [leftCollapsed]);

  useEffect(() => {
    const p = terminalPanelRef.current;
    if (!p) return;
    if (terminalCollapsed && !p.isCollapsed()) p.collapse();
    if (!terminalCollapsed && p.isCollapsed()) p.expand();
  }, [terminalCollapsed]);

  return (
    <Group
      orientation="horizontal"
      id="kilroy.outer"
      className="flex min-h-0 flex-1"
    >
      {/* LEFT_PLUS_CENTER — explorer + editor + terminal. */}
      <Panel defaultSize={74} minSize={20}>
        <div className="flex h-full min-h-0 min-w-0 flex-col">
          <Group
            orientation="vertical"
            id="kilroy.vertical"
            className="flex-1"
          >
            {/* TOP_ROW — explorer + editor */}
            <Panel defaultSize={100 - terminalSize} minSize={8}>
              <Group
                orientation="horizontal"
                id="kilroy.horizontal"
                className="h-full"
              >
                <Panel
                  panelRef={leftPanelRef}
                  minSize={5}
                  collapsible
                  collapsedSize={0}
                >
                  <div className="flex h-full min-h-0 flex-col bg-bg-1">
                    <div className="min-h-0 flex-1">
                      <FileExplorer />
                    </div>
                    <AgentModeSelector />
                  </div>
                </Panel>
                <Separator className="pgh-vertical" />
                <Panel minSize={8}>
                  <div className="h-full bg-bg-0">
                    <MonacoPane />
                  </div>
                </Panel>
              </Group>
            </Panel>

            {/*
              TERMINAL — collapsible. When collapsed, the panel above
              (TOP_ROW) claims the full vertical space, which extends the
              File Explorer all the way to the bottom of the window.
            */}
            <Separator
              className={terminalCollapsed ? "h-0 overflow-hidden" : "pgh-horizontal"}
              disabled={terminalCollapsed}
            />
            <Panel
              panelRef={terminalPanelRef}
              defaultSize={terminalSize}
              minSize={6}
              collapsible
              collapsedSize={0}
              onResize={(panelSize) => {
                if (panelSize.asPercentage > 0) setTerminalSize(panelSize.asPercentage);
              }}
            >
              <div className="h-full bg-bg-1">
                <TerminalPanel />
              </div>
            </Panel>
          </Group>
        </div>
      </Panel>
      <Separator className="pgh-vertical" />
      <Panel panelRef={chatPanelRef} defaultSize={26} minSize={12}>
        <div className="h-full border-l border-line bg-bg-1">
          <AgentHub />
        </div>
      </Panel>
    </Group>
  );
}
