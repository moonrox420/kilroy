"""
Regression tests for security and logic fixes applied in the Kilroy audit.

Covers:
  - ALLOWED_EXTRA_IMPORTS sandbox escape prevention
  - Timeout infrastructure wiring
  - Hive state machine verification gate integrity
  - State transition graph completeness
"""

from __future__ import annotations

import pytest

from smartcoder.agents.coding_assistant import (
    ALLOWED_EXTRA_IMPORTS,
    TimeoutException,
    _sanitize_authorized_imports,
)
from smartcoder.legacy.hive import (
    HiveState,
    StateTransitionError,
    WorkflowController,
)


# ---------------------------------------------------------------------------
# 1. Sandbox-escape imports are blocked
# ---------------------------------------------------------------------------
class TestImportAllowlist:
    def test_os_and_sys_are_excluded(self):
        assert "os" not in ALLOWED_EXTRA_IMPORTS
        assert "sys" not in ALLOWED_EXTRA_IMPORTS

    def test_safe_stdlib_allowed(self):
        safe = {"math", "json", "re", "datetime", "collections", "itertools"}
        for mod in safe:
            assert mod in ALLOWED_EXTRA_IMPORTS, f"{mod} should be allowed"

    def test_sanitize_rejects_os(self):
        with pytest.raises(ValueError, match="Unauthorized import"):
            _sanitize_authorized_imports(["os"])

    def test_sanitize_rejects_sys(self):
        with pytest.raises(ValueError, match="Unauthorized import"):
            _sanitize_authorized_imports(["sys"])

    def test_sanitize_accepts_safe_modules(self):
        result = _sanitize_authorized_imports(["json", "math"])
        assert result == ["json", "math"]

    def test_sanitize_dedupes(self):
        result = _sanitize_authorized_imports(["json", "JSON", "Json"])
        assert result == ["json"]

    def test_sanitize_empty(self):
        assert _sanitize_authorized_imports([]) == []
        assert _sanitize_authorized_imports(None or []) == []


# ---------------------------------------------------------------------------
# 2. Timeout infrastructure is wired and importable
# ---------------------------------------------------------------------------
class TestTimeoutInfrastructure:
    def test_timeout_exception_is_exception_subclass(self):
        assert issubclass(TimeoutException, Exception)

    def test_timeout_exception_message(self):
        exc = TimeoutException("test timeout")
        assert "test timeout" in str(exc)


# ---------------------------------------------------------------------------
# 3. Verification gate integrity (verified=False for failure paths)
# ---------------------------------------------------------------------------
class DummyAssistant:
    def ask(self, prompt: str) -> str:
        return "placeholder output"


class TestVerificationGate:
    def test_requirements_mismatch_sets_verified_false(self):
        ctrl = WorkflowController(DummyAssistant(), "/tmp")
        ctrl.current_state = HiveState.VERIFYING  # bypass graph check
        token = ctrl.verify_and_advance("some output", failure_mode="requirements_mismatch")
        assert token["verified"] is False
        assert token["failure_mode"] == "requirements_mismatch"
        assert token["extracted_metrics"]["routed_to"] == "PLANNING"

    def test_implementation_defect_sets_verified_false(self):
        ctrl = WorkflowController(DummyAssistant(), "/tmp")
        ctrl.current_state = HiveState.VERIFYING
        token = ctrl.verify_and_advance("some output", failure_mode="implementation_defect")
        assert token["verified"] is False
        assert token["failure_mode"] == "implementation_defect"
        assert token["extracted_metrics"]["routed_to"] == "IMPLEMENTING"

    def test_clean_verify_sets_verified_true(self):
        ctrl = WorkflowController(DummyAssistant(), "/tmp")
        ctrl.current_state = HiveState.VERIFYING
        token = ctrl.verify_and_advance("test_pass_rate: 100%", failure_mode=None)
        assert token["verified"] is True
        assert token["failure_mode"] is None


# ---------------------------------------------------------------------------
# 4. State transition graph completeness
# ---------------------------------------------------------------------------
class TestTransitionGraph:
    def test_architecting_can_replan(self):
        assert HiveState.PLANNING in WorkflowController._VALID_TRANSITIONS[HiveState.ARCHITECTING]

    def test_architecting_to_implementing_allowed(self):
        assert (
            HiveState.IMPLEMENTING in WorkflowController._VALID_TRANSITIONS[HiveState.ARCHITECTING]
        )

    def test_architecting_to_halted_allowed(self):
        assert HiveState.HALTED in WorkflowController._VALID_TRANSITIONS[HiveState.ARCHITECTING]

    def test_halted_has_no_outgoing(self):
        assert WorkflowController._VALID_TRANSITIONS[HiveState.HALTED] == []

    def test_invalid_transition_raises(self):
        ctrl = WorkflowController(DummyAssistant(), "/tmp")
        ctrl.current_state = HiveState.PLANNING
        with pytest.raises(StateTransitionError, match="Invalid transition"):
            ctrl.transition_to(HiveState.VERIFYING, {"verified": True})

    def test_halted_transition_raises(self):
        ctrl = WorkflowController(DummyAssistant(), "/tmp")
        ctrl.current_state = HiveState.HALTED
        with pytest.raises(StateTransitionError, match="Cannot transition from HALTED"):
            ctrl.transition_to(HiveState.PLANNING, {"verified": True})
