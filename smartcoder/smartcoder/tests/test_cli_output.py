from __future__ import annotations

from smartcoder.cli.parser import main
from smartcoder.controllers import maestro


def test_supervised_ask_prints_only_final_report(
    monkeypatch,
    capsys,
) -> None:
    class StubController:
        def __init__(self, *_args, **_kwargs) -> None:
            pass

        def run(self, _task: str) -> str:
            return "Architecture report"

    monkeypatch.setenv("SMARTCODER_SUPERVISED", "1")
    monkeypatch.setattr(maestro, "SmartCoderController", StubController)

    exit_code = main(["ask", "Inspect", "the", "project"])

    assert exit_code == 0
    assert capsys.readouterr().out == "Architecture report\n"
