"""
Architecture Guardian — Feature 8.

FIXES:
  * P3-1: import time moved to module level (was re-imported on every evaluate() call).
  * P2-5: Security pillar now catches the full subprocess / os.exec* family, not
    just subprocess.call and os.system. Added os.popen, subprocess.run,
    subprocess.Popen, subprocess.check_call, subprocess.check_output, os.execv,
    os.execve, __import__("subprocess"), importlib.import_module patterns.
  * P3-8: _veto_log is capped at a rolling window of MAX_VETO_LOG entries to
    prevent unbounded memory growth in long-running server deployments.
  * override_veto() / has_unresolved_vetoes / unresolved_vetoes unchanged —
    these were already correct. SmartCoderController.override_guardian_veto()
    wraps override_veto() (implemented in maestro.py as part of P0-1).
"""

from __future__ import annotations

import logging
import re
import time
from collections import deque
from dataclasses import dataclass, field
from typing import Any

logger = logging.getLogger("smartcoder.guardian")

_AVOID_LINE_RE = re.compile(r"(?im)^\s*(?:avoid|banned)\s*:\s*(.+)$")

# P3-8: rolling cap on veto log to prevent unbounded memory growth.
MAX_VETO_LOG = 1_000


@dataclass
class VetoRecord:
    veto_id: str
    target: str
    reason: str
    dimensions: list[str]
    overridden: bool = False
    override_reason: str = ""
    timestamp: float = 0.0


@dataclass
class ArchitectureGuardianReport:
    passed: bool
    vetoes: list[VetoRecord] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    recommendations: list[str] = field(default_factory=list)
    summary: str = ""


class ArchitectureGuardian:
    """Permanent architecture evaluation layer.

    Evaluates proposed changes against five pillars and may issue vetoes.
    Vetoes can only be overridden via override_veto() (audit-logged).
    """

    PILLARS = [
        "conventions",
        "architecture",
        "performance",
        "maintainability",
        "security",
    ]

    def __init__(self, registry: Any | None = None) -> None:
        self._registry = registry
        # P3-8: bounded deque so the log doesn't grow without limit.
        self._veto_log: deque[VetoRecord] = deque(maxlen=MAX_VETO_LOG)

    def evaluate(
        self,
        change_content: str,
        context: dict[str, Any] | None = None,
    ) -> ArchitectureGuardianReport:
        """Evaluate a proposed or applied code change."""
        context = context or {}
        vetoes: list[VetoRecord] = []
        warnings: list[str] = []
        recommendations: list[str] = []

        for pillar in self.PILLARS:
            issues = self._check_pillar(pillar, change_content, context)
            for issue in issues:
                severity = issue.get("severity", "warning")
                msg = issue["message"]
                if severity == "veto":
                    vetoes.append(
                        VetoRecord(
                            veto_id=issue.get("id", f"veto-{len(vetoes) + 1}"),
                            target=change_content[:120],
                            reason=msg,
                            dimensions=[pillar],
                            timestamp=time.time(),  # P3-1: module-level import
                        )
                    )
                elif severity == "warning":
                    warnings.append(f"[{pillar}] {msg}")
                else:
                    recommendations.append(f"[{pillar}] {msg}")

        has_blocking = bool(vetoes)
        passed = not has_blocking

        self._veto_log.extend(vetoes)
        summary = (
            f"Guardian: {'PASS' if passed else 'VETO'} — "
            f"vetoes={len(vetoes)}, warnings={len(warnings)}"
        )
        logger.info(summary)
        return ArchitectureGuardianReport(
            passed=passed,
            vetoes=list(vetoes),
            warnings=list(warnings),
            recommendations=list(recommendations),
            summary=summary,
        )

    def override_veto(self, veto_id: str, by: str, reason: str) -> bool:
        """Mark a veto as overridden. Returns True if a match was found."""
        for veto in self._veto_log:
            if veto.veto_id == veto_id and not veto.overridden:
                veto.overridden = True
                veto.override_reason = reason
                logger.warning("Guardian veto %s overridden by %s: %s", veto_id, by, reason)
                return True
        return False

    @property
    def has_unresolved_vetoes(self) -> bool:
        return any(not v.overridden for v in self._veto_log)

    @property
    def unresolved_vetoes(self) -> list[VetoRecord]:
        return [v for v in self._veto_log if not v.overridden]

    # -- internals --------------------------------------------------

    def _check_pillar(
        self, pillar: str, content: str, context: dict[str, Any]
    ) -> list[dict[str, Any]]:
        c = content.lower()
        issues: list[dict[str, Any]] = []

        if pillar == "conventions":
            anti_patterns = [
                {
                    "pattern": "print(",
                    "severity": "warning",
                    "message": "print() detected; use logging.",
                    "id_suffix": "print",
                },
                {
                    "pattern": "except:",
                    "severity": "veto",
                    "message": "Bare except clause forbidden by convention.",
                    "id_suffix": "bare-except",
                },
                {
                    "pattern": "import *",
                    "severity": "warning",
                    "message": "Wildcard import violates namespace conventions.",
                    "id_suffix": "wildcard-import",
                },
                {
                    "pattern": "exec(",
                    "severity": "veto",
                    "message": "exec() is a critical security risk.",
                    "id_suffix": "exec",
                },
                {
                    "pattern": "eval(",
                    "severity": "veto",
                    "message": "eval() is a critical security risk.",
                    "id_suffix": "eval",
                },
            ]
            for ap in anti_patterns:
                if ap["pattern"] in c:
                    issues.append(
                        {
                            "id": f"convention-{ap['id_suffix']}",
                            "severity": ap["severity"],
                            "message": ap["message"],
                            "pattern": ap["pattern"],
                        }
                    )

        elif pillar == "architecture":
            existing = context.get("existing_architecture", "") or ""
            if existing:
                banned_terms = [
                    m.strip().lower() for m in _AVOID_LINE_RE.findall(existing) if m.strip()
                ]
                for term in banned_terms:
                    if term and term in c:
                        issues.append(
                            {
                                "id": "arch-avoid-violation",
                                "severity": "veto",
                                "message": (
                                    f"Change appears to violate an explicit architecture "
                                    f"constraint: 'AVOID: {term}'."
                                ),
                            }
                        )
                        break

        elif pillar == "performance":
            perf_risks = [
                {
                    "pattern": "for .* in .*:",
                    "severity": "info",
                    "message": "Loop detected; verify complexity is acceptable.",
                    "id_suffix": "loop",
                },
                {
                    "pattern": "select * ",
                    "severity": "warning",
                    "message": "Unbounded SELECT * may cause performance issues.",
                    "id_suffix": "select-star",
                },
            ]
            for pr in perf_risks:
                if re.search(pr["pattern"], c):
                    issues.append(
                        {
                            "id": f"perf-{pr['id_suffix']}",
                            "severity": pr["severity"],
                            "message": pr["message"],
                        }
                    )

        elif pillar == "maintainability":
            if len(content) > 50_000:
                issues.append(
                    {
                        "id": "maint-too-long",
                        "severity": "warning",
                        "message": "Output exceeds 50 kB — consider splitting.",
                    }
                )
            if content.count("\n") > 2000:
                issues.append(
                    {
                        "id": "maint-too-many-lines",
                        "severity": "warning",
                        "message": "Output is very long; unit tests should cover key paths.",
                    }
                )

        elif pillar == "security":
            # P2-5: full subprocess / os.exec* family, not just .call and os.system.
            sec_risks = [
                # Credential exposure
                {
                    "pattern": "password",
                    "severity": "warning",
                    "message": "Potential credential exposure.",
                    "id_suffix": "password",
                },
                {
                    "pattern": "api_key",
                    "severity": "warning",
                    "message": "Potential API key exposure.",
                    "id_suffix": "api-key",
                },
                {
                    "pattern": "secret",
                    "severity": "warning",
                    "message": "Potential secret exposure.",
                    "id_suffix": "secret",
                },
                # subprocess family — veto all variants
                {
                    "pattern": "subprocess.call",
                    "severity": "veto",
                    "message": "subprocess.call with unsanitised input is a critical risk.",
                    "id_suffix": "subprocess-call",
                },
                {
                    "pattern": "subprocess.run",
                    "severity": "veto",
                    "message": "subprocess.run with unsanitised input is a critical risk.",
                    "id_suffix": "subprocess-run",
                },
                {
                    "pattern": "subprocess.popen",
                    "severity": "veto",
                    "message": "subprocess.Popen with unsanitised input is a critical risk.",
                    "id_suffix": "subprocess-popen",
                },
                {
                    "pattern": "subprocess.check_call",
                    "severity": "veto",
                    "message": "subprocess.check_call with unsanitised input is a critical risk.",
                    "id_suffix": "subprocess-check-call",
                },
                {
                    "pattern": "subprocess.check_output",
                    "severity": "veto",
                    "message": "subprocess.check_output with unsanitised input is a critical risk.",
                    "id_suffix": "subprocess-check-output",
                },
                # os shell execution family
                {
                    "pattern": "os.system",
                    "severity": "veto",
                    "message": "os.system with unsanitised input is a critical risk.",
                    "id_suffix": "os-system",
                },
                {
                    "pattern": "os.popen",
                    "severity": "veto",
                    "message": "os.popen with unsanitised input is a critical risk.",
                    "id_suffix": "os-popen",
                },
                {
                    "pattern": "os.execv",
                    "severity": "veto",
                    "message": "os.execv replaces the process — critical risk.",
                    "id_suffix": "os-execv",
                },
                {
                    "pattern": "os.execve",
                    "severity": "veto",
                    "message": "os.execve replaces the process — critical risk.",
                    "id_suffix": "os-execve",
                },
            ]
            for sr in sec_risks:
                if sr["pattern"] in c:
                    issues.append(
                        {
                            "id": f"sec-{sr['id_suffix']}",
                            "severity": sr["severity"],
                            "message": sr["message"],
                        }
                    )

            # Dynamic import of dangerous modules
            dynamic_import_patterns = [
                (
                    r'__import__\s*\(\s*["\'](?:subprocess|os)["\']',
                    "sec-dynamic-import-os",
                ),
                (
                    r'importlib\.import_module\s*\(\s*["\'](?:subprocess|os)["\']',
                    "sec-importlib-os",
                ),
            ]
            for pat, issue_id in dynamic_import_patterns:
                if re.search(pat, c):
                    issues.append(
                        {
                            "id": issue_id,
                            "severity": "veto",
                            "message": "Dynamic import of dangerous module (subprocess/os) detected.",
                        }
                    )

        return issues
