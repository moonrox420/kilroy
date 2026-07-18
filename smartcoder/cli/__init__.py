"""
CLI Layer — Argument parsing and command dispatch.

Extracted from the original kilroy_smartcoder.py's CLI section. This module
owns the argument parser, the config_from_args function, and the main entry
point. It dispatches to the appropriate controller or infrastructure function
based on the subcommand.

The CLI layer knows about:
  - Argument parsing and validation
  - Command routing
  - Output formatting

The CLI layer should NOT know about:
  - Agent internals
  - Workflow orchestration details
  - Model implementation specifics
"""
