/**
 * Tab strip across the top of the editor.
 *
 * Click to activate, middle-click or X to close. Dirty tabs show an
 * unfilled dot in place of the close button until hovered.
 *
 * Right side carries a small lint-scope indicator: Monaco only ships
 * language servers for TS / JS / JSON / CSS / HTML, so files in those
 * languages get red-squiggle linting on errors; every other language
 * (Python, Rust, Go, PowerShell, etc.) gets token highlighting only.
 * The indicator pill makes that obvious so the user doesn't expect typo
 * detection on a .py file.
 */
import { Sparkles, X, Circle } from "lucide-react";
import { useWorkspace } from "@/store/workspace";
import { cn } from "@/lib/utils";

/** Set of Monaco language ids that have built-in diagnostic providers. */
const LINTED_LANGS = new Set([
  "typescript",
  "javascript",
  "json",
  "css",
  "scss",
  "less",
  "html",
]);

export function EditorTabs() {
  const tabs = useWorkspace((s) => s.openTabs);
  const activePath = useWorkspace((s) => s.activePath);
  const setActive = useWorkspace((s) => s.setActive);
  const closeTab = useWorkspace((s) => s.closeTab);

  if (tabs.length === 0) return null;

  const activeTab = tabs.find((t) => t.path === activePath);
  const lintedActive =
    activeTab && LINTED_LANGS.has(activeTab.language);

  return (
    <div className="flex h-8 shrink-0 items-end border-b border-line bg-bg-0">
      <div className="flex h-full flex-1 items-end overflow-x-auto">
        {tabs.map((t) => {
          const active = t.path === activePath;
          return (
            <div
              key={t.path}
              onClick={() => setActive(t.path)}
              onAuxClick={(e) => {
                if (e.button === 1) {
                  e.preventDefault();
                  closeTab(t.path);
                }
              }}
              className={cn(
                "group flex h-full cursor-pointer items-center gap-1.5 border-r border-line px-3 text-[12px] transition-colors",
                active
                  ? "border-t-2 border-t-amber bg-bg-1 text-ink"
                  : "border-t-2 border-t-transparent text-ink-muted hover:bg-bg-2 hover:text-ink",
              )}
            >
              <span className="max-w-[200px] truncate">{t.name}</span>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(t.path);
                }}
                className="ml-1 flex h-4 w-4 items-center justify-center rounded-sm hover:bg-bg-3"
                aria-label="Close tab"
              >
                {t.dirty ? (
                  <>
                    <Circle className="h-2.5 w-2.5 fill-current text-ink-muted group-hover:hidden" />
                    <X className="hidden h-3 w-3 text-ink group-hover:block" />
                  </>
                ) : (
                  <X className="h-3 w-3 text-ink-subtle group-hover:text-ink" />
                )}
              </button>
            </div>
          );
        })}
      </div>
      {activeTab && (
        <div
          className={cn(
            "mr-2 flex h-5 items-center gap-1 rounded-sm border px-1.5 self-center text-[9px] uppercase tracking-wider",
            lintedActive
              ? "border-ok/40 bg-ok/10 text-ok"
              : "border-line bg-bg-1 text-ink-subtle",
          )}
          title={
            lintedActive
              ? `${activeTab.language} files get full diagnostics: red squiggles on errors, autocompletion, hover info.`
              : `${activeTab.language} files get syntax colors only. Monaco doesn't ship a diagnostic provider for this language — error squiggles only appear in TS / JS / JSON / CSS / HTML files.`
          }
        >
          <Sparkles className="h-2.5 w-2.5" />
          {lintedActive ? "lint: on" : "colors only"}
        </div>
      )}
    </div>
  );
}
