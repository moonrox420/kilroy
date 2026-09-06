/**
 * Left-pane File Explorer.
 *
 * Empty state offers a single CTA to open a folder. When a root is set,
 * we render the recursive FileTree. Header includes the folder name
 * with an open-folder button.
 */
import { FolderOpen, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useWorkspace } from "@/store/workspace";
import { FileTree } from "./FileTree";
import { useState } from "react";

export function FileExplorer() {
  const rootPath = useWorkspace((s) => s.rootPath);
  const openFolder = useWorkspace((s) => s.openFolder);
  const [version, setVersion] = useState(0);

  return (
    <div className="flex h-full flex-col">
      <div className="panel-header">
        <span>Explorer</span>
        <div className="flex items-center gap-0.5">
          <Button
            variant="ghost"
            size="icon"
            title="Refresh"
            onClick={() => setVersion((v) => v + 1)}
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            title="Open folder"
            onClick={() => openFolder()}
          >
            <FolderOpen className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
      <div className="panel-body">
        {rootPath ? (
          <FileTree key={`${rootPath}-${version}`} rootPath={rootPath} />
        ) : (
          <EmptyState onOpen={() => openFolder()} />
        )}
      </div>
    </div>
  );
}

function EmptyState({ onOpen }: { onOpen: () => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-4 text-center">
      <p className="text-[12px] text-ink-muted">
        No folder open.
      </p>
      <Button onClick={onOpen}>
        <FolderOpen className="h-3.5 w-3.5" />
        Open Folder
      </Button>
      <p className="text-[11px] text-ink-ghost leading-snug">
        Or press <kbd className="rounded bg-bg-2 px-1 py-0.5 font-mono text-[10px] text-ink">Ctrl+O</kbd>
      </p>
    </div>
  );
}
