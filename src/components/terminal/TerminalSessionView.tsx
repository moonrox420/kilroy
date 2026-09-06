/**
 * One xterm + PTY pair, scoped to a single TermSession.
 *
 * Mounted by `TerminalPanel` per session. Hidden via `display: none`
 * when its tab is inactive — that keeps the xterm buffer alive (so
 * scrollback survives tab switches) while only the active tab gets
 * input focus. On re-activation we fit() so the canvas matches the new
 * container size.
 *
 * INPUT MODEL (this is the part that kept breaking, so it's spelled out):
 *   1. PRIMARY — xterm's own `onData`, wired the instant xterm opens
 *      (not deferred to attach — attach only buffers *output*). When the
 *      hidden textarea has focus, keystrokes flow straight to the PTY.
 *   2. FALLBACK — a document-level capture-phase keydown listener that
 *      fires ONLY when this terminal is the active tab AND xterm's
 *      textarea does NOT hold focus AND focus isn't in another editable
 *      field. It forwards the keystroke and pulls focus into xterm.
 *      Critically, the host wrapper is NOT treated as "primary" — only
 *      `.xterm-helper-textarea` is — or focus on the wrapper became a
 *      dead zone where neither path forwarded keys.
 *   3. Copy / paste run through xterm's own key handler.
 *
 * If a write to the PTY is rejected (dead/with-no-backend build), we
 * print a loud red line into the terminal instead of silently eating
 * the keystroke — so "I can't type" always has a visible reason.
 */
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { attachTerminalSession, subscribeTerminalSession } from "@/lib/terminalBridge";
import { term } from "@/lib/tauri";
import { useTerminals, type TermSession } from "@/store/terminals";
import { notify } from "@/store/notifications";

interface Props {
  session: TermSession;
  isActive: boolean;
}

export function TerminalSessionView({ session, isActive }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  // Stable ref so the document-level fallback can check "is this the
  // active terminal?" without re-binding on every toggle.
  const isActiveRef = useRef(isActive);
  isActiveRef.current = isActive;
  const markExited = useTerminals((s) => s.markExited);

  // One-time setup: create xterm, wire it to the PTY at `session.id`.
  useEffect(() => {
    if (!hostRef.current) return;

    // Surface a real PTY write failure once (loud + visible), then stay
    // quiet so a dead PTY doesn't spam on every keystroke.
    let didReportWriteError = false;

    const xterm = new Terminal({
      fontFamily:
        "'JetBrains Mono', ui-monospace, Consolas, Cascadia Code, monospace",
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      cursorStyle: "block",
      allowProposedApi: true,
      theme: {
        background: "#0b0d10",
        foreground: "#d8dde2",
        cursor: "#f59e0b",
        cursorAccent: "#0b0d10",
        selectionBackground: "#f59e0b40",
        black: "#1a1f25",
        red: "#e15a5a",
        green: "#7ec27a",
        yellow: "#f5c66b",
        blue: "#7da8e2",
        magenta: "#c594c5",
        cyan: "#5fb3b3",
        white: "#d8dde2",
        brightBlack: "#3a4047",
        brightRed: "#f17878",
        brightGreen: "#94d094",
        brightYellow: "#fbd585",
        brightBlue: "#9ec0eb",
        brightMagenta: "#d4a8d4",
        brightCyan: "#82c8c8",
        brightWhite: "#f0f3f5",
      },
    });
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    xterm.loadAddon(new SearchAddon());
    xterm.loadAddon(new WebLinksAddon());
    xterm.open(hostRef.current);
    try {
      fit.fit();
    } catch {
      /* container probably 0×0 because tab inactive; we'll fit again on activate */
    }
    termRef.current = xterm;
    fitRef.current = fit;

    const shortId = session.id.slice(0, 8);
    let cancelled = false;

    // Single funnel for everything we send to the PTY. On rejection it
    // shows a visible, one-time reason inside the terminal so a missing
    // backend never looks like "the terminal silently won't type".
    const writeToPty = (data: string) => {
      term.write(session.id, data).catch((err) => {
        console.error(`term.write[${shortId}]:`, err);
        if (!didReportWriteError) {
          didReportWriteError = true;
          xterm.writeln(
            "\r\n\x1b[31m[kilroy] the terminal backend isn't responding — keystrokes have nowhere to go.\x1b[0m",
          );
          xterm.writeln(
            "\x1b[31m         the Rust backend likely isn't running. Run .\\bootstrap.ps1, then relaunch.\x1b[0m",
          );
          notify.error(`Terminal write failed (${shortId})`, String(err));
        }
      });
    };

    // Wire keyboard → PTY immediately. Attach only gates *output* buffering;
    // deferring onData until attach meant a slow/failed attach looked like
    // "the terminal won't type" even though the PTY was alive.
    const dataDisposable = xterm.onData((data) => {
      if (cancelled) return;
      writeToPty(data);
    });

    // Copy / paste through xterm's key handler. Returning false tells
    // xterm to NOT also process the key (so Ctrl+C as copy doesn't also
    // send SIGINT). Ctrl+C with no selection returns true so xterm emits
    // the interrupt (\x03) via onData above — exactly what a shell wants.
    xterm.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const ctrl = e.ctrlKey || e.metaKey;
      if (!ctrl) return true;
      const k = e.key.toLowerCase();
      if (k === "c") {
        if (maybeCopySelection(xterm, e)) {
          return false;
        }
        return true; // no selection → let xterm send the interrupt
      }
      if (k === "v") {
        void navigator.clipboard
          .readText()
          .then((textToPaste) => {
            if (textToPaste) writeToPty(textToPaste);
          })
          .catch((err) => console.warn("clipboard.readText:", err));
        return false;
      }
      return true;
    });

    // PTY → xterm: stream chunks in. The `cancelled` flag protects
    // against a tight unmount race where listeners register after
    // cleanup ran (React StrictMode runs mount→cleanup→mount, so this
    // is the NORMAL dev-mode path, not an edge case). `gotFirstChunk`
    // lets the prompt-primer stop nudging as soon as the shell produces
    // any output, and is the most reliable moment to grab focus (the
    // shell just drew its prompt).
    let gotFirstChunk = false;
    const primerTimers: number[] = [];

    const applyPtyOutput = (chunk: string) => {
      if (cancelled) return;
      if (!gotFirstChunk) {
        gotFirstChunk = true;
      }
      xterm.write(chunk);
    };

    const onPtyData = (chunk: string) => applyPtyOutput(chunk);
    const onPtyExit = () => {
      if (cancelled) return;
      xterm.writeln("\r\n\x1b[33m[process exited]\x1b[0m");
      markExited(session.id);
    };

    const unsubscribe = subscribeTerminalSession(session.id, {
      onData: onPtyData,
      onExit: onPtyExit,
    });

    // ── ATTACH HANDSHAKE ─────────────────────────────────────────────
    // Backlog bytes come back from the attach invoke (not events). Deferred
    // one macrotask so StrictMode cleanup runs first; attach itself is
    // deduplicated per session id in terminalBridge.
    const attachTimer = window.setTimeout(() => {
      if (cancelled) return;
      attachTerminalSession(session.id)
        .then((attached) => {
          if (cancelled) return;
          for (const chunk of attached.chunks) {
            applyPtyOutput(chunk);
          }
          if (attached.exited) {
            xterm.writeln("\r\n\x1b[33m[process exited]\x1b[0m");
            markExited(session.id);
          }
          try {
            fit.fit();
          } catch {
            /* container may still be settling */
          }
          const { cols, rows } = xterm;
          if (cols > 0 && rows > 0) {
            void term.resize(session.id, cols, rows).catch((err) =>
              console.error(`term.resize[${shortId}]:`, err),
            );
          }
          return term.flush(session.id);
        })
        .catch((err) => {
          console.error(`term.attach[${shortId}]:`, err);
          notify.fromError(`Terminal attach (${shortId})`, err);
        });
    }, 0);
    primerTimers.push(attachTimer);

    const nudge = () => {
      if (cancelled || gotFirstChunk) return;
      term
        .write(session.id, "\r")
        .catch((err) => console.error("prompt primer:", err));
    };
    primerTimers.push(
      window.setTimeout(nudge, 500),
      window.setTimeout(nudge, 1500),
    );

    // Resize hints only — xterm tells us its new cols/rows, we forward to the PTY.
    const resizeDisposable = xterm.onResize(({ cols, rows }) => {
      term.resize(session.id, cols, rows).catch((err) =>
        console.error(`term.resize[${shortId}]:`, err),
      );
    });

    // ── FALLBACK INPUT PATH ───────────────────────────────────────────
    // Fires only when THIS terminal is active AND focus is NOT in xterm's
    // textarea and NOT in any other editable element. That's precisely
    // the "app just opened, nothing is focused yet" case where xterm's
    // own onData would never fire. We forward the key, then pull focus
    // into xterm so every subsequent key flows through the primary path.
    // Because the two paths are gated on opposite focus locations they
    // can never both fire for the same keystroke — no double-send.
    const onDocKeydown = (e: KeyboardEvent) => {
      if (!isActiveRef.current) return;
      const host = hostRef.current;
      if (!host) return;
      const active = document.activeElement as HTMLElement | null;
      // Only skip when xterm's hidden textarea owns focus. The host wrapper
      // is also inside `host` (tabIndex / focus retries) — treating
      // *any* descendant as "primary" created a dead zone where neither
      // path forwarded keystrokes.
      if (isXtermInputFocused(active)) return;
      // Focus in the editor / chat / any field → never steal it.
      if (isEditableTarget(active)) return;

      const data = keyEventToShellInput(e);
      if (data === null) return;
      if (maybeCopySelection(xterm, e)) return;
      e.preventDefault();
      xterm.focus(); // hand off to the primary path for the next key
      writeToPty(data);
    };
    // Capture phase so we see the key before anything else.
    document.addEventListener("keydown", onDocKeydown, true);

    const onResize = () => {
      try {
        fit.fit();
      } catch {
        /* noop while detached */
      }
    };
    const ro = new ResizeObserver(onResize);
    ro.observe(hostRef.current);

    return () => {
      cancelled = true;
      ro.disconnect();
      document.removeEventListener("keydown", onDocKeydown, true);
      primerTimers.forEach((t) => clearTimeout(t));
      unsubscribe();
      dataDisposable?.dispose();
      resizeDisposable.dispose();
      xterm.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // session.id is stable for the lifetime of this component; we never
    // remount on a session id change because the parent uses it as the
    // React key, so it's already correctly scoped.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session.id]);

  // When this tab becomes active (including the very first render on app
  // launch), recompute the xterm size and grab focus so the terminal is
  // immediately ready to type into — no click required. The Tauri webview
  // sometimes drops focus on the first frame, so we retry on an escalating
  // schedule rather than focusing once.
  useEffect(() => {
    if (!isActive) return;
    const xterm = termRef.current;
    const fit = fitRef.current;
    if (!xterm || !fit) return;
    const focusNow = () => {
      try {
        fit.fit();
      } catch {
        /* noop */
      }
    };
    const raf = requestAnimationFrame(focusNow);
    const timers = [120, 350, 700].map((ms) => window.setTimeout(focusNow, ms));
    return () => {
      cancelAnimationFrame(raf);
      timers.forEach((t) => clearTimeout(t));
    };
  }, [isActive]);

  // Clicking the padding-area around xterm should hand focus to xterm's
  // textarea. preventDefault stops the host wrapper from keeping focus
  // (which would land in the dead zone described above).
  const focusXterm = (e: React.MouseEvent) => {
    e.preventDefault();
    termRef.current?.focus();
  };

  const redirectHostFocus = () => {
    termRef.current?.focus();
  };

  // Right-click pastes (PowerShell / Windows Terminal convention). If
  // there's an active selection, right-click copies instead — then the
  // next right-click pastes.
  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const xterm = termRef.current;
    if (!xterm) return;
    const sel = xterm.getSelection();
    if (sel) {
      copySelection(xterm);
      xterm.clearSelection();
    } else {
      void navigator.clipboard
        .readText()
        .then((text) => {
          if (text) void term.write(session.id, text);
        })
        .catch((err) => console.warn("clipboard.readText:", err));
    }
  };

  return (
    <div
      ref={hostRef}
      onMouseDown={focusXterm}
      onFocus={redirectHostFocus}
      onContextMenu={onContextMenu}
      className="absolute inset-0 cursor-text bg-[#0b0d10] p-1 outline-none"
      style={{
        display: isActive ? "block" : "none",
      }}
    />
  );
}

function copySelection(xterm: Terminal) {
  const sel = xterm.getSelection() || window.getSelection()?.toString();
  if (!sel) return;
  void navigator.clipboard.writeText(sel).catch((err) =>
    console.warn("clipboard.writeText:", err),
  );
}

function maybeCopySelection(xterm: Terminal, e: KeyboardEvent): boolean {
  const ctrl = e.ctrlKey || e.metaKey;
  if (!ctrl) return false;
  if (e.key.toLowerCase() !== "c") return false;
  const sel = xterm.getSelection() || window.getSelection()?.toString();
  if (!sel) return false;
  e.preventDefault();
  void navigator.clipboard.writeText(sel).catch((err) =>
    console.warn("clipboard.writeText:", err),
  );
  xterm.clearSelection();
  return true;
}

/** True if focus is on an element that owns its own text input — so the
 *  terminal fallback must not steal keystrokes from it (Monaco, the chat
 *  box, any input/textarea/select/contenteditable). */
function isEditableTarget(el: HTMLElement | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return el.isContentEditable;
}

/** True when xterm's hidden textarea holds focus — the primary input path. */
function isXtermInputFocused(el: HTMLElement | null): boolean {
  if (!el) return false;
  return el.classList.contains("xterm-helper-textarea");
}

/**
 * Translate a browser KeyboardEvent into the bytes a PTY-attached shell
 * expects. Returns null for keys we shouldn't forward (bare modifiers,
 * unmapped function keys). Used by the document-level fallback when
 * xterm's textarea doesn't hold focus (e.g. right after app launch).
 */
function keyEventToShellInput(e: KeyboardEvent): string | null {
  // Bare modifier press — nothing to send.
  if (
    e.key === "Shift" ||
    e.key === "Alt" ||
    e.key === "Control" ||
    e.key === "Meta"
  ) {
    return null;
  }
  // Printable single character (Ctrl/Meta combos become control codes below).
  if (e.key.length === 1 && !e.ctrlKey && !e.metaKey) {
    return e.key;
  }
  // Ctrl + letter -> control code (Ctrl+C = 0x03, Ctrl+D = 0x04, …).
  if (e.ctrlKey && !e.altKey && e.key.length === 1) {
    const code = e.key.toUpperCase().charCodeAt(0);
    if (code >= 64 && code <= 95) {
      return String.fromCharCode(code - 64);
    }
  }
  switch (e.key) {
    case "Enter":
      return "\r";
    case "Backspace":
      return "\x7f";
    case "Tab":
      return e.shiftKey ? "\x1b[Z" : "\t";
    case "Escape":
      return "\x1b";
    case "ArrowUp":
      return "\x1b[A";
    case "ArrowDown":
      return "\x1b[B";
    case "ArrowRight":
      return "\x1b[C";
    case "ArrowLeft":
      return "\x1b[D";
    case "Home":
      return "\x1b[H";
    case "End":
      return "\x1b[F";
    case "Delete":
      return "\x1b[3~";
    case "PageUp":
      return "\x1b[5~";
    case "PageDown":
      return "\x1b[6~";
    default:
      return null;
  }
}
