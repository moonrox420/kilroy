"""
Constants and shared literals used throughout SmartCoder.

All hard-coded values, enums, and default configurations live here
so they can be changed in one place without hunting through layers.

REFACTOR NOTES (see remediation PRD):
  * P2-10: `DEFAULT_OLLAMA_HOST`/`DEFAULT_OLLAMA_MODEL` used to log a
    `logger.warning(...)` as an *import-time side effect* — meaning every
    single import of `smartcoder` (including `list-datasets`, `--help`, or
    a llama_cpp-backend run that never touches Ollama) printed a spurious
    warning, before `setup_logging()` had even configured formatting. The
    values are still computed eagerly (they're just constants), but the
    warnings are now emitted lazily via `warn_if_ollama_defaults_unset()`,
    which callers invoke only when the ollama backend is actually selected
    (see `infrastructure/models.py::build_model`).
  * P2-8: `_normalize_ollama_host`'s previous comment claimed to "reject
    SSRF payloads," which overstated what the code did (it only checked
    that *some* hostname was present). This version keeps that same
    modest scope but is honest about it, and additionally restricts the
    scheme to http/https so at least `file://`, `gopher://`, etc. can't
    slip through a config typo. This is still not a general SSRF defense
    (no loopback/link-local/metadata-IP blocking) — see the PRD's open
    question about whether `ollama_host` should ever be treated as
    untrusted input at all.
"""

from __future__ import annotations

import logging
import os
from urllib.parse import urlparse

logger = logging.getLogger("smartcoder")

# =============================================================================
# Backend definitions
# =============================================================================

# Backends we support. Local-only on purpose: cloud backends (OpenAI, HF
# Inference) were removed — they required API keys / tokens, which Kilroy's
# fully-local policy forbids.
VALID_BACKENDS = ("ollama", "langchain_ollama", "llama_cpp")
# "e2b" was removed for the same reason: it is a cloud sandbox behind an API
# key. "local" runs in-process; "docker" targets the LOCAL Docker daemon.
VALID_SANDBOXES = ("local", "docker")

# =============================================================================
# Ollama host helpers
# =============================================================================

_ALLOWED_HOST_SCHEMES = ("http", "https")


def _normalize_ollama_host(url: str) -> str:
    """Ensure a scheme and a resolvable hostname — LiteLLM rejects bare
    `host:port`. This is a *sanity check*, not a general SSRF defense: it
    verifies the value looks like a plausible local/remote HTTP(S) endpoint
    and restricts the scheme to http/https. It does not block loopback,
    link-local, or cloud-metadata addresses — `ollama_host` is expected to
    come from Kilroy's own trusted local settings, not from untrusted
    project/network content. If that assumption ever changes, this function
    needs real SSRF hardening (host allow-list or IP-range denial).
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

# Default coding model when --model is not passed. Kilroy forwards its
# Settings -> chat model via --model, so this is mainly for standalone CLI use.
DEFAULT_OLLAMA_MODEL = "qwen2.5-coder:14b-instruct-q8_0"


def warn_if_ollama_defaults_unset() -> None:
    """Emit informational warnings about unset OLLAMA_HOST/OLLAMA_MODEL env
    vars, but only when a caller actually cares (i.e. the ollama backend is
    in use). Call this from the backend-selection path, not at import time —
    see P2-10 in the remediation PRD for why the old eager version was a
    problem (spurious warnings for commands that never touch Ollama, printed
    before `setup_logging()` had configured formatting).
    """
    if not os.environ.get("OLLAMA_HOST"):
        logger.warning(
            "OLLAMA_HOST env var not set — using default %s", DEFAULT_OLLAMA_HOST
        )
    if not os.environ.get("OLLAMA_MODEL"):
        logger.warning(
            "OLLAMA_MODEL env var not set — using default %s", DEFAULT_OLLAMA_MODEL
        )


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

# Backward-compatible alias for tests and external references.
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

DEFAULT_AUTHORIZED_IMPORTS = [
    "json",
    "re",
    "math",
    "statistics",
    "random",
    "itertools",
    "collections",
    "functools",
    "datetime",
    "pathlib",
    "typing",
    "textwrap",
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
]

# Hard ceiling on CodeAgent max_steps (see coding_assistant.py). Kept as a
# named constant (was a bare literal `8`) so the default (12, see
# runtime/config.py) doesn't silently get clamped below its own default —
# P1-2 in the remediation PRD.
MAX_AGENT_STEPS = 20
