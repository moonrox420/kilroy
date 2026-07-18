"""Pure-NumPy cosine-similarity vector store (zero pickle)."""

from __future__ import annotations

import json
import logging
import os
from pathlib import Path
from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from langchain_core.documents import Document as LCDocument

logger = logging.getLogger("smartcoder.retrieval")


class PureNumPyVectorStore:
    """A safe, pickle-free vector store. Same interface as FAISS."""

    def __init__(self, embeddings_engine: Any, index_dir: Path) -> None:
        self.embeddings_engine = embeddings_engine
        self.index_dir = Path(index_dir)
        self._embeddings: Any = None
        self._documents: list["LCDocument"] = []

    @property
    def vectors_path(self) -> Path:
        return self.index_dir / "embeddings.npy"

    @property
    def metadata_path(self) -> Path:
        return self.index_dir / "documents.jsonl"

    def _ensure_numpy(self) -> Any:
        try:
            import numpy as np

            return np
        except ImportError as exc:
            raise RuntimeError("NumPy required: pip install numpy") from exc

    @classmethod
    def from_documents(
        cls,
        documents: list["LCDocument"],
        embedding: Any,
        *,
        index_dir: str | Path = Path("."),
    ) -> "PureNumPyVectorStore":
        store = cls(embeddings_engine=embedding, index_dir=Path(index_dir))
        np = store._ensure_numpy()
        texts = [doc.page_content for doc in documents]
        logger.info("Computing embeddings for %d document chunks...", len(texts))
        store._embeddings = np.array(embedding.embed_documents(texts), dtype=np.float32)
        store._documents = list(documents)
        return store

    def save_local(self, index_dir: str | Path) -> None:
        np = self._ensure_numpy()
        self.index_dir = Path(index_dir)
        self.index_dir.mkdir(parents=True, exist_ok=True)
        self.index_dir.chmod(0o700)
        np.save(str(self.vectors_path), self._embeddings)
        self.vectors_path.chmod(0o600)
        with open(self.metadata_path, "w", encoding="utf-8") as fh:
            for doc in self._documents:
                fh.write(
                    json.dumps({"page_content": doc.page_content, "metadata": doc.metadata}) + "\n"
                )
        self.metadata_path.chmod(0o600)

    @classmethod
    def load_local(
        cls, index_dir: str | Path, embeddings: Any, **kwargs: Any
    ) -> "PureNumPyVectorStore":
        store = cls(embeddings_engine=embeddings, index_dir=Path(index_dir))
        if not store.load():
            raise FileNotFoundError(f"Safe vector store files missing from index: {index_dir}")
        return store

    def load(self) -> bool:
        np = self._ensure_numpy()
        from langchain_core.documents import Document as LCDocument

        if not (self.vectors_path.exists() and self.metadata_path.exists()):
            return False
        try:
            getuid_fn = getattr(os, "getuid", None)
            if getuid_fn is not None:
                current_uid = getuid_fn()
                for path in [self.vectors_path, self.metadata_path]:
                    if path.stat().st_uid != current_uid:
                        raise PermissionError(
                            f"Security: Vector file owned by foreign user: {path}"
                        )
            self._embeddings = np.load(str(self.vectors_path))
            self._documents = []
            with open(self.metadata_path, "r", encoding="utf-8") as fh:
                for line in fh:
                    if line.strip():
                        data = json.loads(line)
                        self._documents.append(
                            LCDocument(
                                page_content=data["page_content"],
                                metadata=data["metadata"],
                            )
                        )
            return True
        except Exception as exc:
            logger.error("Failed to load numpy vector store: %s", exc)
            return False

    def similarity_search(self, query: str, k: int) -> list["LCDocument"]:
        if self._embeddings is None or not self._documents:
            return []
        np = self._ensure_numpy()
        query_vector = np.array(self.embeddings_engine.embed_query(query), dtype=np.float32)
        emb_norms = np.maximum(np.linalg.norm(self._embeddings, axis=1, keepdims=True), 1e-12)
        query_norm = max(float(np.linalg.norm(query_vector)), 1e-12)
        scores = (self._embeddings / emb_norms) @ (query_vector / query_norm)
        top_indices = np.argsort(scores)[::-1][:k]
        results = []
        for idx in top_indices:
            orig = self._documents[int(idx)]
            results.append(
                orig.__class__(
                    page_content=orig.page_content,
                    metadata={
                        **orig.metadata,
                        "similarity_score": float(scores[int(idx)]),
                    },
                )
            )
        return results
