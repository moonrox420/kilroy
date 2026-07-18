/**
 * Recursive file tree — lazily lists children on expand.
 *
 * Folders cache their child entries the first time they're opened.
 * Clicking a file routes through the workspace store, which adds a tab
 * and reads the contents.
 */
import { useEffect, useState, useCallback } from "react";
import { ChevronRight, File, Folder, FolderOpen } from "lucide-react";
import { fs, type DirEntry } from "@/lib/tauri";
import { useWorkspace } from "@/store/workspace";
import { cn } from "@/lib/utils";

interface TreeProps {
  rootPath: string;
}

export function FileTree({ rootPath }: TreeProps) {
  return (
    <div className="py-1 text-[12px] text-ink-muted">
      <DirNode path={rootPath} name={lastSegment(rootPath)} depth={0} initiallyOpen />
    </div>
  );
}

function DirNode({
  path,
  name,
  depth,
  initiallyOpen = false,
}: {
  path: string;
  name: string;
  depth: number;
  initiallyOpen?: boolean;
}) {
  const [open, setOpen] = useState(initiallyOpen);
  const [entries, setEntries] = useState<DirEntry[] | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || entries) return;
    let cancelled = false;
    setLoading(true);
    fs.listDir(path)
      .then((res) => {
        if (!cancelled) setEntries(res);
      })
      .catch((err) => {
        console.error("listDir", path, err);
        if (!cancelled) setEntries([]);
      })
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [open, path, entries]);

  return (
    <div>
      <Row depth={depth} onClick={() => setOpen((v) => !v)}>
        <ChevronRight
          className={cn(
            "h-3 w-3 shrink-0 transition-transform",
            open && "rotate-90",
          )}
        />
        {open ? (
          <FolderOpen className="h-3.5 w-3.5 shrink-0 text-amber/80" />
        ) : (
          <Folder className="h-3.5 w-3.5 shrink-0 text-ink-subtle" />
        )}
        <span className="truncate">{name}</span>
      </Row>
      {open && (
        <div>
          {loading && (
            <Row depth={depth + 1}>
              <span className="text-ink-ghost text-[11px]">loading…</span>
            </Row>
          )}
          {entries?.map((e) =>
            e.is_dir ? (
              <DirNode key={e.path} path={e.path} name={e.name} depth={depth + 1} />
            ) : (
              <FileNode key={e.path} entry={e} depth={depth + 1} />
            ),
          )}
          {entries && entries.length === 0 && !loading && (
            <Row depth={depth + 1}>
              <span className="text-ink-ghost text-[11px]">empty</span>
            </Row>
          )}
        </div>
      )}
    </div>
  );
}

function FileNode({ entry, depth }: { entry: DirEntry; depth: number }) {
  const openFile = useWorkspace((s) => s.openFile);
  const activePath = useWorkspace((s) => s.activePath);
  const isActive = activePath === entry.path;

  const onOpen = useCallback(() => {
    void openFile(entry.path);
  }, [entry.path, openFile]);

  return (
    <Row depth={depth} onClick={onOpen} active={isActive}>
      <span className="h-3 w-3 shrink-0" />
      <File className="h-3.5 w-3.5 shrink-0 text-ink-subtle" />
      <span className="truncate">{entry.name}</span>
    </Row>
  );
}

function Row({
  depth,
  active,
  onClick,
  children,
}: {
  depth: number;
  active?: boolean;
  onClick?: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      style={{ paddingLeft: 8 + depth * 12 }}
      className={cn(
        "flex w-full items-center gap-1.5 truncate rounded-sm px-1.5 py-[3px] text-left transition-colors",
        active
          ? "bg-bg-3 text-ink"
          : "text-ink-muted hover:bg-bg-2 hover:text-ink",
      )}
    >
      {children}
    </button>
  );
}

function lastSegment(path: string): string {
  const norm = path.replace(/\\/g, "/").replace(/\/$/, "");
  const i = norm.lastIndexOf("/");
  return i === -1 ? norm : norm.slice(i + 1);
}
