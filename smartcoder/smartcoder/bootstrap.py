"""
Kilroy SmartCoder bootstrap & runtime gate.

This module validates the local project virtualenv and ensures the runtime can
import the SmartCoder package from the local source tree using the current
requirements-based workflow. It avoids the old editable-install assumptions and
checks the actual environment that the app launches against.
"""

import sys
import pathlib
import importlib.util
import importlib.metadata
import logging

# Configure strict, structured logging to standard output
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    handlers=[logging.StreamHandler(sys.stdout)],
)
logger = logging.getLogger("kilroy.bootstrap")

# --- Architectural Guardrails ---
EXPECTED_VENV_NAME = ".venv"
TARGET_PROJECT_DIR = "kilroy"
FORBIDDEN_PROJECT_DIR = "ollama-python"
REQUIREMENTS_FILE = "requirements.txt"
PROJECT_ROOT = pathlib.Path(__file__).resolve().parent.parent

if str(PROJECT_ROOT) not in sys.path:
    sys.path.insert(0, str(PROJECT_ROOT))

# Explicit downstream sub-dependency conflict definitions
CRITICAL_DEPENDENCY_CONSTRAINTS = {
    "pydantic": {"min": "2.13.0"},
    "httpx": {"min": "0.28.0", "max": "0.29.0"},
}


class EnvironmentValidationError(Exception):
    """Raised when the runtime environment violates architectural invariants."""

    pass


def resolve_environment_geometry() -> pathlib.Path:
    prefix = pathlib.Path(sys.prefix).resolve()
    base_prefix = pathlib.Path(sys.base_prefix).resolve()

    if prefix == base_prefix:
        raise EnvironmentValidationError(
            f"Execution rejected: Running on global Python interpreter or unsandboxed environment: {prefix}"
        )

    cfg_path = prefix / "pyvenv.cfg"
    if not cfg_path.exists():
        cfg_path = prefix.parent / "pyvenv.cfg"
        if not cfg_path.exists():
            raise EnvironmentValidationError(
                f"Virtual environment corruption: 'pyvenv.cfg' missing from prefix: {prefix}"
            )

    logger.info(f"Environment geometry verified. Sandbox Prefix: {prefix}")
    return prefix


def enforce_boundary_isolation(venv_root: pathlib.Path) -> None:
    normalized_path = venv_root.as_posix().lower()

    if FORBIDDEN_PROJECT_DIR in normalized_path:
        raise EnvironmentValidationError(
            f"CROSS-CONTAMINATION DETECTED: Runtime executing out of forbidden boundary space: '{venv_root}'. "
            f"You are inside the '{FORBIDDEN_PROJECT_DIR}' context. Halting execution."
        )

    if TARGET_PROJECT_DIR not in normalized_path:
        logger.warning(
            f"BOUNDARY WARN: Sandbox path '{venv_root}' does not explicitly "
            f"contain target project marker '{TARGET_PROJECT_DIR}'."
        )


def verify_local_source_runtime() -> str:
    try:
        import smartcoder.kilroy_retrieval  # noqa: F401
    except Exception as exc:
        raise EnvironmentValidationError(f"SmartCoder local runtime imports failed: {exc}") from exc

    spec = importlib.util.find_spec("smartcoder")
    origin_path = spec.origin if spec else None
    if not origin_path:
        raise EnvironmentValidationError(
            "SmartCoder package could not be located in the active environment."
        )

    logger.info(f"Verified local runtime import path: {origin_path}")
    return str(origin_path)


def verify_dependency_graph_coherence() -> None:
    logger.info("Executing deep sub-dependency constraint audit...")

    for package, constraints in CRITICAL_DEPENDENCY_CONSTRAINTS.items():
        try:
            version_str = importlib.metadata.version(package)
            from packaging import version

            current_version = version.parse(version_str)

            if "min" in constraints:
                if current_version < version.parse(constraints["min"]):
                    raise EnvironmentValidationError(
                        f"Incompatible dependency graph: {package}=={version_str} is below requested minimum version {constraints['min']}"
                    )
            if "max" in constraints:
                if current_version >= version.parse(constraints["max"]):
                    raise EnvironmentValidationError(
                        f"Incompatible dependency graph: {package}=={version_str} breaches requested maximum version limit {constraints['max']}"
                    )

            logger.info(f"Coherence check passed: {package}=={version_str} satisfies constraints.")
        except importlib.metadata.PackageNotFoundError:
            raise EnvironmentValidationError(
                f"Missing critical sub-dependency chain node: '{package}' must be present in runtime path."
            )


def execute_behavioral_smoke_tests() -> None:
    logger.info("Executing behavioral smoke tests...")

    try:
        from pydantic import BaseModel, Field

        class RuntimeTestModel(BaseModel):
            agent_id: str = Field(..., min_length=3)
            execution_loop_safe: bool = True

        validated = RuntimeTestModel(agent_id="kilroy_sys_test")
        assert validated.execution_loop_safe is True
        logger.info("Behavior test passed: Pydantic engine execution is nominal.")
    except Exception as e:
        raise EnvironmentValidationError(
            f"Behavioral execution crash on Pydantic sub-layer: {str(e)}"
        )

    try:
        import httpx

        with httpx.Client() as client:
            _ = client.build_request("GET", "http://localhost:11434")
        logger.info("Behavior test passed: HTTPx engine initialization is nominal.")
    except Exception as e:
        raise EnvironmentValidationError(
            f"Behavioral execution crash on HTTPx / Async linkage layer: {str(e)}"
        )


def main() -> None:
    logger.info("=== Initializing Kilroy Production Boot Gate ===")
    try:
        venv_root = resolve_environment_geometry()
        enforce_boundary_isolation(venv_root)
        verify_local_source_runtime()
        verify_dependency_graph_coherence()
        execute_behavioral_smoke_tests()
        logger.info("=== SUCCESS: Runtime graph matches enterprise-grade integrity constraints ===")
        print("-" * 80)

    except EnvironmentValidationError as error:
        logger.critical(f"FATAL REJECTION: Environment layout corrupted: {str(error)}")
        print("-" * 80)
        sys.exit(1)
    except Exception as unexpected:
        logger.critical(f"UNHANDLED BOOTSTRAP EXCEPTION: {str(unexpected)}", exc_info=True)
        print("-" * 80)
        sys.exit(1)


if __name__ == "__main__":
    main()
