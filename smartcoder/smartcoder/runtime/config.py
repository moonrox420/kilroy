"""
Runtime configuration — AppConfig dataclass with validation.

FIXES:
  * P2-3: Removed object.__setattr__() from a non-frozen dataclass. Plain
    assignment is correct and less misleading.
  * P3-6: with_role() now maps all defined roles (orchestrator, engineer,
    coder, tester) instead of silently defaulting unknown roles to "code".
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
    temperature: float = 0.2
    max_tokens: int = 2048
    num_ctx: int = 8192

    # Project grounding
    context_file: str | None = None
    project_root: str | None = None
    use_dataset_rag: bool = False
    task_role: str | None = None
    task_type: str | None = None

    # Retrieval / datasets
    embedding_model: str = "BAAI/bge-small-en-v1.5"
    index_dir: str = "vector_store"
    datasets: tuple[str, ...] = ()
    max_items_per_dataset: int | None = 5_000
    force_rebuild: bool = False

    # Agent execution
    sandbox: SandboxName = "local"
    max_steps: int = 12
    use_web_search: bool = False
    authorized_imports: list[str] = field(default_factory=lambda: list(DEFAULT_AUTHORIZED_IMPORTS))

    # Context truncation limits
    max_context_chunks: int = 0
    max_context_decisions: int = 0
    max_recent_messages: int = 0

    def __post_init__(self) -> None:
        # P2-3: plain assignment — this is not a frozen dataclass.
        self.ollama_host = _normalize_ollama_host(self.ollama_host)

        if self.backend not in VALID_BACKENDS:
            raise ValueError(f"Invalid backend {self.backend!r}. Choose from {VALID_BACKENDS}.")
        if self.sandbox not in VALID_SANDBOXES:
            raise ValueError(f"Invalid sandbox {self.sandbox!r}. Choose from {VALID_SANDBOXES}.")
        if self.backend == "llama_cpp" and not self.llama_model_path:
            raise ValueError("backend 'llama_cpp' requires --llama-model-path to a .gguf file.")
        if self.backend == "llama_cpp" and self.llama_model_path:
            if not Path(self.llama_model_path).is_file():
                raise FileNotFoundError(f"GGUF model not found: {self.llama_model_path}")
        if self.context_file and not Path(self.context_file).is_file():
            raise FileNotFoundError(f"context_file not found: {self.context_file}")
        if self.project_root and not Path(self.project_root).is_dir():
            raise FileNotFoundError(f"project_root not found: {self.project_root}")
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
        # Normalize None → default list so consumers can always assume a list.
        if self.authorized_imports is None:
            self.authorized_imports = list(DEFAULT_AUTHORIZED_IMPORTS)

    @property
    def verbosity_level(self) -> int:
        return {"DEBUG": 2, "INFO": 1, "WARNING": 1, "ERROR": 1, "CRITICAL": 1}.get(
            self.log_level.upper(), 0
        )

    def with_role(self, role: str) -> AppConfig:
        """Return a new config with the given task_role, preserving all other fields.

        P3-6: All defined roles are now mapped. Previously orchestrator, engineer,
        coder, and tester silently fell back to "code" task type.
        """
        _role_to_task_type: dict[str, str] = {
            "planner": "plan",
            "architect": "analysis",
            "developer": "code",
            "coder": "code",
            "engineer": "code",
            "qa": "test",
            "tester": "test",
            "reviewer": "review",
            "orchestrator": "analysis",
        }
        task_type = _role_to_task_type.get(role.lower().strip(), "code")
        return dataclasses.replace(self, task_role=role, task_type=task_type)


def setup_logging(level: str) -> None:
    """Configure root logger. Guard against duplicate handlers."""
    root = logging.getLogger()
    if not root.handlers:
        logging.basicConfig(
            level=getattr(logging, level.upper(), logging.INFO),
            format="%(asctime)s | %(levelname)-8s | %(name)s | %(message)s",
            datefmt="%H:%M:%S",
        )
