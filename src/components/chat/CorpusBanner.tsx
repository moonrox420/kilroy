/**
 * Distillation corpus banner — sits above the chat scroller when the
 * project has accumulated blessed exchanges.
 *
 * Three states:
 *   * Hidden — no project open, OR corpus.count === 0
 *   * Growing — 0 < count < train_threshold ("9 / 20 saved exchanges")
 *   * Ready — count >= train_threshold ("Train a custom model now →")
 *
 * Clicking "Train" opens the Datasets dialog, which (in a follow-up
 * pass) will accept an optional pre-loaded corpus path so the user
 * skips the file-picker step. For now it just opens the dialog and the
 * user picks `.kilroy/corpus/training.jsonl` themselves — the path is
 * surfaced via `corpus.openFolder()`.
 */
import { Sparkles, FolderOpen } from "lucide-react";
import { useCorpus } from "@/store/corpus";
import { useDatasets } from "@/store/datasets";
import { corpus as corpusApi } from "@/lib/tauri";
import { notify } from "@/store/notifications";
import { cn } from "@/lib/utils";

export function CorpusBanner() {
  const stats = useCorpus((s) => s.stats);
  const openDatasetsWithPath = useDatasets((s) => s.openDialogWithPath);

  if (!stats || !stats.path || stats.count === 0) return null;

  const ready = stats.count >= stats.train_threshold;

  return (
    <div
      className={cn(
        "mx-3 mt-2 flex items-center justify-between gap-2 rounded-md border px-2.5 py-1.5 text-[11px]",
        ready
          ? "border-ok/50 bg-ok/5 text-ok"
          : "border-line bg-bg-1 text-ink-muted",
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <Sparkles className={cn("h-3 w-3 shrink-0", ready ? "text-ok" : "text-amber")} />
        <span className="truncate">
          {ready ? (
            <>
              <span className="font-medium">{stats.count}</span> blessed exchanges saved —
              ready to train a custom model.
            </>
          ) : (
            <>
              Distillation corpus: <span className="font-medium">{stats.count}</span> /{" "}
              {stats.train_threshold} blessed exchanges. Keep saving 👍 to unlock training.
            </>
          )}
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <button
          type="button"
          onClick={() => {
            corpusApi
              .openFolder()
              .catch((err) => notify.error("Open folder failed", String(err)));
          }}
          title="Open the corpus folder in Explorer"
          className="rounded-md p-1 hover:bg-bg-2"
        >
          <FolderOpen className="h-3 w-3" />
        </button>
        {ready && (
          <button
            type="button"
            onClick={() => openDatasetsWithPath(stats.path)}
            className="rounded-md border border-ok/40 bg-ok/10 px-2 py-0.5 text-[10.5px] font-medium text-ok hover:bg-ok/15"
          >
            Train custom model →
          </button>
        )}
      </div>
    </div>
  );
}
