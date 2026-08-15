# Kilroy Smart Coder backend

This directory is the canonical `smartcoder` Python package used by both the
standalone CLI and Kilroy's Tauri desktop runtime. It is not a separate product
or a second application: Smart Coder provides project-grounded analysis and
Kilroy's Rust runtime applies any proposed mutations through its approval gate.

The package uses local model backends only. Ollama is the default. Optional
dataset retrieval persists a pickle-free NumPy index as `embeddings.npy` plus
`documents.jsonl`; no FAISS pickle is required by the desktop path.

Install from the parent `smartcoder/` packaging root:

```powershell
uv pip install --python ../.venv/Scripts/python.exe --no-deps .
```

The supported entry points are `smartcoder` and `smartcoder-build-index`. See
the parent `README.md` for setup, bootstrap, and desktop integration details.
