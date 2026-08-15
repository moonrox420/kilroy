# smartcoder/tests/test_confidence.py
# Confidence-calibration regression tests.
#
# Run directly:  python -m smartcoder.tests.test_confidence
# Or via pytest: pytest smartcoder/tests/test_confidence.py
#
# These tests pin the behavior of:
#   * SmartCoderController._calculate_confidence (positive signals,
#     length penalties, traceback detection, quality penalties)
#   * QualityGate._check_testing and _check_review (BLOCKED detection
#     as critical severity)
#   * SmartCoderController._build_consensus (preserves performance multiplier)

from __future__ import annotations

import sys
from pathlib import Path

# Ensure project root is importable when run directly
_SCRIPT_DIR = Path(__file__).resolve().parent
_PROJECT_ROOT = _SCRIPT_DIR.parent.parent
if str(_PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(_PROJECT_ROOT))

from smartcoder.controllers.quality import QualityGate, QualityReport
from smartcoder.controllers.workflow import WorkflowState
from smartcoder.intelligence import (
    AgentConflict,
    AgentOpinion,
    DecisionRegistry,
    ExecutionMemory,
    ExecutionTelemetry,
    LockedDecisionError,
)

# ---------------------------------------------------------------------------
# Tiny test harness (no external test runner dependency)
# ---------------------------------------------------------------------------

_FAILURES: list[str] = []
_PASSED: int = 0


def _check(name: str, condition: bool, detail: str = "") -> None:
    global _PASSED
    if condition:
        _PASSED += 1
        print(f"  [PASS] {name}")
    else:
        _FAILURES.append(f"{name}: {detail}")
        print(f"  [FAIL] {name}: {detail}")


def _section(title: str) -> None:
    print(f"\n=== {title} ===")


# ---------------------------------------------------------------------------
# Test helpers
# ---------------------------------------------------------------------------


def _make_controller():
    """Build a minimal SmartCoderController for confidence math only.
    Bypasses full __init__ to avoid LLM/backend dependencies."""
    from smartcoder.controllers.maestro import SmartCoderController

    ctrl = SmartCoderController.__new__(SmartCoderController)
    ctrl.memory = ExecutionMemory(task_type="code")
    ctrl.telemetry = ExecutionTelemetry()
    ctrl.decisions = DecisionRegistry()
    return ctrl


def _empty_quality_report() -> QualityReport:
    """Empty report contributes zero penalty."""
    return QualityReport()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_package_root_reexports_canonical_intelligence() -> None:
    import smartcoder

    assert smartcoder.DecisionRegistry is DecisionRegistry
    assert smartcoder.LockedDecisionError is LockedDecisionError


def test_locked_decision_requires_explicit_override() -> None:
    registry = DecisionRegistry()
    original = registry.record("storage", "sqlite", lock=True)

    try:
        registry.record("storage", "postgres")
    except LockedDecisionError:
        pass
    else:
        raise AssertionError("locked decision was silently overwritten")

    replacement = registry.record("storage", "postgres", allow_override=True)
    assert replacement is original
    assert replacement.decision == "postgres"
    assert replacement.override_log


def test_empty_output_is_strong_negative() -> None:
    _section("Empty output")
    ctrl = _make_controller()
    c = ctrl._calculate_confidence("", _empty_quality_report())
    _check("empty string -> 0.1", abs(c - 0.1) < 0.001, f"got {c}")

    c = ctrl._calculate_confidence("   \n\t  ", _empty_quality_report())
    _check("whitespace -> 0.1", abs(c - 0.1) < 0.001, f"got {c}")


def test_short_output_penalised() -> None:
    _section("Short output")
    ctrl = _make_controller()
    out = "Function added successfully."
    c = ctrl._calculate_confidence(out, _empty_quality_report())
    _check(
        "short output lands in 0.30-0.55 range",
        0.30 <= c <= 0.55,
        f"got {c:.3f}",
    )


def test_positive_signals_boost_confidence() -> None:
    _section("Positive signals")
    ctrl = _make_controller()

    output_with_tests = """
    def add_numbers(a: int, b: int) -> int:
        return a + b

    # Test the function
    assert add_numbers(2, 3) == 5
    assert add_numbers(-1, 1) == 0
    print("All test cases passed!")
    """
    c = ctrl._calculate_confidence(output_with_tests, _empty_quality_report())
    _check(
        "assertions + 'all test cases passed' yields >= 0.80",
        c >= 0.80,
        f"got {c:.3f}",
    )

    big_pass = "All tests passed.\n" + ("detail line\n" * 30)
    c = ctrl._calculate_confidence(big_pass, _empty_quality_report())
    _check(
        "big output with 'all tests passed' >= 0.80",
        c >= 0.80,
        f"got {c:.3f}",
    )


def test_traceback_near_end_penalised() -> None:
    _section("Traceback near end")
    ctrl = _make_controller()
    out = (
        "Starting execution...\n"
        + ("some log line\n" * 30)
        + "Traceback (most recent call last):\n"
        + '  File "x.py", line 5\n'
        + "NameError: name 'foo' is not defined\n"
    )
    c = ctrl._calculate_confidence(out, _empty_quality_report())
    _check(
        "traceback near end drops below 0.50",
        c < 0.50,
        f"got {c:.3f}",
    )


def test_blocked_signal_via_quality_gate_penalises() -> None:
    _section("BLOCKED detection in quality gates")
    gate = QualityGate()

    # Tester state with BLOCKED output
    report = gate.evaluate(
        state=WorkflowState.TESTING,
        agent_output="BLOCKED: Insufficient context to test this code.",
        agent_role="tester",
    )
    _check("tester BLOCKED fails the gate", not report.passed)
    _check(
        "tester BLOCKED is critical severity",
        any(g.details.get("severity") == "critical" for g in report.gates if not g.passed),
    )
    _check(
        "critical penalty pulls confidence low",
        report.confidence_penalty >= 0.30,
        f"penalty={report.confidence_penalty}",
    )

    # Reviewer state with BLOCKED output
    gate2 = QualityGate()
    report2 = gate2.evaluate(
        state=WorkflowState.REVIEW,
        agent_output="BLOCKED: No prior diff provided.",
        agent_role="reviewer",
    )
    _check("reviewer BLOCKED fails the gate", not report2.passed)
    _check(
        "reviewer BLOCKED is critical severity",
        any(g.details.get("severity") == "critical" for g in report2.gates if not g.passed),
    )


def test_consensus_no_longer_halves_multiplier() -> None:
    _section("Consensus blending (preserves multiplier)")
    ctrl = _make_controller()

    # High-performing developer should dominate
    ctrl.memory.update_role_performance("developer", True, 0.9)
    ctrl.memory.update_role_performance("developer", True, 0.9)

    opinions = [
        AgentOpinion(
            agent_role="developer",
            content="Function and test added successfully.",
            confidence=0.85,
            evidence=["assertions ran", "all test cases passed"],
        ),
        AgentOpinion(
            agent_role="qa",
            content="BLOCKED: cannot test without framework info.",
            confidence=0.30,
            evidence=[],
        ),
    ]
    conflict = AgentConflict(
        conflict_id="test-conflict",
        question="How confident are we in the result?",
        positions=opinions,
    )

    consensus = ctrl._build_consensus(conflict)

    _check(
        "developer wins (highest conf)",
        "developer" in consensus.participating_roles,
    )
    _check(
        "consensus preserves multiplier (clamped near 0.95)",
        consensus.confidence >= 0.90,
        f"got {consensus.confidence:.3f}",
    )


def test_self_correction_boost_is_small_but_present() -> None:
    _section("Self-correction context")
    ctrl = _make_controller()
    out = (
        "Initial attempt failed with import error. I caught the issue "
        "and fixed it by adding the missing dependency. After retry, "
        "the test suite passed cleanly. Final status: all assertions "
        "verified, no further action needed.\n" + ("line\n" * 20)
    )
    c = ctrl._calculate_confidence(out, _empty_quality_report())
    _check(
        "self-correction + length boost lands above 0.65",
        c >= 0.65,
        f"got {c:.3f}",
    )


def test_short_term_consistency_with_old_threshold() -> None:
    """Guard against silent regressions on the original behavior."""
    _section("Backward-compat sanity check")
    ctrl = _make_controller()
    out = "x" * 60
    c = ctrl._calculate_confidence(out, _empty_quality_report())
    _check(
        "neutral 60-char output >= 0.60 (raised base)",
        c >= 0.60,
        f"got {c:.3f}",
    )


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------


def run_all() -> int:
    test_empty_output_is_strong_negative()
    test_short_output_penalised()
    test_positive_signals_boost_confidence()
    test_traceback_near_end_penalised()
    test_blocked_signal_via_quality_gate_penalises()
    test_consensus_no_longer_halves_multiplier()
    test_self_correction_boost_is_small_but_present()
    test_short_term_consistency_with_old_threshold()

    print()
    print(f"=== Summary: {_PASSED} passed, {len(_FAILURES)} failed ===")
    if _FAILURES:
        for f in _FAILURES:
            print(f"  - {f}")
        return 1
    print("All confidence calibration tests passed.")
    return 0


if __name__ == "__main__":
    sys.exit(run_all())
