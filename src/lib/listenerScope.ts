/** Owns async IPC subscriptions, including registrations completed after teardown. */
export function createListenerScope(onError: (error: unknown) => void) {
  let disposed = false;
  const listeners = new Set<() => void>();
  const cleanup = (listener: () => void) => {
    try { listener(); } catch (error) { onError(error); }
  };
  return {
    add(listener: () => void) {
      if (disposed) cleanup(listener);
      else listeners.add(listener);
    },
    report: onError,
    dispose() {
      if (disposed) return;
      disposed = true;
      listeners.forEach(cleanup);
      listeners.clear();
    },
  };
}
