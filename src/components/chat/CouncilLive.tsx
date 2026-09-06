/**
 * Live-streaming UI for the four-voice Council.
 *
 * Rendered inline in the chat history while the four voices + the
 * synthesizer are streaming. Each voice gets its own card with a status
 * dot (·  spinning · ✓) and a live text buffer. The synthesis card
 * sits below the voices and only fills in once all four voices finish
 * — visually mirroring the backend's "fan out then synthesize" flow.
 *
 * After the synthesis completes the backend persists a regular agent
 * message with all four voices + synthesis as Markdown, so this card
 * is transient: it disappears when the chat panel resets the store
 * after the message lands.
 */
import { Loader2, CircleCheck } from "lucide-react";
import { useCouncil } from "@/store/council";
import type { CouncilVoice } from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface VoiceSpec {
  id: CouncilVoice;
  label: string;
  accent: string; // tailwind class fragment for the accent strip
  emoji: string;
}

const VOICE_REGISTRY: VoiceSpec[] = [
  // Council quartet — design / decision questions
  { id: "velocity", label: "Velocity", accent: "bg-amber", emoji: "⚡" },
  { id: "maintainability", label: "Maintainability", accent: "bg-ok", emoji: "🔧" },
  { id: "security", label: "Security", accent: "bg-err", emoji: "🛡️" },
  { id: "correctness", label: "Correctness", accent: "bg-info", emoji: "🎯" },
];

export function CouncilLive() {
  const voices = useCouncil((s) => s.voices);
  const voicesDone = useCouncil((s) => s.voicesDone);
  const synthesis = useCouncil((s) => s.synthesis);
  const synthesisDone = useCouncil((s) => s.synthesisDone);

  return (
    <div className="flex flex-col gap-3 rounded-md border border-line bg-bg-1 p-3">
      <div className="flex items-center gap-2 text-[11px] uppercase tracking-wider text-ink-subtle">
        <span>🗣️ Council in session</span>
        <span className="text-ink-subtle/60">· 4 voices · adversarial debate</span>
      </div>

      {/* 4 voice cards. On wide layouts they sit 2x2; on narrow chat
          they stack. Each card maintains its own scrolling area so a
          chatty voice doesn't push the others off-screen. */}
      <div className="grid grid-cols-1 gap-2 lg:grid-cols-2">
        {VOICE_REGISTRY.map((v) => (
          <VoiceCard
            key={v.id}
            spec={v}
            content={voices[v.id] ?? ""}
            done={voicesDone[v.id] ?? false}
          />
        ))}
      </div>

      {/* Synthesis only fills in once any of the voices finish, but the
          backend gates synthesis on ALL voices done. We show the card
          shell upfront with a "waiting" state so the user understands
          the structure. */}
      <SynthesisCard
        content={synthesis}
        done={synthesisDone}
        anyVoiceDone={Object.values(voicesDone).some(Boolean)}
        label="🧭 Synthesis"
      />
    </div>
  );
}

function VoiceCard({
  spec,
  content,
  done,
}: {
  spec: VoiceSpec;
  content: string;
  done: boolean;
}) {
  const empty = content.length === 0;
  return (
    <div className="overflow-hidden rounded-md border border-line bg-bg-0">
      {/* Accent strip + header */}
      <div className="flex items-center gap-2 border-b border-line px-2 py-1">
        <span className={cn("h-2 w-2 rounded-full", spec.accent)} />
        <span className="text-[11px] font-medium text-ink">
          {spec.emoji} {spec.label}
        </span>
        <span className="ml-auto">
          {done ? (
            <CircleCheck className="h-3 w-3 text-ok" />
          ) : empty ? (
            <span className="text-[10px] text-ink-subtle">waiting…</span>
          ) : (
            <Loader2 className="h-3 w-3 animate-spin text-ink-subtle" />
          )}
        </span>
      </div>
      {/* Streaming text. Cap height so a chatty voice doesn't dominate
          — the user can read the full content from the persisted
          message bubble after the turn completes. */}
      <div className="max-h-44 overflow-y-auto px-2 py-1.5 text-[11.5px] text-ink whitespace-pre-wrap">
        {empty ? (
          <span className="text-ink-subtle italic">…</span>
        ) : (
          content
        )}
      </div>
    </div>
  );
}

function SynthesisCard({
  content,
  done,
  anyVoiceDone,
  label,
}: {
  content: string;
  done: boolean;
  anyVoiceDone: boolean;
  label: string;
}) {
  const empty = content.length === 0;
  return (
    <div
      className={cn(
        "overflow-hidden rounded-md border bg-bg-0",
        done ? "border-ok/60" : "border-amber/60",
      )}
    >
      <div className="flex items-center gap-2 border-b border-line px-2 py-1">
        <span className={cn("h-2 w-2 rounded-full", done ? "bg-ok" : "bg-amber")} />
        <span className="text-[11px] font-medium text-ink">{label}</span>
        <span className="ml-auto">
          {done ? (
            <CircleCheck className="h-3 w-3 text-ok" />
          ) : empty && !anyVoiceDone ? (
            <span className="text-[10px] text-ink-subtle">awaiting voices…</span>
          ) : (
            <Loader2 className="h-3 w-3 animate-spin text-ink-subtle" />
          )}
        </span>
      </div>
      <div className="max-h-64 overflow-y-auto px-2 py-1.5 text-[11.5px] text-ink whitespace-pre-wrap">
        {empty ? (
          <span className="text-ink-subtle italic">…</span>
        ) : (
          content
        )}
      </div>
    </div>
  );
}
