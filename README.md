# Kilroy

**Local AI agentic software engineering platform** — Windows 11 native, Tauri 2 + Rust + React.

> **No API keys. No remote auth. No cloud model round-trip.** Kilroy's supported model backends are local Ollama, local llama.cpp, and LangChain's local Ollama adapter. The Python dependency lock may contain SDKs required transitively by agent libraries, but Kilroy does not expose or select cloud model backends.

This is the full application, not a shell. Every panel is wired, the file explorer browses real disk, the terminal spawns a real PowerShell PTY, and the agent chat round-trips through Rust. The "brain" is implemented and wired end to end: the planner decomposes a goal into a task graph (`runtime/planner.rs`), the executor runs the tasks and streams their output live (`runtime/executor.rs`), the actuator parses code blocks into reviewable file-write / patch / shell actions that apply only on your Accept (`actuator/`), and per-project memory + retrieval persist to SQLite + sqlite-vec. Model calls are settings-driven against local Ollama (`generation.rs`); pick any installed model in Settings.

```
┌── Kilroy ────────────────────────────────────────────────────┐
│  TitleBar (custom, drag region, native min/max/close)        │
│  MenuBar (File / Edit / View / Go / Terminal / Agent / Help) │
│  ┌───────────────────────────────────┬───────────────────┐   │
│  │  Explorer ⟂ Editor (Monaco)       │  Agent Chat       │   │
│  │  ───────────────────              │  (fixed 360px,    │   │
│  │  Terminal (xterm + PTY)           │   full height)    │   │
│  └───────────────────────────────────┴───────────────────┘   │
│  StatusBar (file • language • mode • model • working pulse)  │
└──────────────────────────────────────────────────────────────┘
```

When the terminal collapses, the **File Explorer extends down to the status bar**. When you reopen it (Ctrl+\` or the toggle in the status bar), the explorer shrinks back to make room.

---

## First steps: from install to launch

Use this order on a fresh machine:

1. Install the prerequisites:
   - Rust + MSVC build tools
   - Node.js LTS
   - Python 3.10+
   - Ollama
   - uv
2. Open PowerShell in the project root.
3. Run the bootstrap script:

```powershell
.\bootstrap.ps1
```

That script installs the missing toolchain pieces, creates the SmartCoder virtualenv, installs the Python requirements with uv, and then launches the app in dev mode.

If you want to do the same steps manually, use this exact order:

```powershell
uv --version
uv venv .venv
uv pip install --python .venv\Scripts\python.exe -r requirements.txt
uv pip install --python .venv\Scripts\python.exe --no-deps .\smartcoder
npm install
npm run tauri:dev
```

> The first launch can take a while while Cargo and Vite populate their caches. After that, subsequent launches are much faster.

---

## Quick start — `bootstrap.ps1`

The repo ships with a self-contained PowerShell bootstrap script that handles a clean Windows 11 machine end-to-end. From PowerShell in the extracted project root:

```powershell
.\bootstrap.ps1
```

Seven phases, idempotent (re-run anytime):

1. **winget** check
2. **Toolchain** — Rust, Node LTS, Python, MSVC Build Tools, WebView2 (installs only what's missing)
3. **SmartCoder** — Python venv + RAG agent dependencies
4. **Ollama** — installs, starts the service, pulls the chat + embedding models
5. **Windows Sandbox feature** — enables `Containers-DisposableClientVM` if running elevated, otherwise prints the manual one-liner
6. **Project deps** — `npm install` / `npm ci` + `cargo fetch` pre-pull
7. **Launch** — `npm run tauri:dev` (or `-Build` for installer, or `-NoRun` to stop here)

Flags worth knowing:

| Flag | Effect |
| --- | --- |
| `-SkipSandbox` | Don't touch the Windows Sandbox feature |
| `-SkipModels` | Skip the Ollama model pulls (pick your own later via Settings) |
| `-NoRun` | Set up everything but don't launch dev mode |
| `-Build` | After setup, produce a release `.msi` instead of running dev mode |
| `-ChatModel <tag>` | Override chat model (default `qwen2.5-coder:14b-instruct-q8_0`) |
| `-EmbedModel <tag>` | Override embedding model (default `nomic-embed-text`) |

If you'd rather drive the install by hand, the manual sequence is below.

---

## Manual setup — prerequisites (any Windows 11 box)

You need three toolchains. The exact PowerShell commands are below.

1. **Rust** — `1.77+`, stable, with the MSVC toolchain.
2. **Node** — `18+` (LTS recommended).
3. **WebView2 Runtime** — ships with Windows 11. If you stripped it, install from <https://developer.microsoft.com/microsoft-edge/webview2/>.

```powershell
# 1) Rust — installs the MSVC ABI by default on Windows.
winget install --id Rustlang.Rustup -e
# Restart PowerShell, then:
rustup default stable
rustup target add x86_64-pc-windows-msvc

# 2) Node LTS
winget install --id OpenJS.NodeJS.LTS -e

# 3) Verify
rustc --version
cargo --version
node --version
```

The first time you build, Cargo needs the **Microsoft C++ Build Tools** (for the MSVC linker). If `cargo build` complains about `link.exe`, install them once:

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

---

## Run it (dev mode)

```powershell
# from the kilroy/ folder
npm install
npm run tauri:dev
```

First boot is slow — Cargo downloads ~140 crates, then Vite pre-bundles Monaco and xterm. After that, hot reload is near-instant for frontend changes; Rust changes auto-recompile.

If you see a "permissions denied" error on the file explorer or terminal, check `src-tauri/capabilities/default.json` and add the relevant `core:*` or plugin permission.

## Build a release `.msi` and `.exe`

```powershell
npm run tauri:build
```

Output lands in `src-tauri/target/release/bundle/`:
- `msi/Kilroy_0.1.0_x64_en-US.msi` — installer
- `nsis/Kilroy_0.1.0_x64-setup.exe` — NSIS-style installer
- `target/release/kilroy.exe` — raw binary

---

## What works in this scaffold

| Subsystem             | Status   | Notes                                                                 |
| --------------------- | -------- | --------------------------------------------------------------------- |
| Custom title bar      | ✅       | Drag, min, maximize/restore, close. Matches Windows 11 conventions.   |
| Top menu bar          | ✅       | All items dispatch. Keyboard shortcuts active.                         |
| File Explorer         | ✅       | Open folder via dialog or Ctrl+O. Lazy recursive tree.                |
| Monaco editor         | ✅       | Tabs, dirty indicator, language detection, custom amber theme.        |
| Terminal              | ✅       | Real PTY via `portable-pty`. PowerShell 7 → 5.1 → cmd fallback. **Multi-tab** — `+` to add, X / middle-click to close, double-click label to rename. |
| Agent Mode            | ✅       | Copilot / Autonomous / Multi-Agent / Governance, state persisted.     |
| **Memory DB**         | ✅       | Per-project SQLite + sqlite-vec. Chat, code chunks, decisions, tasks, actions, activity. |
| **Embeddings**        | ✅       | Ollama (`nomic-embed-text` default). Health check on boot.            |
| **Project indexing**  | ✅       | `Index Project` walks, chunks, embeds, stores. Live progress.         |
| **Retrieval**         | ✅       | Every chat turn pulls top-k chunks + decisions. Shown under reply.    |
| **Chat generation**   | ✅       | Streamed `/api/chat` against Ollama in Copilot + Governance.          |
| **Task graph**        | ✅       | Planner (JSON-mode) → executor walks the DAG, live in chat.           |
| **Plan Editor**       | ✅       | Modal to rename / edit / delete / append tasks before execute.        |
| **Actuator**          | ✅       | Parses fenced blocks from task output → reviewable file writes + shells. |
| **Diff Accept/Reject**| ✅       | Unified diff in chat, per-action Accept/Reject. Writes only on accept. |
| **Memory panel**      | ✅       | Tabbed dialog over the project DB (Sessions / Decisions / Files / Tasks). |
| **Decision composer** | ✅       | Modal form to log architectural decisions for retrieval.              |
| **Activity feed**     | ✅       | Modal timeline of every meaningful agent action (session or all).     |
| **Settings panel**    | ✅       | Tabbed dialog: chat / embedding models, sandbox default, timeout, retrieval & chunk knobs. Persists to `settings.json`. |
| **Command Palette**   | ✅       | `Ctrl+Shift+P` fuzzy launcher. Every menu action, recent files, open tabs, terminal sessions. |
| System tray           | ✅       | Show / Hide / Quit. Status pulse hooks ready for runtime wiring.      |
| Window state          | ✅       | Position + size persist across launches.                              |

## Execution sandbox model — cross-platform by default

Kilroy’s sandbox layer is OS-aware from the ground up. The runtime `SandboxKind` enum decides where a shell action runs based on both user choice and host capability:

| Sandbox | Works on | What it does | Status |
| --- | --- | --- | --- |
| **`Host`** | Everywhere | Runs the command in the app’s own shell. Fastest; no isolation. | Implemented across Windows, macOS, Linux |
| **`WindowsSandbox`** | Windows 11 only | Disposable VM, full Win32, no host risk. Disabled on non-Windows at compile time. | Implemented — generates `.wsb`, launches `WindowsSandbox.exe`, captures stdout/stderr/exit |
| **`Docker`** | Anywhere `docker` + daemon | Disposable `debian:stable-slim` container with the project bind-mounted at `/work`. Overridable via `KILROY_DOCKER_IMAGE`. | Implemented |

The default varies by platform: Windows 11 defaults to `WindowsSandbox`; all other OSes default to `Host`. Docker is offered as an explicit cross-platform escape hatch and is also the practical option on non-Windows hosts where stronger isolation is desired.

### Windows Sandbox capture (Windows 11 path)

When the chosen sandbox is `WindowsSandbox`:

1. Kilroy creates `%TEMP%\kilroy-sb-<id>\` on the host.
2. Writes a PowerShell `run.ps1` into it that:
   * `cd`s to the mapped project root,
   * runs the user-approved command,
   * writes `stdout.txt`, `stderr.txt`, `exit.txt`,
   * drops a `done.flag`,
   * calls `shutdown /p /f` so the sandbox closes itself.
3. Writes a `.wsb` config that maps `%TEMP%\kilroy-sb-<id>` to `C:\Users\WDAGUtilityAccount\Desktop\kilroy` (read-write) and the project root to `…\Desktop\work` (read-write).
4. `LogonCommand` runs `powershell -File …\kilroy\run.ps1` on boot.
5. The host polls `done.flag` once per second, up to 5 min.
6. When done, Kilroy reads `stdout.txt`, `stderr.txt`, `exit.txt` and surfaces them on the ActionCard as the result.

Networking, vGPU, audio, clipboard, and printer redirection are disabled by default. To debug, edit `actuator/sandbox.rs::SandboxMount::write_wsb` and re-enable what you need.

### Portability boundary

First-class host-OS detection lives in `src-tauri/src/platform.rs`:

* `Os::detect()` — `std::env::consts::OS`, compile-time baked in.
* `platform_info` Tauri command — surfaces OS, available sandboxes, default sandbox, shell kind, modifier key, path separator.
* The UI uses this to render only valid choices: Windows Sandbox appears only on Windows; modifier hints switch Ctrl/Cmd; shell labels adapt.

Non-Windows ports need no sandbox stub, no broken menu item, and no runtime guard on the default — the abstraction is already total.

## All three previous "last mile" items shipped

* **`WindowsSandbox` execution body** — `actuator/sandbox.rs` now generates a `.wsb`, maps a writable temp dir + the project root, launches `WindowsSandbox.exe`, polls a `done.flag` for completion, reads back `stdout/stderr/exit`, and auto-`shutdown /p /f`s the sandbox when the script finishes. Configurable timeout via `KILROY_SANDBOX_TIMEOUT_SECS` (default 300s).
* **`file_patch` actions** — `diffy` parses + applies unified diffs. The agent emits `` ```diff <path> `` blocks for edits; the actuator parses them as `file_patch` rather than full-file replace.
* **Per-hunk Accept/Reject** — `file_patch` ActionCards split the diff on `@@` markers and render each hunk with a checkbox. On Accept, only the selected hunks are reassembled and sent to the backend as an `override_diff`. The status footer shows `n/total hunks selected`.

## Memory architecture

```
<project root>/
  .kilroy/
    memory.db          ← SQLite + WAL + sqlite-vec extension
```

Open a folder → Kilroy creates `.kilroy/memory.db` and applies migrations. Add it to `.gitignore` (or don't — committing memory makes your repo agent-aware for everyone).

**Tables:**

| Table                | What it holds                                                     |
| -------------------- | ----------------------------------------------------------------- |
| `projects`           | Root path + first/last opened timestamps                          |
| `sessions`           | Conversation sessions per project                                 |
| `messages`           | Every chat turn — including agent reply context as JSON metadata  |
| `files` + `chunks`   | Indexed code, content-hashed, sliding 30/22-line windows          |
| `chunk_embeddings`   | sqlite-vec virtual table — `FLOAT[768]` per chunk                 |
| `tasks`              | Task graph audit log — type, agent, status, retry count           |
| `tool_calls`         | Every tool invocation with args, result, duration                 |
| `decisions`          | Architectural decision log — title, summary, rationale            |
| `decision_embeddings`| sqlite-vec virtual table — `FLOAT[768]` per decision              |
| `activity`           | UI activity feed                                                  |

**Retrieval flow** (what happens when you hit Send):

1. Persist the user message to `messages`.
2. Embed the message via Ollama.
3. KNN over `chunk_embeddings` (k=5) and `decision_embeddings` (k=3).
4. Tail 8 most-recent messages for short-term context.
5. Compose prompt + (eventually) call the LLM.
6. Persist the agent reply with the retrieved context as metadata.
7. Return reply + context blob — the chat panel renders both.

**Ollama setup**

```powershell
# install Ollama on Windows
winget install --id Ollama.Ollama -e

# embedding model (768-dim) — used for retrieval
ollama pull nomic-embed-text

# chat model — used for both Copilot replies and the planner / executor
ollama pull qwen2.5-coder:14b-instruct-q8_0
```

The status bar shows Ollama health: green bolt = up + embedding model installed, yellow = up but missing model, red = unreachable.

## Settings

`Ctrl+,` (or **File → Settings…** or the gear in the status bar) opens a tabbed dialog. Three tabs:

* **Models** — Ollama endpoint URL, chat model, embedding model. Each model field shows a dropdown of currently-installed Ollama tags plus a free-text override. The **Test connection** button hits `/api/tags` and reports which configured models are actually present.
* **Sandbox** — radio for the default sandbox kind (`Windows Sandbox` / `Host` / `Docker`-reserved) and a slider for the Windows Sandbox timeout (30s – 30min).
* **Memory** — k for code-chunk and decision retrieval; window + stride for the indexer. Tells you the overlap so you can tune precision vs. coverage.

Settings persist to `<app config dir>\settings.json`:

* Windows: `%APPDATA%\com.kilroy.desktop\settings.json`
* macOS: `~/Library/Application Support/com.kilroy.desktop/settings.json`
* Linux: `~/.config/com.kilroy.desktop/settings.json`

Env vars (`KILROY_OLLAMA_URL`, `KILROY_CHAT_MODEL`, `KILROY_EMBEDDING_MODEL`, `KILROY_SANDBOX_TIMEOUT_SECS`) seed defaults on first launch only. After that, the JSON file is the single source of truth — edit it directly or use the Settings dialog.

Changes are picked up by the embedder, chat client, and sandbox on the very next call — no restart needed.

## Agent modes — what each one does

| Mode | Behaviour |
| --- | --- |
| **Copilot** | Single-shot streamed reply. You drive, Kilroy advises. Tokens appear in chat as they generate. |
| **Autonomous Engineer** | Planner agent decomposes your goal into 2–6 tasks (JSON), executor walks them sequentially. Each task streams its output into a live card in chat. |
| **Multi-Agent Org** | Same as Autonomous; tasks use role-specific system prompts (planner / architect / developer / qa / reviewer). |
| **Governance** | Audit-only. Single-shot reply with low temperature, structured findings/risks/recommendations. Never proposes edits. |

## Skills — teach Kilroy domain knowledge

Skills are user-authored Markdown files that Kilroy carries as durable context on every chat turn. Use them for:

- **Domain glossaries** ("In this codebase, a *Run* is …")
- **Coding conventions** ("Always use `Result<T, anyhow::Error>` for fallible Rust returns")
- **Library preferences** ("Prefer `pydantic-settings` over `dynaconf`")
- **Decision histories** ("We considered FastAPI vs Litestar; chose FastAPI because …")
- **Anything you'd otherwise re-explain to the agent every time**

### Two scopes

| Scope | Path | When to use |
| --- | --- | --- |
| **Global** | `<app config dir>/skills/*.md` | Apply across all projects (your personal conventions) |
| **Project** | `<project root>/.kilroy/skills/*.md` | Project-specific (commit them with the repo if you want team-wide knowledge) |

Open both from the **Memory → Open … Skills Folder…** menu entries.

### Format

Plain Markdown, one file per skill, named like `python-testing.md`:

```markdown
# Python testing conventions

We use pytest, NOT unittest. Every new test goes in `tests/` mirroring the
source layout. Fixtures live in `conftest.py`. Run with `pytest -xvs` during
debugging, `pytest --cov` in CI.

## Naming
- Test files: `test_<unit>.py`
- Test functions: `test_<scenario>_<expected>`

## Async
Use `pytest-asyncio` with `@pytest.mark.asyncio`. Mark slow tests with
`@pytest.mark.slow` and exclude from default runs.
```

The agent automatically picks the first `# Heading` as the title and the first paragraph as the summary. Skills smaller than 4 KB are **inlined verbatim** into the system prompt; larger skills get a summary stub.

### Total budget

Kilroy injects up to ~16 KB of full-skill content + unlimited summaries on each turn, so a couple dozen small skills are fine. Bigger PDFs of architecture docs should live in the codebase and be indexed — skills are for the rules and conventions that don't change file by file.

## Memory panel

Open via the **Memory** menu. Four tabs:

* **Sessions** — every conversation in this project. Switch active session, start a fresh one.
* **Decisions** — the architectural log. Click "Log Decision" to add one (Title + Summary + Rationale). Decisions are embedded and surfaced to the agent on every turn.
* **Files** — semantic search over the indexed chunks. Type a query, get the top matching chunks + the files they live in. Click a file to open it.
* **Tasks** — recent task graph runs for the current session, with full input/output.

---

## Architecture map

```
src/
├── App.tsx                    root tree: TitleBar / MenuBar / IDELayout / StatusBar
├── main.tsx                   Monaco worker bootstrap, React mount
├── components/
│   ├── layout/                shell chrome
│   │   ├── TitleBar.tsx       Windows 11 title bar (decorations: false)
│   │   ├── MenuBar.tsx        top menu + global shortcuts
│   │   ├── StatusBar.tsx      bottom strip + panel toggles
│   │   └── IDELayout.tsx      nested PanelGroups (the layout brain)
│   ├── explorer/              File Explorer + Agent Mode Selector
│   ├── editor/                Monaco + tabs + watermark
│   ├── terminal/              xterm.js + PTY wiring (multi-tab: TerminalPanel + Tabs + SessionView per tab)
│   ├── chat/                  Agent Chat panel (fixed-width, stationary)
│   ├── common/KilroyMark.tsx  the silhouette logo
│   └── ui/                    vendored shadcn primitives
├── lib/
│   ├── tauri.ts               typed Tauri command wrappers
│   └── utils.ts               cn(), language detection, fmtBytes
├── store/
│   ├── workspace.ts           open folder, open tabs, dirty state
│   ├── ui.ts                  panel collapse + sizes (localStorage-persisted)
│   └── agent.ts               chat history, mode, send()
└── styles/globals.css         theme tokens (deep neutrals + tactical amber)

src-tauri/
├── tauri.conf.json            window config, CSP, bundle settings
├── capabilities/default.json  permission grants
├── icons/                     amber Kilroy mark — placeholder, swap freely
└── src/
    ├── main.rs                entry shim
    ├── lib.rs                 plugins, command registration, tray install
    ├── state.rs               shared state (PTY registry, mode, memory, embedder, chat client)
    ├── tray.rs                system tray icon + menu
    ├── embeddings.rs          Ollama embeddings client (settings-driven)
    ├── generation.rs          Ollama /api/chat streaming + JSON mode for the planner (settings-driven)
    ├── settings.rs            persisted Settings struct + load/save
    ├── runtime/
    │   ├── mod.rs             runtime module
    │   ├── events.rs          run/task event payloads emitted over Tauri
    │   ├── planner.rs         JSON-mode call that decomposes goal → task plan
    │   └── executor.rs        walks plan; emits live events; extracts actions
    ├── actuator/
    │   ├── mod.rs             ActionPayload enum + diff/apply helpers
    │   ├── parser.rs          fenced-block extractor (path-aware)
    │   └── sandbox.rs         Host / WindowsSandbox / Docker dispatcher
    ├── db/
    │   ├── mod.rs             Memory struct, sqlite-vec auto-registration
    │   ├── schema.rs          migrations runner
    │   ├── migrations/        append-only SQL (001_initial, 002_actions_and_activity)
    │   ├── projects.rs        upsert / fetch
    │   ├── sessions.rs        lifecycle
    │   ├── messages.rs        chat persistence
    │   ├── files.rs           file metadata + content-hash dedupe
    │   ├── chunks.rs          chunker + vec insert/KNN
    │   ├── decisions.rs       architectural decisions + KNN
    │   ├── tasks.rs           task graph audit log
    │   ├── actions.rs         pending actuator actions
    │   └── activity.rs        append-only timeline
    └── commands/
        ├── fs.rs              list_dir, read_file, write_file, pick_folder
        ├── terminal.rs        terminal_spawn/write/resize/kill
        ├── agent.rs           plan or single-shot reply (mode-dispatched)
        ├── plan.rs            update / delete / insert / cancel / execute_plan
        ├── actions.rs         list / accept / reject pending actions
        ├── activity.rs        list_activity
        ├── memory.rs          open_project, index_project, search_memory, ...
        ├── settings.rs        get_settings, update_settings, ollama_health
        └── app.rs             app_info
```

---

## Keyboard shortcuts

| Combo            | Action                  |
| ---------------- | ----------------------- |
| `Ctrl+N`         | New untitled file       |
| `Ctrl+O`         | Open folder             |
| `Ctrl+S`         | Save current file (prompts on first save of an Untitled tab) |
| `Ctrl+Shift+S`   | Save all                |
| `Ctrl+W`         | Close current tab       |
| `Ctrl+B`         | Toggle Explorer         |
| `` Ctrl+` ``     | Toggle Terminal         |
| `Ctrl+Shift+I`   | Index Project (memory)  |
| `Ctrl+,`         | Open Settings           |
| `Ctrl+Shift+P`   | Command Palette         |
| `Ctrl+Shift+D`   | Diagnostics panel       |
| `` Ctrl+Shift+` ``| New Terminal tab        |

---

## Tech stack notes

- **Tauri 2 + Rust** — orchestration core, sandbox, deterministic IPC. Single small binary, no Electron bloat.
- **React + TypeScript + Vite** — frontend, with strict TS, path aliases, HMR.
- **Tailwind 4 + shadcn/ui** — config lives in CSS (`@theme` block in `src/styles/globals.css`). No `tailwind.config.ts`.
- **Monaco** — bundled locally so the editor works offline. Workers via Vite's `?worker` syntax.
- **xterm.js + portable-pty** — real terminals. PowerShell 7 preferred, falls back to `powershell.exe` then `cmd.exe`.
- **react-resizable-panels** — the nested-split shell. Imperative panel handles drive the collapse behavior.
- **zustand** — small, no-context state. Multiple per-concern stores.

### Dependency policy

Every entry in `package.json` and `Cargo.toml` is pinned to the version the lockfiles had already resolved — the known-good set this app was built and tested against. `package.json` uses caret ranges (e.g. `^19.2.7`: patch + minor updates flow, majors don't); `Cargo.toml` uses bare versions, which Cargo treats as compatible ranges (e.g. `1.0.228` = `>=1.0.228, <2.0.0`). `Cargo.lock` and `package-lock.json` remain the exact-version source of truth.

Why not `"*"` anymore: fully-unpinned manifests made every fresh checkout a dice roll — any dependency could silently jump a major and break the build with no diff to point at. Pinning to the lockfile-resolved floor keeps fresh installs reproducible while still letting compatible updates through.

**One command to bump everything:**

```powershell
npm run bump            # → npm update + cargo update across the whole tree
npm run bump:check      # → show what's outdated before deciding to bump
```

The lockfiles are the safety net: if a fresh `bump` introduces breakage, `git checkout` the lockfiles and you're back to the working set instantly. To take a NEW major deliberately, raise the pin in the manifest, run `bump`, and commit both manifest and lockfile together.

### Known drift surfaces (what a major bump may eventually surprise you with)

| Area | If it drifts in a breaking way you'll see… | Quick fix |
| --- | --- | --- |
| **Tauri major** | `cargo build` errors on plugin or capability shape | Majors are already pinned out by the compatible ranges; raise the pin deliberately when ready |
| **React forwardRef** | dev-mode console warnings only; runtime still works | Migrate shadcn primitives to ref-as-prop (cosmetic refactor) |
| **Tailwind utility renames** | `shadow-sm`, `outline-none`, `blur-sm` may rename | Tailwind ships a compat layer; if it lapses, run `npx @tailwindcss/upgrade` |
| **Radix component prop renames** | TypeScript errors on the affected primitive's props | Read the migration note in the failing component and rename |
| **Lucide icon removals** | TS error: `… is not exported from "lucide-react"` | Swap to the renamed icon (Lucide keeps a deprecation map) |

Everything else (Vite config shape, `?worker` imports, Monaco bootstrap, xterm scoped packages, zustand `create`, Radix portal pattern, the Tauri 2 IPC client, sqlite-vec auto-extension dance) has been stable across multiple majors and is unlikely to break under `bump`.

---

## Local model setup

Your hardware: **RTX 5070 Laptop (8GB VRAM) + Intel Arc 140T (16GB unified, set to 27GB)**, Vulkan 1.4 across both GPUs, Ollama already serving on the NVIDIA at PID-ish 3752 per your `nvidia-smi` dump.

Kilroy talks to a single local Ollama endpoint, chosen in Settings (it reads `ollama_url` + `chat_model` fresh on every call, so a change takes effect on the next message). There is no cloud routing and no separate router process — just point it at the model you want:
1. Default endpoint → Ollama on `http://localhost:11434`.
2. Any Ollama-compatible chat model — Kilroy is **model-agnostic**. Sensible options across the cost/quality curve:
   - Coder specialists: `qwen2.5-coder:14b-instruct-q8_0`, `deepseek-coder-v2:16b-lite-instruct-q5_K_M`, `codestral:22b`
   - General instruct: `llama3.1:8b-instruct-q5_K_M`, `phi3:14b-medium-128k-instruct-q5_K_M`
   - MoE for headroom on planning/review: `mixtral:8x7b-instruct-q4_K_M`
3. Vulkan path → run `llama.cpp`'s OpenAI-compatible server and set Kilroy's Ollama URL to it, split between RTX and Arc via `--n-gpu-layers` / `--split-mode row`.

The client that drives this lives in `src-tauri/src/generation.rs` (chat + JSON-mode), consumed by `runtime/planner.rs` and `runtime/executor.rs`.

---

## Troubleshooting

- **`link.exe` not found** → install MSVC Build Tools (see Prerequisites).
- **Monaco fails to load** → make sure `npm install` finished cleanly; check the dev-tools console.
- **Terminal opens then closes immediately** → `pwsh.exe` / `powershell.exe` / `cmd.exe` all missing? Add a custom shell in `src-tauri/src/commands/terminal.rs::default_shell()`.
- **WebView2 errors on launch** → install the runtime (link in Prerequisites).
- **Tray icon missing** → low-res placeholder. Drop your real `icon.ico` into `src-tauri/icons/` and rebuild. Or run `npm run icons:regen` to rasterize the source SVG.

---

## Developer mode vs Consumer install

Kilroy ships two distinct distribution stories. Both produce a Windows-native experience; they differ in what the recipient gets and how much setup happens on first launch.

### Developer mode (this repo)

What you have right now. Full source tree. You run `npm run tauri:dev` for hot-reload iteration, or `npm run tauri:build` to produce an installer for personal testing.

- Source: full Rust + TypeScript tree on disk
- Build pipeline: `npm install`, `cargo build`, `tauri build`
- Prerequisites: Rust toolchain, Node, MSVC Build Tools, WebView2 — see `npm run doctor`
- Use case: you (or another developer) iterating on Kilroy itself

### Consumer install (the NSIS `.exe` you ship to buyers)

What `npm run build:release` produces. The output is a single self-contained installer at `src-tauri\target\release\bundle\nsis\Kilroy_<version>_x64-setup.exe`.

What the consumer gets:

- **One `.exe` file**, ~500 MB (binary + bundled Ollama). No source code, no node_modules, no Cargo tree.
- **No prerequisites on their machine.** The installer is self-bootstrapping. WebView2 is handled by Tauri's installer; Ollama is bundled inside the package.
- **First-run setup wizard** opens automatically on first launch, walks them through Ollama detection, chat-model pull, project picker. After they click Finish, the setting flips and the wizard never appears again.
- **Sandboxed agent actions.** Every shell command the agent proposes runs inside Windows Sandbox by default (visible "sandboxed" badge on each action card). File writes/patches gate behind Accept clicks.
- **License screen** in the NSIS installer (drawn from `src-tauri/LICENSE.txt`).

What's NOT in the consumer installer:

- Source code — the Rust binary is opaque machine code; the JS is minified and tree-shaken
- Models — the chat model (~7 GB for `qwen2.5-coder:14b-instruct-q8_0`) is pulled by Ollama on first launch from inside the setup wizard. The user picks which model to pull from a dropdown of options
- Auto-updater — deferred for a future pass (the plumbing for `tauri-plugin-updater` is straightforward to add when you're ready)

### Producing a consumer build

```powershell
# One-time prep on your build machine:
npm install
npm run doctor                  # verify environment

# Each release:
npm run fetch:ollama            # download the bundled Ollama binary (~50 MB)
npm run build:release           # = doctor + fetch:ollama + cache:clear + tauri:build
```

The installer lands in `src-tauri\target\release\bundle\nsis\`. Sign it with an Authenticode cert (optional but recommended — without one, Windows SmartScreen shows a "More info → Run anyway" prompt on first launch) and you can ship it.

### Continuous integration / delivery

Kilroy ships with a GitHub Actions pipeline under `.github/workflows/`:

| Workflow | Trigger | What it does |
|---|---|---|
| `ci.yml` | push to `main`, every PR | **frontend**: `tsc -b` typecheck + Vite build · **rust**: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` · **audit**: `cargo audit` + `npm audit` (advisory) |
| `release.yml` | push a `v*.*.*` tag | Builds the Windows NSIS installer on `windows-latest` via `tauri-action` and attaches it to a **draft** GitHub Release |

The CI Rust job installs the Tauri v2 Linux system libraries and builds the frontend first, because `tauri::generate_context!()` validates that `frontendDist` exists at compile time. `RUSTFLAGS: -D warnings` makes "zero warnings" an enforced contract — this is the gate that catches a non-compiling commit (e.g. a type that's out of scope) before it can ship.

Cut a release:

```bash
git tag v0.1.0
git push origin v0.1.0   # release.yml builds the installer into a draft Release
```

Optional Authenticode signing is wired through the `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets; unset, the build still produces an unsigned installer.

**Local pre-commit gate.** Enable the bundled hook once per clone to run the fast CI checks (rustfmt + tsc) before each commit:

```bash
git config core.hooksPath .githooks
```

> **Note:** `package-lock.json` is committed, so the workflows use `npm ci` for reproducible, faster CI installs.

### Model flexibility

The chat model is **not hardcoded**. The default in `settings.json` is `qwen2.5-coder:14b-instruct-q8_0`, but:

- Any Ollama-installed model works (Llama 3.1, DeepSeek-Coder, Mistral, Phi, CodeLlama, …)
- The Settings dialog has a dropdown populated from `ollama list` — pick anything, Kilroy uses it for the next message
- Override the default via `KILROY_CHAT_MODEL=<tag>` env var on first run, or by editing `%APPDATA%\com.kilroy.desktop\settings.json` directly

The embedding model has a tighter constraint (`embedding_dim` in settings has to match the model's output dim, default 768 for `nomic-embed-text`). Change both fields together.

---

## Script cheatsheet

```powershell
# Dev workflow
npm run tauri:dev               # dev mode, hot reload
npm run tauri:dev:fresh         # clear Vite cache first, then dev mode
npm run tauri:build             # produce NSIS installer (without Ollama bundle)
npm run build:release           # full consumer-ready build: doctor + fetch ollama + cache clear + tauri build

# Maintenance
npm run doctor                  # check every prerequisite, report pass/fail
npm run cache:clear             # purge node_modules/.vite (after vite config changes)
npm run fetch:ollama            # download bundled Ollama binary into src-tauri/resources/ollama/
npm run icons:regen             # regenerate tray icons from src-tauri/icons/icon.svg

# Uninstall variants (after a release build is installed)
npm run wipe                    # uninstall Kilroy + clean settings
npm run wipe:force              # WebView2-ghost-aware uninstall (use when normal uninstall fails)
npm run wipe:dry                # preview what wipe would remove
npm run wipe:nuke               # wipe + delete build artifacts + project .kilroy data
.\wipe.cmd                      # same as wipe:force, no npm needed

# Process management (debugging)
npm run ps:list                 # list every Kilroy / WebView2 process
npm run ps:kill                 # kill every Kilroy / WebView2 process
```

---

## Toast notification system

Every IPC failure that used to live in DevTools console now surfaces as a visible toast card in the bottom-right corner (above the status bar). The console.error still fires for forensics — but users no longer need DevTools open to know something failed.

Use `notify` from `src/store/notifications.ts` instead of bare `console.error`:

```ts
import { notify } from "@/store/notifications";

try {
  await term.write(id, data);
} catch (err) {
  notify.fromError(`term.write[${id.slice(0, 8)}]`, err);
}
```

`notify.fromError(label, err)` is the canonical helper — it does both the console.error AND the toast, so the dev experience and user experience are both correct.

Currently migrated: settings load/save, project open, file open, terminal spawn, index project, list shells, action accept results. Spammy spots (per-keystroke `term.write`, frequent `ollama_health` polls) intentionally stay console-only so they don't drown the toast queue.

---

## Diagnostics panel

Open with `Ctrl+Shift+D` or the Activity-icon button in the status bar (between the `activity` text-button and the settings gear). Shows live runtime health:

- Ollama daemon URL, reachability, list of installed models
- Chat-model + embedding-model installation status
- Active PTY sessions with their IDs and exited-flag
- Open project + active session
- App version + commit (from `app_info`)
- Last 5 errors that fired as toasts (replayable scrollback)

Refreshes every 5 seconds while open. Use this when something feels off — usually faster than opening DevTools.

---

## Updates — two paths

**Path A — Manual rebuild (zero remote contact, default).** Re-run `npm run build:release` from the latest source tree, install the new NSIS .exe. The app never reaches out for updates. Best fit for the local-first stance.

**Path B — Auto-updater (opt-in, available if you want it later).** `tauri-plugin-updater` ships updates by checking a release endpoint you control (typically GitHub Releases) and verifying authenticity with an Ed25519 keypair you generate **locally**. The keypair is a code-signing key, not an API key — the private half stays on your build machine, signs each release, and never leaves; the public half ships embedded in the binary so Kilroy can verify a downloaded update was built by you. No third-party service involved.

The four-step wiring if you ever flip it on:

1. **Generate the signing keypair** (one time, on your build machine):
   ```powershell
   npx @tauri-apps/cli signer generate -w $HOME\.tauri\kilroy_updater_key
   ```
   Outputs a public key (copy into config below) and a private key file (never commit; sign releases with it).

2. **Add the plugin** to `src-tauri/Cargo.toml` (`cargo add tauri-plugin-updater`) and register it in `src-tauri/src/lib.rs`:
   ```rust
   .plugin(tauri_plugin_updater::Builder::new().build())
   ```

3. **Configure the endpoint** in `tauri.conf.json`:
   ```json
   "plugins": {
     "updater": {
       "endpoints": ["https://github.com/moonrox420/kilroy/releases/latest/download/latest.json"],
       "pubkey": "<your-public-key-from-step-1>"
     }
   }
   ```

4. **Each release**: `tauri build` produces a signed installer + a `latest.json` manifest. Upload both to GitHub Releases. Existing installs check the endpoint on launch and offer the update.

This is opt-in and not wired in v0.1 — paste me the public key when you've generated it and I'll add steps 2 and 3 in one patch.

---

## License

TBD — your call when Kilroy ships.
# kilroy
# kilroy
# kilroy
# kilroy
