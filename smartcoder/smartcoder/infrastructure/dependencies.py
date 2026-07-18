"""
smartcoder/infrastructure/dependencies.py

Dependency management — non-blocking optional checks with actionable requirements.
Provides complete import-free probing and version resolution to avoid startup overhead.
"""

from __future__ import annotations

import logging
import sys
import importlib.util
import importlib.metadata

logger = logging.getLogger("smartcoder")


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

    def __init__(self) -> None:
        self.available: dict[str, bool] = {}

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

    def _check_version(self, import_name: str, pip_name: str, min_version: str) -> str | None:
        """Return an error string if the installed *import_name* is below *min_version*.

        Safely uses importlib.metadata to check the package version without importing
        the actual module, falling back to lazy module inspection if metadata is missing.
        """
        version_str: str | None = None

        # Primary check: check pip metadata directly without executing module code
        try:
            version_str = importlib.metadata.version(pip_name)
        except importlib.metadata.PackageNotFoundError:
            # Plan B: Fall back to importing only if metadata isn't resolved
            try:
                mod = sys.modules.get(import_name)
                if mod is None:
                    mod = __import__(import_name)
                version_str = getattr(mod, "__version__", None)
            except Exception:
                return None  # treated as unverifiable, assume OK

        if version_str is None:
            return None

        if not self._probe("packaging"):
            return None

        from packaging.version import Version

        try:
            if Version(str(version_str)) < Version(min_version):
                return (
                    f"{pip_name}=={version_str} is too old; need >={min_version}. "
                    f"Run: pip install --upgrade {pip_name}"
                )
        except Exception as exc:
            logger.debug("Failed to compare package version for %s (%s)", pip_name, exc)

        return None

    def _probe(self, package: str) -> bool:
        """Check if a package is available on disk without loading/executing its namespace."""
        # Short-circuit on both cached True *and* cached False to avoid
        # repeatedly probing missing packages.
        if package in self.available:
            return self.available[package]

        if package in sys.modules:
            self.available[package] = True
            return True

        try:
            spec = importlib.util.find_spec(package)
            is_available = spec is not None
        except Exception:
            is_available = False

        self.available[package] = is_available
        return is_available

    def optional(self, package: str) -> bool:
        return self._probe(package)

    def require(self, *packages: str) -> None:
        missing: list[str] = []
        outdated: list[str] = []

        # If any requested package declares a min_version, `packaging` is a
        # transitive hard requirement for checking it — surface that plainly.
        needs_version_check = any(self._packages.get(p, {}).get("min_version") for p in packages)
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
                    pip_name = meta["pip_name"] or p  # type: ignore[assignment]
                    err = self._check_version(p, pip_name, mv)
                    if err is not None:
                        outdated.append(err)

        if missing or outdated:
            lines: list[str] = []
            if missing:
                pip_names = " ".join(self._pkg(p) for p in missing)
                lines.append(f"Missing required package(s): {', '.join(missing)}")
                lines.append(f"Install with:  pip install {pip_names}")
            if outdated:
                lines.append("Outdated package(s):")
                lines.extend(f"  - {e}" for e in outdated)
            raise MissingDependencyError("\n".join(lines))
