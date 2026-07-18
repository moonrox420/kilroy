/**
 * 👍 button on agent message bubbles — captures the (user, agent)
 * exchange into the project's distillation corpus.
 *
 * Stays subtle until hovered: a small ghost button under the timestamp.
 * Once clicked, flips to a "saved" check state and disables — the
 * caller's `savedIds` set in `useCorpus` survives further re-renders.
 *
 * `priorUserMessage` is the immediately-preceding user turn from the
 * messages array; ChatMessages computes it during iteration and hands
 * it in so this component doesn't have to grep the chat history itself.
 */
import { useState } from "react";
import { ThumbsUp, Check, Loader2 } from "lucide-react";
import { corpus } from "@/lib/tauri";
import { useCorpus } from "@/store/corpus";
import { notify } from "@/store/notifications";
import { cn } from "@/lib/utils";

interface Props {
  messageId: string;
  /** The immediately-preceding user turn's content. Optional because
   *  the very first agent message has no preceding user turn (boot
   *  banner case) — render nothing in that case. */
  priorUserMessage: string | null;
  /** The agent reply content itself. */
  agentMessage: string;
}

export function FeedbackButton({
  messageId,
  priorUserMessage,
  agentMessage,
}: Props) {
  const saved = useCorpus((s) => s.savedIds.has(messageId));
  const stats = useCorpus((s) => s.stats);
  const markSaved = useCorpus((s) => s.markSaved);
  const [busy, setBusy] = useState(false);

  // Don't render at all if there's nothing to save (no prior user turn,
  // e.g. the boot banner or a system message slipping through).
  if (!priorUserMessage) return null;
  // Also no-op when no project is open — corpus is per-project. The
  // stats endpoint returns `path: ""` in that case.
  if (stats && !stats.path) return null;

  const click = async () => {
    if (busy || saved) return;
    setBusy(true);
    try {
      const next = await corpus.append({
        user_message: priorUserMessage,
        agent_message: agentMessage,
      });
      markSaved(messageId, next);
      // Quiet confirmation — full toast would be noise for a one-click
      // affirmation users will do dozens of times in a session. The
      // banner / counter does the heavier nudging.
      if (next.count === next.train_threshold) {
        notify.success(
          "Corpus ready to train",
          `You hit ${next.train_threshold} blessed exchanges. Train a custom model from Agent → Train Custom Model.`,
        );
      }
    } catch (err) {
      notify.error("Save to corpus failed", String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      type="button"
      onClick={click}
      disabled={busy || saved}
      title={
        saved
          ? "Saved to project distillation corpus"
          : "Save this exchange for fine-tuning a custom model later"
      }
      className={cn(
        "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px]",
        "transition-colors",
        saved
          ? "border border-ok/40 bg-ok/5 text-ok cursor-default"
          : "border border-line bg-bg-1 text-ink-subtle hover:bg-bg-2 hover:text-ink",
      )}
    >
      {busy ? (
        <Loader2 className="h-2.5 w-2.5 animate-spin" />
      ) : saved ? (
        <Check className="h-2.5 w-2.5" />
      ) : (
        <ThumbsUp className="h-2.5 w-2.5" />
      )}
      {saved ? "saved" : "save for training"}
    </button>
  );
}
