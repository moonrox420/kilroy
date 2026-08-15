# smartcoder/tests/test_maestro.py
# Maestro integration tests — workflow loop, fail/block/advance.
#
# Run via pytest:  pytest smartcoder/tests/test_maestro.py -v
#
# These tests mock CodingAssistant to avoid LLM dependencies and verify
# the state machine logic: short-circuit, normal flow, failure propagation,
# and block termination.

from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest
from smartcoder.controllers.maestro import SmartCoderController
from smartcoder.runtime.config import AppConfig

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def config() -> AppConfig:
    return AppConfig(
        task_type="code",
        task_role="developer",
    )


@pytest.fixture
def controller(config: AppConfig, monkeypatch: pytest.MonkeyPatch) -> SmartCoderController:
    """Controller with a mocked CodingAssistant so no LLM is called."""
    mock_assistant = MagicMock()
    mock_assistant.ask.return_value = "def add(a, b): return a + b"
    monkeypatch.setattr(
        "smartcoder.controllers.maestro.CodingAssistant",
        lambda *_args, **_kwargs: mock_assistant,
    )
    ctrl = SmartCoderController(config)
    ctrl._assistant = mock_assistant
    return ctrl


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_trivial_task_short_circuits(controller: SmartCoderController) -> None:
    """A trivial task should bypass the full workflow and go direct."""
    result = controller.run("Add two numbers")
    assert result is not None
    assert controller._short_circuit_count == 1
    # The workflow should not have advanced past INITIALIZED
    assert controller.workflow.current_index == 0


def test_normal_workflow_advances_through_states(
    controller: SmartCoderController,
) -> None:
    """A non-trivial task should advance through the workflow states."""
    # Patch _is_trivial_task to return False so we exercise the full path.
    with patch.object(controller, "_is_trivial_task", return_value=False):
        result = controller.run("Implement a binary search tree in Python")
    assert result is not None
    # The workflow should have completed (is_complete = True)
    assert controller.workflow.is_complete
    # Telemetry should have recorded the task
    assert controller.telemetry.timeline is not None


def test_agent_failure_propagates(controller: SmartCoderController) -> None:
    """A specialist crash must fail the command instead of becoming output."""
    controller._assistant.ask.side_effect = RuntimeError("Simulated agent failure")
    with (
        patch.object(controller, "_is_trivial_task", return_value=False),
        pytest.raises(RuntimeError, match="Simulated agent failure"),
    ):
        controller.run("Add two numbers")


def test_block_terminates_workflow(controller: SmartCoderController) -> None:
    """A BLOCKED signal from quality gate should terminate the workflow."""
    # Force the quality gate to produce a BLOCKED result by making the
    # agent output something the gate recognises as blocked.
    controller._assistant.ask.return_value = "BLOCKED: Cannot proceed without more context."

    with (
        patch.object(controller, "_is_trivial_task", return_value=False),
        pytest.raises(RuntimeError, match="Agent reported a blocker"),
    ):
        controller.run("Do something impossible")
    assert controller.workflow.is_complete


def test_reset_clears_state(controller: SmartCoderController) -> None:
    """reset() should clear workflow, memory, and decisions for a fresh run."""
    with patch.object(controller, "_is_trivial_task", return_value=False):
        controller.run("Some task")
    assert controller.workflow.is_complete

    controller.reset()
    assert controller.workflow.current_index == 0
    assert not controller.workflow.is_complete
    assert len(controller.memory.stage_outputs) == 0
    assert len(controller._results) == 0
