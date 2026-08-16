from __future__ import annotations

from types import SimpleNamespace
from unittest.mock import patch

from smartcoder.infrastructure.models import _make_ollama_model
from smartcoder.runtime.config import AppConfig


def test_direct_ollama_model_uses_configured_local_endpoint() -> None:
    config = AppConfig(
        model_name="nemotron-3.5-lightning:30b",
        ollama_host="http://localhost:11434",
        temperature=0.2,
        max_tokens=2048,
        num_ctx=8192,
    )

    with patch("ollama.Client") as client_type:
        client = client_type.return_value
        client.chat.return_value = SimpleNamespace(
            message=SimpleNamespace(content="analysis complete")
        )

        model = _make_ollama_model(config)
        result = model.generate([{"role": "user", "content": "Inspect the project"}])

    assert result.content == "analysis complete"
    client_type.assert_called_once_with(host="http://localhost:11434")
    client.chat.assert_called_once_with(
        model="nemotron-3.5-lightning:30b",
        messages=[{"role": "user", "content": "Inspect the project"}],
        format=None,
        options={
            "temperature": 0.2,
            "num_ctx": 8192,
            "num_predict": 2048,
        },
    )
