"""Vector store backends for the smartcoder RAG pipeline.

Each backend exposes the same interface:
  * ``from_documents(documents, embedding, *, index_dir)`` -> store
  * ``store.save_local(index_dir)`` -> None
  * ``<Backend>.load_local(index_dir, embeddings)`` -> store
  * ``store.similarity_search(query, k)`` -> list[Document]
"""

from .numpy import PureNumPyVectorStore
from .faiss import FaissVectorStore

__all__ = ["PureNumPyVectorStore", "FaissVectorStore"]
