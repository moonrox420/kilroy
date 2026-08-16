from __future__ import annotations

import sys
from types import ModuleType
from unittest.mock import MagicMock, patch

from smartcoder.agents.coding_assistant import CodingAssistant
from smartcoder.infrastructure.dependencies import DependencyManager
from smartcoder.runtime.config import AppConfig


def test_kilroy_supervision_avoids_nested_multiprocessing(monkeypatch) -> None:
    monkeypatch.setenv("SMARTCODER_SUPERVISED", "1")
    assistant = CodingAssistant(AppConfig(), DependencyManager())

    with (
        patch.object(assistant, "_ask_inline", return_value="analysis complete") as ask_inline,
        patch("smartcoder.agents.coding_assistant.multiprocessing.get_context") as get_context,
    ):
        result = assistant.ask("Inspect the project", timeout_seconds=300)

    assert result == "analysis complete"
    ask_inline.assert_called_once_with("Inspect the project", None)
    get_context.assert_not_called()


def test_kilroy_supervision_uses_supported_error_verbosity(monkeypatch) -> None:
    monkeypatch.setenv("SMARTCODER_SUPERVISED", "1")
    smolagents = ModuleType("smolagents")

    class StubCodeAgent:
        def __init__(self, **kwargs) -> None:
            self.kwargs = kwargs

    class StubFinalAnswerTool:
        pass

    smolagents.CodeAgent = StubCodeAgent
    smolagents.FinalAnswerTool = StubFinalAnswerTool
    monkeypatch.setitem(sys.modules, "smolagents", smolagents)
    monkeypatch.setattr(
        "smartcoder.agents.coding_assistant.build_model",
        lambda *_args, **_kwargs: object(),
    )

    deps = MagicMock(spec=DependencyManager)
    assistant = CodingAssistant(AppConfig(), deps)

    agent = assistant._build_agent()

    assert agent.kwargs["verbosity_level"] == 0
