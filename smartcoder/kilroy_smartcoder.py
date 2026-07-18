"""Compatibility entry point for Kilroy's optional Python data utilities.

The desktop agent runtime is implemented in Rust. This module intentionally
exports only dataset/index commands that still depend on the Python ML stack.
"""

from smartcoder.cli.handlers import handle_build_index, handle_list_datasets
from smartcoder.cli.parser import build_parser, config_from_args, main
from smartcoder.infrastructure.dependencies import DependencyManager
from smartcoder.runtime.config import AppConfig, setup_logging

__all__ = [
    "AppConfig",
    "DependencyManager",
    "build_parser",
    "config_from_args",
    "handle_build_index",
    "handle_list_datasets",
    "main",
    "setup_logging",
]


if __name__ == "__main__":
    raise SystemExit(main())
