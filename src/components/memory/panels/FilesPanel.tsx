/**
 * Files tab — every file the indexer has chunked + embedded.
 *
 * Click a file path to open it in the editor. The list also shows
 * how many files are indexed and the last indexing summary so the
 * user knows whether they should re-run Index Project.
 */
import { useEffect, useMemo, useState } from "react";
import { File, RotateCcw } from "lucide-react";
import { memory, type SearchResult } from "@/lib/tauri";
import { useMemory } from "@/store/memory";
import { useWorkspace } from "@/store/workspace";
import { useMemoryPanel } from "@/store/memoryPanel";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { EmptyState } from "../MemoryPanel";

export function FilesPanel() {
  const project = useMemory((s) => s.project);
  const indexing = useMemory((s) => s.indexing);
  const beginIndex = useMemory((s) => s.beginIndex);
  const lastIndex = useMemory((s) => s.lastIndex);
  const openFile = useWorkspace((s) => s.openFile);
  const closePanel = useMemoryPanel((s) => s.close);

  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult | null>(null);
  const [searching, setSearching] = useState(false);

  // Indexed files come from the most recent search OR a placeholder
  // until the user searches. We surface the recent matches list to give
  // a sense of what's actually retrievable, instead of dumping all paths.
  const onSearch = async () => {
    if (!query.trim() || !project) return;
    setSearching(true);
    try {
      const r = await memory.searchMemory(query.trim(), 30);
      setResults(r);
    } catch (err) {
      console.error("searchMemory:", err);
      setResults({ chunks: [], decisions: [] });
    } finally {
      setSearching(false);
    }
  };

  useEffect(() => {
    setResults(null);
  }, [project?.id]);

  const uniqueFiles = useMemo(() => {
    if (!results) return null;
    const seen = new Set<string>();
    const out: string[] = [];
    for (const c of results.chunks) {
      if (!seen.has(c.file_path)) {
        seen.add(c.file_path);
        out.push(c.file_path);
      }
    }
    return out;
  }, [results]);

  if (!project) return <EmptyState title="No project" body="Open a folder first." />;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-col gap-2 border-b border-line px-3 py-2">
        <div className="flex items-center justify-between gap-2">
          <span className="text-[11px] text-ink-subtle">
            {lastIndex
              ? `${lastIndex.chunks_inserted} chunks across ${lastIndex.files_indexed} files (${lastIndex.duration_ms}ms)`
              : "Project not indexed yet"}
          </span>
          <Button
            variant="default"
            size="sm"
            onClick={() => beginIndex()}
            disabled={indexing}
          >
            <RotateCcw className="h-3 w-3" />
            {indexing ? "Indexing…" : "Re-index"}
          </Button>
        </div>
        <div className="flex items-center gap-2">
          <Input
            placeholder="Semantic search (e.g. 'function that resizes the terminal')"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && onSearch()}
          />
          <Button onClick={onSearch} disabled={searching || !query.trim()}>
            Search
          </Button>
        </div>
      </div>
      {results === null ? (
        <EmptyState
          title="Search the index"
          body="Type a natural-language query above to find matching chunks. Kilroy embeds your query and runs k-NN over chunk embeddings."
        />
      ) : results.chunks.length === 0 ? (
        <EmptyState
          title="No matches"
          body={
            indexing
              ? "Indexing in progress…"
              : "Try a different query, or re-index the project if you've added new files."
          }
        />
      ) : (
        <div className="flex flex-1 min-h-0 flex-col gap-2 overflow-y-auto p-3">
          <section>
            <h3 className="mb-1 text-[10px] uppercase tracking-wider text-ink-subtle">
              Files mentioned ({uniqueFiles?.length ?? 0})
            </h3>
            <ul className="flex flex-col gap-0.5">
              {uniqueFiles?.map((f) => (
                <li key={f}>
                  <button
                    onClick={() => {
                      const root = project.root_path;
                      const abs = joinPath(root, f);
                      void openFile(abs);
                      closePanel();
                    }}
                    className="flex w-full items-center gap-2 rounded-sm px-1 py-0.5 text-left hover:bg-bg-2"
                  >
                    <File className="h-3 w-3 text-ink-subtle" />
                    <span className="truncate text-[12px] text-ink">{f}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
          <section>
            <h3 className="mb-1 text-[10px] uppercase tracking-wider text-ink-subtle">
              Top chunks ({results.chunks.length})
            </h3>
            <ul className="flex flex-col gap-1.5">
              {results.chunks.map((c) => (
                <li
                  key={c.chunk_id}
                  className="rounded-md border border-line bg-bg-0/50 p-2"
                >
                  <header className="flex items-center justify-between gap-2 text-[11px]">
                    <span className="truncate text-ink">
                      {c.file_path}
                      <span className="text-ink-subtle">
                        :{c.start_line}–{c.end_line}
                      </span>
                    </span>
                    <span className="shrink-0 font-mono text-[10px] text-ink-ghost">
                      d={c.distance.toFixed(3)}
                    </span>
                  </header>
                  <pre className="mt-1 max-h-[140px] overflow-auto whitespace-pre-wrap font-mono text-[10.5px] leading-snug text-ink-muted">
                    {c.content}
                  </pre>
                </li>
              ))}
            </ul>
          </section>
        </div>
      )}
    </div>
  );
}

function joinPath(root: string, rel: string): string {
  const sep = root.includes("\\") ? "\\" : "/";
  if (root.endsWith(sep)) return root + rel.replace(/[\\/]/g, sep);
  return root + sep + rel.replace(/[\\/]/g, sep);
}
