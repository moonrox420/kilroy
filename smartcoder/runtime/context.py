"""
Project context loading and formatting.

Extracted from the original kilroy_smartcoder.py's load_kilroy_context()
and format_kilroy_context() functions. These are responsible for reading
the JSON context file written by Kilroy's Rust bridge and rendering it
as a prompt preamble.

REFACTOR NOTES (see remediation PRD):
  * P1-7: `_sanitize()` used `text.replace("##", "\\#").replace("###", "\\###")`.
    Confirmed by direct repro that this (a) never escapes a single leading
    `#` (an H1), so untrusted content could inject a real, unescaped H1 into
    the assembled prompt, and (b) the second `.replace("###", ...)` call was
    dead code — the first `.replace("##", ...)` call already consumes every
    run of 2+ hashes before the second call ever runs, so no literal `"###"`
    substring survives to match it. Replaced with a single line-anchored
    regex that escapes any real ATX heading (1–6 `#` characters at the start
    of a line, followed by whitespace) and leaves inline `#` usage (hashtags,
    comments, etc.) untouched.
  * P2-1: `max_context_chunks` / `max_context_decisions` / `max_recent_messages`
    are now accepted as explicit overrides from `AppConfig` (threaded in by
    `CodingAssistant`), taking priority over the context-file JSON's own
    `max_context_chunks` etc. keys when set to a non-zero value. Previously
    `AppConfig` declared these fields but nothing ever read them — the caps
    were only ever sourced from the (Rust-bridge-authored) JSON file itself,
    with no CLI-level override possible.
"""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path
from typing import Any

logger = logging.getLogger("smartcoder.context")

# Matches a real ATX heading: 1-6 '#' characters at the very start of a
# line, followed by whitespace (per the CommonMark ATX heading rule — a
# bare "#" with no following space, or a "#" that isn't at line-start, is
# not a heading and is left alone).
_HEADING_LINE_RE = re.compile(r"(?m)^(#{1,6})(\s)")


def load_clinerules(project_root: str | Path) -> str:
    """Load every `.clinerules/*.md` file and concatenate them as markdown.

    Returns an empty string if the directory or any file is unreadable.
    The caller may prepend/append this to the agent's instructions.
    """
    root = Path(project_root)
    rules_dir = root / ".clinerules"
    if not rules_dir.is_dir():
        return ""

    parts: list[str] = []
    for md in sorted(rules_dir.glob("*.md")):
        try:
            text = md.read_text(encoding="utf-8").strip()
            if text:
                parts.append(f"\n\n## {md.name}\n{text}")
        except (OSError, UnicodeDecodeError) as exc:
            logger.warning("Skipping .clinerules/%s: %s", md.name, exc)

    return "".join(parts)


def load_kilroy_context(path: str) -> dict[str, Any]:
    """Load JSON project context written by Kilroy's Rust bridge."""
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except FileNotFoundError:
        logger.warning("Context file not found: %s", path)
        return {}
    except (PermissionError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"Invalid context file {path}: {exc}") from exc


def _sanitize(text: str) -> str:
    """Escape real ATX markdown headings to prevent prompt-injection via
    context data (untrusted project content flowing into `chunks`,
    `decisions`, `recent_messages`, or `note`)."""
    return _HEADING_LINE_RE.sub(lambda m: f"\\{m.group(1)}{m.group(2)}", text)


def format_kilroy_context(
    data: dict[str, Any],
    *,
    max_chunks_override: int | None = None,
    max_decisions_override: int | None = None,
    max_recent_override: int | None = None,
) -> str:
    """Render Kilroy project context as a prompt preamble.

    `*_override` values, when provided (non-None, non-zero), take priority
    over the equivalent `max_context_chunks` / `max_context_decisions` /
    `max_recent_messages` keys in `data` — this is how `AppConfig`'s fields
    of the same name actually reach this function (see P2-1).
    """
    parts: list[str] = ["# Kilroy project context"]

    overview = data.get("project_overview")
    if overview:
        parts.append(_sanitize(str(overview).strip()))

    chunks = data.get("chunks") or []
    if chunks:
        parts.append("\n## Retrieved code chunks")
        max_chunks = max_chunks_override or data.get("max_context_chunks", 12)
        for i, chunk in enumerate(chunks[:max_chunks], 1):
            file_path = chunk.get("file_path", "[unknown file]")
            start = chunk.get("start_line", "[?]")
            end = chunk.get("end_line", "[?]")
            raw_content = chunk.get("content", "")
            parts.append(
                f"\n### Chunk {i}: {file_path}:{start}-{end}\n{_sanitize(raw_content)}"
            )

    decisions = data.get("decisions") or []
    if decisions:
        parts.append("\n## Prior architectural decisions")
        max_decisions = max_decisions_override or data.get("max_context_decisions", 8)
        for d in decisions[:max_decisions]:
            title = d.get("title", "")
            summary = d.get("summary", "")
            parts.append(f"- **{title}**: {summary}")

    recent = data.get("recent_messages") or []
    if recent:
        parts.append("\n## Recent chat")
        max_recent = max_recent_override or data.get("max_recent_messages", 6)
        for msg in recent[-max_recent:]:
            role = msg.get("role", "user")
            content = str(msg.get("content", "")).strip()
            if content:
                preview = content if len(content) <= 600 else content[:600] + "\u2026"
                parts.append(f"\n**{role}**: {preview}")

    if data.get("note"):
        parts.append(f"\n## Note\n{data['note']}")

    indexed = data.get("indexed_chunk_count")
    if not indexed:
        parts.append(
            "\n## Indexing\nProject not indexed \u2014 you see file paths but limited file contents. "
            "Do not invent code for paths you have not been shown."
        )

    return "\n".join(parts)
