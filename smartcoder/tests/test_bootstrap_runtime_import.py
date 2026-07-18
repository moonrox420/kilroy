import pathlib
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))


def test_retrieval_imports_cleanly():
    import smartcoder.infrastructure.retrieval  # noqa: F401
