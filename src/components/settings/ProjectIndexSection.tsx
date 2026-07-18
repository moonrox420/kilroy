/**
 * Settings → Memory → Project index — stats + isolated clear control.
 *
 * Kept separate from the chat "Index Project" banner so a mis-click cannot
 * wipe embeddings. Clearing requires typing CLEAR in a confirm dialog.
 */
import { useCallback, useEffect, useState } from "react";
import { Loader2, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { memory, type ProjectIndexStatus } from "@/lib/tauri";
import { useMemory } from "@/store/memory";

interface Props {
  settingsOpen: boolean;
}

export function ProjectIndexSection({ settingsOpen }: Props) {
  const project = useMemory((s) => s.project);
  const indexing = useMemory((s) => s.indexing);
  const lastIndex = useMemory((s) => s.lastIndex);
  const clearIndex = useMemory((s) => s.clearIndex);

  const [status, setStatus] = useState<ProjectIndexStatus | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [confirmText, setConfirmText] = useState("");
  const [clearing, setClearing] = useState(false);

  const refreshStatus = useCallback(() => {
    if (!project) {
      setStatus(null);
      return;
    }
    void memory
      .projectIndexStatus()
      .then(setStatus)
      .catch((err) => console.error("project_index_status:", err));
  }, [project]);

  useEffect(() => {
    if (!settingsOpen) return;
    refreshStatus();
  }, [settingsOpen, project?.id, lastIndex, indexing, refreshStatus]);

  const rootPath = project?.root_path ?? null;
  const hasIndex = (status?.chunks_indexed ?? 0) > 0;
  const canClear = !!project && hasIndex && !indexing && !clearing;

  async function onConfirmClear() {
    if (confirmText !== "CLEAR") return;
    setClearing(true);
    try {
      const result = await clearIndex();
      if (result) {
        setConfirmOpen(false);
        setConfirmText("");
        refreshStatus();
      }
    } finally {
      setClearing(false);
    }
  }

  return (
    <>
      <section className="mb-5 flex flex-col gap-2 border-t border-line pt-5">
        <div>
          <h3 className="text-[12px] font-semibold text-ink">Project index</h3>
          <p className="text-[11px] text-ink-subtle">
            Semantic search chunks for the currently open folder. Clear here if you indexed the
            wrong root (e.g. your whole user profile) and want to re-index one project at a time.
          </p>
        </div>

        {!project ? (
          <p className="text-[11px] text-ink-muted">Open a project folder first.</p>
        ) : (
          <>
            <p className="font-mono text-[10.5px] text-ink-muted break-all">{rootPath}</p>
            <p className="text-[11px] text-ink">
              {status
                ? `${status.files_indexed} files · ${status.chunks_indexed} chunks`
                : "—"}
            </p>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="w-fit border-err/40 text-err hover:bg-err/10"
              disabled={!canClear}
              onClick={() => {
                setConfirmText("");
                setConfirmOpen(true);
              }}
            >
              <Trash2 className="h-3 w-3" />
              Clear project index
            </Button>
            {!hasIndex && project && (
              <p className="text-[10.5px] text-ink-subtle">Nothing indexed for this folder yet.</p>
            )}
          </>
        )}
      </section>

      <Dialog
        open={confirmOpen}
        onOpenChange={(v) => {
          if (!clearing) {
            setConfirmOpen(v);
            if (!v) setConfirmText("");
          }
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Clear project index?</DialogTitle>
            <DialogDescription>
              Removes all indexed file chunks and embeddings for{" "}
              <span className="font-mono text-ink">{rootPath ?? "this folder"}</span>. Chat
              history is kept. You can re-index from Agent Chat afterward.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-1.5 py-1">
            <label className="text-[11px] text-ink-muted" htmlFor="clear-confirm">
              Type <span className="font-mono font-semibold text-ink">CLEAR</span> to confirm
            </label>
            <Input
              id="clear-confirm"
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              placeholder="CLEAR"
              autoComplete="off"
              disabled={clearing}
            />
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setConfirmOpen(false)}
              disabled={clearing}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={confirmText !== "CLEAR" || clearing}
              onClick={() => void onConfirmClear()}
            >
              {clearing ? (
                <>
                  <Loader2 className="h-3 w-3 animate-spin" />
                  Clearing…
                </>
              ) : (
                "Clear index"
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}