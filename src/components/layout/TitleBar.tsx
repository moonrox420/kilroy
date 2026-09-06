/**
 * Custom Windows 11 title bar.
 *
 * The window is configured with `decorations: false` in tauri.conf.json
 * so we draw our own. The bar itself is a drag region; controls opt out
 * via `.no-drag`. We mirror Windows 11 conventions: min / max / close on
 * the right, app icon + title on the left, optional centerpiece in the
 * middle (we drop a status pill there).
 */
import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X, Copy } from "lucide-react";
import { cn } from "@/lib/utils";
import { KilroyMark } from "@/components/common/KilroyMark";
import { useAgent } from "@/store/agent";

const win = getCurrentWindow();

export function TitleBar() {
  const [maximized, setMaximized] = useState(false);
  const mode = useAgent((s) => s.mode);
  const isThinking = useAgent((s) => s.isThinking);

  useEffect(() => {
    let cancelled = false;
    const sync = async () => {
      try {
        const m = await win.isMaximized();
        if (!cancelled) setMaximized(m);
      } catch {
        /* noop */
      }
    };
    sync();
    const unlistenP = win.onResized(() => sync());
    return () => {
      cancelled = true;
      unlistenP.then((u) => u()).catch(() => {});
    };
  }, []);

  const modeLabel: string =
    ({
      copilot: "Chat",
      autonomous: "Plan / Execute",
      multi_agent: "Multi-Agent",
      governance: "Governance",
      debug: "Review / Debug",
      test_first: "Test",
      council: "Council",
      code_agent: "Code",
    } as Partial<Record<string, string>>)[mode] ?? mode;

  return (
    <header
      className="drag-region flex items-center justify-between border-b border-line bg-bg-0 select-none"
      style={{ height: "var(--titlebar-h)" }}
    >
      {/* left — brand */}
      <div className="flex h-full items-center gap-2 pl-3">
        <KilroyMark size={18} />
        <span className="text-[12px] font-semibold tracking-tight text-ink">
          Kilroy
        </span>
        <span className="text-[11px] text-ink-subtle">— Local Engineering Runtime</span>
      </div>

      {/* center — status pill */}
      <div className="no-drag flex items-center gap-2">
        <div
          className={cn(
            "flex items-center gap-2 rounded-full border border-line bg-bg-1 px-2.5 py-0.5 text-[10.5px] uppercase tracking-wider text-ink-muted",
          )}
        >
          <span
            className={cn(
              "h-1.5 w-1.5 rounded-full",
              isThinking
                ? "bg-amber animate-pulse-amber"
                : "bg-ok",
            )}
          />
          <span>Mode</span>
          <span className="text-ink">{modeLabel}</span>
          <span className="text-ink-ghost">·</span>
          <span className="text-ink-muted">Local</span>
        </div>
      </div>

      {/* right — window controls */}
      <div className="no-drag flex h-full items-stretch">
        <WinBtn
          onClick={() => win.minimize()}
          aria-label="Minimize"
          tone="neutral"
        >
          <Minus className="h-3.5 w-3.5" />
        </WinBtn>
        <WinBtn
          onClick={() => win.toggleMaximize()}
          aria-label={maximized ? "Restore" : "Maximize"}
          tone="neutral"
        >
          {maximized ? (
            <Copy className="h-3 w-3" />
          ) : (
            <Square className="h-3 w-3" />
          )}
        </WinBtn>
        <WinBtn
          onClick={() => win.close()}
          aria-label="Close"
          tone="danger"
        >
          <X className="h-3.5 w-3.5" />
        </WinBtn>
      </div>
    </header>
  );
}

function WinBtn({
  children,
  tone,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  tone: "neutral" | "danger";
}) {
  return (
    <button
      {...rest}
      className={cn(
        "flex h-full w-11.5 items-center justify-center text-ink-muted transition-colors",
        tone === "neutral" && "hover:bg-bg-2 hover:text-ink",
        tone === "danger" && "hover:bg-err hover:text-ink",
      )}
    />
  );
}
