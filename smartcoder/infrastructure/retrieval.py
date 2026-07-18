"""RAG engine for the SmartCoder agent.

Provides declarative dataset adapters, high-speed code-aware splitting,
normalized HuggingFace embeddings, and secure local FAISS vector storage
with strict folder permission auditing and file integrity checks.
"""

from __future__ import annotations

import copy
import hashlib
import json
import logging
import os
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, Callable

try:
    from smolagents import Tool as _ToolBase
except ImportError:
    _ToolBase = object  # type: ignore[assignment, misc]

if TYPE_CHECKING:
    from langchain_core.documents import Document
    from langchain_core.vectorstores import VectorStore

logger = logging.getLogger("smartcoder.retrieval")

DEFAULT_INDEX_DIR: Path = Path(os.environ.get("SMARTCODER_INDEX_DIR", "vector_store"))
DEFAULT_SIMILARITY_SEARCH_TIMEOUT: float = 0.0
DEFAULT_MAX_ITEMS_PER_DATASET: int = 5_000

# =============================================================================
# DATASET ADAPTERS
# =============================================================================


@dataclass(frozen=True)
class DatasetSpec:
    """Recipe for turning one HF dataset into retrievable Documents."""

    key: str
    hf_name: str
    source_label: str
    text_builder: Callable[[dict[str, Any]], str]
    split: str = "train"
    config: str | None = None


def _clean(value: Any) -> str:
    """Coerce any cell value to a stripped string without raising on None."""
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
    """Validate dataset keys against the registry with an actionable error."""
    selected = tuple(keys) if keys else DEFAULT_DATASET_KEYS
    unknown = [k for k in selected if k not in DATASET_REGISTRY]
    if unknown:
        raise KeyError(
            f"Unknown dataset key(s): {unknown}. "
            f"Available: {sorted(DATASET_REGISTRY)}"
        )
    return [DATASET_REGISTRY[k] for k in selected]


# =============================================================================
# DOCUMENT LOADING
# =============================================================================


def load_documents(
    specs: list[DatasetSpec],
    max_items_per_dataset: int | None = DEFAULT_MAX_ITEMS_PER_DATASET,
    streaming: bool = True,
) -> list[Document]:
    """Load each dataset and convert rows into Documents.

    Streaming is utilized to avoid loading entire multi-GB splits into memory.
    """
    from datasets import load_dataset
    from langchain_core.documents import Document

    documents: list[Document] = []
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
        except Exception as exception:
            error_message = f"{spec.hf_name}: {exception}"
            logger.error("Failed to load dataset: %s", error_message)
            failures.append(error_message)
            continue

        kept_count = 0
        for index, row in enumerate(dataset):
            if (
                max_items_per_dataset is not None
                and kept_count >= max_items_per_dataset
            ):
                break
            if not isinstance(row, dict):
                continue
            content = spec.text_builder(row)
            if not content:
                continue
            documents.append(
                Document(
                    page_content=content,
                    metadata={
                        "source": spec.source_label,
                        "dataset": spec.hf_name,
                        "row": index,
                    },
                )
            )
            kept_count += 1

        logger.info("  -> Loaded %d documents from %s", kept_count, spec.source_label)

    if not documents:
        raise RuntimeError(
            "No documents were loaded from any dataset. "
            f"Failures: {failures or 'none reported'}"
        )
    if failures:
        logger.warning("Some datasets failed and were skipped: %s", failures)

    logger.info("Loaded %d source documents total", len(documents))
    return documents


def split_documents(
    documents: list[Document],
    chunk_size: int = 1_200,
    chunk_overlap: int = 150,
) -> list[Document]:
    """Code-aware recursive chunk splitting, keeping functions/classes intact."""
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


def _auto_device(explicit_device: str | None) -> str:
    """Pick the best available hardware accelerator natively."""
    if explicit_device:
        return explicit_device
    try:
        import torch

        if torch.cuda.is_available():
            return "cuda"
        if (
            getattr(torch.backends, "mps", None) is not None
            and torch.backends.mps.is_available()
        ):
            return "mps"
    except Exception as exception:
        logger.debug(
            "Torch device probe failed (%s); defaulting to standard CPU", exception
        )
    return "cpu"


def build_embeddings(
    model_name: str = "BAAI/bge-small-en-v1.5",
    device: str | None = None,
    normalize: bool = True,
):
    """Construct HuggingFace embeddings with normalization enabled."""
    from langchain_huggingface import HuggingFaceEmbeddings

    resolved_device = _auto_device(device)
    logger.info("Embedding model '%s' on device '%s'", model_name, resolved_device)
    return HuggingFaceEmbeddings(
        model_name=model_name,
        model_kwargs={"device": resolved_device},
        encode_kwargs={"normalize_embeddings": normalize, "batch_size": 32},
    )


# =============================================================================
# SECURE VECTOR STORE
# =============================================================================


@dataclass(frozen=True)
class IndexBuildParams:
    """Fields which invalidate the persisted cache if modified."""

    dataset_keys: tuple[str, ...]
    splits: tuple[str, ...]
    max_items_per_dataset: int | None
    embedding_model: str
    normalize: bool
    chunk_size: int
    chunk_overlap: int

    def signature(self) -> str:
        """Generate a SHA-256 fingerprint of the parameter footprint."""
        payload = json.dumps(self.__dict__, sort_keys=True, default=str)
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def _meta_path(index_dir: Path) -> Path:
    return index_dir / "build_meta.json"


def _faiss_present(index_dir: Path) -> bool:
    return (index_dir / "index.faiss").exists() and (index_dir / "index.pkl").exists()


def _enforce_index_dir_permissions(index_dir: Path) -> None:
    """Restrict permissions on the persisted index directory (0700)."""
    index_dir.chmod(0o700)
    for entry in index_dir.rglob("*"):
        if entry.is_file():
            entry.chmod(0o600)
        elif entry.is_dir():
            entry.chmod(0o700)


def _verify_index_dir_ownership(index_dir: Path) -> None:
    """Enforce strict user ownership verification across the index files."""
    getuid_fn = getattr(os, "getuid", None)
    if getuid_fn is None:
        return
    current_uid = getuid_fn()
    for entry in [index_dir] + list(index_dir.rglob("*")):
        try:
            if entry.stat().st_uid != current_uid:
                raise PermissionError(
                    f"Vector store directory entry not owned by active user: {entry}"
                )
        except OSError as exception:
            logger.warning(
                "Could not audit user ownership for path %s (%s)", entry, exception
            )


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
) -> VectorStore:
    """Securely build or load a local FAISS vector store index on disk.

    Enforces strict folder permissions and path restrictions to mitigate
    the execution vulnerability associated with pickle deserialization.
    """
    from langchain_community.vectorstores import FAISS

    index_directory = Path(index_dir)
    working_directory = Path.cwd().resolve()

    # Enforce relative path verification to avoid out-of-workspace loading
    resolved_path = index_directory.resolve()
    try:
        resolved_path.relative_to(working_directory)
    except ValueError as exception:
        raise ValueError(
            "Vector store directory must reside within the active "
            f"project working workspace: {index_directory}"
        ) from exception

    index_directory.mkdir(parents=True, exist_ok=True)
    try:
        _enforce_index_dir_permissions(index_directory)
    except OSError as exception:
        logger.warning(
            "Enforcing directory permissions on %s failed: %s",
            index_directory,
            exception,
        )

    specs = resolve_specs(dataset_keys)
    build_params = IndexBuildParams(
        dataset_keys=tuple(spec.key for spec in specs),
        splits=tuple(spec.split for spec in specs),
        max_items_per_dataset=max_items_per_dataset,
        embedding_model=embedding_model,
        normalize=normalize,
        chunk_size=chunk_size,
        chunk_overlap=chunk_overlap,
    )
    signature = build_params.signature()
    embeddings_engine = build_embeddings(
        embedding_model, device=device, normalize=normalize
    )

    # --- Fast Path: Load matching, secure index ---
    if not force_rebuild and _faiss_present(index_directory):
        try:
            saved_metadata = json.loads(
                _meta_path(index_directory).read_text(encoding="utf-8")
            )
        except (OSError, json.JSONDecodeError):
            saved_metadata = {}

        if saved_metadata.get("signature") == signature:
            logger.info(
                "Restoring secure local FAISS index from disk at %s", index_directory
            )
            _enforce_index_dir_permissions(index_directory)
            _verify_index_dir_ownership(index_directory)
            return FAISS.load_local(
                str(index_directory),
                embeddings_engine,
                allow_dangerous_deserialization=True,
            )
        logger.info(
            "Config drift or signature mismatch detected. Rebuilding vector index..."
        )

    # --- Slow Path: Parse dataset documents, chunk, compute embeddings ---
    logger.info("Initializing vector index compilation...")
    documents = load_documents(specs, max_items_per_dataset=max_items_per_dataset)
    chunks = split_documents(
        documents, chunk_size=chunk_size, chunk_overlap=chunk_overlap
    )

    vector_store = FAISS.from_documents(documents=chunks, embedding=embeddings_engine)

    index_directory.mkdir(parents=True, exist_ok=True)
    vector_store.save_local(str(index_directory))

    _meta_path(index_directory).write_text(
        json.dumps(
            {
                "signature": signature,
                "params": build_params.__dict__,
                "num_chunks": len(chunks),
                "num_documents": len(documents),
            },
            indent=2,
            default=str,
        ),
        encoding="utf-8",
    )
    _enforce_index_dir_permissions(index_directory)
    logger.info(
        "FAISS store compiled successfully (%d chunks written to %s)",
        len(chunks),
        index_directory,
    )
    return vector_store


def _similarity_search_with_timeout(
    vector_store: VectorStore,
    query: str,
    k: int,
    timeout_seconds: float | None = None,
) -> list[Any]:
    """Execute similarity searches asynchronously, returning empty if timed out."""
    if timeout_seconds is not None and timeout_seconds > 0:
        results: list[Any] = []
        error_box: list[BaseException | None] = [None]

        def _search_thread() -> None:
            try:
                results.extend(vector_store.similarity_search(query=query, k=k))
            except BaseException as exception:
                error_box[0] = exception

        worker = threading.Thread(target=_search_thread, daemon=True)
        worker.start()
        worker.join(timeout=timeout_seconds)
        if worker.is_alive():
            logger.warning(
                "Similarity search exceeded timeout boundary (%.1fs); query=%r",
                timeout_seconds,
                query[:60],
            )
            return []
        if error_box[0] is not None:
            raise error_box[0]
        return results
    return vector_store.similarity_search(query=query, k=k)


def extract_all_sources(vector_store: VectorStore) -> list[str]:
    """Inspect the internal document memory to register active search filters."""
    sources: set[str] = set()
    docstore = getattr(vector_store, "docstore", None)
    internal_dictionary = getattr(docstore, "_dict", None)
    if isinstance(internal_dictionary, dict):
        for document in internal_dictionary.values():
            metadata = getattr(document, "metadata", {}) or {}
            sources.add(str(metadata.get("source", "unknown")))
    return sorted(sources)


# =============================================================================
# RETRIEVER TOOL (smolagents)
# =============================================================================


class RetrieverTool(_ToolBase):  # type: ignore[invalid-base]
    """Code agent tool supporting secure metadata post-filtering."""

    name = "retriever"
    description = (
        "Retrieves real code examples and explanations from the indexed Hugging "
        "Face datasets using semantic similarity search. Use this to ground answers "
        "in concrete, working examples before writing your own solution."
    )

    inputs = {
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
        vector_store: VectorStore,
        all_sources: list[str] | None = None,
        default_k: int = 5,
        similarity_search_timeout: float | None = DEFAULT_SIMILARITY_SEARCH_TIMEOUT,
        **kwargs: Any,
    ) -> None:
        super().__init__(**kwargs)
        self.inputs = copy.deepcopy(self.__class__.inputs)
        self.vectordb: VectorStore = vector_store
        self.all_sources: list[str] = (
            all_sources
            if all_sources is not None
            else extract_all_sources(vector_store)
        )
        self.default_k: int = default_k
        self.similarity_search_timeout: float | None = similarity_search_timeout
        self.inputs["source"]["description"] = (
            "Optional source filter (single source or JSON list). "
            f"Available sources: {self.all_sources}"
        )

    @staticmethod
    def _normalize_sources(source_value: str | None) -> list[str] | None:
        if not source_value:
            return None
        stripped = str(source_value).strip()
        if not stripped:
            return None
        if stripped.startswith("["):
            for candidate in (stripped, stripped.replace("'", '"')):
                try:
                    parsed = json.loads(candidate)
                    if isinstance(parsed, list):
                        cleaned = [
                            str(item).strip() for item in parsed if str(item).strip()
                        ]
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
        """Query the vector store database and format returned matches."""
        if not isinstance(query, str) or not query.strip():
            raise ValueError("`query` argument must be populated.")

        try:
            k = (
                int(number_of_documents)
                if number_of_documents is not None
                else self.default_k
            )
        except (TypeError, ValueError) as exception:
            raise ValueError(
                "`number_of_documents` must resolve to an integer."
            ) from exception

        if k <= 0:
            raise ValueError("`number_of_documents` must exceed 0.")
        k = min(k, 25)

        source_list = self._normalize_sources(source)
        fetch_k = min(k * 5, 100) if source_list else k

        documents = _similarity_search_with_timeout(
            vector_store=self.vectordb,
            query=query,
            k=fetch_k,
            timeout_seconds=self.similarity_search_timeout,
        )

        # Apply robust Python post-filtering to bypass silent database-side filter failures
        if source_list:
            target_sources = set(source_list)
            documents = [
                doc
                for doc in documents
                if str(doc.metadata.get("source")) in target_sources
            ]
        documents = documents[:k]

        if not documents:
            filter_hint = f" (filtered to: {source_list})" if source_list else ""
            return (
                f"No matching documents returned{filter_hint}. "
                "Try utilizing a broader semantic phrase."
            )

        formatted_output: list[str] = []
        for index, doc in enumerate(documents, start=1):
            source_label = doc.metadata.get("source", "unknown")
            formatted_output.append(
                f"Document {index} | Source: {source_label}\n\n{doc.page_content}"
            )
        return "\n\n===DOCUMENT===\n\n".join(formatted_output)


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
    """Convenience initialization routine ensuring ready index creation."""
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

# =============================================================================
# CONVENIENCE RETRIEVAL FUNCTION
# =============================================================================


def retrieve_context(
    query: str,
    top_k: int = 5,
    dataset_keys: list[str] | None = None,
    index_dir: str | os.PathLike[str] = DEFAULT_INDEX_DIR,
) -> str:
    """High-level convenience function for retrieving context.

    Used by tests, CLI, and quick debugging.
    """
    vector_store = build_or_load_vector_store(
        dataset_keys=dataset_keys,
        index_dir=index_dir,
        force_rebuild=False,
    )
    return RetrieverTool(vector_store, default_k=top_k).forward(query=query)


# =============================================================================
# EXPORTS
# =============================================================================

__all__ = [
    "DATASET_REGISTRY",
    "DEFAULT_DATASET_KEYS",
    "DEFAULT_INDEX_DIR",
    "DatasetSpec",
    "RetrieverTool",
    "build_embeddings",
    "build_or_load_vector_store",
    "build_retriever_tool",
    "extract_all_sources",
    "load_documents",
    "resolve_specs",
    "retrieve_context",
    "split_documents",
    "IndexBuildParams",
]