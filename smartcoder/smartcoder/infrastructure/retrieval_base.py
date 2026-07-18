r"""
retrieval_base.py — Shared RAG engine for the SmartCoder agent.

Single source of truth for the parts that don't change between backends:
  * Dataset adapters     : map raw HF rows -> langchain Documents
  * Chunking             : code-aware splitting
  * Embeddings           : HuggingFace bge models, normalized, GPU/MPS/CPU
  * Build orchestration  : build/load/persist via the chosen backend
  * RetrieverTool        : a smolagents Tool the CodeAgent calls

The vector-store backend (FAISS vs PureNumPy) is selected once at import
time of ``smartcoder.infrastructure.retrieval`` via the env var
``SMARTCODER_VECTOR_BACKEND`` (default: ``numpy``). The orchestrator here
only knows the abstract ``from_documents / save_local / load_local /
similarity_search`` interface.

Import-safety: heavy ML deps (datasets, torch, sentence-transformers,
langchain_*) are imported lazily so this module is cheap to import for
type definitions, --help, or smoke tests.
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

# Guarded smolagents import so light uses don't trigger the full ML stack.
try:
    from smolagents import Tool as _ToolBase
except ImportError:
    _ToolBase = object  # type: ignore[assignment, misc]

if TYPE_CHECKING:  # for static type checkers only
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
    """A declarative recipe for turning one HF dataset into retrievable Documents."""

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
# DOCUMENT LOADING + CHUNKING
# =============================================================================


def load_documents(
    specs: list[DatasetSpec],
    max_items_per_dataset: int | None = DEFAULT_MAX_ITEMS_PER_DATASET,
    streaming: bool = True,
) -> list["LCDocument"]:
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
        except Exception as exc:
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
    documents: list["LCDocument"],
    chunk_size: int = 1_200,
    chunk_overlap: int = 150,
) -> list["LCDocument"]:
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
    except Exception as exc:  # noqa: BLE001
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
# BUILD ORCHESTRATION (backend-agnostic)
# =============================================================================


@dataclass(frozen=True)
class IndexBuildParams:
    """Everything that, if changed, must invalidate a persisted index."""

    dataset_keys: tuple[str, ...]
    splits: tuple[str, ...]
    max_items_per_dataset: int | None
    embedding_model: str
    normalize: bool
    chunk_size: int
    chunk_overlap: int
    backend: str

    def signature(self) -> str:
        payload = json.dumps(self.__dict__, sort_keys=True, default=str)
        return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def _meta_path(index_dir: Path) -> Path:
    return index_dir / "build_meta.json"


def _index_present(index_dir: Path, backend: str) -> bool:
    """Check if a valid on-disk index exists for the given backend."""
    if backend == "numpy":
        return (index_dir / "embeddings.npy").exists() and (index_dir / "documents.jsonl").exists()
    # faiss
    return (index_dir / "index.faiss").exists() and (index_dir / "index.pkl").exists()


def _enforce_index_dir_permissions(index_dir: Path) -> None:
    try:
        index_dir.chmod(0o700)
    except OSError as exc:
        logger.warning("Could not enforce index_dir permissions (%s)", exc)


def _verify_index_dir_ownership(index_dir: Path) -> None:
    getuid = getattr(os, "getuid", None)
    if getuid is None:
        return  # Windows
    uid = getuid()
    for entry in [index_dir] + list(index_dir.rglob("*")):
        try:
            if entry.stat().st_uid != uid:
                raise PermissionError(f"index_dir entry not owned by current user: {entry}")
        except OSError as exc:
            logger.warning("Ownership check failed for %s (%s)", entry, exc)


def _select_backend() -> str:
    """Return the active backend name. Override via SMARTCODER_VECTOR_BACKEND."""
    return os.environ.get("SMARTCODER_VECTOR_BACKEND", "numpy").lower()


def _store_class(backend: str):
    """Map backend name -> store class. Imports lazily to keep startup cheap."""
    if backend == "numpy":
        from .retrieval_backends.numpy import PureNumPyVectorStore

        return PureNumPyVectorStore
    if backend == "faiss":
        from .retrieval_backends.faiss import FaissVectorStore

        return FaissVectorStore
    raise ValueError(f"Unknown vector backend: {backend!r}. Use 'numpy' or 'faiss'.")


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
    backend: str | None = None,
):
    r"""
    Return a ready-to-query vector store for the selected backend.

    Backend selection: ``backend`` arg wins, else ``SMARTCODER_VECTOR_BACKEND``
    env var, else ``"numpy"``.

    SECURITY: ``index_dir`` must be inside the current working directory,
    is created with mode 0700, and its contents are ownership-checked
    on load. Never point ``index_dir`` at a world-writable location.
    """
    backend = (backend or _select_backend()).lower()
    StoreClass = _store_class(backend)

    index_dir = Path(index_dir)
    cwd = Path.cwd().resolve()
    resolved = index_dir.resolve()
    try:
        resolved.relative_to(cwd)
    except ValueError:
        raise ValueError(
            f"index_dir must be inside the current working directory ({cwd}): {index_dir}"
        )

    index_dir.mkdir(parents=True, exist_ok=True)
    _enforce_index_dir_permissions(index_dir)

    specs = resolve_specs(dataset_keys)
    params = IndexBuildParams(
        dataset_keys=tuple(s.key for s in specs),
        splits=tuple(s.split for s in specs),
        max_items_per_dataset=max_items_per_dataset,
        embedding_model=embedding_model,
        normalize=normalize,
        chunk_size=chunk_size,
        chunk_overlap=chunk_overlap,
        backend=backend,
    )
    signature = params.signature()
    embeddings = build_embeddings(embedding_model, device=device, normalize=normalize)

    # ---- Fast path: reuse matching persisted index ----------------------
    if not force_rebuild and _index_present(index_dir, backend):
        try:
            saved_meta = json.loads(_meta_path(index_dir).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            saved_meta = {}
        if saved_meta.get("signature") == signature:
            logger.info("Loading persisted %s index from %s", backend, index_dir)
            _enforce_index_dir_permissions(index_dir)
            _verify_index_dir_ownership(index_dir)
            return StoreClass.load_local(str(index_dir), embeddings)

        logger.info("Persisted index signature mismatch — rebuilding.")

    # ---- Slow path: build, persist, record signature -------------------
    logger.info("Building %s index (one-time cost)...", backend)
    documents = load_documents(specs, max_items_per_dataset=max_items_per_dataset)
    chunks = split_documents(documents, chunk_size=chunk_size, chunk_overlap=chunk_overlap)

    vector_store = StoreClass.from_documents(
        documents=chunks,
        embedding=embeddings,
        index_dir=str(index_dir),
    )
    index_dir.mkdir(parents=True, exist_ok=True)
    vector_store.save_local(str(index_dir))

    _meta_path(index_dir).write_text(
        json.dumps(
            {
                "signature": signature,
                "params": params.__dict__,
                "num_chunks": len(chunks),
                "num_documents": len(documents),
                "backend": backend,
            },
            indent=2,
            default=str,
        ),
        encoding="utf-8",
    )
    logger.info("Built %d chunks and saved to %s (backend=%s)", len(chunks), index_dir, backend)
    return vector_store


def _similarity_search_with_timeout(
    vectordb: Any,
    query: str,
    k: int,
    timeout: float | None = None,
) -> list[Any]:
    """Run similarity_search with an optional timeout. NumPy is sync (~20ms) so
    the timeout is a no-op for the numpy backend but useful for FAISS."""
    if timeout is not None and timeout > 0:
        result: list[Any] = []
        error_box: list[BaseException | None] = [None]

        def _target() -> None:
            try:
                result.extend(vectordb.similarity_search(query=query, k=k))
            except BaseException as exc:  # noqa: BLE001
                error_box[0] = exc

        worker = threading.Thread(target=_target, daemon=True)
        worker.start()
        worker.join(timeout=timeout)
        if worker.is_alive():
            logger.warning("similarity_search timed out after %.1fs; returning []", timeout)
            return []
        if error_box[0] is not None:
            raise error_box[0]
        return result
    return vectordb.similarity_search(query=query, k=k)


def extract_all_sources(vector_store: Any) -> list[str]:
    """Enumerate distinct `source` metadata values present in the store."""
    sources: set[str] = set()
    docs = getattr(vector_store, "_documents", None)
    if docs is None:
        internal = getattr(getattr(vector_store, "docstore", None), "_dict", None)
        docs = internal.values() if isinstance(internal, dict) else []
    for doc in docs:
        metadata = getattr(doc, "metadata", {}) or {}
        sources.add(str(metadata.get("source", "unknown")))
    return sorted(sources)


# =============================================================================
# RETRIEVER TOOL (smolagents)
# =============================================================================


class RetrieverTool(_ToolBase):  # type: ignore[invalid-base]
    r"""Semantic search over the vector store, exposed to the CodeAgent as a tool."""

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
        vectordb: Any,
        all_sources: list[str] | None = None,
        default_k: int = 5,
        similarity_search_timeout: float | None = DEFAULT_SIMILARITY_SEARCH_TIMEOUT,
        **kwargs: Any,
    ) -> None:
        super().__init__(**kwargs)
        # Deep-copy class-level inputs so per-instance mutation doesn't
        # corrupt the class schema for sibling instances.
        self.inputs = copy.deepcopy(self.__class__.inputs)
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
    """One-call convenience: ensure the index exists, then return a ready tool."""
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


def _configure_standalone_logging() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s | %(levelname)-8s | %(name)s | %(message)s",
        datefmt="%H:%M:%S",
    )


def main() -> None:
    """Standalone entrypoint: build (or refresh) the default index, then smoke-test it."""
    import argparse

    _configure_standalone_logging()
    parser = argparse.ArgumentParser(description="Build/refresh the SmartCoder RAG index.")
    parser.add_argument(
        "--datasets",
        nargs="+",
        default=list(DEFAULT_DATASET_KEYS),
        help=f"Dataset keys to index. Options: {sorted(DATASET_REGISTRY)}",
    )
    parser.add_argument("--embedding-model", default="BAAI/bge-small-en-v1.5")
    parser.add_argument("--index-dir", default=str(DEFAULT_INDEX_DIR))
    parser.add_argument(
        "--max-items",
        type=int,
        default=DEFAULT_MAX_ITEMS_PER_DATASET,
        help="Rows per dataset (-1 for all).",
    )
    parser.add_argument("--force-rebuild", action="store_true")
    parser.add_argument(
        "--backend", default=None, help="Vector backend: 'numpy' (default) or 'faiss'."
    )
    parser.add_argument("--query", default="Write a function to reverse a linked list in Python.")
    args = parser.parse_args()

    max_items = None if args.max_items is not None and args.max_items < 0 else args.max_items
    if args.backend:
        os.environ["SMARTCODER_VECTOR_BACKEND"] = args.backend
    tool = build_retriever_tool(
        dataset_keys=args.datasets,
        embedding_model=args.embedding_model,
        index_dir=args.index_dir,
        max_items_per_dataset=max_items,
        force_rebuild=args.force_rebuild,
    )
    print("=" * 80)
    print(f"Index ready (backend={_select_backend()}). Sources: {tool.all_sources}")
    print(f"Smoke-test query: {args.query!r}")
    print("=" * 80)
    print(tool.forward(args.query, number_of_documents=3))


def retrieve_context(query: str, top_k: int = 3) -> str:
    """Convenience wrapper: initialize the index and run a semantic search."""
    tool = build_retriever_tool(default_k=top_k)
    return tool.forward(query=query, number_of_documents=top_k)


if __name__ == "__main__":
    main()
