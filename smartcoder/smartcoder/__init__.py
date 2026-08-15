"""Kilroy Smart Coder backend package.

Orchestration intelligence is implemented once in :mod:`smartcoder.intelligence`
and re-exported here to preserve the package's public API.
"""

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
