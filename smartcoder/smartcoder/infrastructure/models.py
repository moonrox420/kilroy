"""
Model factories — build smolagents-compatible Model instances for each backend.

Extracted from the original kilroy_smartcoder.py's build_model() and the three
model subclasses (_make_ollama_model, _make_llama_cpp_model, _make_langchain_ollama_model).
Infrastructure layer only. Knows nothing about agents, tasks, or workflows.

REFACTOR NOTES (see remediation PRD):
  * P2-10: `build_model()` now calls
    `constants.warn_if_ollama_defaults_unset()` right before building an
    Ollama-backed model — this is the one place that actually cares whether
    OLLAMA_HOST/OLLAMA_MODEL env vars were set, so the warning now only
    fires when it's relevant (not on every `import smartcoder`).
  * P3-1: `LangChainOllamaModel.generate()`'s `bind_tools(tools_to_call_from)`
    call is very unlikely to accept raw smolagents `Tool` objects as-is
    (`ChatOllama.bind_tools()` expects LangChain-shaped tool schemas). The
    previous bare `except Exception: bound = llm` swallowed this silently
    with only a debug-level-adjacent warning. This version keeps the same
    graceful fallback (a real schema converter is out of scope without a
    smolagents install to validate against) but raises the log to a level
    that makes the limitation obvious the first time it happens, and calls
    it out explicitly in the docstring so it isn't mistaken for "tool
    calling works fine on this backend."
"""

from __future__ import annotations

import logging
import os
import threading
from pathlib import Path
from typing import Any, cast

from smartcoder.infrastructure.dependencies import DependencyManager
from smartcoder.runtime import constants
from smartcoder.runtime.config import AppConfig

logger = logging.getLogger("smartcoder")


def _make_llama_cpp_model(config: AppConfig):
    """
    Build a REAL smolagents Model backed by a local GGUF via llama-cpp-python.
    """
    from smolagents import Model

    try:
        from smolagents import ChatMessage
    except ImportError:  # older/newer layout
        from smolagents.models import ChatMessage  # type: ignore

    class LlamaCppModel(Model):
        def __init__(
            self, model_path: str, n_ctx: int, temperature: float, max_tokens: int
        ) -> None:
            super().__init__(model_id=Path(model_path).name, flatten_messages_as_text=True)
            self.model_path = model_path
            self.n_ctx = n_ctx
            self.temperature = temperature
            self.max_tokens = max_tokens
            self._llama = None
            self._lock = threading.Lock()

        def _ensure_loaded(self):
            with self._lock:
                if self._llama is None:
                    from llama_cpp import Llama  # type: ignore[import-not-found]

                    gpu_layers = int(os.environ.get("LLAMA_GPU_LAYERS", "-1"))
                    logger.info(
                        "Loading GGUF model %s (n_gpu_layers=%d)...",
                        self.model_path,
                        gpu_layers,
                    )
                    self._llama = Llama(
                        model_path=self.model_path,
                        n_ctx=self.n_ctx,
                        n_gpu_layers=gpu_layers,
                        verbose=False,
                    )
            return self._llama

        @staticmethod
        def _flatten_content(content: Any) -> str:
            if isinstance(content, str):
                return content
            if isinstance(content, list):
                parts = []
                for chunk in content:
                    if isinstance(chunk, dict):
                        parts.append(str(chunk.get("text", "")))
                    else:
                        parts.append(str(chunk))
                return "".join(parts)
            return str(content)

        def _normalize(self, messages: list[Any]) -> list[dict[str, str]]:
            normalized: list[dict[str, str]] = []
            for message in messages:
                if isinstance(message, dict):
                    role = message.get("role", "user")
                    content = message.get("content", "")
                else:
                    role = getattr(message, "role", "user")
                    content = getattr(message, "content", "")
                role = getattr(role, "value", role)
                normalized.append({"role": str(role), "content": self._flatten_content(content)})
            return normalized

        def generate(
            self,
            messages: list[Any],
            stop_sequences: list[str] | None = None,
            response_format: dict[str, str] | None = None,
            tools_to_call_from: list[Any] | None = None,
            **kwargs: Any,
        ):
            llm = self._ensure_loaded()
            chat_messages = self._normalize(messages)
            completion = llm.create_chat_completion(
                messages=chat_messages,
                temperature=kwargs.get("temperature", self.temperature),
                max_tokens=kwargs.get("max_tokens", self.max_tokens),
                stop=stop_sequences or None,
            )
            try:
                text = completion["choices"][0]["message"]["content"]
            except (KeyError, IndexError, TypeError) as exc:
                raise RuntimeError(f"Unexpected model response format: {completion}") from exc
            return ChatMessage(role="assistant", content=text)  # type: ignore[arg-type]

    return LlamaCppModel(
        model_path=cast(str, config.llama_model_path),  # validated non-None in __post_init__
        n_ctx=config.num_ctx,
        temperature=config.temperature,
        max_tokens=config.max_tokens,
    )


def _make_ollama_model(config: AppConfig):
    """Build a smolagents Model backed directly by the local Ollama API."""
    from ollama import Client
    from smolagents import Model

    try:
        from smolagents import ChatMessage
    except ImportError:
        from smolagents.models import ChatMessage  # type: ignore

    class OllamaModel(Model):
        def __init__(
            self,
            model_name: str,
            host: str,
            temperature: float,
            max_tokens: int,
            num_ctx: int,
        ) -> None:
            super().__init__(model_id=model_name, flatten_messages_as_text=False)
            self.model_name = model_name
            self.host = host
            self.temperature = temperature
            self.max_tokens = max_tokens
            self.num_ctx = num_ctx
            self._client = Client(host=host)

        @staticmethod
        def _content_as_text(content: Any) -> str:
            if isinstance(content, str):
                return content
            if isinstance(content, list):
                parts: list[str] = []
                for chunk in content:
                    if isinstance(chunk, dict):
                        text = chunk.get("text") or chunk.get("content") or ""
                    else:
                        text = (
                            getattr(chunk, "text", None)
                            or getattr(chunk, "content", None)
                            or str(chunk)
                        )
                    if text:
                        parts.append(str(text))
                return "\n".join(parts)
            return str(content)

        @classmethod
        def _normalize_messages(cls, messages: list[Any]) -> list[dict[str, str]]:
            normalized: list[dict[str, str]] = []
            for message in messages:
                if isinstance(message, dict):
                    role = message.get("role", "user")
                    content = message.get("content", "")
                else:
                    role = getattr(message, "role", "user")
                    content = getattr(message, "content", "")

                role_name = str(getattr(role, "value", role)).lower().replace("_", "-")
                if role_name == "tool-call":
                    role_name = "assistant"
                elif role_name == "tool-response":
                    role_name = "user"
                elif role_name not in {"system", "user", "assistant", "tool"}:
                    role_name = "user"

                normalized.append(
                    {
                        "role": role_name,
                        "content": cls._content_as_text(content),
                    }
                )
            return normalized

        def generate(
            self,
            messages: list[Any],
            stop_sequences: list[str] | None = None,
            response_format: dict[str, str] | None = None,
            tools_to_call_from: list[Any] | None = None,
            **kwargs: Any,
        ):
            if tools_to_call_from:
                logger.debug(
                    "Ignoring native tool schemas for CodeAgent; "
                    "tools execute through its local executor."
                )

            options: dict[str, Any] = {
                "temperature": kwargs.get("temperature", self.temperature),
                "num_ctx": self.num_ctx,
                "num_predict": kwargs.get("max_tokens", self.max_tokens),
            }
            if stop_sequences:
                options["stop"] = stop_sequences

            ollama_format: str | dict[str, str] | None = None
            if response_format:
                ollama_format = (
                    "json"
                    if response_format.get("type") == "json_object"
                    else response_format
                )

            response = self._client.chat(
                model=self.model_name,
                messages=self._normalize_messages(messages),
                format=ollama_format,
                options=options,
            )
            response_message = getattr(response, "message", None)
            content = getattr(response_message, "content", None)
            if content is None and isinstance(response_message, dict):
                content = response_message.get("content")
            if not content:
                raise RuntimeError(
                    f"Ollama model {self.model_name!r} returned an empty response."
                )
            return ChatMessage(role="assistant", content=str(content))

    return OllamaModel(
        model_name=config.model_name,
        host=config.ollama_host,
        temperature=config.temperature,
        max_tokens=config.max_tokens,
        num_ctx=config.num_ctx,
    )


def _make_langchain_ollama_model(config: AppConfig):
    """
    Build a smolagents-compatible Model that uses the OFFICIAL langchain-ollama
    ChatOllama integration.

    KNOWN LIMITATION (P3-1): tool-calling is not reliably supported on this
    backend. `tools_to_call_from` (smolagents `Tool` objects) are passed
    straight to `ChatOllama.bind_tools()`, which expects LangChain-shaped
    tool schemas — binding will very likely fail and fall back silently to
    an unbound model. Prefer the `ollama` (LiteLLM) backend when tool use
    matters; this backend is best suited to plain CodeAgent-style text/code
    generation, which doesn't depend on native tool-calling.
    """
    from smolagents import Model

    try:
        from smolagents import ChatMessage
    except ImportError:
        from smolagents.models import ChatMessage  # type: ignore

    class LangChainOllamaModel(Model):
        def __init__(
            self,
            model_name: str,
            base_url: str,
            temperature: float,
            max_tokens: int,
            num_ctx: int,
        ) -> None:
            super().__init__(model_id=model_name, flatten_messages_as_text=False)
            self.model_name = model_name
            self.base_url = base_url
            self.temperature = temperature
            self.max_tokens = max_tokens
            self.num_ctx = num_ctx
            self._llm = None
            self._lock = threading.Lock()
            self._tool_binding_warned = False

        def _ensure_llm(self):
            with self._lock:
                if self._llm is None:
                    from langchain_ollama import ChatOllama

                    logger.info(
                        "Initializing official ChatOllama (langchain-ollama) model=%s base_url=%s",
                        self.model_name,
                        self.base_url,
                    )
                    self._llm = ChatOllama(
                        model=self.model_name,
                        base_url=self.base_url,
                        temperature=self.temperature,
                        num_ctx=self.num_ctx,
                    )
            return self._llm

        def _to_langchain_messages(self, messages: list[Any]) -> list[Any]:
            from langchain_core.messages import (
                AIMessage,
                HumanMessage,
                SystemMessage,
            )

            lc_messages = []
            for m in messages:
                if isinstance(m, dict):
                    role = m.get("role", "user")
                    content = m.get("content", "")
                else:
                    role = getattr(m, "role", "user")
                    content = getattr(m, "content", "")
                role = getattr(role, "value", role)

                if role == "system":
                    lc_messages.append(SystemMessage(content=content))
                elif role == "assistant":
                    lc_messages.append(AIMessage(content=content))
                else:
                    lc_messages.append(HumanMessage(content=content))
            return lc_messages

        def generate(
            self,
            messages: list[Any],
            stop_sequences: list[str] | None = None,
            response_format: dict[str, str] | None = None,
            tools_to_call_from: list[Any] | None = None,
            **kwargs: Any,
        ):
            llm = self._ensure_llm()

            lc_messages = self._to_langchain_messages(messages)

            if tools_to_call_from:
                try:
                    bound = llm.bind_tools(tools_to_call_from)
                except Exception as exc:
                    if not self._tool_binding_warned:
                        logger.warning(
                            "langchain_ollama backend cannot bind smolagents tools "
                            "(%s) — proceeding WITHOUT tool-calling support for this "
                            "session. Use --backend ollama if tool use is required.",
                            exc,
                        )
                        self._tool_binding_warned = True
                    bound = llm
            else:
                bound = llm

            call_kwargs = {
                "temperature": kwargs.get("temperature", self.temperature),
            }
            if self.max_tokens:
                call_kwargs["max_tokens"] = kwargs.get("max_tokens", self.max_tokens)
            if stop_sequences:
                call_kwargs["stop"] = stop_sequences

            result = bound.invoke(lc_messages, **call_kwargs)

            content = getattr(result, "content", "") or ""
            return ChatMessage(role="assistant", content=content)  # type: ignore[arg-type]

    return LangChainOllamaModel(
        model_name=config.model_name,
        base_url=config.ollama_host,
        temperature=config.temperature,
        max_tokens=config.max_tokens,
        num_ctx=config.num_ctx,
    )


def build_model(config: AppConfig, deps: DependencyManager):
    """Return a configured smolagents Model for the selected backend."""
    deps.require("smolagents")

    if config.backend == "ollama":
        constants.warn_if_ollama_defaults_unset()
        deps.require("ollama")
        logger.info(
            "Backend: direct Ollama client (model=%s, host=%s)",
            config.model_name,
            config.ollama_host,
        )
        return _make_ollama_model(config)

    if config.backend == "langchain_ollama":
        constants.warn_if_ollama_defaults_unset()
        deps.require("langchain_ollama")
        logger.info(
            "Backend: Ollama via official langchain-ollama ChatOllama (model=%s, host=%s)",
            config.model_name,
            config.ollama_host,
        )
        return _make_langchain_ollama_model(config)

    if config.backend == "llama_cpp":
        deps.require("llama_cpp")
        logger.info("Backend: llama.cpp (path=%s)", config.llama_model_path)
        return _make_llama_cpp_model(config)

    raise ValueError(f"Unhandled backend: {config.backend}")


def build_web_search_tool():
    """Return a web-search tool if smolagents exposes one and its backend is installed."""
    try:
        from smolagents import DuckDuckGoSearchTool

        return DuckDuckGoSearchTool()
    except Exception as exc:
        logger.debug("DuckDuckGoSearchTool unavailable (%s); trying WebSearchTool", exc)
    try:
        from smolagents import WebSearchTool

        return WebSearchTool()
    except Exception as exc:
        logger.warning("No web search tool available (%s); continuing without it.", exc)
        return None
