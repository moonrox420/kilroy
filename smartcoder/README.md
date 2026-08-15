# SmartCoder — local RAG coding agent

A self-correcting coding agent: a [smolagents](https://github.com/huggingface/smolagents)
`CodeAgent` that writes Python, executes it, reads the traceback when it fails, and
fixes itself — grounded by retrieval-augmented generation (RAG) over Hugging Face code
datasets (`glaiveai/glaive-code-assistant` + `HuggingFaceH4/CodeAlpaca_20K`) via a
pickle-free NumPy index. It talks to the same local **Ollama** daemon Kilroy uses, so no
API keys and nothing leaves your machine.

One canonical package lives under `smartcoder/smartcoder/`; repository-root
compatibility shims and the installed console commands both resolve that same
implementation.

| File                   | Role                                                                                                                                                                                                |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `smartcoder/infrastructure/retrieval.py` | Pickle-free RAG engine — dataset adapters, code-aware chunking, BGE embeddings, NumPy persistence, and the `RetrieverTool`. |
| `smartcoder/kilroy_smartcoder.py` | Compatibility API for the local agent, Maestro, and `build-index` / `ask` / `chat` / `list-datasets` commands. |

## Environment setup

From the Kilroy repository root:

```powershell
uv venv .venv
uv pip install --python .venv\Scripts\python.exe -r requirements.txt
uv pip install --python .venv\Scripts\python.exe --no-deps .\smartcoder
```

Kilroy runs the vendored source tree in development and installs the same package
to expose the `smartcoder` and `smartcoder-build-index` console commands.

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

> **Security:** `--sandbox local` (the default) executes model-generated Python in a
> terminable child process with the current user's OS permissions; it is not an OS
> sandbox. Use `--sandbox docker` (local Docker daemon) for untrusted workloads.

### Prebuilt index

The safe index consists of `embeddings.npy`, `documents.jsonl`, and
`build_meta.json`. The engine reloads it when the build signature matches and
otherwise rebuilds it. Pickle indexes are not deserialized.

## Kilroy desktop integration

Kilroy's default CodeAgent invokes Smart Coder through the Tauri bridge and feeds
its project-grounded analysis into the durable Rust executor. The Rust executor
retains typed inspection, evidence recording, and user approval for every file or
shell mutation.

- `smartcoder_status()` — probes for Python, required dependencies, and this
  vendored source tree and reports whether it can run.
- `smartcoder_run(subcommand, args)` — launches the agent, streams its output to the UI on
  `smartcoder://output` (per line, tagged `stdout`/`stderr`), and finishes on
  `smartcoder://done` with the exit code.

The bridge launches the vendored compatibility script with the repository-root
virtualenv and streams `smartcoder://output` / `smartcoder://done` events.
