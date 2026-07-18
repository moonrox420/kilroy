"""
Constants and shared literals used throughout SmartCoder.

All hard-coded values, enums, and default configurations live here
so they can be changed in one place without hunting through layers.

REFACTOR NOTES:
  * P1-7: ALLOWED_EXTRA_IMPORTS is now the single authoritative allowlist
    for imports the agent may use. DEFAULT_AUTHORIZED_IMPORTS is derived
    from it so the two can never diverge. Previously they were maintained
    separately in constants.py and coding_assistant.py; that split caused
    silent ValueError on every default CLI invocation whenever anyone added
    a module to one list but forgot the other.
  * P2-10 (Ollama): warn_if_ollama_defaults_unset() unchanged — emitted
    lazily, not at import time.
  * P2-8 (SSRF note): _normalize_ollama_host unchanged; see prior notes.
"""

from __future__ import annotations

import logging
import os
from urllib.parse import urlparse

logger = logging.getLogger("smartcoder")

# =============================================================================
# Backend definitions
# =============================================================================

VALID_BACKENDS = ("ollama", "langchain_ollama", "llama_cpp")
VALID_SANDBOXES = ("local", "docker")

# =============================================================================
# Ollama host helpers
# =============================================================================

_ALLOWED_HOST_SCHEMES = ("http", "https")


def _normalize_ollama_host(url: str) -> str:
    """Sanity-check an Ollama host URL and ensure it has a scheme.

    This is a *sanity check*, not a general SSRF defense: it verifies the
    value looks like a plausible HTTP(S) endpoint and rejects non-http(s)
    schemes. It does not block loopback, link-local, or metadata addresses.
    """
    trimmed = url.strip()
    if not trimmed:
        return "http://localhost:11434"
    if "://" not in trimmed:
        trimmed = f"http://{trimmed}"
    parsed = urlparse(trimmed)
    if parsed.scheme not in _ALLOWED_HOST_SCHEMES:
        raise ValueError(
            f"Invalid Ollama host: {url!r} — scheme must be one of "
            f"{_ALLOWED_HOST_SCHEMES}, got {parsed.scheme!r}"
        )
    if not parsed.hostname or parsed.hostname in ("", "/"):
        raise ValueError(f"Invalid Ollama host: {url!r} — could not resolve hostname")
    return trimmed


DEFAULT_OLLAMA_HOST = _normalize_ollama_host(os.environ.get("OLLAMA_HOST", ""))
DEFAULT_OLLAMA_MODEL = "qwen2.5-coder:14b-instruct-q8_0"


def warn_if_ollama_defaults_unset() -> None:
    """Emit warnings about unset OLLAMA_HOST/OLLAMA_MODEL only when relevant."""
    if not os.environ.get("OLLAMA_HOST"):
        logger.warning("OLLAMA_HOST env var not set — using default %s", DEFAULT_OLLAMA_HOST)
    if not os.environ.get("OLLAMA_MODEL"):
        logger.warning("OLLAMA_MODEL env var not set — using default %s", DEFAULT_OLLAMA_MODEL)


# =============================================================================
# Agent instructions
# =============================================================================

KILROY_AGENT_INSTRUCTIONS = """\
You are Kilroy's agent (SmartCoder) — an elite senior software engineer working on the user's \
OPEN PROJECT. You produce correct, complete, production-grade output and VERIFY code by executing it.

Operating procedure:
1. Ground every solution in the project context provided in the task (file list, retrieved \
code chunks, prior decisions). Match existing style, paths, and conventions. Never fabricate \
file contents you have not been shown.
2. **Explanation / Q&A** (what does X do, how does Y work, describe this project): if the answer \
is already in the provided context, call `final_answer(...)` immediately in step 1. Do NOT execute \
Python to re-read files shown in context. Do NOT use `open()` for files already quoted below.
3. **Implementation tasks** (fix, add, build, refactor): write clean, fully-implemented code — \
never placeholders, TODOs, `pass`, or omitted logic. Execute your code to test it. If it raises, \
read the traceback, fix the ROOT CAUSE, and re-run until it works.
4. Prefer the standard library and dependencies already present in the project.
5. Finish with `final_answer(...)` containing your deliverable plus a short explanation.

VIRTUALENV RULES (mandatory):
- If the project has `.venv` or `venv`, NEVER delete, recreate, or replace it.
- NEVER run `git clean -fdx`, `Remove-Item -Recurse .venv`, `rm -rf .venv`, or similar.
- NEVER pip-install into a fresh environment when a project venv already exists — use the existing one.
- Do not assume Python packages are missing; Kilroy launches SmartCoder with the project venv automatically.

FILE EDITING OUTPUT FORMAT (when proposing repo changes):
* For EDITS to an existing file, emit a unified diff in a fenced block whose info string \
is `diff` followed by the path, e.g. ```diff src/lib.rs```. Include `--- a/<path>` and \
`+++ b/<path>` headers plus standard `@@` hunks.
* For BRAND NEW files, emit the complete contents in a fenced block whose info string is \
the language followed by the path, e.g. ```rust src/new.rs```.
* For shell commands, use ```powershell or ```bash.
"""

PROJECT_CODING_INSTRUCTIONS = KILROY_AGENT_INSTRUCTIONS

STUCK_CLAUSE = (
    "If the task is under-specified or the retrieved context is insufficient to complete it "
    "confidently, STOP and emit a single line `BLOCKED: <reason>` followed by what you'd need "
    "to proceed. Do not fabricate file contents you have not been shown.\n\n"
    "OUTPUT FORMAT (mandatory — the executor parses <code> blocks with regex):\n"
    "  Thoughts: Your reasoning about why the task is blocked.\n"
    "  <code>\n"
    '  final_answer("BLOCKED: <reason>. What I need: <clarification>")\n'
    "  </code>\n"
    "Wrap the BLOCKED response inside a single final_answer() call within <code> tags. "
    "Do NOT emit bare text without the <code>final_answer(...)</code> wrapper."
)

DATASET_CODING_INSTRUCTIONS = """\
You are SmartCoder with Hugging Face dataset RAG enabled. For non-trivial tasks, call the \
`retriever` tool first, then write, execute, and self-correct until the code works. \
Finish with `final_answer(...)`.
"""

# =============================================================================
# Authorized imports
# =============================================================================

# SINGLE source of truth for what the agent is allowed to import.
# coding_assistant.py derives its allowlist from this set — the two must
# never be maintained separately (P1-7). Excluded: os, sys (shell escape risk).
ALLOWED_EXTRA_IMPORTS: frozenset[str] = frozenset(
    {
        "math",
        "datetime",
        "re",
        "json",
        "collections",
        "itertools",
        "functools",
        "statistics",
        "random",
        "pathlib",
        "typing",
        "dataclasses",
        "enum",
        "heapq",
        "bisect",
        "copy",
        "hashlib",
        "string",
        "io",
        "csv",
        "decimal",
        "fractions",
        "operator",
        "unittest",
        "pytest",
        "textwrap",
        "abc",
        "contextlib",
    }
)

# Stable sorted list — used by AppConfig and CLI defaults.
DEFAULT_AUTHORIZED_IMPORTS: list[str] = sorted(ALLOWED_EXTRA_IMPORTS)

# Hard ceiling on CodeAgent max_steps.
MAX_AGENT_STEPS = 20
