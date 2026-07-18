"""
Hive — Kilroy's structural workflow controller.

Wraps the existing CodingAssistant / smolagents execution layer with a strict
finite-state lifecycle. Enforces phase completion gates before allowing
progression. Delegates all code-level sandboxing and import containment
directly to the underlying smolagents CodeAgent via additional_authorized_imports.
"""

from __future__ import annotations

import json
import logging
from enum import Enum, auto
from typing import Any, Dict, List, Optional

logger = logging.getLogger("kilroy.hive")


class HiveState(Enum):
    """Structural phases in the engineering lifecycle."""

    PLANNING = auto()
    ARCHITECTING = auto()
    IMPLEMENTING = auto()
    VERIFYING = auto()
    HALTED = auto()


class StateTransitionError(Exception):
    """Raised when an invalid state jump is attempted."""

    pass


class WorkflowController:
    """
    High-level workflow engine that enforces a strict lifecycle across
    Kilroy's agent execution loop.

    Responsibilities:
      - Enforce valid state transitions via immutable transition graph
      - Require verification tokens before advancing phases
      - Delegate all code execution and sandboxing to CodingAssistant/smolagents
      - Maintain audit trail of phase completions and state changes

    Does NOT:
      - Inspect Python AST for malicious imports (smolagents handles this)
      - Validate individual command safety (smolagents sandbox handles this)
      - Allow arbitrary transition skipping (graph enforcement prevents this)
    """

    # Immutable transition graph — only valid edges allowed
    _VALID_TRANSITIONS: dict[HiveState, list[HiveState]] = {
        HiveState.PLANNING: [HiveState.ARCHITECTING, HiveState.HALTED],
        HiveState.ARCHITECTING: [
            HiveState.IMPLEMENTING,
            HiveState.PLANNING,  # Replan allowed if architecture is flawed
            HiveState.HALTED,
        ],
        HiveState.IMPLEMENTING: [HiveState.VERIFYING, HiveState.HALTED],
        HiveState.VERIFYING: [
            HiveState.PLANNING,
            HiveState.IMPLEMENTING,  # Back to implementation for defect fixes
            HiveState.HALTED,
        ],
        HiveState.HALTED: [],  # Terminal — requires explicit reset
    }

    def __init__(self, coding_assistant: Any, workspace_root: str) -> None:
        """
        Initialize the workflow controller.

        Args:
            coding_assistant: An instance of CodingAssistant (or compatible)
                with an .ask(prompt: str) -> str method.
            workspace_root: Absolute path to the workspace for context injection.
        """
        self.assistant = coding_assistant
        self.workspace_root = workspace_root
        self.current_state: HiveState = HiveState.PLANNING
        self.audit_trail: List[Dict[str, Any]] = []
        self._halt_reason: Optional[str] = None

    def transition_to(self, next_state: HiveState, verification_token: Dict[str, Any]) -> None:
        """
        Attempt a state transition, enforcing the graph and verification gates.

        Args:
            next_state: The target HiveState.
            verification_token: Dict with at least {"verified": bool, ...}.
                Must have verified=True for non-HALTED transitions.

        Raises:
            StateTransitionError: If the transition violates the graph or
                lacks a valid verification token.
        """
        # Terminal state guard
        if self.current_state == HiveState.HALTED:
            raise StateTransitionError("Cannot transition from HALTED. Call reset() to resume.")

        # Graph enforcement
        if next_state not in self._VALID_TRANSITIONS[self.current_state]:
            raise StateTransitionError(
                f"Invalid transition: {self.current_state.name} "
                f"cannot reach {next_state.name}. "
                f"Valid targets: "
                f"{[s.name for s in self._VALID_TRANSITIONS[self.current_state]]}"
            )

        # Verification gate — transitions to PLANNING or IMPLEMENTING from
        # ARCHITECTING or VERIFYING are allowed if a failure_mode diagnostic
        # is present (routing metadata). Only transitions WITHOUT any
        # verification signal are rejected.
        has_verification = (
            verification_token.get("verified", False)
            or verification_token.get("failure_mode") is not None
        )
        if next_state != HiveState.HALTED and not has_verification:
            raise StateTransitionError(
                f"Transition to {next_state.name} rejected: "
                f"verification token not satisfied. "
                f"Token: {verification_token}"
            )

        logger.info(
            "Hive transition: %s -> %s",
            self.current_state.name,
            next_state.name,
        )

        self.audit_trail.append(
            {
                "from": self.current_state.name,
                "to": next_state.name,
                "verification": verification_token,
            }
        )

        self.current_state = next_state

    def execute_phase(self, phase_prompt: str) -> str:
        """
        Execute a prompt within the current phase via the CodingAssistant.

        The phase context is injected into the prompt so the model understands
        its functional boundaries. All code-level safety is delegated to
        smolagents (additional_authorized_imports sandbox).

        Args:
            phase_prompt: The task prompt for the current phase.

        Returns:
            The assistant's raw response string.

        Raises:
            RuntimeError: If the controller is in HALTED state.
            Exception: Any exception from the assistant runtime, which
                automatically transitions to HALTED.
        """
        if self.current_state == HiveState.HALTED:
            raise RuntimeError(f"Cannot execute: Kilroy is HALTED. Reason: {self._halt_reason}")

        contextual_prompt = (
            f"[WORKFLOW PHASE: {self.current_state.name}]\n"
            f"Workspace: {self.workspace_root}\n"
            f"Execute strictly within this phase's constraints:\n"
            f"{phase_prompt}"
        )

        try:
            response = self.assistant.ask(contextual_prompt)
            return str(response)
        except Exception as exc:
            logger.critical(
                "Execution failure in %s phase: %s",
                self.current_state.name,
                exc,
            )
            self._enter_halted(str(exc))
            raise

    def verify_and_advance(
        self,
        raw_output: str,
        failure_mode: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Validate phase output and advance the state machine.

        Verification criteria per phase:
          - PLANNING:    Output parses as JSON with "objectives" and "success_criteria"
          - ARCHITECTING: Output contains "interface_definitions" or "schema"
          - IMPLEMENTING: Output is non-empty (execution succeeded in smolagents)
          - VERIFYING:   Output contains "test_pass_rate" OR explicit failure_mode

        Args:
            raw_output: The raw string output from the phase execution.
            failure_mode: For VERIFYING phase only: "requirements_mismatch"
                (route back to PLANNING) or "implementation_defect"
                (route back to IMPLEMENTING). None means no failure.

        Returns:
            Verification token dict with keys:
                - verified (bool): Whether the phase passed verification
                - phase (str): The phase that was verified
                - failure_mode (str|None): Classification if verification failed
                - extracted_metrics (dict): Any parsed data from the output
        """
        token: Dict[str, Any] = {
            "verified": False,
            "phase": self.current_state.name,
            "failure_mode": failure_mode,
            "extracted_metrics": {},
        }

        try:
            if self.current_state == HiveState.PLANNING:
                parsed = json.loads(raw_output)
                if "objectives" in parsed and "success_criteria" in parsed:
                    token["verified"] = True
                    token["extracted_metrics"] = parsed
                    self.transition_to(HiveState.ARCHITECTING, token)

            elif self.current_state == HiveState.ARCHITECTING:
                if ("interface_definitions" in raw_output) or ("schema" in raw_output):
                    token["verified"] = True
                    self.transition_to(HiveState.IMPLEMENTING, token)

            elif self.current_state == HiveState.IMPLEMENTING:
                if len(raw_output.strip()) > 0:
                    token["verified"] = True
                    self.transition_to(HiveState.VERIFYING, token)
                else:
                    token["failure_mode"] = "empty_output"

            elif self.current_state == HiveState.VERIFYING:
                if failure_mode is None and "test_pass_rate" in raw_output:
                    token["verified"] = True
                    self.transition_to(HiveState.PLANNING, token)
                elif failure_mode == "requirements_mismatch":
                    # Architecture is sound, but doesn't satisfy objectives
                    # Route back to PLANNING — do NOT mark as verified
                    token["verified"] = False
                    token["failure_mode"] = "requirements_mismatch"
                    token["extracted_metrics"] = {"routed_to": "PLANNING"}
                    self.transition_to(HiveState.PLANNING, token)
                elif failure_mode == "implementation_defect":
                    # Code is broken but architecture holds
                    # Route back to IMPLEMENTING — do NOT mark as verified
                    token["verified"] = False
                    token["failure_mode"] = "implementation_defect"
                    token["extracted_metrics"] = {"routed_to": "IMPLEMENTING"}
                    self.transition_to(HiveState.IMPLEMENTING, token)
                else:
                    token["failure_mode"] = failure_mode or "verification_incomplete"

        except StateTransitionError:
            # Re-raise — transition errors are fatal to the workflow
            raise
        except Exception as exc:
            logger.error("Verification parsing failed: %s", exc)
            token["error"] = str(exc)

        return token

    def reset(self) -> None:
        """
        Resume from HALTED state back to PLANNING.

        Caller is responsible for ensuring the underlying issue is resolved.
        """
        if self.current_state != HiveState.HALTED:
            logger.warning("reset() called while not in HALTED state — no-op.")
            return

        logger.info("Resetting from HALTED to PLANNING")
        self._halt_reason = None
        self.current_state = HiveState.PLANNING

    def halt(self, reason: str) -> None:
        """
        Intentionally halt the workflow with a diagnostic reason.

        This is different from the automatic _enter_halted() triggered by
        execution failures — this is for caller-initiated stops.
        """
        self._enter_halted(reason)

    def _enter_halted(self, reason: str) -> None:
        """Internal: transition to HALTED with audit trail entry."""
        logger.error("Kilroy HALTED: %s", reason)
        self._halt_reason = reason
        self.audit_trail.append(
            {
                "event": "halted",
                "reason": reason,
                "state": self.current_state.name,
            }
        )
        self.current_state = HiveState.HALTED

    @property
    def is_halted(self) -> bool:
        return self.current_state == HiveState.HALTED

    @property
    def progress(self) -> str:
        """Human-readable progress: 'PLANNING (phase 1/4)'."""
        active_states = [
            HiveState.PLANNING,
            HiveState.ARCHITECTING,
            HiveState.IMPLEMENTING,
            HiveState.VERIFYING,
        ]
        idx = (
            active_states.index(self.current_state) + 1
            if self.current_state in active_states
            else 0
        )
        return f"{self.current_state.name} (phase {idx}/{len(active_states)})"
