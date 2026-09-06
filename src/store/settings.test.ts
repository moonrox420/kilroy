import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SettingsView } from "@/lib/tauri";
const api = vi.hoisted(() => ({ get: vi.fn(), update: vi.fn(), ollamaHealth: vi.fn() }));
vi.mock("@/lib/tauri", () => ({ settings: api }));
vi.mock("./notifications", () => ({ notify: { fromError: vi.fn() } }));
import { useSettings } from "./settings";

describe("settings failure handling", () => {
  beforeEach(() => { vi.resetAllMocks(); useSettings.setState({ current: null, loading: false, saving: false }); });
  it("retains the last confirmed settings when validation or persistence fails", async () => {
    const original = { chat_model: "local-model" } as SettingsView;
    useSettings.setState({ current: original });
    api.update.mockRejectedValue(new Error("invalid embedding dimensions"));
    expect(await useSettings.getState().save({ embedding_model: "wrong-dimension" })).toBeNull();
    expect(useSettings.getState().current).toBe(original);
    expect(useSettings.getState().saving).toBe(false);
  });
  it("clears the loading state after a failed initial load", async () => {
    api.get.mockRejectedValue(new Error("unavailable"));
    await useSettings.getState().load();
    expect(useSettings.getState().loading).toBe(false);
  });
});
