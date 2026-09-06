/**
 * Splits an agent chat message into prose + code-block segments.
 *
 * We don't pull a full markdown engine — the agent's output is plain
 * text with fenced ``` blocks plus the occasional inline `code` span.
 * That's all we need to special-case. Everything else renders as a
 * preserved-whitespace paragraph so the agent's own line breaks survive.
 *
 * Code fences understood:
 *     ```                              → no language, no path
 *     ```python                        → language only
 *     ```python src/foo.py             → language + path
 *     ```diff path=src/foo.py          → diff with explicit path token
 */
import { ChatCodeBlock } from "./ChatCodeBlock";

interface Segment {
  type: "text" | "code";
  value: string;
  language?: string;
  path?: string;
}

function parseInfoString(info: string): { language?: string; path?: string } {
  const tokens = info.trim().split(/\s+/).filter(Boolean);
  let language: string | undefined;
  let path: string | undefined;
  for (const t of tokens) {
    if (t.startsWith("path=")) {
      path = t.slice(5);
    } else if (t.startsWith("file=")) {
      path = t.slice(5);
    } else if (!language && /^[A-Za-z0-9_+#-]+$/.test(t) && !t.includes(".")) {
      // single-word bareword without a dot → language
      language = t;
    } else if (
      !path &&
      (t.includes("/") || t.includes("\\") || /\.\w+$/.test(t))
    ) {
      path = t;
    }
  }
  return { language, path };
}

function parseContent(text: string): Segment[] {
  const out: Segment[] = [];
  // Fenced block: ```info\n...```. Non-greedy body, requires closing fence on a line.
  const re = /```([^\n]*)\n([\s\S]*?)```/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) !== null) {
    if (match.index > lastIndex) {
      out.push({ type: "text", value: text.slice(lastIndex, match.index) });
    }
    const { language, path } = parseInfoString(match[1]);
    out.push({
      type: "code",
      value: match[2].replace(/\n$/, ""), // strip the final newline before the closing fence
      language,
      path,
    });
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < text.length) {
    out.push({ type: "text", value: text.slice(lastIndex) });
  }
  return out;
}

/** Render inline `code spans` inside a text segment. */
function renderInline(text: string): React.ReactNode[] {
  const out: React.ReactNode[] = [];
  const re = /`([^`\n]+)`/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let key = 0;
  while ((match = re.exec(text)) !== null) {
    if (match.index > lastIndex) {
      out.push(text.slice(lastIndex, match.index));
    }
    out.push(
      <code
        key={`c${key++}`}
        className="rounded-sm border border-line bg-bg-0 px-1 py-[1px] font-mono text-[11px] text-amber"
      >
        {match[1]}
      </code>,
    );
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < text.length) {
    out.push(text.slice(lastIndex));
  }
  return out;
}

export function ChatContent({ text }: { text: string }) {
  const segments = parseContent(text);
  return (
    <>
      {segments.map((s, i) =>
        s.type === "code" ? (
          <ChatCodeBlock
            key={i}
            language={s.language}
            path={s.path}
            code={s.value}
          />
        ) : (
          <span
            key={i}
            className="block whitespace-pre-wrap break-words"
          >
            {renderInline(s.value)}
          </span>
        ),
      )}
    </>
  );
}
