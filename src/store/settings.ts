/**
 * Settings store — local mirror of the backend's `settings.json`.
 *
 * `load` reads the current settings from Rust on app boot (and after
 * every Save). `save` posts a partial patch and replaces the local copy
 * with whatever the backend returns (which has been clamped /
 * validated). `checkOllama` calls the typed health endpoint that drives
 * the Settings dialog's "test connection" UI.
 */
import { create } from "zustand";
import {
  settings as api,
  type OllamaHealthFull,
  type SettingsPatch,
  type SettingsView,
} from "@/lib/tauri";
import { notify } from "./notifications";

interface SettingsState {
  current: SettingsView | null;
  loading: boolean;
  saving: boolean;
  health: OllamaHealthFull | null;
  healthCheckedAt: number | null;
  load: () => Promise<void>;
  save: (patch: SettingsPatch) => Promise<SettingsView | null>;
  checkOllama: () => Promise<OllamaHealthFull | null>;
}

export const useSettings = create<SettingsState>((set) => ({
  current: null,
  loading: false,
  saving: false,
  health: null,
  healthCheckedAt: null,

  async load() {
    set({ loading: true });
    try {
      const s = await api.get();
      set({ current: s, loading: false });
    } catch (err) {
      // Boot-time settings load — silent failure here means the app
      // limps along with defaults and the user can't tell why their
      // saved preferences didn't take. Make it loud.
      notify.fromError("Load settings", err);
      set({ loading: false });
    }
  },

  async save(patch) {
    set({ saving: true });
    try {
      const updated = await api.update(patch);
      set({ current: updated, saving: false });
      return updated;
    } catch (err) {
      notify.fromError("Save settings", err);
      set({ saving: false });
      return null;
    }
  },

  async checkOllama() {
    try {
      const h = await api.ollamaHealth();
      set({ health: h, healthCheckedAt: Date.now() });
      return h;
    } catch (err) {
      // Health check fails silently a lot (Ollama just isn't up yet on
      // first launch). Don't toast — the status bar's Ollama pill
      // already conveys reachability. Console for forensics only.
      console.warn("ollama_health:", err);
      return null;
    }
  },
}));
