/**
 * Singleton PTY event bridge — one Tauri `listen()` per session id.
 *
 * React StrictMode and Vite HMR both mount terminal views more than once.
 * Each extra `listen("terminal://{id}/data")` delivers every chunk twice to
 * xterm. This module keeps exactly one listener per session on `globalThis`
 * and swaps a single output handler when the surviving xterm remounts.
 */
import { term } from "@/lib/tauri";
import { type UnlistenFn } from "@tauri-apps/api/event";
import { notify } from "@/store/notifications";

type ChunkHandler = (chunk: string) => void;

type SessionBridge = {
  /** Latest xterm callback — replaced on each subscribe, never duplicated. */
  onData: ChunkHandler | null;
  onExit: (() => void) | null;
  unlistenData: UnlistenFn | null;
  unlistenExit: UnlistenFn | null;
  unlistenBytesExceeded: UnlistenFn | null;
  dataListenGen: number;
  exitListenGen: number;
  bytesExceededListenGen: number;
  /** Deferred teardown so StrictMode cleanup→remount keeps the listener alive. */
  teardownTimer: ReturnType<typeof setTimeout> | null;
  attachInFlight: Promise<{ chunks: string[]; exited: boolean }> | null;
  attachDone: boolean;
};

type BridgeHost = typeof globalThis & {
  __kilroyTerminalBridge?: Map<string, SessionBridge>;
};

const GLOBAL_KEY = "__kilroyTerminalBridge";

function bridgeMap(): Map<string, SessionBridge> {
  const host = globalThis as BridgeHost;
  if (!host[GLOBAL_KEY]) {
    host[GLOBAL_KEY] = new Map();
  }
  return host[GLOBAL_KEY]!;
}

function getOrCreateBridge(id: string): SessionBridge {
  const map = bridgeMap();
  let bridge = map.get(id);
  if (!bridge) {
    bridge = {
      onData: null,
      onExit: null,
      unlistenData: null,
      unlistenExit: null,
      unlistenBytesExceeded: null,
      dataListenGen: 0,
      exitListenGen: 0,
      bytesExceededListenGen: 0,
      teardownTimer: null,
      attachInFlight: null,
      attachDone: false,
    };
    map.set(id, bridge);
  }
  return bridge;
}

function stopDataListener(bridge: SessionBridge) {
  bridge.dataListenGen += 1;
  bridge.unlistenData?.();
  bridge.unlistenData = null;
}

function stopExitListener(bridge: SessionBridge) {
  bridge.exitListenGen += 1;
  bridge.unlistenExit?.();
  bridge.unlistenExit = null;
}

function stopBytesExceededListener(bridge: SessionBridge) {
  bridge.bytesExceededListenGen += 1;
  bridge.unlistenBytesExceeded?.();
  bridge.unlistenBytesExceeded = null;
}

function cancelTeardown(bridge: SessionBridge) {
  if (bridge.teardownTimer !== null) {
    clearTimeout(bridge.teardownTimer);
    bridge.teardownTimer = null;
  }
}

function scheduleTeardown(id: string, bridge: SessionBridge) {
  cancelTeardown(bridge);
  bridge.teardownTimer = setTimeout(() => {
    bridge.teardownTimer = null;
    teardownIfIdle(id, bridge);
  }, 0);
}

function ensureDataListener(id: string, bridge: SessionBridge) {
  if (bridge.unlistenData) return;

  const gen = ++bridge.dataListenGen;
  void term
    .onData(id, (chunk) => {
      if (bridge.dataListenGen !== gen) return;
      try {
        bridge.onData?.(chunk);
      } catch (err) {
        console.error(`terminalBridge.onData[${id.slice(0, 8)}]:`, err);
      }
    })
    .then((unlisten) => {
      if (bridge.dataListenGen !== gen) {
        unlisten();
        return;
      }
      bridge.unlistenData = unlisten;
    })
    .catch((err) => {
      console.error(`terminalBridge.listen(data)[${id.slice(0, 8)}]:`, err);
    });
}

function ensureExitListener(id: string, bridge: SessionBridge) {
  if (bridge.unlistenExit) return;

  const gen = ++bridge.exitListenGen;
  void term
    .onExit(id, () => {
      if (bridge.exitListenGen !== gen) return;
      try {
        bridge.onExit?.();
      } catch (err) {
        console.error(`terminalBridge.onExit[${id.slice(0, 8)}]:`, err);
      }
    })
    .then((unlisten) => {
      if (bridge.exitListenGen !== gen) {
        unlisten();
        return;
      }
      bridge.unlistenExit = unlisten;
    })
    .catch((err) => {
      console.error(`terminalBridge.listen(exit)[${id.slice(0, 8)}]:`, err);
    });
}

function ensureBytesExceededListener(id: string, bridge: SessionBridge) {
  if (bridge.unlistenBytesExceeded) return;

  const gen = ++bridge.bytesExceededListenGen;
  void term
    .onBytesExceeded(id, () => {
      if (bridge.bytesExceededListenGen !== gen) return;
      notify.error(
        "Terminal output limit exceeded",
        `Session ${id.slice(0, 8)} hit the 16 MiB byte cap and was killed.`,
      );
    })
    .then((unlisten) => {
      if (bridge.bytesExceededListenGen !== gen) {
        unlisten();
        return;
      }
      bridge.unlistenBytesExceeded = unlisten;
    })
    .catch((err) => {
      console.error(
        `terminalBridge.listen(bytes_exceeded)[${id.slice(0, 8)}]:`,
        err,
      );
    });
}

function teardownIfIdle(id: string, bridge: SessionBridge) {
  if (bridge.onData || bridge.onExit) return;
  stopDataListener(bridge);
  stopExitListener(bridge);
  stopBytesExceededListener(bridge);
  bridgeMap().delete(id);
}

/**
 * Register PTY output handlers for a session. Only the latest handler receives
 * chunks; StrictMode remounts replace the previous callback instead of adding
 * a second one.
 */
export function subscribeTerminalSession(
  id: string,
  handlers: { onData: ChunkHandler; onExit?: () => void },
): () => void {
  const bridge = getOrCreateBridge(id);
  cancelTeardown(bridge);
  bridge.onData = handlers.onData;
  bridge.onExit = handlers.onExit ?? null;
  ensureDataListener(id, bridge);
  ensureExitListener(id, bridge);
  ensureBytesExceededListener(id, bridge);

  return () => {
    if (bridge.onData === handlers.onData) {
      bridge.onData = null;
    }
    if (bridge.onExit === handlers.onExit) {
      bridge.onExit = null;
    }
    scheduleTeardown(id, bridge);
  };
}

/** Tear down listeners when a PTY session is killed (tab closed). */
export function destroyTerminalBridge(id: string) {
  const bridge = bridgeMap().get(id);
  if (!bridge) return;
  cancelTeardown(bridge);
  bridge.onData = null;
  bridge.onExit = null;
  stopDataListener(bridge);
  stopExitListener(bridge);
  stopBytesExceededListener(bridge);
  bridge.attachDone = false;
  bridge.attachInFlight = null;
  bridgeMap().delete(id);
}

/**
 * Attach handshake — deduped per session id. Returns buffered shell output
 * from the invoke payload (not events).
 */
export async function attachTerminalSession(
  id: string,
): Promise<{ chunks: string[]; exited: boolean }> {
  const bridge = getOrCreateBridge(id);
  if (bridge.attachDone) {
    return { chunks: [], exited: false };
  }
  if (bridge.attachInFlight) {
    return bridge.attachInFlight;
  }

  bridge.attachInFlight = (async () => {
    try {
      const attached = await term.attach(id);
      bridge.attachDone = true;
      return attached;
    } finally {
      bridge.attachInFlight = null;
    }
  })();

  return bridge.attachInFlight;
}

/**
 * Per-session `bytes_exceeded` listener — fires when the PTY reader pump hits
 * the 16 MiB byte cap. The listener is owned by the bridge and follows the
 * same teardown lifecycle as `data` / `exit`. We register one per session id
 * (matching the backend's `terminal://{id}/bytes_exceeded` emission) instead
 * of a wildcard event name — Tauri 2 rejects `*` in event identifiers.
 */

/** Drop orphaned Tauri listeners after Vite HMR reloads this module. */
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    for (const bridge of bridgeMap().values()) {
      cancelTeardown(bridge);
      stopDataListener(bridge);
      stopExitListener(bridge);
      stopBytesExceededListener(bridge);
    }
    bridgeMap().clear();
  });
}