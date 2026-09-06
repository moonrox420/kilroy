/**
 * Platform store — the frontend mirror of the Rust OS smart detector.
 *
 * Loaded once at app boot. Components read `info` to render only what
 * fits the host: the Windows Sandbox option appears only on Windows,
 * keyboard hints show Cmd on macOS, shell labels adapt. Until the first
 * load resolves, `info` is null and callers should fall back to safe
 * defaults (the helpers below do this for you).
 */
import { create } from "zustand";
import { platform, type PlatformInfo, type SandboxDefault } from "@/lib/tauri";

interface PlatformState {
  info: PlatformInfo | null;
  load: () => Promise<void>;
}

export const usePlatform = create<PlatformState>((set, get) => ({
  info: null,
  async load() {
    if (get().info) return; // detector result never changes within a session
    try {
      const info = await platform.info();
      set({ info });
    } catch (err) {
      console.error("platform.info:", err);
    }
  },
}));

/**
 * Modifier-key label for keyboard hints. Defaults to "Ctrl" before the
 * detector resolves and on every non-mac OS.
 */
export function useModifierKey(): string {
  return usePlatform((s) => s.info?.modifier_key ?? "Ctrl");
}

/**
 * Sandbox kinds valid on this host. Defaults to the full set before the
 * detector resolves so the UI never hides options on a Windows box
 * during the brief boot window.
 */
export function useAvailableSandboxes(): SandboxDefault[] {
  return usePlatform(
    (s) => s.info?.available_sandboxes ?? ["host", "windows_sandbox", "docker"],
  );
}
