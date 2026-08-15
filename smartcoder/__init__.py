"""Kilroy Smart Coder compatibility package.

The production implementation lives in ``smartcoder/smartcoder`` so it can be
built as the ``kilroy-smartcoder`` distribution.  A repository checkout is
also executable directly with ``python -m smartcoder.kilroy_smartcoder``.
Expose the production package directory first in ``__path__`` so both launch
methods resolve the exact same modules instead of the historical, divergent
compatibility copies beside this file.
"""

from pathlib import Path

_CANONICAL_PACKAGE_DIR = Path(__file__).resolve().parent / "smartcoder"
if _CANONICAL_PACKAGE_DIR.is_dir():
    __path__.insert(0, str(_CANONICAL_PACKAGE_DIR))

from .intelligence import (
    AgentConflict,
    AgentConsensus,
    AgentOpinion,
    AgentParticipation,
    AgentResult,
    DecisionRegistry,
    ExecutionMemory,
    ExecutionTelemetry,
    ExecutionTimeline,
    FailureAnalysis,
    FailureAnalysisMode,
    KilroyPersona,
    LockedDecision,
    LockedDecisionError,
    ReviewDimension,
    ReviewReport,
    TechnicalReviewBoard,
)

__version__ = "0.2.0"

__all__ = [
    "AgentConflict",
    "AgentConsensus",
    "AgentOpinion",
    "AgentParticipation",
    "AgentResult",
    "DecisionRegistry",
    "ExecutionMemory",
    "ExecutionTelemetry",
    "ExecutionTimeline",
    "FailureAnalysis",
    "FailureAnalysisMode",
    "KilroyPersona",
    "LockedDecision",
    "LockedDecisionError",
    "ReviewDimension",
    "ReviewReport",
    "TechnicalReviewBoard",
    "__version__",
]
