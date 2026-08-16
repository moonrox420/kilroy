"""
SmartCoderController (Maestro) — Kilroy's central task coordinator.

Phase 2: Intelligent Maestro Orchestration with smart short-circuiting.

The Maestro now:
  1. Receives a user request
  2. Initialises ExecutionMemory and DecisionRegistry
  3. Detects task complexity early and short-circuits trivial tasks
  4. Dispatches to specialist agents with full context (when needed)
  5. Records agent opinions, detects conflicts, builds consensus
  6. Applies the Architecture Guardian and Technical Review Board
  7. Manages Failure Analysis Mode when things go wrong
  8. Locks decisions that should not be reopened
  9. Synthesises a coherent final response (never raw specialist output)
  10. Produces full execution telemetry for audit
  11. Adheres to the Kilroy persona throughout

Backward compatibility: public API (run, chat, reset) is unchanged.
"""

import hashlib
import logging
import re
import sys
import time
from typing import Any

from smartcoder.agents.coding_assistant import CodingAssistant
from smartcoder.controllers.quality import QualityGate, QualityReport
from smartcoder.controllers.workflow import WorkflowEngine, WorkflowState
from smartcoder.infrastructure.dependencies import DependencyManager
from smartcoder.intelligence import (
    AgentConflict,
    AgentConsensus,
    AgentOpinion,
    AgentResult,
    DecisionRegistry,
    ExecutionMemory,
    ExecutionTelemetry,
    FailureAnalysisMode,
    KilroyPersona,
    TechnicalReviewBoard,
)
from smartcoder.intelligence.guardian import (
    ArchitectureGuardian,
    ArchitectureGuardianReport,
)
from smartcoder.runtime.config import AppConfig

logger = logging.getLogger("smartcoder.maestro")


class SmartCoderController:
    """
    Maestro — coordinates the multi-agent execution workflow.

    The controller consumes infrastructure components and dispatches to
    agents. It owns memory, decisions, telemetry, and synthesis.
    """

    def __init__(self, config: AppConfig, deps: DependencyManager | None = None) -> None:
        self.config = config
        self.deps = deps or DependencyManager()

        # Phase 1 plumbing (unchanged)
        self.workflow = WorkflowEngine(task_type=config.task_type or "code")
        self.quality_gate = QualityGate()

        # ---------- Phase 2 intelligence layer ----------
        self.memory = ExecutionMemory(
            task_type=config.task_type or "code",
        )
        self.decisions = DecisionRegistry()
        self.telemetry = ExecutionTelemetry()
        self.persona = KilroyPersona()
        self.failure_mode = FailureAnalysisMode(memory=self.memory)
        self.guardian = ArchitectureGuardian()
        self.trb = TechnicalReviewBoard(registry=self.decisions)

        self._assistant: CodingAssistant | None = None
        self._results: dict[str, Any] = {}
        self._agent_results: dict[str, AgentResult] = {}

        self._retrying: bool = False

        # -- Telemetry counters --
        self._short_circuit_count: int = 0

    # ------------------------------------------------------------------
    # Public API  (unchanged signatures for backward compatibility)
    # ------------------------------------------------------------------

    def run(self, task: str) -> Any:
        """
        Execute a task through the full intelligent workflow pipeline.

        Returns the final synthesised result after all workflow stages,
        quality gates, deliberation, and review.
        """
        logger.info(
            "Maestro starting task (type=%s, role=%s)",
            self.config.task_type or "code",
            self.config.task_role or "auto",
        )

        # Guard: prevent re-entry without reset() to avoid stale state carry-over.
        if self.workflow.is_complete or self.workflow.current_index != 0:
            raise RuntimeError("Controller already ran. Call reset() before starting a new task.")

        self.telemetry.timeline.record("task_start", f"Type={self.config.task_type}")
        self.memory.task_description = task

        # Record initial constraints from config
        if self.config.context_file:
            self.memory.constraints.append(f"context_file={self.config.context_file}")
        if self.config.use_dataset_rag and self.config.datasets:
            self.memory.constraints.append(f"datasets={list(self.config.datasets)}")

        if (
            self.config.task_type == "analysis"
            and self.config.task_role == "architect"
        ):
            return self._run_targeted_architect_analysis(task)

        # === CRITICAL: Task complexity detection for short-circuit ===
        is_trivial = self._is_trivial_task(task)
        if is_trivial:
            logger.info("Trivial task detected — short-circuiting to direct implementation")
            self.decisions.record(
                "approach",
                "Direct developer implementation (trivial task)",
                rationale="Task complexity detection triggered short-circuit",
                confidence=0.9,
                lock=True,
            )
            self._short_circuit_count += 1
            # Bypass full ceremony — run developer agent only
            result = self._execute_direct_developer(task)
            self.telemetry.timeline.record(
                "task_complete",
                f"Short-circuit success (count={self._short_circuit_count})",
            )
            logger.info(
                "Short-circuit path completed (total_short_circuits=%d)",
                self._short_circuit_count,
            )
            return result

        # Record locked high-level decisions for complex tasks
        self.decisions.record(
            "approach",
            f"Standard {self.config.task_type or 'code'} workflow through "
            f"{len(self.workflow.path)} states.",
            rationale="Default execution strategy",
            confidence=0.7,
            lock=True,
        )

        # Phase 2+: explicit state machine — run agent, gate, then advance/retry/fail
        agent_output: Any = ""
        self._retrying = False
        while not self.workflow.is_complete:
            # Determine the next state to process. On first entry after INITIALIZED
            # we advance; on retry we stay on the same state.
            if not self._retrying:
                self.workflow.advance()
            else:
                self._retrying = False

            self.telemetry.timeline.record(
                "workflow_advance",
                f"{self.workflow.current_state.name}",
            )

            # Execute specialist agent for this state (before gate)
            if self.workflow.current_state in (
                WorkflowState.PLANNING,
                WorkflowState.ARCHITECTURE,
                WorkflowState.IMPLEMENTATION,
                WorkflowState.TESTING,
                WorkflowState.REVIEW,
            ):
                agent_role = self.workflow.agent_for_state()
                if agent_role:
                    prior_ctx = self._build_prior_stage_context(exclude_role=agent_role)
                    result = self._run_with_role_intelligent(
                        task,
                        agent_role,
                        prior_stage_context=prior_ctx or None,
                    )
                    agent_output = result.content
                    self._results[agent_role] = result.content
                    self.memory.stage_outputs[agent_role] = result.content

                    # Architecture guardian check on all specialist roles
                    if agent_role not in ("planner", "architect"):
                        report = self._apply_guardian(result.content)
                        if (
                            report is not None
                            and (not report.passed)
                            and self.guardian.has_unresolved_vetoes
                        ):
                            self.workflow.block(f"Guardian veto: {report.summary}")
                            break

            elif self.workflow.current_state == WorkflowState.QUALITY_GATE:
                # Technical Review Board at quality gate
                review_target = agent_output or ""
                review_report = self.trb.review(
                    review_target,
                    context={"constraints": self.memory.constraints},
                )
                self.telemetry.timeline.record(
                    "trb_review",
                    f"{review_report.review_id}: {'PASSED' if review_report.overall_passed else 'FAILED'}",
                )
                if not review_report.overall_passed:
                    for issue in review_report.blocking_issues:
                        logger.warning("TRB blocking issue: %s", issue.notes)
                    self.workflow.block(review_report.summary)
                    break

            elif self.workflow.current_state == WorkflowState.FINALIZATION:
                self._results["final"] = self._synthesize_intelligent(task)
                agent_output = self._results["final"]

            # Quality gate check — runs AFTER agent execution for this state,
            # using the FRESH agent_output. If the gate fails, retry in place
            # (do NOT advance) or fail.
            if self.workflow.current_state in (
                WorkflowState.IMPLEMENTATION,
                WorkflowState.TESTING,
                WorkflowState.REVIEW,
                WorkflowState.QUALITY_GATE,
            ):
                gate_result = self.quality_gate.evaluate(
                    state=self.workflow.current_state,
                    agent_output=agent_output,
                    agent_role=self.workflow.agent_for_state(),
                    workflow_history=self.workflow.history,
                    agent_results=self._agent_results,
                )
                self.telemetry.log_quality_gate(
                    gate_result.passed,
                    self.workflow.current_state.name,
                    gate_result.summary,
                )

                if (
                    not gate_result.passed
                    and self.workflow.current_state != WorkflowState.QUALITY_GATE
                ):
                    logger.warning(
                        "Quality gate failed at %s: %s",
                        self.workflow.current_state.name,
                        gate_result.summary,
                    )

                    terminal_gate = next(
                        (
                            gate
                            for gate in gate_result.gates
                            if not gate.passed
                            and gate.gate_name in {"blocked_signal", "runtime_failure"}
                        ),
                        None,
                    )
                    if terminal_gate is not None:
                        self.workflow.block(terminal_gate.message)
                        break

                    if self.workflow.current_state in (
                        WorkflowState.IMPLEMENTATION,
                        WorkflowState.TESTING,
                    ):
                        attempts_key = (
                            "_impl_attempts"
                            if self.workflow.current_state == WorkflowState.IMPLEMENTATION
                            else "_test_attempts"
                        )
                        attempts = self.memory.stage_outputs.get(attempts_key, 0) + 1
                        self.memory.stage_outputs[attempts_key] = attempts

                        if attempts >= 3:
                            self.workflow.fail(
                                f"{self.workflow.current_state.name} failed {attempts} consecutive times"
                            )
                            continue

                        logger.info(
                            "Routing back for retry (%s attempt %d)",
                            self.workflow.current_state.name,
                            attempts,
                        )
                        self._retrying = True
                        agent_output = self._rerun_agent(task)
                        continue

                    if self.workflow.current_state == WorkflowState.REVIEW:
                        review_attempts = self.memory.stage_outputs.get("_review_attempts", 0) + 1
                        self.memory.stage_outputs["_review_attempts"] = review_attempts
                        if review_attempts >= 2:
                            self.workflow.block(
                                "Review found unresolved blocking issues after retry"
                            )
                            break
                        self._retrying = True
                        agent_output = self._rerun_agent(task)
                        continue

        if self.workflow.current_state in (WorkflowState.FAILED, WorkflowState.BLOCKED):
            reason = self.workflow.metadata.get(
                "failed_reason",
                self.workflow.metadata.get(
                    "blocked_reason", "Smart Coder workflow did not complete"
                ),
            )
            self.telemetry.timeline.record("task_complete", f"Failed: {reason}")
            raise RuntimeError(str(reason))

        logger.info(
            "Maestro completed task (states=%d, result_keys=%s)",
            len(self.workflow.history),
            list(self._results.keys()),
        )
        self.telemetry.timeline.record("task_complete", "Success")
        return self._build_final_output(agent_output)

    def _run_targeted_architect_analysis(self, task: str) -> str:
        """Run Kilroy's project analysis as one explicit architect turn."""
        self.decisions.record(
            "approach",
            "Direct architect analysis",
            rationale="Kilroy requested an explicit project-grounded architect report",
            confidence=0.9,
            lock=True,
        )
        try:
            result = self._run_with_role_intelligent(task, "architect")
        except Exception as exc:
            self.workflow.fail(str(exc))
            self.telemetry.timeline.record("task_complete", f"Failed: {exc}")
            raise

        self._results["architect"] = result.content
        self.memory.stage_outputs["architect"] = result.content
        self.workflow.complete("targeted architect analysis")
        self.telemetry.timeline.record("task_complete", "Success")
        logger.info("Targeted architect analysis completed")
        return result.content

    def chat(self) -> None:
        """Interactive session — delegate to CodingAssistant (unchanged)."""
        assistant = self._get_assistant()
        assistant.chat()

    def reset(self) -> None:
        """Reset workflow, memory, and cached agent for a new task."""
        self.workflow = WorkflowEngine(task_type=self.config.task_type or "code")
        self.quality_gate = QualityGate()
        self.memory = ExecutionMemory(task_type=self.config.task_type or "code")
        self.decisions = DecisionRegistry()
        self.telemetry = ExecutionTelemetry()
        self.failure_mode = FailureAnalysisMode(memory=self.memory)
        self._results = {}
        self._agent_results = {}
        self._short_circuit_count = 0
        if self._assistant:
            self._assistant.reset_agent()
            self._assistant = None

    # ------------------------------------------------------------------
    # New: Task complexity detection
    # ------------------------------------------------------------------

    def _is_trivial_task(self, task: str) -> bool:
        """Detect tasks that should bypass full multi-agent ceremony.

        A task is considered trivial when it is short (<120 chars) AND
        contains unambiguous simple-task keywords. System-level keywords
        (database, auth, API, etc.) block the short-circuit even if the
        task is short, because those tasks genuinely need architecture
        and review.
        """
        task_lower = task.lower().strip()

        # Never short-circuit system-level / infrastructure tasks — they
        # always need planning, architecture, and review.
        non_trivial_signals = [
            "database",
            "api",
            "auth",
            "migration",
            "deploy",
            "config",
            "endpoint",
            "route",
            "schema",
            "async",
            "websocket",
            "middleware",
            "plugin",
            "module",
            "container",
            "docker",
            "kubernetes",
            "k8s",
            "helm",
            "ci/cd",
            "pipeline",
            "monitor",
            "observability",
            "security",
            "encrypt",
            "oauth",
            "jwt",
            "session",
        ]
        if any(sig in task_lower for sig in non_trivial_signals):
            return False

        # Trivial tasks: short, simple, self-contained
        trivial_keywords = [
            "simple function",
            "add two numbers",
            "write a function that",
            "hello world",
            "print",
            "sum",
            "calculate",
            "basic",
            "small script",
            "tiny",
            "one-liner",
            "a function",
        ]
        keyword_match = any(kw in task_lower for kw in trivial_keywords)
        length_score = len(task) < 120

        return length_score and keyword_match

    def _execute_direct_developer(self, task: str) -> Any:
        """Direct execution path for trivial tasks — developer only.

        Skips planner, architect, QA, and reviewer agents entirely.
        The result still goes through confidence scoring via
        _wrap_and_store_result() so downstream consumers see consistent
        metrics.
        """
        assistant = self._get_assistant()
        result_text = assistant.ask(task)

        # Wrap and store — still produces confidence score / AgentResult
        wrapped = self._wrap_and_store_result("developer", result_text)

        self._results["final"] = result_text
        logger.info(
            "Direct developer path completed (conf=%.0f%%)",
            wrapped.confidence * 100 if hasattr(wrapped, "confidence") else 0,
        )
        return result_text

    # ------------------------------------------------------------------
    # Internal methods
    # ------------------------------------------------------------------

    def _get_assistant(self) -> CodingAssistant:
        """Lazy-build and return the CodingAssistant."""
        if self._assistant is None:
            self._assistant = CodingAssistant(self.config, self.deps)
        return self._assistant

    def _execute_current_agent(self, task: str) -> Any:
        """Execute the current workflow stage using the configured agent."""
        assistant = self._get_assistant()
        role = self.workflow.agent_for_state()
        if role and role != "developer":
            role_config = self._config_for_role(role)
            role_assistant = CodingAssistant(role_config, self.deps)
            result_text = role_assistant.ask(task)
        else:
            result_text = assistant.ask(task)

        self._wrap_and_store_result(role or self.config.task_role or "default", result_text)
        self._results[self.workflow.current_state.name.lower()] = result_text
        return result_text

    def _rerun_agent(self, task: str) -> Any:
        """Re-run current agent; reset to force fresh context."""
        if self._assistant:
            self._assistant.reset_agent()
            self._assistant = None
        return self._execute_current_agent(task)

    def _run_with_role(
        self,
        task: str,
        role: str,
        prior_stage_context: str | None = None,
    ) -> Any:
        """Run task with a specific agent role (specialist)."""
        role_config = self._config_for_role(role)
        role_assistant = CodingAssistant(role_config, self.deps)
        result = role_assistant.ask(
            task,
            prior_stage_context=prior_stage_context,
        )
        logger.info("Role agent '%s' completed", role)
        return result

    def _calculate_confidence(self, raw: Any, quality_report: QualityReport) -> float:
        """Calibrate confidence from observable output signals and quality gates.

        Starts from a base of 0.6 and adjusts based on:
        - Strong positive signals (test pass, build success, assertions)
        - Output length/structure adequacy
        - Self-correction context (agent caught and handled an error)
        - Unresolved error indicators near the end (traceback)
        - Quality gate penalties (graduated, not binary)
        """
        output = str(raw)

        # Empty output is a strong negative signal — short-circuit early
        if not output or not output.strip():
            return 0.1

        base = 0.6  # Coherent structured output is itself a positive signal
        lower = output.lower()

        # -- Strong positive signals (cumulative, capped at +0.25) -------
        # Detecting successful execution is the single most important signal
        # that the task actually succeeded. Multi-pattern regex avoids both
        # false positives (e.g. "no errors" inside a debug log) and substring
        # collisions (e.g. "subprocess.call" matching "call").
        positive_signals: list[tuple[str, float]] = [
            (r"\ball tests? (passed|cases? passed)\b", 0.20),
            (r"\b0 (failed|errors?|failures?)\b", 0.15),
            (r"\bbuild succeeded\b|\bcompilation successful\b", 0.20),
            (r"\b1 passed\b", 0.10),
            (r"^\s*assert .+ == .+", 0.10),
            (r"\bpytest\b.{0,40}\bpassed\b", 0.20),
            (r"\bok\b\s*\d*\s*\bpassed\b", 0.10),
        ]
        positive_total = 0.0
        for pattern, bonus in positive_signals:
            if re.search(pattern, output, re.IGNORECASE | re.MULTILINE):
                positive_total += bonus
        positive_total = min(positive_total, 0.25)
        base += positive_total

        # -- Length/structure signals -------------------------------------
        length = len(output)
        if length < 50:
            base -= 0.15  # Very short output is suspicious
        elif length > 500:
            base += 0.05  # Substantial output is a positive signal

        # -- Self-correction context ---------------------------------------
        if any(kw in lower for kw in ("caught", "handling", "recovered", "retry", "fix")):
            base += 0.05  # Agent is actively managing errors

        # -- Unresolved error indicators near the end ----------------------
        # Only traceback is flagged because "error:", "exception:", and
        # "failed:" appear in legitimate error-handling code and generate
        # too many false positives. A traceback near the end of output
        # strongly suggests the agent itself crashed.
        output_len = len(output)
        if "traceback" in lower:
            traceback_idx = lower.rfind("traceback")
            if traceback_idx > max(output_len - 200, 0):
                base -= 0.20  # Raw traceback near end = unhandled crash

        # -- Clamp before quality penalty ----------------------------------
        base = max(0.0, min(base, 0.95))

        # Apply quality gate penalty (graduated, not binary)
        penalty = quality_report.confidence_penalty
        calibrated = base - penalty

        return max(0.0, min(calibrated, 1.0))

    def _run_with_role_intelligent(
        self,
        task: str,
        role: str,
        prior_stage_context: str | None = None,
    ) -> AgentResult:
        """
        Run task with a specific role, producing an AgentResult with
        confidence scoring, and feed the opinion into ExecutionMemory.
        """
        ap = self.telemetry.log_agent_start(role)
        try:
            raw = self._run_with_role(task, role, prior_stage_context=prior_stage_context)

            # Evaluate quality to inform confidence
            quality = self.quality_gate.evaluate(
                state=self.workflow.current_state,
                agent_output=str(raw),
                agent_role=role,
                workflow_history=self.workflow.history,
            )
            confidence = self._calculate_confidence(raw, quality)

            result = AgentResult(
                role=role,
                content=str(raw),
                confidence=confidence,
                evidence=[f"Agent {role} produced output"],
            )
            self._agent_results[role] = result
            self.memory.add_opinion(result.to_opinion())
            self.memory.update_role_performance(role, success=True, confidence=result.confidence)
            self._detect_conflicts(role)
            self.telemetry.log_agent_end(ap, True, len(str(raw)))
            return result
        except Exception as exc:
            logger.error("Agent %s failed: %s", role, exc)
            self.memory.update_role_performance(role, success=False, confidence=0.0)
            self.telemetry.log_agent_end(ap, False, 0)
            if not self.failure_mode.active:
                self.failure_mode.activate(trigger=str(exc))
            raise

    def _wrap_and_store_result(self, role: str, raw: Any) -> AgentResult:
        """Wrap a raw result into an AgentResult; store and feed to memory."""
        quality = self.quality_gate.evaluate(
            state=self.workflow.current_state,
            agent_output=str(raw),
            agent_role=role,
            workflow_history=self.workflow.history,
        )
        confidence = self._calculate_confidence(raw, quality)
        result = AgentResult(
            role=role,
            content=str(raw),
            confidence=confidence,
            evidence=["raw output"],
        )
        self._agent_results[role] = result
        self.memory.add_opinion(result.to_opinion())
        return result

    def _detect_conflicts(self, new_role: str) -> None:
        """
        Compare the new agent's opinion against prior ones. If there are
        substantive disagreements, build a conflict record.
        """
        prior_roles = [r for r in self._agent_results if r != new_role]
        if not prior_roles:
            return
        new_result = self._agent_results[new_role]
        conflicts: list[AgentOpinion] = []
        for prior_role in prior_roles:
            prior = self._agent_results[prior_role]
            # Simple heuristic: if one agent says "BLOCKED" and another
            # does not, that's a conflict worth recording.
            if "BLOCKED" in new_result.content and "BLOCKED" not in prior.content:
                conflicts.append(prior.to_opinion())
                conflicts.append(new_result.to_opinion())
        if len(conflicts) >= 2:
            conflict = AgentConflict(
                conflict_id=_short_id("conflict"),
                question=f"Disagreement between {new_role} and {prior_roles}",
                positions=conflicts,
            )
            self.memory.record_conflict(conflict)
            self.telemetry.log_conflict(conflict)
            self._build_consensus(conflict)

    def _build_consensus(self, conflict: AgentConflict) -> AgentConsensus:
        """
        Build a consensus from conflicting positions.

        Uses weighted scoring that combines:
        - Reported confidence from the agent
        - Historical performance of the role on similar tasks
        - Evidence richness (number of evidence items)

        This prevents a single high-confidence-but-unreliable opinion from
        dominating over more measured, historically accurate positions.
        """
        positions = conflict.positions
        if not positions:
            return AgentConsensus(
                consensus_id=_short_id("consensus"),
                question=conflict.question,
                final_decision="No positions to resolve.",
                confidence=0.0,
            )

        def _weighted_score(position: AgentOpinion) -> tuple[float, int, float]:
            """Compute weighted score for ranking positions."""
            # Calibrate confidence with role performance history
            perf_mult = self.memory.role_confidence_multiplier(position.agent_role)
            calibrated_conf = position.confidence * perf_mult
            # Primary sort: calibrated confidence (float)
            # Secondary sort: evidence count (int, richer argument wins ties)
            # Tertiary sort: recency (float timestamp, newer wins further ties)
            return (
                calibrated_conf,
                len(position.evidence),
                position.timestamp,
            )

        best = max(positions, key=_weighted_score)
        # Apply the role-performance multiplier directly to the winning
        # confidence. The previous formula `(c + c*pm)/2` always halved the
        # multiplier's effect (a 1.2x boost became 1.1x). Multiplication
        # preserves the signal: a strong agent on a historically accurate
        # role should report that strength, not be diluted toward 1.0.
        perf_mult = self.memory.role_confidence_multiplier(best.agent_role)
        blended_confidence = min(best.confidence * perf_mult, 0.95)

        consensus = AgentConsensus(
            consensus_id=_short_id("consensus"),
            question=conflict.question,
            conflicts=[conflict],
            final_decision=best.content[:500],
            participating_roles=[p.agent_role for p in positions],
            confidence=blended_confidence,
        )
        conflict.resolution = consensus.final_decision
        conflict.resolved_by = "maestro"
        conflict.resolution_confidence = consensus.confidence
        self.memory.record_consensus(consensus)
        self.telemetry.log_consensus(consensus)
        self.decisions.record(
            consensus.consensus_id,
            consensus.final_decision,
            rationale=f"Resolved conflict: {conflict.question}",
            confidence=consensus.confidence,
            lock=True,
        )
        logger.info(
            "Consensus %s reached (conf=%.0f%%): %s",
            consensus.consensus_id,
            consensus.confidence * 100,
            consensus.final_decision[:80],
        )
        return consensus

    def _apply_guardian(self, content: str) -> ArchitectureGuardianReport | None:
        """Run the Architecture Guardian; return the report (and record vetoes)."""
        report = self.guardian.evaluate(
            content,
            context={
                "existing_architecture": self.memory.stage_outputs.get("architect", "") or "",
            },
        )
        if not report.passed:
            logger.warning("Guardian vetoed change: %s", report.summary)
            self.decisions.record(
                _short_id("guardian"),
                "Change vetoed by guardian",
                rationale=report.summary,
                confidence=0.9,
                lock=False,
            )
        return report

    def _config_for_role(self, role: str) -> AppConfig:
        """Return a copy of the config with a specific task role set.
        Uses AppConfig.with_role() which is validated and thread-safe."""
        return self.config.with_role(role)

    def _synthesize(self, task: str) -> str:
        """Produce a final synthesised response from all agent outputs."""
        return self._synthesize_intelligent(task)

    def _synthesize_intelligent(self, task: str) -> str:
        """
        Maestro synthesises a coherent response.

        P0-7 remediation: the user-facing primary deliverable must never be
        truncated. Specialist contributions may be summarized.
        """
        parts: list[str] = []
        parts.append("## Kilroy Maestro Synthesis\n")

        # Summarise decisions and constraints
        if self.memory.constraints:
            parts.append("**Constraints considered:**")
            for c in self.memory.constraints:
                parts.append(f"  - {c}")
            parts.append("")

        # Specialist contributions (summarized; capped)
        parts.append("### Specialist Contributions\n")
        for role, result in self._agent_results.items():
            if role in ("default",):
                continue
            bar = _confidence_bar(result.confidence)
            snippet = result.content.strip()
            if len(snippet) > 400:
                snippet = snippet[:400].rstrip() + "…"
            parts.append(f"- **{role}**: {bar} {snippet}")
        parts.append("")

        # Primary deliverable (never truncated)
        primary_role = None
        for candidate in ("developer", "code", "coder"):
            if candidate in self._agent_results:
                primary_role = candidate
                break
        if primary_role is None and self._agent_results:
            # Fallback: use the longest content as "primary"
            primary_role = max(
                self._agent_results.keys(),
                key=lambda r: len(self._agent_results[r].content or ""),
            )

        if primary_role and primary_role in self._agent_results:
            parts.append("---")
            parts.append("## Primary Deliverable\n")
            parts.append(self._agent_results[primary_role].content or "")
            parts.append("")

        # Conflicts and resolutions
        if self.memory.conflict_log:
            parts.append("### Conflicts Resolved\n")
            for conflict in self.memory.conflict_log:
                parts.append(
                    f"- **{conflict.conflict_id[:8]}**: {conflict.question} "
                    f"→ resolved by {conflict.resolved_by}"
                )
            parts.append("")

        # Decision registry highlight
        if self.decisions.all():
            parts.append("### Key Decisions\n")
            for d in self.decisions.all()[-5:]:
                parts.append(f"- [{d.decision_id}] {d.decision[:120]} (conf={d.confidence:.0%})")
            parts.append("")

        # Failure analysis if active
        if self.failure_mode.active:
            parts.append("### Failure Analysis\n")
            # Will be populated when finalise() is called

        parts.append("---")
        parts.append(
            f"Workflow: {len(self.workflow.history)} states traversed. "
            f"Decisions: {len(self.decisions.all())}. "
            f"Conflicts: {len(self.memory.conflict_log)}."
            f" Short-circuits: {self._short_circuit_count}."
        )
        return "\n".join(parts)

    def _build_prior_stage_context(self, exclude_role: str) -> str:
        """Build a compact prior-stage context from ExecutionMemory.stage_outputs."""
        parts: list[str] = []
        for role, output in self.memory.stage_outputs.items():
            if role.startswith("_"):
                continue
            if role == exclude_role:
                continue
            parts.append(f"### {role} output\n{str(output)[:2000]}")
        return "\n\n".join(parts)

    def _build_final_output(self, primary_result: Any) -> Any:
        """Build the final output returned to the caller (unchanged contract)."""
        if self.quality_gate.report.passed:
            return primary_result
        # If quality issues found, append report
        return f"{primary_result}\n\n---\n{self.quality_gate.report.summary}"


# ------------------------------------------------------------------
# Helpers
# ------------------------------------------------------------------


def _short_id(prefix: str) -> str:
    return hashlib.sha256(f"{prefix}{time.time()}".encode()).hexdigest()[:12]


def _confidence_bar(conf: float) -> str:
    pct = int(conf * 100)
    filled = pct // 10
    bar_unicode = f"[{'█' * filled}{'░' * (10 - filled)}] {pct}%"
    bar_ascii = f"[{'#' * filled}{'-' * (10 - filled)}] {pct}%"
    encoding = getattr(sys.stdout, "encoding", "") or ""
    return bar_ascii if "utf" not in encoding.lower() else bar_unicode
