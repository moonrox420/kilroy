import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ActionView } from "@/lib/tauri";

const api = vi.hoisted(() => ({
  list: vi.fn(), listPendingForTask: vi.fn(), accept: vi.fn(), reject: vi.fn(),
}));
vi.mock("@/lib/tauri", () => ({ actions: api }));
vi.mock("./notifications", () => ({ notify: { fromError: vi.fn() } }));
import { useActions } from "./actions";

const action: ActionView = {
  id: 1, task_id: null, session_id: 1, kind: "shell", target: null,
  payload: { command: "test", sandbox: "docker" }, diff: null,
  status: "pending", error: null, created_at: 1, resolved_at: null,
};

describe("approval state", () => {
  beforeEach(() => { vi.resetAllMocks(); useActions.getState().reset(); });

  it("keeps an action pending when the IPC request fails", async () => {
    useActions.getState().upsert(action);
    api.accept.mockRejectedValue(new Error("transport failed"));
    await expect(useActions.getState().accept(1)).rejects.toThrow("transport failed");
    expect(useActions.getState().byId[1].status).toBe("pending");
  });

  it("preserves the user's selected patch hunks", async () => {
    api.accept.mockResolvedValue({ action_id: 1, status: "applied", error: null, follow_up_action_ids: [] });
    await useActions.getState().accept(1, "selected diff");
    expect(api.accept).toHaveBeenCalledWith({ action_id: 1, override_diff: "selected diff" });
  });

  it("does not load an old project's actions after a project reset", async () => {
    let finish!: (rows: ActionView[]) => void;
    api.list.mockReturnValue(new Promise<ActionView[]>((resolve) => { finish = resolve; }));
    const load = useActions.getState().loadForTask(0);
    useActions.getState().reset();
    finish([action]);
    await load;
    expect(useActions.getState().byId).toEqual({});
  });

  it("reloads staged approvals from the command response even if its event is lost", async () => {
    useActions.getState().upsert(action);
    api.accept.mockResolvedValue({ action_id: 1, status: "applied", error: null, follow_up_action_ids: [2] });
    api.list.mockResolvedValue([{ ...action, id: 2, kind: "file_change" }]);
    await useActions.getState().accept(1);
    expect(useActions.getState().byTask[0]).toEqual([2]);
    expect(useActions.getState().byId[2].kind).toBe("file_change");
  });
});
