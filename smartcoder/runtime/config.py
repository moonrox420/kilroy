"""
Runtime configuration — AppConfig dataclass with validation.

Extracted from the original kilroy_smartcoder.py's AppConfig, setup_logging,
and supporting functions. SmartCoderController uses this to bootstrap itself;
nothing in this module knows about agents, tasks, or workflows.

REFACTOR NOTES (see remediation PRD):
  * P1-1: `sandbox` previously defaulted to `"docker"` here but `"local"` in
    `cli/parser.py` — meaning the *same* config had two different real-world
    defaults depending on whether it was built via the CLI or constructed
    directly (as tests and any non-CLI integration do). Resolved by
    defaulting to `"local"` in both places, since that's what actually works
    without a Docker daemon installed/running, and is what the CLI already
    defaulted to for real invocations. If Docker should actually be the
    preferred default once it's reliably available, flip both defaults
    together — see the PRD's open question on this.
  * P1-2: `max_steps` had no upper bound here, but `agents/coding_assistant.py`
    silently clamped it to a hard-coded `8` — below this module's own
    default of 12. Added a real upper bound (`constants.MAX_AGENT_STEPS`) so
    the two layers can't disagree, and `coding_assistant.py` now logs when it
    actually has to clamp (it shouldn't, since this validates first).
"""

from __future__ import annotations

import dataclasses
import logging
from dataclasses import dataclass, field
from pathlib import Path

from typing import Literal

from smartcoder.runtime.constants import (
    DEFAULT_AUTHORIZED_IMPORTS,
    DEFAULT_OLLAMA_HOST,
    DEFAULT_OLLAMA_MODEL,
    MAX_AGENT_STEPS,
    VALID_BACKENDS,
    VALID_SANDBOXES,
    _normalize_ollama_host,
)

# Derive a precise Literal from VALID_SANDBOXES so adding a sandbox in one
# place automatically tightens the type throughout the codebase.
# Built explicitly (rather than via PEP 646 unpacking) for compatibility with
# Python versions where `Literal[*tuple]` is not yet supported by type
# checkers. The runtime assertion below keeps VALID_SANDBOXES as the source
# of truth — if you add a sandbox there, extend the Literal here too.
assert set(VALID_SANDBOXES) == {"local", "docker"}, (
    "VALID_SANDBOXES changed without updating SandboxName — "
    "extend the Literal in runtime/config.py to match."
)
SandboxName = Literal["local", "docker"]


@dataclass
class AppConfig:
    """Validated runtime configuration."""

    # Identity / logging
    name: str = "smartcoder"
    log_level: str = "INFO"

    # Model backend
    backend: str = "ollama"
    model_name: str = DEFAULT_OLLAMA_MODEL
    ollama_host: str = DEFAULT_OLLAMA_HOST
    llama_model_path: str | None = None
    temperature: float = 0.2  # low temp: precise, deterministic coding
    max_tokens: int = 2048
    num_ctx: int = 8192  # ollama context window

    # Project grounding (Kilroy bridge)
    context_file: str | None = None
    project_root: str | None = None
    use_dataset_rag: bool = False
    task_role: str | None = None
    task_type: str | None = None

    # Retrieval / datasets (optional legacy HF RAG)
    embedding_model: str = "BAAI/bge-small-en-v1.5"
    index_dir: str = "vector_store"
    datasets: tuple[str, ...] = ()
    max_items_per_dataset: int | None = 5_000
    force_rebuild: bool = False

    # Agent execution
    # NOTE (P1-1): must match cli/parser.py's `--sandbox` default. "local"
    # runs in-process (default; needs no extra setup). "docker" targets a
    # LOCAL Docker daemon (opt-in — pass --sandbox docker) and is more
    # isolated, but requires Docker to be installed and running.
    sandbox: SandboxName = "local"
    max_steps: int = 12
    use_web_search: bool = False
    authorized_imports: list[str] = field(
        default_factory=lambda: list(DEFAULT_AUTHORIZED_IMPORTS)
    )

    # Context truncation limits (0 = use built-in defaults baked into
    # context.py / the context-file JSON itself). Wired through to
    # `context.format_kilroy_context()` by `CodingAssistant._compose_task`
    # — see P2-1 in the remediation PRD (previously declared but unused).
    max_context_chunks: int = 0
    max_context_decisions: int = 0
    max_recent_messages: int = 0

    def __post_init__(self) -> None:
        object.__setattr__(
            self, "ollama_host", _normalize_ollama_host(self.ollama_host)
        )
        if self.backend not in VALID_BACKENDS:
            raise ValueError(
                f"Invalid backend {self.backend!r}. Choose from {VALID_BACKENDS}."
            )
        if self.sandbox not in VALID_SANDBOXES:
            raise ValueError(
                f"Invalid sandbox {self.sandbox!r}. Choose from {VALID_SANDBOXES}."
            )
        if self.backend == "llama_cpp" and not self.llama_model_path:
            raise ValueError(
                "backend 'llama_cpp' requires --llama-model-path to a .gguf file."
            )
        if self.backend == "llama_cpp" and self.llama_model_path:
            if not Path(self.llama_model_path).is_file():
                raise FileNotFoundError(
                    f"GGUF model not found: {self.llama_model_path}"
                )
        # Validate optional path fields when provided.
        if self.context_file and not Path(self.context_file).is_file():
            raise FileNotFoundError(f"context_file not found: {self.context_file}")
        if self.project_root and not Path(self.project_root).is_dir():
            raise FileNotFoundError(f"project_root not found: {self.project_root}")
        # Bounds validation — prevents silent failures and OOM from out-of-range values.
        if not 0.0 <= self.temperature <= 2.0:
            raise ValueError(f"temperature must be 0.0–2.0, got {self.temperature}")
        if self.max_tokens < 1:
            raise ValueError(f"max_tokens must be ≥ 1, got {self.max_tokens}")
        if self.num_ctx < 1:
            raise ValueError(f"num_ctx must be ≥ 1, got {self.num_ctx}")
        if not 1 <= self.max_steps <= MAX_AGENT_STEPS:
            raise ValueError(
                f"max_steps must be between 1 and {MAX_AGENT_STEPS}, got {self.max_steps}"
            )
        if self.max_items_per_dataset is not None and self.max_items_per_dataset < 0:
            raise ValueError(
                f"max_items_per_dataset must be ≥ 0 or None, got {self.max_items_per_dataset}"
            )
        # authorized_imports is explicitly allowed to be an empty list (a caller
        # may legitimately want a bare-builtins sandbox) but must not be left
        # as None — that previously produced a silently-empty allow-list via
        # `getattr(...)` fallbacks that never triggered (P0-3). Normalize here
        # so every downstream consumer can assume a list.
        if self.authorized_imports is None:
            object.__setattr__(
                self, "authorized_imports", list(DEFAULT_AUTHORIZED_IMPORTS)
            )

    @property
    def verbosity_level(self) -> int:
        # smolagents verbosity is an IntEnum: 0=OFF/ERROR, 1=INFO, 2=DEBUG.
        return {"DEBUG": 2, "INFO": 1, "WARNING": 1, "ERROR": 1, "CRITICAL": 1}.get(
            self.log_level.upper(), 0
        )

    def with_role(self, role: str) -> AppConfig:
        """Return a new config with the given task_role, preserving all other fields."""
        task_type = {
            "planner": "plan",
            "architect": "analysis",
            "developer": "code",
            "qa": "test",
            "reviewer": "review",
        }.get(role, "code")
        return dataclasses.replace(self, task_role=role, task_type=task_type)


def setup_logging(level: str) -> None:
    """Configure root logger for SmartCoder. Guard against duplicate handlers."""
    root = logging.getLogger()
    if not root.handlers:
        logging.basicConfig(
            level=getattr(logging, level.upper(), logging.INFO),
            format="%(asctime)s | %(levelname)-8s | %(name)s | %(message)s",
            datefmt="%H:%M:%S",
        )
