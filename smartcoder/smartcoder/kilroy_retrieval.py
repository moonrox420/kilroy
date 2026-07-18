"""
kilroy_retrieval.py — Backward-compat shim.

The NumPy-backed retrieval engine moved to:
    smartcoder.infrastructure.retrieval

This file re-exports the public API so any existing
``from kilroy_retrieval import ...`` call sites keep working.
"""

import os as _os

# Default the legacy import to the NumPy (safer) backend.
_os.environ.setdefault("SMARTCODER_VECTOR_BACKEND", "numpy")

from smartcoder.infrastructure.retrieval import (  # noqa: E402,F401
    DATASET_REGISTRY,
    DEFAULT_DATASET_KEYS,
    DEFAULT_INDEX_DIR,
    DatasetSpec,
    RetrieverTool,
    build_embeddings,
    build_or_load_vector_store,
    build_retriever_tool,
    extract_all_sources,
    load_documents,
    main,
    resolve_specs,
    retrieve_context,
    split_documents,
)
from smartcoder.infrastructure.retrieval_backends.numpy import (  # noqa: F401
    PureNumPyVectorStore,
)

__all__ = [
    "DATASET_REGISTRY",
    "DEFAULT_DATASET_KEYS",
    "DEFAULT_INDEX_DIR",
    "DatasetSpec",
    "PureNumPyVectorStore",
    "RetrieverTool",
    "build_embeddings",
    "build_or_load_vector_store",
    "build_retriever_tool",
    "extract_all_sources",
    "load_documents",
    "resolve_specs",
    "retrieve_context",
    "split_documents",
]
