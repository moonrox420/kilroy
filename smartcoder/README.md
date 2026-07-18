# SmartCoder — local RAG coding agent

A self-correcting coding agent: a [smolagents](https://github.com/huggingface/smolagents)
`CodeAgent` that writes Python, executes it, reads the traceback when it fails, and
fixes itself — grounded by retrieval-augmented generation (RAG) over Hugging Face code
datasets (`glaiveai/glaive-code-assistant` + `HuggingFaceH4/CodeAlpaca_20K`) via a
persisted FAISS index. It talks to the same local **Ollama** daemon Kilroy uses, so no
API keys and nothing leaves your machine.

Two modules, flat layout:

| File                   | Role                                                                                                                                                                                                |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `infrastructure/retrieval.py` | RAG engine — dataset adapters, code-aware chunking, bge embeddings, FAISS build/persist/load, and the `RetrieverTool` the agent calls.                                                    |
| `kilroy_smartcoder.py` | The agent + CLI — model backends (Ollama primary; langchain-ollama, llama.cpp, HF Inference, OpenAI fallbacks), the `CodeAgent`, and the `build-index` / `ask` / `chat` / `list-datasets` commands. |

## Environment setup

From the Kilroy repository root:

```powershell
uv venv .venv
uv pip install -r requirements.txt
```

SmartCoder runs directly from the vendored source tree; the repository itself is
not installed as a Python package.

Pull the coding model in Ollama first (default is `qwen2.5-coder:14b-instruct-q8_0`; any
Ollama coding model works — override with `--model`):

```powershell
ollama pull qwen2.5-coder:14b-instruct-q8_0
```

## Use

```powershell
python -m smartcoder.kilroy_smartcoder build-index
python -m smartcoder.kilroy_smartcoder ask "write a thread-safe LRU cache with unit tests"
python -m smartcoder.kilroy_smartcoder chat
python -m smartcoder.kilroy_smartcoder list-datasets --hub
```

Key flags (all subcommands): `--backend {ollama,langchain_ollama,llama_cpp}` (all local by design),
`--model`, `--ollama-host`, `--embedding-model`, `--index-dir`, `--datasets`, `--max-items`,
`--sandbox {local,docker}`, `--web-search` (opt-in; offline by default), `--log-level`.

> **Security:** `--sandbox local` (the default) executes model-generated Python in the
> current process. Use `--sandbox docker` (local Docker daemon) for untrusted workloads.

### Prebuilt index

If you already have an `index.faiss` + `index.pkl` + `build_meta.json` set, drop them in
the `--index-dir` (default `vector_store/`). The engine reloads a persisted index instantly
when its build signature matches; otherwise it rebuilds and re-persists.

## Kilroy desktop integration

The Kilroy app talks to SmartCoder over a Tauri bridge (`src-tauri/src/commands/smartcoder.rs`):

- `smartcoder_status()` — probes for Python, required dependencies, and this
  vendored source tree and reports whether it can run.
- `smartcoder_run(subcommand, args)` — launches the agent, streams its output to the UI on
  `smartcoder://output` (per line, tagged `stdout`/`stderr`), and finishes on
  `smartcoder://done` with the exit code.

The bridge launches `python -m smartcoder.kilroy_smartcoder` from the repository
root, using dependencies installed solely from the root `requirements.txt`.
