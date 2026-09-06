import { describe, expect, it, vi } from "vitest";
import { createListenerScope } from "./listenerScope";

describe("async listener ownership", () => {
  it("immediately removes a subscription delivered after teardown", async () => {
    const cleanup = vi.fn();
    const scope = createListenerScope(vi.fn());
    const registration = Promise.resolve(cleanup).then(scope.add);
    scope.dispose();
    await registration;
    scope.dispose();
    expect(cleanup).toHaveBeenCalledTimes(1);
  });

  it("removes all subscriptions even when one cleanup fails", () => {
    const report = vi.fn();
    const scope = createListenerScope(report);
    const good = vi.fn();
    scope.add(() => { throw new Error("closed"); });
    scope.add(good);
    scope.dispose();
    expect(report).toHaveBeenCalledOnce();
    expect(good).toHaveBeenCalledOnce();
  });
});
