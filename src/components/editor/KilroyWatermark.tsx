/**
 * Translucent watermark shown in the editor area when no file is open.
 *
 * Centers the Kilroy mark with a hairline ring, a short tagline, and
 * the keyboard hints for the most common actions.
 */
import { KilroyMark } from "@/components/common/KilroyMark";
import { useWorkspace } from "@/store/workspace";

export function KilroyWatermark() {
  const openFolder = useWorkspace((s) => s.openFolder);

  return (
    <div className="pointer-events-none absolute inset-0 flex select-none flex-col items-center justify-center">
      <div className="pointer-events-auto flex flex-col items-center gap-6">
        <div className="relative">
          <div className="absolute inset-0 -m-6 rounded-full bg-amber/5 blur-2xl" aria-hidden />
          <KilroyMark size={132} className="opacity-25" />
        </div>
        <div className="flex flex-col items-center gap-1">
          <p className="text-[20px] font-semibold tracking-tight text-ink-muted">
            Kilroy
          </p>
          <p className="max-w-[420px] text-center text-[12px] text-ink-subtle">
            Local AI engineering runtime. Open a folder to begin —
            the editor, terminal, and agent crew are waiting.
          </p>
        </div>
        <div className="grid grid-cols-2 gap-x-6 gap-y-1 text-[11px] text-ink-subtle">
          <Hint label="Open Folder" keys="Ctrl O" onClick={() => openFolder()} />
          <Hint label="Toggle Terminal" keys="Ctrl `" />
          <Hint label="Toggle Explorer" keys="Ctrl B" />
          <Hint label="Save File" keys="Ctrl S" />
        </div>
      </div>
    </div>
  );
}

function Hint({
  label,
  keys,
  onClick,
}: {
  label: string;
  keys: string;
  onClick?: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex items-center justify-between gap-3 rounded px-2 py-1 text-left hover:bg-bg-2 disabled:cursor-default"
      disabled={!onClick}
    >
      <span>{label}</span>
      <kbd className="rounded bg-bg-2 px-1.5 py-[1px] font-mono text-[10px] text-ink">
        {keys}
      </kbd>
    </button>
  );
}
