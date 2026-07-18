"""
Workflow states — explicit stages every task moves through.

FIX P3-9: WorkflowEngine.progress now reports the actual step index for
terminal states (BLOCKED/FAILED) rather than always reporting the full
path length (e.g. "9/9 (Failed)" when the task failed at step 3). A task
that failed at step 3 of 9 now correctly reports "3/9 (Failed)".
"""

from __future__ import annotations

import logging
from enum import Enum, auto
from typing import Any

logger = logging.getLogger("smartcoder.workflow")


class WorkflowState(Enum):
    INITIALIZED = auto()
    PLANNING = auto()
    ARCHITECTURE = auto()
    IMPLEMENTATION = auto()
    TESTING = auto()
    REVIEW = auto()
    QUALITY_GATE = auto()
    FINALIZATION = auto()
    COMPLETED = auto()
    BLOCKED = auto()
    FAILED = auto()


STATE_LABELS: dict[WorkflowState, str] = {
    WorkflowState.INITIALIZED: "Initialized",
    WorkflowState.PLANNING: "Planning",
    WorkflowState.ARCHITECTURE: "Architecture",
    WorkflowState.IMPLEMENTATION: "Implementation",
    WorkflowState.TESTING: "Testing",
    WorkflowState.REVIEW: "Review",
    WorkflowState.QUALITY_GATE: "Quality Gate",
    WorkflowState.FINALIZATION: "Finalization",
    WorkflowState.COMPLETED: "Completed",
    WorkflowState.BLOCKED: "Blocked",
    WorkflowState.FAILED: "Failed",
}

WORKFLOW_PATHS: dict[str, list[WorkflowState]] = {
    "code": [
        WorkflowState.INITIALIZED,
        WorkflowState.PLANNING,
        WorkflowState.ARCHITECTURE,
        WorkflowState.IMPLEMENTATION,
        WorkflowState.TESTING,
        WorkflowState.REVIEW,
        WorkflowState.QUALITY_GATE,
        WorkflowState.FINALIZATION,
        WorkflowState.COMPLETED,
    ],
    "test": [
        WorkflowState.INITIALIZED,
        WorkflowState.PLANNING,
        WorkflowState.TESTING,
        WorkflowState.REVIEW,
        WorkflowState.QUALITY_GATE,
        WorkflowState.FINALIZATION,
        WorkflowState.COMPLETED,
    ],
    "review": [
        WorkflowState.INITIALIZED,
        WorkflowState.REVIEW,
        WorkflowState.FINALIZATION,
        WorkflowState.COMPLETED,
    ],
    "analysis": [
        WorkflowState.INITIALIZED,
        WorkflowState.PLANNING,
        WorkflowState.ARCHITECTURE,
        WorkflowState.QUALITY_GATE,
        WorkflowState.FINALIZATION,
        WorkflowState.COMPLETED,
    ],
    "doc": [
        WorkflowState.INITIALIZED,
        WorkflowState.PLANNING,
        WorkflowState.IMPLEMENTATION,
        WorkflowState.REVIEW,
        WorkflowState.QUALITY_GATE,
        WorkflowState.FINALIZATION,
        WorkflowState.COMPLETED,
    ],
    "plan": [
        WorkflowState.INITIALIZED,
        WorkflowState.PLANNING,
        WorkflowState.QUALITY_GATE,
        WorkflowState.FINALIZATION,
        WorkflowState.COMPLETED,
    ],
    "simple": [
        WorkflowState.INITIALIZED,
        WorkflowState.IMPLEMENTATION,
        WorkflowState.FINALIZATION,
        WorkflowState.COMPLETED,
    ],
}


class WorkflowEngine:
    """Manages task progression through explicit workflow states."""

    def __init__(self, task_type: str = "code") -> None:
        self.task_type = task_type
        self.path = list(WORKFLOW_PATHS.get(task_type, WORKFLOW_PATHS["simple"]))
        self.current_index = 0
        self.history: list[dict[str, Any]] = []
        self.metadata: dict[str, Any] = {}
        self._terminal_state: WorkflowState | None = None

    @property
    def current_state(self) -> WorkflowState:
        if self._terminal_state is not None:
            return self._terminal_state
        if self.current_index < len(self.path):
            return self.path[self.current_index]
        return WorkflowState.COMPLETED

    @property
    def is_complete(self) -> bool:
        return self.current_state in (
            WorkflowState.COMPLETED,
            WorkflowState.FAILED,
            WorkflowState.BLOCKED,
        )

    @property
    def progress(self) -> str:
        """Human-readable progress: '3/9 (Implementation)'.

        P3-9 FIX: terminal states now report the actual step index rather
        than always showing the full path length. A task that FAILED at
        step 3 of 9 previously reported '9/9 (Failed)'; it now reports
        '3/9 (Failed)'.
        """
        label = STATE_LABELS.get(self.current_state, "?")
        total = len(self.path)
        # Use the actual index (capped at total) so we never show N+1/N.
        position = min(self.current_index + 1, total)
        return f"{position}/{total} ({label})"

    def advance(self) -> WorkflowState:
        if self.is_complete:
            return self.current_state
        entry = {
            "from": STATE_LABELS.get(self.current_state, "?"),
            "to": (
                STATE_LABELS.get(self.path[self.current_index + 1], "?")
                if self.current_index + 1 < len(self.path)
                else "end"
            ),
            "state": self.current_state.name,
        }
        self.history.append(entry)
        self.current_index += 1
        logger.debug("Workflow advance: %s -> %s", entry["from"], entry["to"])
        return self.current_state

    def block(self, reason: str) -> WorkflowState:
        self.metadata["blocked_reason"] = reason
        self._terminal_state = WorkflowState.BLOCKED
        # Keep current_index where it is so progress reflects actual position.
        logger.info("Workflow blocked at step %d: %s", self.current_index, reason)
        return WorkflowState.BLOCKED

    def fail(self, reason: str) -> WorkflowState:
        self.metadata["failed_reason"] = reason
        self._terminal_state = WorkflowState.FAILED
        # Keep current_index where it is so progress reflects actual position.
        logger.error("Workflow failed at step %d: %s", self.current_index, reason)
        return WorkflowState.FAILED

    def agent_for_state(self) -> str | None:
        _agent_map = {
            WorkflowState.PLANNING: "planner",
            WorkflowState.ARCHITECTURE: "architect",
            WorkflowState.IMPLEMENTATION: "developer",
            WorkflowState.TESTING: "qa",
            WorkflowState.REVIEW: "reviewer",
        }
        return _agent_map.get(self.current_state)
