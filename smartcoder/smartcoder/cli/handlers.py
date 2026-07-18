"""
smartcoder/cli/handlers.py

CLI subcommand handlers — dispatched from parser.main().
Each handler function implements one CLI subcommand. They keep the
parser module clean and make each command testable independently.
"""

from __future__ import annotations

import logging

from smartcoder.infrastructure.dependencies import DependencyManager
from smartcoder.runtime.config import AppConfig

logger = logging.getLogger("smartcoder")


def handle_build_index(config: AppConfig, deps: DependencyManager) -> None:
    """Build or refresh the safe, pure-NumPy retrieval index."""
    # Enforce safe RAG engine requirements (eliminated faiss-cpu dependency)
    deps.require(
        "datasets",
        "numpy",
        "langchain_core",
        "langchain_huggingface",
        "langchain_text_splitters",
        "sentence_transformers",
    )
    from smartcoder.infrastructure import retrieval

    dataset_keys = [d for d in config.datasets if d and d.strip()] or [
        "glaive",
        "codealpaca",
    ]
    retrieval.build_or_load_vector_store(
        dataset_keys=dataset_keys,
        embedding_model=config.embedding_model,
        index_dir=config.index_dir,
        max_items_per_dataset=config.max_items_per_dataset,
        force_rebuild=config.force_rebuild,
    )
    print(f"Index ready at: {config.index_dir}")


def handle_list_datasets(
    query_hub: bool,
    filter_str: str | None,
    limit: int,
    deps: DependencyManager,
) -> None:
    """List pre-wired datasets and optionally query the Hugging Face Hub."""
    from smartcoder.infrastructure import retrieval

    print("Pre-wired datasets (ready for retrieval):")
    for key, spec in retrieval.DATASET_REGISTRY.items():
        default_mark = "*" if key in retrieval.DEFAULT_DATASET_KEYS else " "
        print(f"  [{default_mark}] {key:<12} -> {spec.hf_name} (source='{spec.source_label}')")
    print("  (* = enabled by default)")

    if not query_hub:
        return

    deps.require("huggingface_hub")
    from huggingface_hub import HfApi

    print(f"\nHugging Face Hub datasets (filter={filter_str!r}, limit={limit}):")
    api = HfApi()
    try:
        for dataset in api.list_datasets(filter=filter_str, limit=limit):
            print(f"  {getattr(dataset, 'id', dataset)}")
    except Exception as exc:
        logger.error("HF Hub listing failed: %s", exc)
        raise
