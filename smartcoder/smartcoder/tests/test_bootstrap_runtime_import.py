import pathlib
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))


def test_kilroy_retrieval_imports_cleanly():
    import smartcoder.kilroy_retrieval  # noqa: F401
