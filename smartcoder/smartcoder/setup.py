"""
Thin setuptools shim for SmartCoder.

All package metadata (name, version, dependencies, entry points, module layout)
lives in pyproject.toml under [project] and [tool.setuptools], following PEP 621.
Modern setuptools (>=64) reads that configuration directly, so this file exists
only to keep `python setup.py ...` and older tooling working.

Do NOT duplicate metadata here — adding fields to setup() would shadow
pyproject.toml and create silent drift between the two sources of truth.

Normal usage does not require this file:
    python -m build            # build sdist + wheel
    pip install .              # install
    uv pip install -r requirements.txt
"""

from setuptools import setup

if __name__ == "__main__":
    setup()
