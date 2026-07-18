"""
Controllers Layer — Workflow orchestration, quality gates, and the Maestro.

SmartCoderController is the central coordinator. It receives user requests,
determines execution paths, dispatches agents, aggregates outputs, applies
quality gates, and produces final responses.

The controller knows about:
  - Agent selection
  - Task routing
  - Context injection
  - Multi-agent coordination
  - Validation checkpoints
  - Final response generation

The controller should NOT directly solve implementation tasks unless no
specialized agent exists.
"""
