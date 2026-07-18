"""
CodingAssistant — the smolagents CodeAgent execution layer.

FIXES:
  * P1-7: ALLOWED_EXTRA_IMPORTS is now imported from constants.py (single
    source of truth). The local duplicate set has been removed. The two lists
    can no longer diverge and cause ValueError on default invocations.
  * P2-7: _build_agent() is now guarded by a threading.Lock to prevent a
    TOCTOU race where two concurrent callers both see self._agent is None
    and both build agents simultaneously.
"""

from __future__ import annotations

import concurrent.futures
import logging
import threading
from pathlib import Path
from typing import Any

from smartcoder.agents.roles import role_brief_for
from smartcoder.infrastructure.dependencies import DependencyManager
from smartcoder.infrastructure.models import build_model, build_web_search_tool
from smartcoder.runtime.config import AppConfig
from smartcoder.runtime.constants import (
    ALLOWED_EXTRA_IMPORTS,  # P1-7: single source; was a local duplicate set
    DATASET_CODING_INSTRUCTIONS,
    DEFAULT_AUTHORIZED_IMPORTS,
    KILROY_AGENT_INSTRUCTIONS,
    MAX_AGENT_STEPS,
    STUCK_CLAUSE,
)
from smartcoder.runtime.context import (
    format_kilroy_context,
    load_clinerules,
    load_kilroy_context,
)

logger = logging.getLogger("smartcoder.agent")


class TimeoutException(Exception):
    """Raised when agent execution exceeds the allowed time limit."""

    pass


class _TimeoutContext:
    """Context manager for timeout-based execution using concurrent.futures."""

    def __init__(self, seconds: int) -> None:
        self.seconds = seconds
        self._pool: concurrent.futures.ThreadPoolExecutor | None = None
        self._future: concurrent.futures.Future | None = None

    def __enter__(self) -> "_TimeoutContext":
        self._pool = concurrent.futures.ThreadPoolExecutor(max_workers=1)
        return self

    def __exit__(self, *args: Any) -> None:
        if self._future is not None and not self._future.done():
            self._future.cancel()
        if self._pool is not None:
            self._pool.shutdown(wait=False)

    def run(self, fn, *args, **kwargs):
        self._future = self._pool.submit(fn, *args, **kwargs)
        try:
            return self._future.result(timeout=self.seconds)
        except concurrent.futures.TimeoutError:
            self._future.cancel()
            raise TimeoutException(
                f"Agent execution exceeded {self.seconds}s timeout. "
                f"Possible infinite loop or model misconfiguration."
            )


class CodingAssistant:
    """Lazily assembles and runs the smolagents CodeAgent grounded by RAG."""

    def __init__(self, config: AppConfig, deps: DependencyManager) -> None:
        self.config = config
        self.deps = deps
        self._agent: Any | None = None
        self._agent_lock = threading.Lock()  # P2-7: prevent TOCTOU on lazy build

    def _build_agent(self) -> Any:
        # P2-7: fast path without lock (already built).
        if self._agent is not None:
            return self._agent

        with self._agent_lock:
            # Double-check inside the lock in case another thread just built it.
            if self._agent is not None:
                return self._agent

            self.deps.require("smolagents")

            if self.config.backend == "ollama":
                self.deps.require("litellm")
            elif self.config.backend == "langchain_ollama":
                self.deps.require("langchain_ollama", "langchain_core")

            from smolagents import CodeAgent, FinalAnswerTool

            authorized_imports = _sanitize_authorized_imports(
                self.config.authorized_imports or list(DEFAULT_AUTHORIZED_IMPORTS)
            )

            model = build_model(self.config, self.deps)

            tools: list[Any] = []

            if self.config.use_web_search:
                web_tool = build_web_search_tool()
                if web_tool is not None:
                    tools.append(web_tool)

            tools.append(FinalAnswerTool())

            instructions = KILROY_AGENT_INSTRUCTIONS

            if self.config.use_dataset_rag and self.config.datasets:
                self.deps.require(
                    "datasets",
                    "faiss",
                    "langchain_core",
                    "langchain_community",
                    "langchain_huggingface",
                    "langchain_text_splitters",
                    "sentence_transformers",
                )
                from smartcoder.infrastructure import retrieval

                retriever_tool = retrieval.build_retriever_tool(
                    dataset_keys=list(self.config.datasets),
                    embedding_model=self.config.embedding_model,
                    index_dir=self.config.index_dir,
                    max_items_per_dataset=self.config.max_items_per_dataset,
                    force_rebuild=self.config.force_rebuild,
                )

                tools.append(retriever_tool)
                instructions = DATASET_CODING_INSTRUCTIONS

            sandbox = self.config.sandbox

            requested_steps = self.config.max_steps
            capped_steps = min(max(requested_steps, 1), MAX_AGENT_STEPS)
            if capped_steps != requested_steps:
                logger.warning(
                    "max_steps=%d exceeds the hard ceiling (%d); clamping.",
                    requested_steps,
                    MAX_AGENT_STEPS,
                )

            self._agent = CodeAgent(
                tools=tools,
                model=model,
                additional_authorized_imports=authorized_imports,
                max_steps=capped_steps,
                verbosity_level=self.config.verbosity_level,
                executor_type=sandbox,
                instructions=instructions,
                add_base_tools=True,
            )

            logger.info(
                "CodeAgent ready (tools=%s)",
                [getattr(t, "name", type(t).__name__) for t in tools],
            )

        return self._agent

    def _compose_task(self, task: str, prior_stage_context: str | None = None) -> str:
        parts: list[str] = []

        if self.config.task_role:
            parts.append(f"# Role: {self.config.task_role}")
            parts.append(role_brief_for(self.config.task_role))
            parts.append(STUCK_CLAUSE)

        if self.config.context_file and Path(self.config.context_file).is_file():
            ctx = load_kilroy_context(self.config.context_file)
            parts.append(
                format_kilroy_context(
                    ctx,
                    max_chunks_override=self.config.max_context_chunks or None,
                    max_decisions_override=self.config.max_context_decisions or None,
                    max_recent_override=self.config.max_recent_messages or None,
                )
            )
            project_root = (
                Path(self.config.project_root)
                if self.config.project_root
                else Path(self.config.context_file).parent
            )
            clinerules_text = load_clinerules(project_root)
        else:
            clinerules_text = load_clinerules(Path.cwd())

        if clinerules_text:
            parts.append(
                "# Project rules (.clinerules)\n"
                f"The following rules from `.clinerules/*.md` apply to this project:\n"
                f"{clinerules_text}"
            )

        if prior_stage_context:
            parts.append(
                "# Prior specialist output (this workflow, earlier stages)\n"
                "Earlier stages of this same task already produced the output below. "
                "Build on it directly — do not redo their work or ignore it.\n\n"
                f"{prior_stage_context}"
            )

        parts.append(
            "IMPORTANT:\n"
            "When the task is complete, immediately call the final answer tool.\n"
            "Do not repeat successful executions.\n"
            "Do not rerun identical code after tests pass."
        )

        parts.append(f"# Task\n{task.strip()}")

        return "\n\n".join(parts)

    def ask(
        self,
        task: str,
        timeout_seconds: int = 300,
        prior_stage_context: str | None = None,
    ) -> str:
        """Execute a one-shot task with a time limit.

        Args:
            task: The task prompt string.
            timeout_seconds: Maximum wall-clock seconds. Defaults to 300.
            prior_stage_context: Optional summary of what earlier workflow
                stages already produced, so this agent isn't operating blind.
        """
        if not task or not task.strip():
            raise ValueError("Task prompt must be a non-empty string.")

        agent = self._build_agent()

        logger.info("Executing task with timeout=%ds", timeout_seconds)

        try:
            ctx = _TimeoutContext(timeout_seconds)
            with ctx:
                result = ctx.run(agent.run, self._compose_task(task, prior_stage_context))
        except TimeoutException:
            logger.error("Task timed out after %ds", timeout_seconds)
            raise

        logger.debug("Agent result type=%s", type(result).__name__)

        if result is None:
            raise RuntimeError(
                "Agent completed without returning a final answer. "
                "Check model prompts and tool configuration."
            )

        return str(result)

    def chat(self) -> None:
        agent = self._build_agent()

        print("SmartCoder interactive session. Type 'exit' to quit.")

        while True:
            try:
                task = input("you> ").strip()
            except (EOFError, KeyboardInterrupt):
                print("\nbye.")
                return

            if task.lower() in {"exit", "quit", ":q"}:
                print("bye.")
                return

            if not task:
                continue

            try:
                print(f"\nsmartcoder> {agent.run(task, reset=False)}\n")
            except Exception:
                logger.exception("Turn failed")
                print("\nsmartcoder> An error occurred. Check the log for details.\n")

    def reset_agent(self) -> None:
        """Force agent rebuild on next call."""
        with self._agent_lock:
            self._agent = None


def _sanitize_authorized_imports(raw: list[str]) -> list[str]:
    """Validate and deduplicate the authorized imports list.

    P1-7: Validates against ALLOWED_EXTRA_IMPORTS imported from constants.py
    (single source of truth). Raises ValueError with a helpful message listing
    allowed modules when an unrecognized import is requested.
    """
    if not raw:
        return []

    sanitized: list[str] = []
    seen: set[str] = set()

    for item in raw:
        normalized = item.strip().lower()

        if normalized not in ALLOWED_EXTRA_IMPORTS:
            raise ValueError(
                f"Unauthorized import in authorized_imports: {item!r}. "
                f"Allowed modules: {sorted(ALLOWED_EXTRA_IMPORTS)}"
            )

        if normalized not in seen:
            seen.add(normalized)
            sanitized.append(normalized)

    return sanitized


if __name__ == "__main__":
    raise SystemExit("CodingAssistant is a library component and should not be executed directly.")
