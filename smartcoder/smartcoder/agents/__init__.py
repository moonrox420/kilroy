"""
Agents Layer — Role-specific execution agents.

Each module owns its own role definition. Agents know how to execute a specific
kind of task (planning, architecture, development, QA, review). They receive
context from the Maestro Controller and return results.

An agent should NOT know about:
  - UI state
  - Project ownership
  - Workflow orchestration
  - Which agent comes next
"""
