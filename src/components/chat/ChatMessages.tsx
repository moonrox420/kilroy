/**
 * Scrollable chat history.
 *
 * User and agent messages are visually offset — user on the right with
 * an amber tint, agent on the left with neutral surface. System
 * messages are centered and quiet. Agent messages from autonomous runs
 * render an inline TaskStream card. While the agent is thinking, a
 * live preview bubble shows the streaming Copilot tokens.
 */
import { useEffect, useMemo, useRef } from "react";
import { useAgent, type ChatMessage } from "@/store/agent";
import { useRuntime, type LiveRun } from "@/store/runtime";
import { useCouncil } from "@/store/council";
import { useCorpus } from "@/store/corpus";
import { cn } from "@/lib/utils";
import { KilroyMark } from "@/components/common/KilroyMark";
import { ContextBlock } from "./ContextBlock";
import { TaskStream } from "./TaskStream";
import { PlanControls } from "./PlanControls";
import { ChatContent } from "./ChatContent";
import { CouncilLive } from "./CouncilLive";
import { FeedbackButton } from "./FeedbackButton";
import { CorpusBanner } from "./CorpusBanner";

export function ChatMessages() {
  const messages = useAgent((s) => s.messages);
  const isThinking = useAgent((s) => s.isThinking);
  const streamingBuffer = useRuntime((s) => s.streamingBuffer);
  const runs = useRuntime((s) => s.runs);
  const clearStream = useRuntime((s) => s.clearStream);

  const scrollerRef = useRef<HTMLDivElement>(null);
  const stickyRef = useRef(true);

  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const onScroll = () => {
      const atBottom =
        el.scrollHeight - el.scrollTop - el.clientHeight < 20;
      stickyRef.current = atBottom;
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    if (stickyRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [messages.length, isThinking, streamingBuffer]);

  // When a non-streaming reply lands, clear any leftover buffer so the
  // next turn starts clean.
  useEffect(() => {
    if (!isThinking && streamingBuffer) {
      clearStream();
    }
  }, [isThinking, streamingBuffer, clearStream]);

  // The currently-running autonomous run, if any. We surface it under
  // the thinking indicator so the user sees plan + task progress live.
  const liveRunInProgress: LiveRun | null = (() => {
    const incomplete = Object.values(runs).find((r) => !r.completed);
    return incomplete ?? null;
  })();

  // Council mode — show the live 4-voice card while the council is in
  // session. We reset the council store once the persisted agent
  // message for this turn has landed (isThinking transitions to false
  // AND a new agent message exists). Until then, the card stays visible
  // under the user's message.
  const councilActive = useCouncil((s) => s.active);
  const councilSynthesisDone = useCouncil((s) => s.synthesisDone);
  const resetCouncil = useCouncil((s) => s.reset);

  // Refresh distillation corpus stats once on mount and whenever the
  // message list grows. Cheap (one stat() + line count) and keeps the
  // banner / counter accurate after the user accepts a 👍.
  const refreshCorpus = useCorpus((s) => s.refresh);
  useEffect(() => {
    void refreshCorpus();
  }, [refreshCorpus, messages.length]);

  // Pre-compute, for each agent message, the immediately-preceding
  // user message. This is what the FeedbackButton wants to bundle into
  // a corpus row. Building it once here saves Bubble from re-scanning
  // the array on every render.
  const priorUserByIndex = useMemo(() => {
    const out = new Map<string, string>();
    let lastUser: string | null = null;
    for (const m of messages) {
      if (m.role === "user") lastUser = m.content;
      else if (m.role === "agent" && lastUser) out.set(m.id, lastUser);
    }
    return out;
  }, [messages]);

  useEffect(() => {
    // Drop the live card once both: the synthesis stream has finished
    // AND the persisted agent message has been appended. Without the
    // `!isThinking` gate we'd flash the card off before the bubble
    // appears, which looks jumpy.
    if (councilSynthesisDone && !isThinking) {
      // Short delay so the user sees the "all done" green checkmarks
      // for a beat before the live card transitions to the bubble.
      const t = setTimeout(() => resetCouncil(), 600);
      return () => clearTimeout(t);
    }
  }, [councilSynthesisDone, isThinking, resetCouncil]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <CorpusBanner />
      <div
        ref={scrollerRef}
        className="flex-1 min-h-0 overflow-y-auto px-3 py-3"
      >
        <div className="flex flex-col gap-3">
          {messages.map((m) => (
            <Bubble
              key={m.id}
              msg={m}
              liveRun={m.run_id ? runs[m.run_id] : undefined}
              priorUserMessage={priorUserByIndex.get(m.id) ?? null}
            />
          ))}
          {councilActive && <CouncilLive />}
          {isThinking && !councilActive && (
            <ThinkingBlock
              streamingText={streamingBuffer}
              liveRun={liveRunInProgress}
            />
          )}
        </div>
      </div>
    </div>
  );
}

function Bubble({
  msg,
  liveRun,
  priorUserMessage,
}: {
  msg: ChatMessage;
  liveRun?: LiveRun;
  /** The user message that immediately preceded this one. Required for
   *  the FeedbackButton to know what to bundle into the corpus row. */
  priorUserMessage: string | null;
}) {
  if (msg.role === "system") {
    return (
      <div className="self-center max-w-[90%] rounded-md border border-dashed border-line bg-bg-0/30 px-2.5 py-1 text-center text-[11px] text-ink-subtle">
        {msg.content}
      </div>
    );
  }

  const isUser = msg.role === "user";
  // Prefer the live run state if it's still in memory; otherwise build a
  // synthetic LiveRun from the persisted task summary.
  const runForRender: LiveRun | null = liveRun
    ? liveRun
    : msg.run_id && msg.tasks && msg.tasks.length > 0
      ? {
          run_id: msg.run_id,
          user_message: "",
          mode: "autonomous",
          overview: msg.content.split("\n")[0] ?? "",
          completed: true,
          success: !msg.tasks.some((t) => t.status === "failed"),
          tasks: msg.tasks.map((t) => ({
            task_id: t.id,
            type: t.type,
            agent: t.agent,
            title: t.title,
            status:
              t.status === "cancelled" || t.status === "pending"
                ? "failed"
                : (t.status as LiveRun["tasks"][number]["status"]),
            output: t.output_preview,
          })),
        }
      : null;

  return (
    <div
      className={cn(
        "flex w-full gap-2",
        isUser ? "flex-row-reverse" : "flex-row",
      )}
    >
      <Avatar role={msg.role} />
      <div className={cn("flex max-w-[78%] flex-col gap-1")}>
        <div
          className={cn(
            "rounded-md px-2.5 py-1.5 text-[12px] leading-snug",
            isUser
              ? "bg-amber/10 text-ink border border-amber/30 whitespace-pre-wrap break-words"
              : "bg-bg-2 text-ink border border-line",
          )}
        >
          {isUser ? msg.content : <ChatContent text={msg.content} />}
        </div>
        {msg.plan_pending && msg.run_id && msg.tasks && msg.tasks.length > 0 && (
          <PlanControls run_id={msg.run_id} tasks={msg.tasks} />
        )}
        {runForRender && <TaskStream run={runForRender} />}
        {msg.role === "agent" && msg.context && (
          <ContextBlock context={msg.context} />
        )}
        {/* Image-count badge on user turns that attached images. The
            base64 payloads themselves are intentionally not stored on
            the message — only the count, so we have something to show. */}
        {isUser && (msg.attached_images_count ?? 0) > 0 && (
          <div className="text-right text-[10px] text-ink-subtle">
            📎 {msg.attached_images_count} image
            {msg.attached_images_count === 1 ? "" : "s"} attached
          </div>
        )}
        {/* 👍 row on agent replies — feeds the distillation corpus. */}
        {msg.role === "agent" && !msg.plan_pending && (
          <div className="flex items-center gap-2">
            <FeedbackButton
              messageId={msg.id}
              priorUserMessage={priorUserMessage}
              agentMessage={msg.content}
            />
          </div>
        )}
        <div
          className={cn(
            "text-[10px] text-ink-subtle",
            isUser ? "text-right" : "text-left",
          )}
        >
          {isUser ? "You" : "Kilroy"} · {fmtTime(msg.timestamp)}
        </div>
      </div>
    </div>
  );
}

function Avatar({ role }: { role: "user" | "agent" }) {
  if (role === "agent") {
    return (
      <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-amber/10">
        <KilroyMark size={16} className="text-amber" />
      </div>
    );
  }
  return (
    <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-bg-3 text-[10px] font-semibold text-ink-muted">
      U
    </div>
  );
}

function ThinkingBlock({
  streamingText,
  liveRun,
}: {
  streamingText: string;
  liveRun: LiveRun | null;
}) {
  if (liveRun) {
    return (
      <div className="flex w-full gap-2">
        <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-amber/10">
          <KilroyMark size={16} className="text-amber" />
        </div>
        <div className="flex max-w-[78%] flex-col gap-1">
          <TaskStream run={liveRun} />
        </div>
      </div>
    );
  }

  if (streamingText) {
    return (
      <div className="flex w-full gap-2">
        <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-amber/10">
          <KilroyMark size={16} className="text-amber" />
        </div>
        <div className="flex max-w-[78%] flex-col gap-1">
          <div className="rounded-md border border-line bg-bg-2 px-2.5 py-1.5 text-[12px] leading-snug text-ink">
            <ChatContent text={streamingText} />
            <Caret />
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex w-full items-center gap-2">
      <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-amber/10">
        <KilroyMark size={16} className="text-amber" />
      </div>
      <div className="rounded-md border border-line bg-bg-2 px-2.5 py-1.5">
        <span className="flex gap-1">
          <Dot delay={0} />
          <Dot delay={120} />
          <Dot delay={240} />
        </span>
      </div>
    </div>
  );
}

function Caret() {
  return (
    <span className="ml-0.5 inline-block h-3.5 w-[2px] translate-y-0.5 animate-pulse bg-amber align-middle" />
  );
}

function Dot({ delay }: { delay: number }) {
  return (
    <span
      className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber"
      style={{ animationDelay: `${delay}ms` }}
    />
  );
}

function fmtTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}
