"""
CLI argument parser for optional dataset retrieval utilities.

Extracted from the original kilroy_smartcoder.py's build_parser(), config_from_args(),
and main() functions. This module handles:
  - All argument definitions
  - Namespace-to-AppConfig conversion
  - Subcommand dispatch
"""

from __future__ import annotations

import argparse
import logging

from smartcoder.infrastructure.dependencies import (
    DependencyManager,
    MissingDependencyError,
)
from smartcoder.runtime.config import AppConfig, setup_logging
from smartcoder.runtime.constants import (
    DEFAULT_OLLAMA_HOST,
    DEFAULT_OLLAMA_MODEL,
    VALID_BACKENDS,
    VALID_SANDBOXES,
)

logger = logging.getLogger("smartcoder")


def build_parser() -> argparse.ArgumentParser:
    """Build the argument parser with all subcommands and options."""
    parser = argparse.ArgumentParser(
        prog="kilroy_smartcoder.py",
        description="Kilroy optional dataset retrieval utilities.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )

    # Shared options (apply to all subcommands).
    parser.add_argument("--backend", choices=VALID_BACKENDS, default="ollama")
    parser.add_argument(
        "--model", default=DEFAULT_OLLAMA_MODEL, help="Model id/name for the backend."
    )
    parser.add_argument(
        "--ollama-host",
        default=DEFAULT_OLLAMA_HOST,
        help="Local Ollama URL (honors the OLLAMA_HOST env var; the Kilroy app passes its settings.json value).",
    )
    parser.add_argument(
        "--llama-model-path", default=None, help="Path to .gguf (llama_cpp backend)."
    )
    parser.add_argument("--temperature", type=float, default=0.2)
    parser.add_argument("--max-tokens", type=int, default=2048)
    parser.add_argument("--num-ctx", type=int, default=8192)

    parser.add_argument(
        "--context-file",
        default=None,
        help="JSON file with Kilroy project context (chunks, files, decisions).",
    )
    parser.add_argument(
        "--project-root",
        default=None,
        help="Absolute path to the open project root.",
    )
    parser.add_argument(
        "--no-dataset-rag",
        action="store_true",
        help="Disable Hugging Face dataset RAG (default when launched from Kilroy).",
    )
    parser.add_argument(
        "--task-role",
        default=None,
        help="Swarm agent role (developer, qa, reviewer, architect, planner, …).",
    )
    parser.add_argument(
        "--task-type",
        default=None,
        help="Swarm task type (code, test, review, analysis, doc, plan, …).",
    )
    parser.add_argument("--embedding-model", default="BAAI/bge-small-en-v1.5")
    parser.add_argument("--index-dir", default="vector_store")
    parser.add_argument(
        "--datasets",
        nargs="*",
        default=[],
        help="HF dataset keys for optional RAG (glaive, codealpaca). Empty = project-only.",
    )
    parser.add_argument(
        "--max-items", type=int, default=5000, help="Rows per dataset (-1 = all)."
    )
    parser.add_argument("--force-rebuild", action="store_true")

    parser.add_argument("--sandbox", choices=VALID_SANDBOXES, default="local")
    parser.add_argument("--max-steps", type=int, default=12)
    parser.add_argument(
        "--web-search",
        action="store_true",
        help="Allow the agent to use a web-search tool (requires network; off by default).",
    )
    parser.add_argument(
        "--no-web-search",
        action="store_true",
        help="Force web search off (default; wins over --web-search if both are passed).",
    )
    parser.add_argument(
        "--log-level",
        choices=["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"],
        default="INFO",
    )
    parser.add_argument(
        "--authorized-imports",
        nargs="*",
        default=None,
        help="Additional module names the agent may import (whitelist).",
    )

    sub = parser.add_subparsers(dest="command")

    sub.add_parser(
        "build-index", help="Build or refresh the FAISS retrieval index, then exit."
    )

    ld = sub.add_parser(
        "list-datasets", help="List pre-wired datasets (and optionally the Hub)."
    )
    ld.add_argument(
        "--hub", action="store_true", help="Also query the Hugging Face Hub."
    )
    ld.add_argument(
        "--filter",
        default=None,
        help="Hub filter string (e.g. 'task_categories:text-generation').",
    )
    ld.add_argument("--limit", type=int, default=20)

    return parser


def config_from_args(args: argparse.Namespace) -> AppConfig:
    """Convert parsed CLI arguments into a validated runtime config."""
    max_items = (
        None if args.max_items is not None and args.max_items < 0 else args.max_items
    )
    datasets = (
        tuple(d.strip() for d in args.datasets if d and d.strip())
        if args.datasets
        else ()
    )
    use_dataset_rag = bool(datasets) and not args.no_dataset_rag
    return AppConfig(
        log_level=args.log_level,
        backend=args.backend,
        model_name=args.model,
        ollama_host=args.ollama_host,
        llama_model_path=args.llama_model_path,
        temperature=args.temperature,
        max_tokens=args.max_tokens,
        num_ctx=args.num_ctx,
        context_file=args.context_file,
        project_root=args.project_root,
        use_dataset_rag=use_dataset_rag,
        task_role=args.task_role,
        task_type=args.task_type,
        embedding_model=args.embedding_model,
        index_dir=args.index_dir,
        datasets=datasets,
        max_items_per_dataset=max_items,
        force_rebuild=args.force_rebuild,
        sandbox=args.sandbox,
        max_steps=args.max_steps,
        use_web_search=args.web_search and not args.no_web_search,
        authorized_imports=args.authorized_imports,
    )


def main(argv: list[str] | None = None) -> int:
    """Main entry point — parse args, dispatch to the appropriate handler."""
    from smartcoder.cli.handlers import handle_build_index, handle_list_datasets

    parser = build_parser()
    args = parser.parse_args(argv)

    setup_logging(args.log_level)

    if not args.command:
        parser.print_help()
        return 1

    deps = DependencyManager()

    try:
        # list-datasets does not need a full (model-validated) config.
        if args.command == "list-datasets":
            handle_list_datasets(
                query_hub=args.hub,
                filter_str=args.filter,
                limit=args.limit,
                deps=deps,
            )
            return 0

        config = config_from_args(args)

        if args.command == "build-index":
            handle_build_index(config, deps)
            return 0

        parser.print_help()
        return 1

    except KeyboardInterrupt:
        logger.info("Interrupted by user.")
        return 130
    except MissingDependencyError as exc:
        logger.error(str(exc))
        return 1
    except Exception as exc:
        logger.error("Fatal error: %s", exc)
        if args.log_level.upper() == "DEBUG":
            import traceback

            traceback.print_exc()
        return 1
    finally:
        logging.shutdown()
