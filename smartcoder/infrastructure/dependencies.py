"""
Dependency management — non-blocking optional checks with actionable hard requirements.

Extracted from the original DependencyManager class in kilroy_smartcoder.py.
Infrastructure layer only. Knows nothing about agents, tasks, or workflows.

REFACTOR NOTES (see remediation PRD):
  * P2-9: `_check_version()` did `from packaging.version import Version`
    inline without `packaging` being declared or guarded anywhere in
    `_packages`. If `packaging` itself were missing, that raised a raw,
    unhandled `ImportError` instead of this module's usual friendly
    `MissingDependencyError`. `packaging` ships as a dependency of pip
    itself so this was low-probability, but it's now guarded explicitly and
    folds into the same friendly error path as everything else.
"""

from __future__ import annotations


class MissingDependencyError(RuntimeError):
    """Raised when a required Python dependency is not installed."""

    pass


class DependencyManager:
    """Non-blocking optional dependency checks with actionable hard requirements."""

    # Map of (import_name) -> {"pip_name": str, "min_version": str | None}
    # When min_version is set, require() checks the installed version and
    # includes the constraint in the error message if the version is too old.
    _packages: dict[str, dict[str, str | None]] = {
        "smolagents": {"pip_name": "smolagents", "min_version": None},
        "litellm": {"pip_name": "litellm", "min_version": None},
        "datasets": {"pip_name": "datasets", "min_version": None},
        "faiss": {"pip_name": "faiss-cpu", "min_version": None},
        "langchain_core": {"pip_name": "langchain-core", "min_version": "0.1.0"},
        "langchain_community": {
            "pip_name": "langchain-community",
            "min_version": "0.1.0",
        },
        "langchain_huggingface": {
            "pip_name": "langchain-huggingface",
            "min_version": None,
        },
        "langchain_text_splitters": {
            "pip_name": "langchain-text-splitters",
            "min_version": None,
        },
        "langchain_ollama": {"pip_name": "langchain-ollama", "min_version": None},
        "sentence_transformers": {
            "pip_name": "sentence-transformers",
            "min_version": "2.0.0",
        },
        "huggingface_hub": {"pip_name": "huggingface-hub", "min_version": None},
        "torch": {"pip_name": "torch", "min_version": None},
        "llama_cpp": {"pip_name": "llama-cpp-python", "min_version": None},
        "ollama": {"pip_name": "ollama", "min_version": None},
        "packaging": {"pip_name": "packaging", "min_version": None},
    }

    def _pkg(self, import_name: str) -> str:
        """Return the pip package name for *import_name*."""
        meta = self._packages.get(import_name)
        if meta is None:
            return import_name
        return meta["pip_name"] or import_name  # type: ignore[return-value]

    def _version_constraint(self, import_name: str) -> str:
        """Return the pip version specifier, e.g. \">=0.1.0\", or empty string."""
        meta = self._packages.get(import_name)
        if meta is None:
            return ""
        mv = meta.get("min_version")
        if mv:
            return f">={mv}"
        return ""

    def _check_version(self, import_name: str, min_version: str) -> str | None:
        """Return an error string if the installed *import_name* is below *min_version*.

        Returns `None` (treated by the caller as "can't verify, assume OK")
        rather than raising if `packaging` itself isn't installed — that
        case is instead surfaced up front by `require()` adding "packaging"
        to its own missing-package check (P2-9), so callers get one clear
        actionable error instead of a raw traceback from deep inside a
        version-comparison helper.
        """
        try:
            mod = __import__(import_name)
        except ImportError:
            return None  # handled by the caller as missing

        v = getattr(mod, "__version__", None)
        if v is None:
            return None

        if not self._probe("packaging"):
            return None

        from packaging.version import Version

        if Version(str(v)) < Version(min_version):
            return (
                f"{pip_name}=={v} is too old; need >={min_version}. "
                "Run: uv pip install -r requirements.txt"
            )
        return None

    def __init__(self) -> None:
        self.available: dict[str, bool] = {}

    def _probe(self, package: str) -> bool:
        # Probe the package. We cache only SUCCESSES so that a runtime
        # install (e.g. pip install in a long-lived session) takes effect
        # on the next check. Cache misses are re-probed each time.
        if package in self.available and self.available[package]:
            return True
        try:
            __import__(package)
            self.available[package] = True
        except ImportError:
            self.available[package] = False
        return self.available[package]

    def optional(self, package: str) -> bool:
        return self._probe(package)

    def require(self, *packages: str) -> None:
        missing: list[str] = []
        outdated: list[str] = []

        # If any requested package declares a min_version, `packaging` is a
        # transitive hard requirement for checking it — surface that plainly
        # instead of letting `_check_version` raise a raw ImportError (P2-9).
        needs_version_check = any(
            self._packages.get(p, {}).get("min_version") for p in packages
        )
        if needs_version_check and not self._probe("packaging"):
            missing.append("packaging")

        for p in packages:
            if not self._probe(p):
                missing.append(p)
                continue
            # Check min_version even when the module loaded — it may be too old.
            meta = self._packages.get(p)
            if meta is not None:
                mv = meta.get("min_version")
                if mv:
                    err = self._check_version(p, mv)
                    if err is not None:
                        outdated.append(err)

        if missing or outdated:
            lines: list[str] = []
            if missing:
                lines.append(f"Missing required package(s): {', '.join(missing)}")
                lines.append("Install with: uv pip install -r requirements.txt")
            if outdated:
                lines.append("Outdated package(s):")
                lines.extend(f"  - {e}" for e in outdated)
            raise MissingDependencyError("\n".join(lines))
