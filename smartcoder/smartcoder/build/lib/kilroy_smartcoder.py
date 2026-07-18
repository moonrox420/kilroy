"""
kilroy_smartcoder.py — COMPATIBILITY SHIM
This file is now a re-export shim for the refactored smartcoder package.
All original functionality has been moved to:
  - smartcoder.runtime.* → Configuration, constants, project context
  - smartcoder.infrastructure.* → Dependency management, model factories
  - smartcoder.agents.* → Role briefs, CodingAssistant
  - smartcoder.controllers.* → Workflow engine, quality gates, Maestro
  - smartcoder.cli.* → Argument parsing, command dispatch
The following public names are re-exported for backward compatibility with
the Rust bridge (src-tauri/src/smartcoder_runner.rs) and any external code
that imports from this module directly.
CLI behavior is unchanged.
"""

from __future__ import annotations
import sys as _sys
from pathlib import Path as _Path

# =============================================================================
# MINIMAL PATH SHIM — Ensures `import smartcoder` works from any context
# without removing entries that other packages may depend on.
# =============================================================================
_script_path = _Path(__file__).resolve()
_script_dir = _script_path.parent  # smartcoder/
_parent_dir = _script_dir.parent  # kilroy/

# Only insert the parent dir if it's not already on sys.path, and do so at
# the front so `import smartcoder` resolves. We no longer remove previous
# entries because that can break unrelated packages whose path happens to
# end with the same directory name.
_parent = str(_parent_dir)
if _parent not in _sys.path:
    _sys.path.insert(0, _parent)

# Debug output (only when --log-level is explicitly set to DEBUG)
if any(flag in " ".join(_sys.argv).lower() for flag in ["--log-level", "debug"]):
    print(">>> kilroy_smartcoder.py PATH SHIM MINIMAL <<<", file=_sys.stderr)
    print(f"Parent dir : {_parent_dir}", file=_sys.stderr)
    print(f"sys.path[0:5]: {_sys.path[:5]}", file=_sys.stderr)

# =============================================================================
# Force smartcoder package availability
# =============================================================================
try:
    import smartcoder
except ImportError:
    # Fallback dynamic loader for extreme cases (Rust bridge / direct subdir run)
    import importlib.util

    init_path = _script_dir / "__init__.py"
    if init_path.exists():
        spec = importlib.util.spec_from_file_location("smartcoder", str(init_path))
        if spec is not None:
            _loader = spec.loader
            if _loader is not None:
                smartcoder = importlib.util.module_from_spec(spec)
                _loader.exec_module(smartcoder)
                _sys.modules["smartcoder"] = smartcoder

# =============================================================================
# Re-export everything the original module exposed
# =============================================================================
# Backend constants
from smartcoder.runtime.constants import (
    DATASET_CODING_INSTRUCTIONS,
    DEFAULT_AUTHORIZED_IMPORTS,
    DEFAULT_OLLAMA_HOST,
    DEFAULT_OLLAMA_MODEL,
    KILROY_AGENT_INSTRUCTIONS,
    PROJECT_CODING_INSTRUCTIONS,
    STUCK_CLAUSE,
    VALID_BACKENDS,
    VALID_SANDBOXES,
    _normalize_ollama_host,
)

# Configuration
from smartcoder.runtime.config import AppConfig, setup_logging

# Project context
from smartcoder.runtime.context import format_kilroy_context, load_kilroy_context

# Role briefs
from smartcoder.agents.roles import ROLE_BRIEFS, role_brief_for

# Infrastructure
from smartcoder.infrastructure.dependencies import DependencyManager
from smartcoder.infrastructure.models import build_model, build_web_search_tool

# Agent
from smartcoder.agents.coding_assistant import CodingAssistant

# Maestro Controller
from smartcoder.controllers.maestro import SmartCoderController

# Workflow & quality
from smartcoder.controllers.workflow import WorkflowEngine, WorkflowState
from smartcoder.controllers.quality import QualityGate, QualityReport, GateResult

# CLI
from smartcoder.cli.parser import build_parser, config_from_args, main

# Dataset listing
from smartcoder.cli.handlers import handle_build_index, handle_list_datasets

# =============================================================================
# Legacy aliases — keep existing imports working without changes
# =============================================================================
__all__ = [
    # Configuration
    "AppConfig",
    "setup_logging",
    # Constants
    "VALID_BACKENDS",
    "VALID_SANDBOXES",
    "DEFAULT_OLLAMA_HOST",
    "DEFAULT_OLLAMA_MODEL",
    "DEFAULT_AUTHORIZED_IMPORTS",
    "KILROY_AGENT_INSTRUCTIONS",
    "PROJECT_CODING_INSTRUCTIONS",
    "STUCK_CLAUSE",
    "DATASET_CODING_INSTRUCTIONS",
    "_normalize_ollama_host",
    # Role briefs
    "ROLE_BRIEFS",
    "role_brief_for",
    # Dependencies
    "DependencyManager",
    # Models
    "build_model",
    "build_web_search_tool",
    # Context
    "load_kilroy_context",
    "format_kilroy_context",
    # Agent
    "CodingAssistant",
    # Maestro
    "SmartCoderController",
    # Workflow
    "WorkflowEngine",
    "WorkflowState",
    "QualityGate",
    "QualityReport",
    "GateResult",
    # CLI
    "build_parser",
    "config_from_args",
    "main",
    "handle_build_index",
    "handle_list_datasets",
]

# Direct execution support (CLI entry point)
if __name__ == "__main__":
    main()
