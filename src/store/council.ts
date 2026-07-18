/**
 * Swarm store — live state for any 4-voice mode (Council or Debug).
 *
 * Each swarm turn produces 4 parallel voice streams plus a synthesis
 * stream. We hold their accumulated buffers here keyed by voice ID
 * (which is what discriminates Council from Debug — same channels,
 * different IDs). When the turn lands as a regular agent message (with
 * all four voices + synthesis baked into Markdown), the chat panel
 * resets this store and drops back to normal bubble view.
 *
 * The name `useCouncil` is preserved for backward compatibility — but
 * the data shape is mode-agnostic: voices are keyed by string ID, so
 * Debug's `error_reader` / `recent_changes` / etc. live in the same
 * map as Council's `velocity` / `maintainability` / etc.
 *
 * StrictMode-safe listener registration mirrors the runtime store: a
 * module-level `_attached` guard ensures we only subscribe once even
 * when React double-mounts in development.
 */
import { create } from "zustand";
import { council, type SwarmVoice } from "@/lib/tauri";

export type CouncilVoiceBuffers = Partial<Record<SwarmVoice, string>>;
export type CouncilVoiceDoneFlags = Partial<Record<SwarmVoice, boolean>>;

interface CouncilState {
  voices: CouncilVoiceBuffers;
  voicesDone: CouncilVoiceDoneFlags;
  synthesis: string;
  synthesisDone: boolean;
  /** True when any voice has started streaming this turn and we haven't
   *  hit the synthesis-done signal yet. Drives the live-card visibility. */
  active: boolean;

  initListeners: () => () => void;
  reset: () => void;
}

// Voice-keyed maps start empty — we let whichever voices arrive in
// `agent://council/voice` events populate themselves. That way Council
// (velocity / maintainability / security / correctness) and Debug
// (error_reader / recent_changes / hypothesis / fix_author) both work
// without separate state buckets.
const EMPTY_VOICES: CouncilVoiceBuffers = {};
const EMPTY_DONE: CouncilVoiceDoneFlags = {};

let _attached = false;
const _disposers: Array<() => void> = [];

export const useCouncil = create<CouncilState>((set, get) => ({
  voices: { ...EMPTY_VOICES },
  voicesDone: { ...EMPTY_DONE },
  synthesis: "",
  synthesisDone: false,
  active: false,

  reset: () =>
    set({
      voices: { ...EMPTY_VOICES },
      voicesDone: { ...EMPTY_DONE },
      synthesis: "",
      synthesisDone: false,
      active: false,
    }),

  initListeners() {
    if (_attached) {
      return () => {};
    }
    _attached = true;

    void council
      .onVoiceChunk((c) => {
        // First chunk of a new turn — fresh state. We treat the first
        // voice delta after a reset as the start signal so we don't
        // need a separate `swarm/started` event from the backend.
        const cur = get();
        if (!cur.active) {
          set({
            voices: { ...EMPTY_VOICES },
            voicesDone: { ...EMPTY_DONE },
            synthesis: "",
            synthesisDone: false,
            active: true,
          });
        }
        set((s) => ({
          voices: {
            ...s.voices,
            [c.voice]: (s.voices[c.voice] ?? "") + c.delta,
          },
        }));
      })
      .then((u) => _disposers.push(u));

    void council
      .onVoiceDone((c) => {
        // Capture the final content authoritatively — covers the rare
        // case where the streaming buffer dropped a chunk (network blip).
        set((s) => ({
          voices: { ...s.voices, [c.voice]: c.content },
          voicesDone: { ...s.voicesDone, [c.voice]: true },
        }));
      })
      .then((u) => _disposers.push(u));

    void council
      .onSynthesis((c) => {
        set((s) => ({ synthesis: s.synthesis + c.delta }));
      })
      .then((u) => _disposers.push(u));

    void council
      .onDone((c) => {
        set((s) => ({
          synthesis: c.synthesis || s.synthesis,
          synthesisDone: true,
          // Keep `active: true` until the chat-panel's effect that
          // detects "regular agent message arrived" calls reset(). That
          // way the live card stays on screen briefly while the message
          // bubble is being rendered, instead of flashing off.
        }));
      })
      .then((u) => _disposers.push(u));

    return () => {
      for (const u of _disposers) {
        try {
          u();
        } catch {
          // already disposed
        }
      }
      _disposers.length = 0;
      _attached = false;
    };
  },
}));
