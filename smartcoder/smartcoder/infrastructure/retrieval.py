"""
smartcoder/infrastructure/retrieval.py

RAG engine for the SmartCoder agent.
Safe Retrieval Engine - Zero-Pickle, Pure-NumPy Cosine Similarity Vector Store.

FIX P0-8: PureNumPyVectorStore.vectors_path and metadata_path are now computed
@properties derived from self.index_dir.  Previously they were set once in
__init__ from the placeholder Path(".") passed by from_documents(), and
save_local() updated self.index_dir without updating those paths — silently
writing embeddings.npy and documents.jsonl into the current working directory
rather than the configured index_dir.
"""

from __future__ import annotations

import copy
import hashlib
import json
import logging
import os
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, ClassVar

try:
    from smolagents import Tool as _ToolBase
except ImportError:
    _ToolBase = object  # type: ignore[assignment, misc]

if TYPE_CHECKING:
    from langchain_core.documents import Document as LCDocument

logger = logging.getLogger("smartcoder.retrieval")

DEFAULT_INDEX_DIR = Path(os.environ.get("SMARTCODER_INDEX_DIR", "vector_store"))
DEFAULT_SIMILARITY_SEARCH_TIMEOUT = 0
DEFAULT_MAX_ITEMS_PER_DATASET = 5_000


# =============================================================================
# DATASET ADAPTERS
# =============================================================================


@dataclass(frozen=True)
class DatasetSpec:
    """Declarative recipe for turning one HF dataset into retrievable Documents."""

    key: str
    hf_name: str
    source_label: str
    text_builder: Callable[[dict[str, Any]], str]
    split: str = "train"
    config: str | None = None


def _clean(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _build_glaive(row: dict[str, Any]) -> str:
    question = _clean(row.get("question"))
    answer = _clean(row.get("answer"))
    if not question and not answer:
        return ""
    return f"### Question:\n{question}\n\n### Answer:\n{answer}".strip()


def _build_codealpaca(row: dict[str, Any]) -> str:
    prompt = _clean(row.get("prompt")) or _clean(row.get("instruction"))
    completion = _clean(row.get("completion")) or _clean(row.get("output"))
    if not prompt and not completion:
        return ""
    return f"### Instruction:\n{prompt}\n\n### Response:\n{completion}".strip()


DATASET_REGISTRY: dict[str, DatasetSpec] = {
    "glaive": DatasetSpec(
        key="glaive",
        hf_name="glaiveai/glaive-code-assistant",
        source_label="glaive-code-assistant",
        text_builder=_build_glaive,
        split="train",
    ),
    "codealpaca": DatasetSpec(
        key="codealpaca",
        hf_name="HuggingFaceH4/CodeAlpaca_20K",
        source_label="codealpaca-20k",
        text_builder=_build_codealpaca,
        split="train",
    ),
}

DEFAULT_DATASET_KEYS: tuple[str, ...] = ("glaive", "codealpaca")


def resolve_specs(keys: list[str] | tuple[str, ...] | None) -> list[DatasetSpec]:
    selected = tuple(keys) if keys else DEFAULT_DATASET_KEYS
    unknown = [k for k in selected if k not in DATASET_REGISTRY]
    if unknown:
        raise KeyError(f"Unknown dataset key(s): {unknown}. Available: {sorted(DATASET_REGISTRY)}")
    return [DATASET_REGISTRY[k] for k in selected]


# =============================================================================
# DOCUMENT LOADING
# =============================================================================


def load_documents(
    specs: list[DatasetSpec],
    max_items_per_dataset: int | None = DEFAULT_MAX_ITEMS_PER_DATASET,
    streaming: bool = True,
) -> list[LCDocument]:
    from datasets import load_dataset
    from langchain_core.documents import Document as LCDocument

    documents: list[LCDocument] = []
    failures: list[str] = []

    for spec in specs:
        logger.info(
            "Loading dataset '%s' (split=%s, streaming=%s, cap=%s)",
            spec.hf_name,
            spec.split,
            streaming,
            max_items_per_dataset,
        )
        try:
            dataset = load_dataset(
                spec.hf_name,
                name=spec.config,
                split=spec.split,
                streaming=streaming,
            )
        except Exception as exc:  # noqa: BLE001 - dataset adapters are third-party code.
            msg = f"{spec.hf_name}: {exc}"
            logger.error("Failed to load %s", msg)
            failures.append(msg)
            continue

        kept = 0
        for index, row in enumerate(dataset):
            if max_items_per_dataset is not None and kept >= max_items_per_dataset:
                break
            if not isinstance(row, dict):
                continue
            content = spec.text_builder(row)
            if not content:
                continue
            documents.append(
                LCDocument(
                    page_content=content,
                    metadata={
                        "source": spec.source_label,
                        "dataset": spec.hf_name,
                        "row": index,
                    },
                )
            )
            kept += 1

        logger.info("  -> %d documents from %s", kept, spec.source_label)

    if not documents:
        raise RuntimeError(
            f"No documents were loaded from any dataset. Failures: {failures or 'none reported'}"
        )
    if failures:
        logger.warning("Some datasets failed and were skipped: %s", failures)

    logger.info("Loaded %d source documents total", len(documents))
    return documents


def split_documents(
    documents: list[LCDocument],
    chunk_size: int = 1_200,
    chunk_overlap: int = 150,
) -> list[LCDocument]:
    from langchain_text_splitters import Language, RecursiveCharacterTextSplitter

    splitter = RecursiveCharacterTextSplitter.from_language(
        language=Language.PYTHON,
        chunk_size=chunk_size,
        chunk_overlap=chunk_overlap,
    )
    chunks = splitter.split_documents(documents)
    logger.info("Split %d documents into %d chunks", len(documents), len(chunks))
    return chunks


# =============================================================================
# EMBEDDINGS
# =============================================================================


def _auto_device(explicit: str | None) -> str:
    if explicit:
        return explicit
    try:
        import torch

        if torch.cuda.is_available():
            return "cuda"
        if getattr(torch.backends, "mps", None) is not None and torch.backends.mps.is_available():
            return "mps"
    except Exception as exc:  # noqa: BLE001 - device probes must safely fall back to CPU.
        logger.debug("Torch device probe failed (%s); defaulting to CPU", exc)
    return "cpu"


def build_embeddings(
    model_name: str = "BAAI/bge-small-en-v1.5",
    device: str | None = None,
    normalize: bool = True,
):
    from langchain_huggingface import HuggingFaceEmbeddings

    resolved_device = _auto_device(device)
    logger.info("Embedding model '%s' on device '%s'", model_name, resolved_device)
    return HuggingFaceEmbeddings(
        model_name=model_name,
        model_kwargs={"device": resolved_device},
        encode_kwargs={"normalize_embeddings": normalize, "batch_size": 32},
    )


# =============================================================================
# ZERO-PICKLE PURE-NUMPY VECTOR STORE
# =============================================================================


class PureNumPyVectorStore:
    """Safe, high-performance, pickle-free vector store.

    P0-8 FIX: vectors_path and metadata_path are now @properties computed
    from self.index_dir on every access. Previously they were set once in
    __init__ from the placeholder Path(".") passed by from_documents(), and
    save_local() updated self.index_dir without refreshing those paths —
    causing all writes to land in the working directory regardless of the
    configured index_dir.
    """

    def __init__(self, embeddings_engine: Any, index_dir: Path) -> None:
        self.embeddings_engine = embeddings_engine
        self.index_dir = Path(index_dir)
        self._embeddings: Any = None  # numpy.ndarray
        self._documents: list[Any] = []

    # ------------------------------------------------------------------
    # P0-8: paths are derived from self.index_dir, not cached at init time
    # ------------------------------------------------------------------

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
            raise RuntimeError(
                "Numpy is required for vector operations. Run: pip install numpy"
            ) from exc

    @classmethod
    def from_documents(cls, documents: list[Any], embedding: Any) -> PureNumPyVectorStore:
        """Compute embeddings and return an in-memory store.

        index_dir is intentionally left as a placeholder here; the caller
        must call save_local(real_path) before the store is persisted.
        Because vectors_path / metadata_path are now @properties, they will
        resolve to the correct location after save_local() updates index_dir.
        """
        store = cls(embeddings_engine=embedding, index_dir=Path("."))
        np = store._ensure_numpy()

        texts = [doc.page_content for doc in documents]
        logger.info("Computing embeddings for %d document chunks...", len(texts))
        embeddings_list = embedding.embed_documents(texts)
        store._embeddings = np.array(embeddings_list, dtype=np.float32)
        store._documents = list(documents)
        return store

    def save_local(self, index_dir: str | Path) -> None:
        """Persist embeddings and metadata to disk.

        Updating self.index_dir here is now sufficient to redirect both
        vectors_path and metadata_path because they are @properties.
        """
        np = self._ensure_numpy()
        self.index_dir = Path(index_dir)
        self.index_dir.mkdir(parents=True, exist_ok=True)
        self.index_dir.chmod(0o700)

        np.save(str(self.vectors_path), self._embeddings)
        self.vectors_path.chmod(0o600)

        with open(self.metadata_path, "w", encoding="utf-8") as fh:
            fh.writelines(
                json.dumps({"page_content": doc.page_content, "metadata": doc.metadata}) + "\n"
                for doc in self._documents
            )
        self.metadata_path.chmod(0o600)

    @classmethod
    def load_local(
        cls, index_dir: str | Path, embeddings: Any, **kwargs: Any
    ) -> PureNumPyVectorStore:
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
        except Exception as exc:  # noqa: BLE001 - a corrupt cache must be rebuildable.
            logger.error("Failed to load numpy vector store: %s", exc)
            return False

    def similarity_search(self, query: str, k: int) -> list[Any]:
        if self._embeddings is None or not self._documents:
            return []

        np = self._ensure_numpy()

        query_vector_raw = self.embeddings_engine.embed_query(query)
        query_vector = np.array(query_vector_raw, dtype=np.float32)

        emb_norms = np.linalg.norm(self._embeddings, axis=1, keepdims=True)
        query_norm = np.linalg.norm(query_vector)

        emb_norms = np.maximum(emb_norms, 1e-12)
        query_norm = max(query_norm, 1e-12)

        norm_embeddings = self._embeddings / emb_norms
        norm_query = query_vector / query_norm

        scores = np.dot(norm_embeddings, norm_query)
        top_indices = np.argsort(scores)[::-1][:k]

        results = []
        for idx in top_indices:
            orig_doc = self._documents[idx]
            doc_copy = orig_doc.__class__(
                page_content=orig_doc.page_content,
                metadata={**orig_doc.metadata, "similarity_score": float(scores[idx])},
            )
            results.append(doc_copy)

        return results


# =============================================================================
# VECTOR STORE COORDINATION
# =============================================================================


@dataclass(frozen=True)
class IndexBuildParams:
    dataset_keys: tuple[str, ...]
    splits: tuple[str, ...]
    max_items_per_dataset: int | None
    embedding_model: str
    normalize: bool
    chunk_size: int
    chunk_overlap: int

    def signature(self) -> str:
        payload = json.dumps(self.__dict__, sort_keys=True, default=str)
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def _meta_path(index_dir: Path) -> Path:
    return index_dir / "build_meta.json"


def _faiss_present(index_dir: Path) -> bool:
    return (index_dir / "embeddings.npy").exists() and (index_dir / "documents.jsonl").exists()


def build_or_load_vector_store(
    *,
    dataset_keys: list[str] | tuple[str, ...] | None = None,
    embedding_model: str = "BAAI/bge-small-en-v1.5",
    index_dir: str | os.PathLike[str] = DEFAULT_INDEX_DIR,
    device: str | None = None,
    normalize: bool = True,
    max_items_per_dataset: int | None = DEFAULT_MAX_ITEMS_PER_DATASET,
    chunk_size: int = 1_200,
    chunk_overlap: int = 150,
    force_rebuild: bool = False,
) -> PureNumPyVectorStore:
    index_dir = Path(index_dir)
    cwd = Path.cwd().resolve()

    resolved = index_dir.resolve()
    try:
        resolved.relative_to(cwd)
    except ValueError:
        raise ValueError(f"index_dir must reside within active working tree ({cwd}): {index_dir}")

    index_dir.mkdir(parents=True, exist_ok=True)
    try:
        index_dir.chmod(0o700)
    except OSError as exc:
        logger.warning("Could not enforce index_dir directory permissions (%s)", exc)

    specs = resolve_specs(dataset_keys)
    params = IndexBuildParams(
        dataset_keys=tuple(s.key for s in specs),
        splits=tuple(s.split for s in specs),
        max_items_per_dataset=max_items_per_dataset,
        embedding_model=embedding_model,
        normalize=normalize,
        chunk_size=chunk_size,
        chunk_overlap=chunk_overlap,
    )
    signature = params.signature()
    embeddings = build_embeddings(embedding_model, device=device, normalize=normalize)

    if not force_rebuild and _faiss_present(index_dir):
        try:
            saved_meta = json.loads(_meta_path(index_dir).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            saved_meta = {}
        if saved_meta.get("signature") == signature:
            logger.info("Restoring safe PureNumPy vector store from: %s", index_dir)
            return PureNumPyVectorStore.load_local(index_dir, embeddings)
        logger.info("Metadata signature drift detected. Recalculating vector buffers...")

    logger.info("Initializing Safe RAG Vector compilation...")
    documents = load_documents(specs, max_items_per_dataset=max_items_per_dataset)
    chunks = split_documents(documents, chunk_size=chunk_size, chunk_overlap=chunk_overlap)

    vector_store = PureNumPyVectorStore.from_documents(documents=chunks, embedding=embeddings)
    vector_store.save_local(index_dir)

    _meta_path(index_dir).write_text(
        json.dumps(
            {
                "signature": signature,
                "params": params.__dict__,
                "num_chunks": len(chunks),
                "num_documents": len(documents),
            },
            indent=2,
            default=str,
        ),
        encoding="utf-8",
    )
    logger.info("Compiled %d float32 vector chunks securely at %s", len(chunks), index_dir)
    return vector_store


def _similarity_search_with_timeout(
    vectordb: Any,
    query: str,
    k: int,
    timeout: float | None = None,
) -> list[Any]:
    """Execute similarity search. timeout parameter is accepted for API
    compatibility but PureNumPy is always synchronous (<20ms)."""
    if timeout:
        logger.debug(
            "timeout=%s passed to PureNumPy search — parameter is ignored "
            "(synchronous execution has no need for it)",
            timeout,
        )
    return vectordb.similarity_search(query=query, k=k)


def extract_all_sources(vector_store: Any) -> list[str]:
    sources: set[str] = set()
    if hasattr(vector_store, "_documents"):
        for doc in vector_store._documents:
            sources.add(str(doc.metadata.get("source", "unknown")))
    else:
        docstore = getattr(vector_store, "docstore", None)
        internal = getattr(docstore, "_dict", None)
        if isinstance(internal, dict):
            for doc in internal.values():
                metadata = getattr(doc, "metadata", {}) or {}
                sources.add(str(metadata.get("source", "unknown")))
    return sorted(sources)


# =============================================================================
# RETRIEVER TOOL (smolagents)
# =============================================================================


class RetrieverTool(_ToolBase):  # type: ignore[invalid-base]
    """Semantic search over the vector store, exposed to the CodeAgent as a tool."""

    name = "retriever"
    description = (
        "Retrieves real code examples and explanations from the indexed Hugging "
        "Face datasets using semantic similarity search. Use this to ground answers "
        "in concrete, working examples before writing your own solution."
    )

    inputs: ClassVar[dict[str, dict[str, Any]]] = {
        "query": {
            "type": "string",
            "description": "Semantic query describing the code/concept you need examples of.",
        },
        "source": {
            "type": "string",
            "description": "Optional source filter (single source or JSON list of sources).",
            "nullable": True,
        },
        "number_of_documents": {
            "type": "integer",
            "description": "How many documents to return (1-10 recommended).",
            "nullable": True,
        },
    }

    output_type = "string"

    def __init__(
        self,
        vectordb: Any,
        all_sources: list[str] | None = None,
        default_k: int = 5,
        similarity_search_timeout: float | None = DEFAULT_SIMILARITY_SEARCH_TIMEOUT,
        **kwargs: Any,
    ) -> None:
        super().__init__(**kwargs)
        object.__setattr__(self, "inputs", copy.deepcopy(type(self).inputs))
        self.vectordb = vectordb
        self.all_sources = all_sources if all_sources is not None else extract_all_sources(vectordb)
        self.default_k = default_k
        self.similarity_search_timeout = similarity_search_timeout
        self.inputs["source"]["description"] = (
            "Optional source filter (single source or JSON list). "
            f"Available sources: {self.all_sources}"
        )

    @staticmethod
    def _normalize_sources(source: str | None) -> list[str] | None:
        if not source:
            return None
        stripped = str(source).strip()
        if not stripped:
            return None
        if stripped.startswith("["):
            for candidate in (stripped, stripped.replace("'", '"')):
                try:
                    parsed = json.loads(candidate)
                    if isinstance(parsed, list):
                        cleaned = [str(x).strip() for x in parsed if str(x).strip()]
                        return cleaned or None
                except json.JSONDecodeError:
                    continue
        return [stripped]

    def forward(
        self,
        query: str,
        source: str | None = None,
        number_of_documents: int | None = None,
    ) -> str:
        if not isinstance(query, str) or not query.strip():
            raise ValueError("`query` must be a non-empty string.")

        try:
            k = int(number_of_documents) if number_of_documents is not None else self.default_k
        except (TypeError, ValueError) as exc:
            raise ValueError("`number_of_documents` must be an integer.") from exc
        if k <= 0:
            raise ValueError("`number_of_documents` must be greater than 0.")
        k = min(k, 25)

        source_list = self._normalize_sources(source)
        fetch_k = min(k * 5, 100) if source_list else k

        docs = _similarity_search_with_timeout(
            vectordb=self.vectordb,
            query=query,
            k=fetch_k,
            timeout=self.similarity_search_timeout,
        )

        if source_list:
            wanted = set(source_list)
            docs = [d for d in docs if str(d.metadata.get("source")) in wanted]
        docs = docs[:k]

        if not docs:
            hint = f" (filtered to sources {source_list})" if source_list else ""
            return (
                f"No documents found for query{hint}. "
                "Try a broader query or remove the source filter."
            )

        formatted: list[str] = []
        for index, doc in enumerate(docs, start=1):
            src = doc.metadata.get("source", "unknown")
            formatted.append(f"Document {index} | source: {src}\n\n{doc.page_content}")
        return "\n\n===DOCUMENT===\n\n".join(formatted)


def build_retriever_tool(
    *,
    dataset_keys: list[str] | tuple[str, ...] | None = None,
    embedding_model: str = "BAAI/bge-small-en-v1.5",
    index_dir: str | os.PathLike[str] = DEFAULT_INDEX_DIR,
    device: str | None = None,
    max_items_per_dataset: int | None = DEFAULT_MAX_ITEMS_PER_DATASET,
    force_rebuild: bool = False,
    default_k: int = 5,
) -> RetrieverTool:
    vector_store = build_or_load_vector_store(
        dataset_keys=dataset_keys,
        embedding_model=embedding_model,
        index_dir=index_dir,
        device=device,
        max_items_per_dataset=max_items_per_dataset,
        force_rebuild=force_rebuild,
    )
    return RetrieverTool(
        vector_store,
        all_sources=extract_all_sources(vector_store),
        default_k=default_k,
    )


def retrieve_context(
    query: str,
    top_k: int = 3,
    dataset_keys: list[str] | tuple[str, ...] | None = None,
    index_dir: str | os.PathLike[str] = DEFAULT_INDEX_DIR,
) -> str:
    """Initialize the safe index and return project-relevant context."""
    tool = build_retriever_tool(
        dataset_keys=dataset_keys,
        index_dir=index_dir,
        default_k=top_k,
    )
    return tool.forward(query=query, number_of_documents=top_k)


def _configure_standalone_logging() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s | %(levelname)-8s | %(name)s | %(message)s",
        datefmt="%H:%M:%S",
    )


def main() -> None:
    import argparse

    _configure_standalone_logging()
    parser = argparse.ArgumentParser(description="Build/refresh the SmartCoder safe RAG index.")
    parser.add_argument("--datasets", nargs="+", default=list(DEFAULT_DATASET_KEYS))
    parser.add_argument("--embedding-model", default="BAAI/bge-small-en-v1.5")
    parser.add_argument("--index-dir", default=str(DEFAULT_INDEX_DIR))
    parser.add_argument("--max-items", type=int, default=DEFAULT_MAX_ITEMS_PER_DATASET)
    parser.add_argument("--force-rebuild", action="store_true")
    parser.add_argument("--query", default="Write a function to reverse a linked list in Python.")
    args = parser.parse_args()

    max_items = None if args.max_items is not None and args.max_items < 0 else args.max_items
    tool = build_retriever_tool(
        dataset_keys=args.datasets,
        embedding_model=args.embedding_model,
        index_dir=args.index_dir,
        max_items_per_dataset=max_items,
        force_rebuild=args.force_rebuild,
    )
    print("=" * 80)
    print(f"Safe index ready. Sources: {tool.all_sources}")
    print(f"Smoke-test query: {args.query!r}")
    print("=" * 80)
    print(tool.forward(args.query, number_of_documents=3))


if __name__ == "__main__":
    main()
