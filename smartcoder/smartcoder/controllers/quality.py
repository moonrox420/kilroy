"""
Quality Gate Layer — Validation checkpoints before completion.

FIXES:
  * P2-1: "BLOCKED" check in _check_testing() and _check_review() is now
    case-insensitive (was case-sensitive while "FAILED" was not — a tester
    outputting lowercase "blocked" would have bypassed the blocker signal).
  * P2-11: _check_implementation() now only flags "Traceback (most recent
    call last)" as the error indicator. The previous generic checks for
    "Error:", "Exception:", "Failed:" produced too many false positives on
    legitimate output (docstrings, logging, example code). An unambiguous
    traceback near the end of output is the only reliable crash signal.
  * P3-7: QualityReport.passed is now a cached_property so all(g.passed …)
    is not recomputed on every access. The cache is invalidated by replacing
    the gates list (add() creates a new list entry, so the property
    recomputes lazily on the next access after add() is called).
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any

from smartcoder.controllers.workflow import WorkflowState

logger = logging.getLogger("smartcoder.quality")


@dataclass
class GateResult:
    gate_name: str
    passed: bool
    message: str = ""
    details: dict[str, Any] = field(default_factory=dict)

    def __bool__(self) -> bool:
        return self.passed


class QualityReport:
    """Aggregated results from all quality gates.

    P3-7: 'passed' is computed once per mutation cycle via a manual
    invalidation flag rather than cached_property (dataclasses and
    cached_property don't compose cleanly). The flag is reset in add().
    """

    def __init__(self) -> None:
        self.gates: list[GateResult] = []
        self._passed_cache: bool | None = None

    @property
    def passed(self) -> bool:
        if self._passed_cache is None:
            self._passed_cache = all(g.passed for g in self.gates)
        return self._passed_cache

    @property
    def confidence_penalty(self) -> float:
        """Graduated penalty: 0.0 (no issues) → 0.5 (critical failures)."""
        if not self.gates:
            return 0.0

        penalty = 0.0
        severity_weights: dict[str, float] = {
            "critical": 0.35,
            "error": 0.25,
            "warning": 0.10,
            "info": 0.0,
        }

        for gate in self.gates:
            if not gate.passed:
                penalty += severity_weights.get(gate.details.get("severity", "error"), 0.15)

        return min(penalty, 0.5)

    @property
    def summary(self) -> str:
        if self.passed:
            return "All quality gates passed."
        failures = [g for g in self.gates if not g.passed]
        parts = [f"{len(failures)} gate(s) failed:"]
        for f in failures:
            parts.append(f"  - {f.gate_name}: {f.message}")
        return "\n".join(parts)

    def add(self, gate_name: str, passed: bool, message: str = "", **details: Any) -> None:
        self.gates.append(
            GateResult(gate_name=gate_name, passed=passed, message=message, details=details)
        )
        # Invalidate cached result so next access recomputes.
        self._passed_cache = None


class QualityGate:
    """Validates task outputs against quality criteria."""

    def __init__(self) -> None:
        self.report = QualityReport()

    @staticmethod
    def _runtime_failure(output: str) -> str | None:
        lowered = output.lower()
        for marker in (
            "missing required package(s):",
            "agent execution exceeded",
            "smart coder worker exited",
            "modulenotfounderror:",
            "importerror:",
        ):
            if marker in lowered:
                return marker.rstrip(":")
        return None

    def evaluate(
        self,
        state: WorkflowState,
        agent_output: str | None,
        agent_role: str | None,
        **context: Any,
    ) -> QualityReport:
        self.report = QualityReport()

        if state == WorkflowState.IMPLEMENTATION:
            self._check_implementation(agent_output, context)
        elif state == WorkflowState.TESTING:
            self._check_testing(agent_output, context)
        elif state == WorkflowState.REVIEW:
            self._check_review(agent_output, context)
        elif state == WorkflowState.QUALITY_GATE:
            self._check_final(context)
        else:
            self.report.add("state_check", True, f"No quality gates for state {state.name}")

        return self.report

    def _check_implementation(self, output: str | None, context: dict[str, Any]) -> None:
        if not output or not output.strip():
            self.report.add("implementation_output", False, "No output produced.")
            return

        runtime_failure = self._runtime_failure(output)
        if runtime_failure:
            self.report.add(
                "runtime_failure",
                False,
                f"Agent runtime failed ({runtime_failure}).",
                severity="critical",
            )
            return

        if "BLOCKED:" in output.upper():
            self.report.add("blocked_signal", False, "Agent reported a blocker.")
            return

        # P2-11: Only flag a genuine unhandled crash signal — the traceback
        # header that Python always emits for unhandled exceptions. Generic
        # keywords like "Error:" appear in docstrings, logging, and example
        # code and produce far too many false positives.
        crash_marker = "traceback (most recent call last)"
        lower = output.lower()
        if crash_marker in lower:
            idx = lower.rfind(crash_marker)
            output_len = len(output)
            near_end = idx > max(output_len - 400, 0)
            context_around = output[max(0, idx - 300) : idx + 300].lower()
            has_recovery = any(
                kw in context_around
                for kw in ("caught", "handling", "recovered", "retry", "fix", "except")
            )
            if near_end and not has_recovery:
                self.report.add(
                    "implementation_errors",
                    False,
                    "Unhandled traceback near end of output — execution likely crashed.",
                    severity="error",
                )
                return

        self.report.add("implementation_output", True, "Implementation produced output.")

    def _check_testing(self, output: str | None, context: dict[str, Any]) -> None:
        if not output or not output.strip():
            self.report.add("test_output", False, "No test output produced.")
            return

        runtime_failure = self._runtime_failure(output)
        if runtime_failure:
            self.report.add(
                "runtime_failure",
                False,
                f"Test agent runtime failed ({runtime_failure}).",
                severity="critical",
            )
            return

        upper = output.upper()

        # P2-1: case-insensitive "BLOCKED" check (was case-sensitive).
        if "BLOCKED" in upper:
            self.report.add(
                "blocked_signal",
                False,
                "Tester reported a blocker.",
                severity="critical",
            )
            return

        if "FAILED" in upper:
            self.report.add("test_results", False, "Tests indicate failures.")
            return

        self.report.add("test_output", True, "Tests produced output.")

    def _check_review(self, output: str | None, context: dict[str, Any]) -> None:
        if not output or not output.strip():
            self.report.add("review_output", False, "No review output produced.")
            return

        runtime_failure = self._runtime_failure(output)
        if runtime_failure:
            self.report.add(
                "runtime_failure",
                False,
                f"Review agent runtime failed ({runtime_failure}).",
                severity="critical",
            )
            return

        upper = output.upper()

        # P2-1: case-insensitive "BLOCKED" check (was case-sensitive).
        if "BLOCKED" in upper:
            self.report.add(
                "blocked_signal",
                False,
                "Reviewer reported a blocker.",
                severity="critical",
            )
            return

        if "BLOCKING ISSUES" in upper:
            self.report.add(
                "blocking_issues",
                False,
                "Review found blocking issues that must be addressed.",
                severity="error",
            )
            return

        self.report.add("review_output", True, "Review completed.")

    def _check_final(self, context: dict[str, Any]) -> None:
        history = context.get("workflow_history", [])
        if not history:
            self.report.add("workflow_history", False, "No workflow history recorded.")
            return

        failed_roles = [
            role
            for role, result in context.get("agent_results", {}).items()
            if getattr(result, "confidence", 1.0) <= 0.0
            or getattr(result, "risk_level", "") == "critical"
        ]
        if failed_roles:
            self.report.add(
                "agent_failures",
                False,
                f"Specialist runtime failures: {', '.join(sorted(failed_roles))}.",
                severity="critical",
            )
            return

        self.report.add("workflow_history", True, f"Workflow completed {len(history)} steps.")
        self.report.add("final_gate", True, "All checks passed.")
