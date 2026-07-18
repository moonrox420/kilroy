"""smartcoder.intelligence package — Phase 2 orchestration intelligence."""

from __future__ import annotations

import hashlib
import logging
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

logger = logging.getLogger("smartcoder.intelligence")

# ---------------------------------------------------------------------------
# Feature 1: Agent Deliberation Layer
# ---------------------------------------------------------------------------


@dataclass
class AgentOpinion:
    """A single agent's reasoned position on a question or output."""

    agent_role: str
    content: str
    reasoning: str = ""
    confidence: float = 0.5
    evidence: list[str] = field(default_factory=list)
    assumptions: list[str] = field(default_factory=list)
    risk_level: str = "medium"
    timestamp: float = field(default_factory=time.time)
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class AgentConflict:
    """A disagreement between two or more agents."""

    conflict_id: str
    question: str
    positions: list[AgentOpinion] = field(default_factory=list)
    resolution: str | None = None
    resolved_by: str | None = None
    resolution_confidence: float = 0.0
    timestamp: float = field(default_factory=time.time)


@dataclass
class AgentConsensus:
    """The resolved agreement across agents after deliberation."""

    consensus_id: str
    question: str
    conflicts: list[AgentConflict] = field(default_factory=list)
    final_decision: str = ""
    participating_roles: list[str] = field(default_factory=list)
    confidence: float = 0.0
    timestamp: float = field(default_factory=time.time)


# ---------------------------------------------------------------------------
# Feature 2: Persistent Working Memory
# ---------------------------------------------------------------------------


@dataclass
class ExecutionMemory:
    """
    Shared working memory owned by the Maestro; agents consume it without
    owning it. Contains every piece of context needed for coherent
    multi-agent execution.
    """

    task_description: str = ""
    task_type: str = "code"
    constraints: list[str] = field(default_factory=list)
    decisions: dict[str, Any] = field(default_factory=dict)
    rejected_approaches: list[str] = field(default_factory=list)
    open_questions: list[str] = field(default_factory=list)
    artifacts: dict[str, Any] = field(default_factory=dict)
    agent_notes: dict[str, list[str]] = field(default_factory=dict)
    context_snippets: dict[str, str] = field(default_factory=dict)
    stage_outputs: dict[str, Any] = field(default_factory=dict)
    metadata: dict[str, Any] = field(default_factory=dict)
    created_at: float = field(default_factory=time.time)

    opinions: dict[str, AgentOpinion] = field(default_factory=dict)
    conflict_log: list[AgentConflict] = field(default_factory=list)
    consensus_log: list[AgentConsensus] = field(default_factory=list)
    # Feature: per-role performance tracking for confidence calibration
    role_performance: dict[str, dict[str, Any]] = field(default_factory=dict)

    def add_opinion(self, opinion: AgentOpinion) -> None:
        key = f"opinion:{opinion.agent_role}:{int(opinion.timestamp)}"
        self.opinions[key] = opinion
        self.agent_notes.setdefault(opinion.agent_role, []).append(
            f"[{datetime.fromtimestamp(opinion.timestamp, tz=timezone.utc).isoformat()}] "
            f"opinion (conf={opinion.confidence:.2f}): {opinion.content[:200]}"
        )

    def record_conflict(self, conflict: AgentConflict) -> None:
        self.conflict_log.append(conflict)
        for pos in conflict.positions:
            self.add_opinion(pos)

    def record_consensus(self, consensus: AgentConsensus) -> None:
        self.consensus_log.append(consensus)
        self.decisions[consensus.consensus_id] = {
            "decision": consensus.final_decision,
            "confidence": consensus.confidence,
            "roles": consensus.participating_roles,
            "timestamp": consensus.timestamp,
        }

    def update_role_performance(self, role: str, success: bool, confidence: float) -> None:
        """Track per-role success rate and average confidence for calibration."""
        perf = self.role_performance.setdefault(
            role,
            {
                "attempts": 0,
                "successes": 0,
                "confidence_sum": 0.0,
                "avg_confidence": 0.5,
            },
        )
        perf["attempts"] += 1
        if success:
            perf["successes"] += 1
        perf["confidence_sum"] += confidence
        perf["avg_confidence"] = perf["confidence_sum"] / perf["attempts"]
        perf["success_rate"] = perf["successes"] / perf["attempts"]

    def role_confidence_multiplier(self, role: str) -> float:
        """Return a multiplier in [0.8, 1.2] based on historical performance.

        Roles with >80% success rate get a boost; roles below 40% get a
        penalty. The multiplier smooths toward 1.0 as more data accumulates.
        """
        perf = self.role_performance.get(role)
        if not perf or perf.get("attempts", 0) < 2:
            return 1.0  # Neutral until we have signal

        success_rate = perf.get("success_rate", 0.5)
        multiplier = 1.0 + (success_rate - 0.5) * 0.4  # 0.8 .. 1.2
        return max(0.8, min(1.2, multiplier))

    def to_dict(self) -> dict[str, Any]:
        return {
            "task_description": self.task_description,
            "task_type": self.task_type,
            "constraints": self.constraints,
            "decisions": self.decisions,
            "rejected_approaches": self.rejected_approaches,
            "open_questions": self.open_questions,
            "artifacts": {k: str(v) for k, v in self.artifacts.items()},
            "agent_notes": self.agent_notes,
            "context_snippets": self.context_snippets,
            "stage_outputs": {k: str(v)[:500] for k, v in self.stage_outputs.items()},
            "memory_age_s": round(time.time() - self.created_at, 2),
            "opinion_count": len(self.opinions),
            "conflict_count": len(self.conflict_log),
            "consensus_count": len(self.consensus_log),
        }


# ---------------------------------------------------------------------------
# Feature 3: Decision Registry
# ---------------------------------------------------------------------------


@dataclass
class LockedDecision:
    """An immutable decision that future agents must respect."""

    decision_id: str
    decision: str
    rationale: str
    timestamp: float = field(default_factory=time.time)
    confidence: float = 0.5
    locked: bool = False
    locked_by: str = "maestro"
    override_log: list[dict[str, Any]] = field(default_factory=list)

    def lock(self) -> None:
        self.locked = True

    def override(self, by: str, reason: str) -> None:
        if not self.locked:
            return
        self.override_log.append(
            {
                "timestamp": time.time(),
                "by": by,
                "reason": reason,
            }
        )
        logger.warning("Decision %s overridden by %s: %s", self.decision_id, by, reason)


class DecisionRegistry:
    """Central store of all decisions made during task execution."""

    def __init__(self) -> None:
        self._decisions: dict[str, LockedDecision] = {}

    def record(
        self,
        decision_id: str,
        decision: str,
        rationale: str = "",
        confidence: float = 0.5,
        lock: bool = False,
    ) -> LockedDecision:
        if decision_id in self._decisions:
            existing = self._decisions[decision_id]
            if existing.locked:
                existing.override(
                    by="maestro",
                    reason=f"Re-recording locked decision {decision_id}",
                )
        entry = LockedDecision(
            decision_id=decision_id,
            decision=decision,
            rationale=rationale,
            confidence=confidence,
            locked=lock,
        )
        if lock:
            entry.lock()
        self._decisions[decision_id] = entry
        logger.debug("Decision recorded: %s (locked=%s)", decision_id, lock)
        return entry

    def get(self, decision_id: str) -> LockedDecision | None:
        return self._decisions.get(decision_id)

    def lock(self, decision_id: str) -> None:
        entry = self._decisions.get(decision_id)
        if entry:
            entry.lock()

    def is_locked(self, decision_id: str) -> bool:
        entry = self._decisions.get(decision_id)
        return bool(entry and entry.locked)

    def all(self) -> list[LockedDecision]:
        return list(self._decisions.values())

    def summary(self) -> str:
        lines = ["## Decision Registry"]
        for d in self._decisions.values():
            status = "LOCKED" if d.locked else "open"
            lines.append(
                f"- [{status}] {d.decision_id}: {d.decision[:120]} "
                f"(conf={d.confidence:.2f}, by={d.locked_by})"
            )
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# Feature 4: Confidence Scoring (AgentResult)
# ---------------------------------------------------------------------------


@dataclass
class AgentResult:
    """Standardised output produced by every specialist agent."""

    role: str
    content: str
    confidence: float = 0.5
    evidence: list[str] = field(default_factory=list)
    assumptions: list[str] = field(default_factory=list)
    risk_level: str = "medium"
    metadata: dict[str, Any] = field(default_factory=dict)
    timestamp: float = field(default_factory=time.time)

    def to_opinion(self) -> AgentOpinion:
        return AgentOpinion(
            agent_role=self.role,
            content=self.content,
            confidence=self.confidence,
            evidence=list(self.evidence),
            assumptions=list(self.assumptions),
            risk_level=self.risk_level,
            timestamp=self.timestamp,
            metadata=dict(self.metadata),
        )


# ---------------------------------------------------------------------------
# Feature 5: Technical Review Board
# ---------------------------------------------------------------------------


@dataclass
class ReviewDimension:
    name: str
    passed: bool
    notes: str = ""
    severity: str = "info"


@dataclass
class ReviewReport:
    review_id: str
    target: str
    dimensions: list[ReviewDimension] = field(default_factory=list)
    overall_passed: bool = True
    summary: str = ""
    reviewer_roles: list[str] = field(default_factory=list)
    timestamp: float = field(default_factory=time.time)

    @property
    def blocking_issues(self) -> list[ReviewDimension]:
        return [d for d in self.dimensions if not d.passed and d.severity in ("error", "critical")]

    @property
    def warnings(self) -> list[ReviewDimension]:
        return [d for d in self.dimensions if not d.passed and d.severity == "warning"]


REVIEW_DIMENSIONS: list[dict[str, str]] = [
    {
        "name": "compilation",
        "prompt": "Does the code compile/parse without syntax errors?",
    },
    {"name": "scalability", "prompt": "Will this scale to larger inputs/loads?"},
    {
        "name": "architecture_fit",
        "prompt": "Is this consistent with existing architecture?",
    },
    {
        "name": "constraint_compliance",
        "prompt": "Does this satisfy the stated task constraints?",
    },
    {
        "name": "risk_assessment",
        "prompt": "What are the failure modes and mitigations?",
    },
]


class TechnicalReviewBoard:
    """Multi-dimension review performed before finalisation."""

    def __init__(self, registry: DecisionRegistry | None = None) -> None:
        self.registry = registry or DecisionRegistry()

    def review(self, target_content: str, context: dict[str, Any] | None = None) -> ReviewReport:
        context = context or {}
        review_id = hashlib.sha256(f"{target_content[:200]}{time.time()}".encode()).hexdigest()[:12]

        dimensions: list[ReviewDimension] = []
        for dim in REVIEW_DIMENSIONS:
            result = self._evaluate_dimension(dim["name"], target_content, context)
            dimensions.append(result)

        overall = not any(not d.passed for d in dimensions if d.severity in ("error", "critical"))
        report = ReviewReport(
            review_id=review_id,
            target=target_content[:200],
            dimensions=dimensions,
            overall_passed=overall,
            summary=f"TRB {review_id}: {'PASSED' if overall else 'FAILED'}",
            reviewer_roles=["maestro"],
        )
        logger.info("TRB %s: %s", review_id, "PASSED" if overall else "FAILED")
        return report

    def _evaluate_dimension(
        self, name: str, content: str, context: dict[str, Any]
    ) -> ReviewDimension:
        c = content.lower()
        passed = True
        notes = ""
        severity = "info"

        if name == "compilation":
            # Use word-boundary regex so substrings inside words/comments
            # like "this is NOT an error:" or "no syntaxerror here" don't
            # trigger a false positive. Only multi-line "traceback" blocks
            # and bare "syntaxerror" tokens are treated as failure signals.
            import re as _re

            for bad_pat, label in [
                (r"(?im)^[^\n]*\btraceback\b[^\n]*$", "traceback"),
                (r"\bsyntaxerror\b", "SyntaxError"),
            ]:
                if _re.search(bad_pat, c):
                    passed = False
                    notes = f"Possible {label} indicator."
                    severity = "error"
                    break
            if not notes:
                notes = "No syntax error indicators found."
        elif name == "constraint_compliance":
            constraints = context.get("constraints", [])
            if constraints:
                # Match each constraint as a word-boundary token in the
                # output. This avoids the substring bug where
                # "use_postgres" matched "use_postgresql".
                import re as _re

                satisfied = sum(
                    1
                    for con in constraints
                    if con and _re.search(rf"\b{_re.escape(con.lower())}\b", c)
                )
                if satisfied < len(constraints) * 0.5:
                    passed = False
                    notes = f"Only {satisfied}/{len(constraints)} constraints addressed."
                    severity = "warning"
                else:
                    notes = f"Addresses {satisfied}/{len(constraints)} constraints."
            else:
                notes = "No explicit constraints provided."
        elif name == "scalability":
            # Real heuristic: any mention of per-input/per-request cost,
            # or absence of obvious O(n^2) anti-patterns in code-heavy
            # output, raises confidence.
            import re as _re

            o_n_sq = _re.findall(r"\bfor\s+\w+\s+in\s+[^:]+\bfor\s+\w+\s+in\b", c)
            if o_n_sq:
                passed = False
                notes = f"Possible O(n^2) nested-loop pattern detected ({len(o_n_sq)} match(es))."
                severity = "warning"
            elif any(k in c for k in ("scalab", "throughput", "latency", "concurrent")):
                notes = "Scalability vocabulary found in output."
            else:
                notes = "No scalability heuristics matched; agent-based review recommended."
                severity = "warning"
        elif name == "architecture_fit":
            import re as _re

            # Look for at least one architectural anchor in the output
            # (a module/class/function name, or a stdlib import). Without
            # any anchor we cannot claim the output fits the existing
            # architecture.
            has_anchor = (
                _re.search(r"^(class|def|fn|function|module)\s+\w+", c, _re.MULTILINE) is not None
                or _re.search(r"^\s*import\s+\w+", c, _re.MULTILINE) is not None
                or _re.search(r"^\s*from\s+\w+\s+import\s+", c, _re.MULTILINE) is not None
            )
            if not has_anchor:
                passed = False
                notes = "No architectural anchor (class/function/import) found."
                severity = "warning"
            else:
                notes = "Architectural anchor(s) present in output."
        elif name == "risk_assessment":
            import re as _re

            # Require explicit, multi-word risk discussion, not just the
            # word "assuming" passing the test.
            risk_phrases = (
                "risk",
                "mitigation",
                "failure mode",
                "trade-off",
                "tradeoff",
                "downside",
                "caveat",
            )
            hits = sum(1 for p in risk_phrases if _re.search(rf"\b{p}\b", c))
            if hits >= 2:
                notes = f"Risk discussion present ({hits} risk-related phrases)."
            elif hits == 1:
                notes = "Single risk-related phrase found; limited discussion."
                severity = "warning"
            else:
                passed = False
                notes = "No explicit risk discussion found."
                severity = "warning"
        else:
            notes = "No heuristic defined for this dimension."

        return ReviewDimension(name=name, passed=passed, notes=notes, severity=severity)


# ---------------------------------------------------------------------------
# Feature 6: Kilroy Persona
# ---------------------------------------------------------------------------

_DEFAULT_PERSONA = """
# Kilroy Operating Persona

## Core Traits
- Direct, technically rigorous, honest about uncertainty
- Pragmatic, collaborative, accountable

## Communication Style
- First person as Kilroy; short, dense technical prose
- No marketing language; calls out inconsistencies immediately
- Never presents raw specialist output; always synthesises

## Orchestration Philosophy
- Maestro decides, specialists advise
- Conflicting opinions are surfaced and reasoned through
- Locked decisions are never silently overturned
- Confidence scores inform but do not dictate decisions
- Every significant change is validated against architecture
"""


class KilroyPersona:
    """Kilroy's stable operating identity."""

    def __init__(self, persona_dir: str | Path | None = None) -> None:
        self._traits: dict[str, Any] = {}
        self._prompt_suffix: str = ""
        persona_file = self._resolve_persona_file(persona_dir)
        self._load(persona_file)

    def _resolve_persona_file(self, persona_dir: str | Path | None) -> Path | None:
        if persona_dir is None:
            candidates = [
                Path.cwd() / "kilroy_persona.md",
                Path(__file__).resolve().parent.parent.parent / "kilroy_persona.md",
            ]
        else:
            candidates = [Path(persona_dir) / "kilroy_persona.md"]
        for c in candidates:
            if c.is_file():
                return c
        return None

    def _load(self, persona_file: Path | None) -> None:
        if persona_file and persona_file.is_file():
            try:
                raw = persona_file.read_text(encoding="utf-8")
                self._parse(raw)
                logger.info("Persona loaded from %s", persona_file)
            except Exception as exc:
                logger.warning("Failed to load persona: %s — using default.", exc)
                self._parse(_DEFAULT_PERSONA)
        else:
            self._parse(_DEFAULT_PERSONA)

    def _parse(self, raw: str) -> None:
        self._traits = {"raw": raw}
        self._prompt_suffix = (
            "\n\n# Kilroy Operating Persona\n"
            "Traits: direct, technically rigorous, honest about uncertainty, "
            "pragmatic, collaborative, accountable."
        )

    @property
    def prompt_suffix(self) -> str:
        return self._prompt_suffix

    @property
    def traits(self) -> dict[str, Any]:
        return dict(self._traits)

    def to_prompt_injection(self) -> str:
        return self._prompt_suffix


# ---------------------------------------------------------------------------
# Feature 7: Failure Analysis Mode
# ---------------------------------------------------------------------------


@dataclass
class FailureAnalysis:
    failure_id: str
    trigger: str = ""
    root_cause: str = ""
    contributing_factors: list[str] = field(default_factory=list)
    architecture_weaknesses: list[str] = field(default_factory=list)
    prevention_strategies: list[str] = field(default_factory=list)
    future_monitoring: list[str] = field(default_factory=list)
    remediation_plan: str = ""
    confidence: float = 0.0
    timestamp: float = field(default_factory=time.time)

    def to_markdown(self) -> str:
        parts = [
            f"## Failure Analysis [{self.failure_id[:8]}]",
            f"Trigger: {self.trigger}",
            f"Root Cause: {self.root_cause}",
        ]
        for label, items in [
            ("Contributing Factors", self.contributing_factors),
            ("Architecture Weaknesses", self.architecture_weaknesses),
            ("Prevention Strategies", self.prevention_strategies),
            ("Future Monitoring", self.future_monitoring),
        ]:
            if items:
                parts.append(f"\n**{label}:**")
                for item in items:
                    parts.append(f"  - {item}")
        parts.append(f"\nRemediation: {self.remediation_plan}")
        parts.append(f"Confidence: {self.confidence:.0%}")
        return "\n".join(parts)


class FailureAnalysisMode:
    """ANALYSIS -> ROOT_CAUSE -> REMEDIATION -> VALIDATION workflow branch."""

    BRANCH_STATES = ("ANALYSIS", "ROOT_CAUSE", "REMEDIATION", "VALIDATION")

    def __init__(self, memory: ExecutionMemory | None = None) -> None:
        self.memory = memory or ExecutionMemory()
        self._active = False
        self._analysis: FailureAnalysis | None = None

    def activate(self, trigger: str) -> None:
        self._active = True
        failure_id = hashlib.sha256(f"{trigger}{time.time()}".encode()).hexdigest()[:12]
        self._analysis = FailureAnalysis(failure_id=failure_id, trigger=trigger)
        logger.info("FailureAnalysisMode activated: %s", trigger)

    @property
    def active(self) -> bool:
        return self._active

    def record(self, field_name: str, value: str | list[str]) -> None:
        if not self._analysis:
            return
        list_fields = {
            "contributing_factors",
            "architecture_weaknesses",
            "prevention_strategies",
            "future_monitoring",
        }
        if field_name in list_fields:
            current = getattr(self._analysis, field_name)
            if isinstance(value, str):
                current.append(value)
            else:
                current.extend(value)
        else:
            setattr(self._analysis, field_name, value)

    def finalise(self, remediation: str, confidence: float = 0.6) -> FailureAnalysis:
        if not self._analysis:
            raise RuntimeError("FailureAnalysisMode not active")
        self._analysis.remediation_plan = remediation
        self._analysis.confidence = confidence
        self.memory.rejected_approaches.append(
            f"FAILURE:{self._analysis.failure_id}:{self._analysis.root_cause}"
        )
        result = self._analysis
        self._active = False
        self._analysis = None
        return result


# ---------------------------------------------------------------------------
# Feature 9: Execution Telemetry
# ---------------------------------------------------------------------------


@dataclass
class AgentParticipation:
    agent_role: str
    start_time: float = 0.0
    end_time: float = 0.0
    success: bool = False
    output_length: int = 0
    retries: int = 0


@dataclass
class ExecutionTimeline:
    events: list[dict[str, Any]] = field(default_factory=list)

    def record(
        self,
        event_type: str,
        detail: str,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        self.events.append(
            {
                "t": time.time(),
                "iso": datetime.now(timezone.utc).isoformat() + "Z",
                "type": event_type,
                "detail": detail,
                "meta": metadata or {},
            }
        )

    def to_markdown(self) -> str:
        lines = ["## Execution Timeline"]
        for ev in self.events:
            lines.append(f"- [{ev['iso']}] **{ev['type']}**: {ev['detail']}")
        return "\n".join(lines)


class ExecutionTelemetry:
    """Comprehensive audit trail of Kilroy's activities."""

    def __init__(self) -> None:
        self.timeline = ExecutionTimeline()
        self.participation: dict[str, AgentParticipation] = {}
        self.decisions_log: list[dict[str, Any]] = []
        self.conflicts_log: list[dict[str, Any]] = []
        self.consensus_log: list[dict[str, Any]] = []
        self.quality_outcomes: list[dict[str, Any]] = []

    def log_agent_start(self, role: str) -> AgentParticipation:
        ap = AgentParticipation(agent_role=role, start_time=time.time())
        key = f"{role}:{int(ap.start_time)}"
        self.participation[key] = ap
        self.timeline.record("agent_start", f"{role} began")
        return ap

    def log_agent_end(self, ap: AgentParticipation, success: bool, output_length: int = 0) -> None:
        ap.end_time = time.time()
        ap.success = success
        ap.output_length = output_length
        self.timeline.record(
            "agent_end",
            f"{ap.agent_role} finished in {ap.end_time - ap.start_time:.2f}s (success={success})",
        )

    def log_decision(self, decision_id: str, decision: str, by: str = "maestro") -> None:
        self.decisions_log.append(
            {
                "decision_id": decision_id,
                "decision": decision,
                "by": by,
                "timestamp": time.time(),
            }
        )
        self.timeline.record(
            "decision",
            f"{decision_id}: {decision[:120]}",
            {"by": by},
        )

    def log_conflict(self, conflict: AgentConflict) -> None:
        self.conflicts_log.append(
            {
                "conflict_id": conflict.conflict_id,
                "question": conflict.question,
                "roles": [p.agent_role for p in conflict.positions],
                "resolved": conflict.resolution is not None,
                "timestamp": conflict.timestamp,
            }
        )
        self.timeline.record(
            "conflict",
            f"{conflict.conflict_id[:8]}: '{conflict.question[:80]}' "
            f"roles={[p.agent_role for p in conflict.positions]}",
        )

    def log_consensus(self, consensus: AgentConsensus) -> None:
        self.consensus_log.append(
            {
                "consensus_id": consensus.consensus_id,
                "question": consensus.question,
                "confidence": consensus.confidence,
                "roles": consensus.participating_roles,
                "timestamp": consensus.timestamp,
            }
        )
        self.timeline.record(
            "consensus",
            f"{consensus.consensus_id[:8]}: '{consensus.question[:80]}' "
            f"(conf={consensus.confidence:.0%})",
        )

    def log_quality_gate(self, passed: bool, gate_name: str, detail: str = "") -> None:
        self.quality_outcomes.append(
            {
                "passed": passed,
                "gate": gate_name,
                "detail": detail,
                "timestamp": time.time(),
            }
        )
        self.timeline.record(
            "quality_gate",
            f"{gate_name}: {'PASS' if passed else 'FAIL'} — {detail[:80]}",
        )

    def to_report(self) -> str:
        parts = [
            "# Execution Telemetry Report",
            "",
            f"Generated: {datetime.now(timezone.utc).isoformat()}Z",
            "",
            f"Agent participations: {len(self.participation)}",
            f"Decisions recorded:  {len(self.decisions_log)}",
            f"Conflicts detected:  {len(self.conflicts_log)}",
            f"Consensus reached:   {len(self.consensus_log)}",
            f"Quality gate checks: {len(self.quality_outcomes)}",
            "",
            self.timeline.to_markdown(),
            "",
            "## Agent Participation",
        ]
        for key, ap in self.participation.items():
            duration = (ap.end_time - ap.start_time) if ap.end_time else 0.0
            parts.append(
                f"- {ap.agent_role}: {duration:.2f}s, "
                f"success={ap.success}, output={ap.output_length} chars"
            )
        parts.append("")
        parts.append("## Quality Gates")
        for q in self.quality_outcomes:
            parts.append(f"- {'PASS' if q['passed'] else 'FAIL'} {q['gate']}: {q['detail'][:80]}")
        parts.append("")
        return "\n".join(parts)
