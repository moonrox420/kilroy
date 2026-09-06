/**
 * Council store — live state for the four-voice Council mode.
 *
 * Each swarm turn produces 4 parallel voice streams plus a synthesis
 * stream. We hold their accumulated buffers here keyed by voice ID
 * (which is what discriminates Council from Debug — same channels,
 * different IDs). When the turn lands as a regular agent message (with
 * all four voices plus synthesis baked into Markdown), the chat panel
 * resets this store and drops back to normal bubble view.
 *
 * Each mount owns and cleans up its async subscriptions.
 */
import { create } from "zustand";
import { council, type CouncilVoice } from "@/lib/tauri";
import { createListenerScope } from "@/lib/listenerScope";
import { notify } from "./notifications";

export type CouncilVoiceBuffers = Partial<Record<CouncilVoice, string>>;
export type CouncilVoiceDoneFlags = Partial<Record<CouncilVoice, boolean>>;

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

// Voice-keyed maps start empty and are populated by Council events.
const EMPTY_VOICES: CouncilVoiceBuffers = {};
const EMPTY_DONE: CouncilVoiceDoneFlags = {};


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
    const scope = createListenerScope((error) => notify.fromError("Council subscriptions", error));

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
      .then(scope.add).catch(scope.report);

    void council
      .onVoiceDone((c) => {
        // Capture the final content authoritatively — covers the rare
        // case where the streaming buffer dropped a chunk (network blip).
        set((s) => ({
          voices: { ...s.voices, [c.voice]: c.content },
          voicesDone: { ...s.voicesDone, [c.voice]: true },
        }));
      })
      .then(scope.add).catch(scope.report);

    void council
      .onSynthesis((c) => {
        set((s) => ({ synthesis: s.synthesis + c.delta }));
      })
      .then(scope.add).catch(scope.report);

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
      .then(scope.add).catch(scope.report);

    return scope.dispose;
  },
}));
