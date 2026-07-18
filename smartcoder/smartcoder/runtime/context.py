"""
Project context loading and formatting.

FIXES:
  * P2-4: format_kilroy_context() now sanitizes file_path from untrusted
    context chunks. Previously only raw_content was sanitized; a crafted
    file_path containing newlines could inject ATX headings into the prompt.
  * P2-10: load_clinerules() now enforces per-file and total size caps
    (256 KB / 1 MB respectively) to prevent memory exhaustion from
    oversized or adversarial .clinerules files.
"""

from __future__ import annotations

import json
import logging
import re
from pathlib import Path
from typing import Any

logger = logging.getLogger("smartcoder.context")

# Matches a real ATX heading: 1-6 '#' at line start followed by whitespace.
_HEADING_LINE_RE = re.compile(r"(?m)^(#{1,6})(\s)")

# P2-10: size caps for .clinerules ingestion.
_MAX_CLINERULE_FILE_BYTES = 256 * 1024  # 256 KB per file
_MAX_CLINERULE_TOTAL_BYTES = 1024 * 1024  # 1 MB total


def load_clinerules(project_root: str | Path) -> str:
    """Load every .clinerules/*.md file and concatenate as markdown.

    P2-10: Files exceeding 256 KB are skipped with a warning. Total
    concatenated content is capped at 1 MB to prevent prompt bloat.
    """
    root = Path(project_root)
    rules_dir = root / ".clinerules"
    if not rules_dir.is_dir():
        return ""

    parts: list[str] = []
    total_bytes = 0

    for md in sorted(rules_dir.glob("*.md")):
        try:
            file_size = md.stat().st_size
            if file_size > _MAX_CLINERULE_FILE_BYTES:
                logger.warning(
                    "Skipping oversized .clinerules/%s (%d bytes > %d byte cap)",
                    md.name,
                    file_size,
                    _MAX_CLINERULE_FILE_BYTES,
                )
                continue

            if total_bytes >= _MAX_CLINERULE_TOTAL_BYTES:
                logger.warning(
                    "Stopping .clinerules ingestion at %s: total cap (%d bytes) reached",
                    md.name,
                    _MAX_CLINERULE_TOTAL_BYTES,
                )
                break

            text = md.read_text(encoding="utf-8").strip()
            if text:
                chunk = f"\n\n## {md.name}\n{text}"
                total_bytes += len(chunk.encode("utf-8"))
                parts.append(chunk)
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
    """Escape real ATX headings to prevent prompt-injection via untrusted content."""
    return _HEADING_LINE_RE.sub(lambda m: f"\\{m.group(1)}{m.group(2)}", text)


def _sanitize_path(text: str) -> str:
    """Sanitize a file path for safe inline use in a markdown header.

    Strips embedded newlines (which could break out of the heading line and
    inject new markdown structure) and then applies the standard heading
    escaper. P2-4 fix: previously file_path was interpolated raw.
    """
    # Remove any newlines or carriage-returns that could terminate the heading
    # line prematurely and allow heading injection on the next line.
    no_newlines = text.replace("\r", " ").replace("\n", " ")
    return _sanitize(no_newlines)


def format_kilroy_context(
    data: dict[str, Any],
    *,
    max_chunks_override: int | None = None,
    max_decisions_override: int | None = None,
    max_recent_override: int | None = None,
) -> str:
    """Render Kilroy project context as a prompt preamble."""
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
            # P2-4: sanitize file_path before embedding in the heading line.
            safe_path = _sanitize_path(str(file_path))
            parts.append(f"\n### Chunk {i}: {safe_path}:{start}-{end}\n{_sanitize(raw_content)}")

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
