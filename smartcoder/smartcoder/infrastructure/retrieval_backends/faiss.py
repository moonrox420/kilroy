"""FAISS-backed vector store adapter.

This wraps langchain_community.vectorstores.FAISS to expose the same
``from_documents`` / ``save_local`` / ``load_local`` / ``similarity_search``
interface as ``PureNumPyVectorStore``, so the orchestrator can swap backends
without touching call sites.

SECURITY: ``load_local`` requires ``allow_dangerous_deserialization=True``
because FAISS pickles the embedding function into ``index.pkl``. The
``smartcoder.infrastructure.retrieval_base`` module restricts ``index_dir``
and validates ownership before this is ever called.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from langchain_core.documents import Document as LCDocument

logger = logging.getLogger("smartcoder.retrieval")


class FaissVectorStore:
    """Thin adapter over langchain_community.vectorstores.FAISS."""

    def __init__(self, _impl: Any, index_dir: Path) -> None:
        self._impl = _impl
        self.index_dir = Path(index_dir)
        self._documents: list["LCDocument"] = []

    @classmethod
    def from_documents(
        cls,
        documents: list["LCDocument"],
        embedding: Any,
        *,
        index_dir: str | Path = Path("."),
    ) -> "FaissVectorStore":
        from langchain_community.vectorstores import FAISS

        impl = FAISS.from_documents(documents=documents, embedding=embedding)
        store = cls(impl, Path(index_dir))
        store._documents = list(documents)
        return store

    def save_local(self, index_dir: str | Path) -> None:
        self.index_dir = Path(index_dir)
        self.index_dir.mkdir(parents=True, exist_ok=True)
        self._impl.save_local(str(self.index_dir))

    @classmethod
    def load_local(
        cls, index_dir: str | Path, embeddings: Any, **kwargs: Any
    ) -> "FaissVectorStore":
        from langchain_community.vectorstores import FAISS

        impl = FAISS.load_local(
            str(index_dir),
            embeddings,
            allow_dangerous_deserialization=True,
        )
        store = cls(impl, Path(index_dir))
        # Re-hydrate doc list from the FAISS docstore
        internal = getattr(getattr(impl, "docstore", None), "_dict", None)
        if isinstance(internal, dict):
            store._documents = list(internal.values())
        return store

    def similarity_search(self, query: str, k: int) -> list["LCDocument"]:
        return self._impl.similarity_search(query=query, k=k)
