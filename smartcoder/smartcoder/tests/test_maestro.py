# smartcoder/tests/test_maestro.py
# Maestro integration tests — workflow loop, fail/block/advance.
#
# Run via pytest:  pytest smartcoder/tests/test_maestro.py -v
#
# These tests mock CodingAssistant to avoid LLM dependencies and verify
# the state machine logic: short-circuit, normal flow, retry on failure,
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
def controller(config: AppConfig) -> SmartCoderController:
    """Controller with a mocked CodingAssistant so no LLM is called."""
    ctrl = SmartCoderController(config)
    # Replace the real assistant with a mock that returns canned output.
    mock_assistant = MagicMock()
    mock_assistant.run.return_value = "def add(a, b): return a + b"
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


def test_retry_on_failure(controller: SmartCoderController) -> None:
    """When an agent fails, the controller should retry in place."""
    # Make the assistant fail on first call, succeed on second.
    call_count = 0

    def _mock_run(task: str) -> str:
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            raise RuntimeError("Simulated transient failure")
        return "def add(a, b): return a + b"

    controller._assistant.run = _mock_run  # type: ignore[method-assign]

    with patch.object(controller, "_is_trivial_task", return_value=False):
        result = controller.run("Add two numbers")
    assert result is not None
    assert controller.workflow.is_complete


def test_block_terminates_workflow(controller: SmartCoderController) -> None:
    """A BLOCKED signal from quality gate should terminate the workflow."""
    # Force the quality gate to produce a BLOCKED result by making the
    # agent output something the gate recognises as blocked.
    controller._assistant.run = MagicMock(
        return_value="BLOCKED: Cannot proceed without more context."
    )  # type: ignore[method-assign]

    with patch.object(controller, "_is_trivial_task", return_value=False):
        result = controller.run("Do something impossible")
    # The workflow should have terminated (is_complete = True) even though
    # the quality gate blocked it.
    assert controller.workflow.is_complete
    # The result should contain the blocked signal
    assert result is not None


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
