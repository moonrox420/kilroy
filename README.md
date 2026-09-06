# Kilroy

**High-performance, local-first, editor-grounded AI coding environment & agentic IDE.**  
Built natively with **Tauri 2 + Rust + React 18 + TypeScript + Monaco + Tailwind CSS**.

> **100% Private. Zero Cloud APIs. Zero Remote Latency. Zero Python Runtime Overhead.**  
> Kilroy runs completely local via [Ollama](https://ollama.com) or [llama.cpp](https://github.com/ggerganov/llama.cpp). Your code, prompt history, embeddings, and workspace memory never leave your workstation.

---

```
┌── Kilroy ─────────────────────────────────────────────────────────────────┐
│  TitleBar (custom drag region, native min/max/close, Windows 11 style)    │
│  MenuBar  (File • Edit • View • Go • Terminal • Agent • Memory • Help)    │
│  ┌──────────────────────────────────────────────┬──────────────────────┐  │
│  │  File Explorer   │  Monaco Editor & Diff     │  Agent Chat Panel    │  │
│  │  ─────────────   │  ──────────────────────── │  ──────────────────  │  │
│  │  - Project tree  │  - Active buffer + tabs   │  - Mode: Code Agent  │  │
│  │  - Agent selector│  - Side-by-side visual    │  - @file mentions    │  │
│  │  - Context items │    diff preview           │  - Streamed tokens   │  │
│  │  ────────────────┴────────────────────────── │  - Reviewable action │  │
│  │  Integrated Multi-Tab Terminal (PTY + xterm) │    approval cards    │  │
│  └──────────────────────────────────────────────┴──────────────────────┘  │
│  StatusBar (file info • language • agent mode • Ollama health • diagnostics) │
└───────────────────────────────────────────────────────────────────────────┘
```

---

## Why Kilroy?

Most AI coding tools either trap you in a slow, wandering autonomous loop that burns tokens without editing code, or force you to copy-paste diffs from a chat window into your editor.

Kilroy takes a different approach: **The Editor-Grounded Coder**.

```
[Active Monaco Buffer] + [@file Mentions] + [SQLite Project Vector Memory]
                           │
                           ▼
          [Local Ollama Model (e.g. Qwen 2.5 Coder)]
                           │
                           ▼
      [Aider-Style Fuzzy SEARCH/REPLACE Block Engine]
                           │
                           ▼
         [Monaco Side-by-Side Visual Diff Preview]
                           │
                 User clicks [Accept]
                           │
                           ▼
            [Automated Toolchain Compiler Gate]
        (cargo check, npx tsc, go vet, ruff check)
                           │
                 ┌─────────┴─────────┐
             [Passed]            [Failed]
                 │                   │
                 ▼                   ▼
           [Commit Ready]   [1-Shot Auto-Repair Pass]
```

1. **Context-Grounded on your active editor:** Kilroy automatically attaches your currently open file buffer, language, and cursor position to your request. No need to explain where you are.
2. **Precision `@file` Mentions:** Tag any project file with `@path/to/file` in chat. Kilroy parses and resolves referenced files automatically.
3. **Aider-Style SEARCH/REPLACE Engine:** Edits are generated as surgical search/replace blocks rather than brittle whole-file regenerations. A multi-tier fuzzy matching algorithm (exact match, whitespace-normalized, indentation-tolerant, and anchor-based) eliminates whitespace errors.
4. **Side-by-Side Monaco Diff Review:** Review every proposed change directly inside Monaco's native `<DiffEditor />` before anything touches your disk.
5. **Post-Approval Compiler & Linter Gate:** When you accept a change, Kilroy instantly triggers your project's native toolchain (`cargo check`, `npx tsc --noEmit`, `go vet`, `ruff check`). If compilation fails, Kilroy feeds the diagnostics back into the model for an automated 1-shot auto-repair loop.
6. **Air-Gapped Local AI:** Uses local SQLite + `sqlite-vec` vector storage and local Ollama embeddings (`nomic-embed-text`).

---

## Quick Start (Fool-Proof)

### Option A: Windows 11 One-Liner (PowerShell)

Open PowerShell as Administrator in the repository root and run:

```powershell
.\bootstrap.ps1
```

**What `bootstrap.ps1` does automatically:**
- Verifies and installs missing toolchains via `winget` (Rustup, Node.js LTS, Ollama, MSVC C++ Build Tools).
- Starts the local Ollama daemon and pulls the default coding model (`qwen2.5-coder:14b-instruct-q8_0`) and embedding model (`nomic-embed-text`).
- Installs frontend dependencies via `npm install` and fetches Rust crates via `cargo fetch`.
- Launches Kilroy immediately in hot-reload development mode (`npm run tauri:dev`).

**Useful `bootstrap.ps1` Flags:**
| Flag | Description |
| :--- | :--- |
| `-SkipModels` | Skip automatic Ollama model downloads if you already have models installed |
| `-SkipSandbox` | Skip Windows Sandbox VM feature verification |
| `-ChatModel <tag>` | Pull an alternative Ollama model (e.g. `-ChatModel qwen2.5-coder:7b`) |
| `-NoRun` | Install prerequisites and dependencies without launching the app |
| `-Build` | Build the standalone release NSIS `.exe` installer instead of launching dev mode |

---

### Option B: Manual Setup on Windows 11 (PowerShell)

If you prefer to configure your environment step-by-step:

#### 1. Install Toolchains
```powershell
# Install Rust (MSVC toolchain)
winget install --id Rustlang.Rustup -e
rustup default stable
rustup target add x86_64-pc-windows-msvc

# Install Node.js LTS
winget install --id OpenJS.NodeJS.LTS -e

# Install Ollama
winget install --id Ollama.Ollama -e

# (Optional) If you don't have C++ build tools installed for Cargo:
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

#### 2. Pull Recommended Local Models
```powershell
# Fast, state-of-the-art coding model
ollama pull qwen2.5-coder:14b-instruct-q8_0

# 768-dimensional embedding model (for local vector project memory)
ollama pull nomic-embed-text
```

#### 3. Install Dependencies & Launch
```powershell
# From the repository root
npm install
npm run tauri:dev
```

---

### Option C: macOS Setup (Bash / zsh)

#### 1. Install Prerequisites via Homebrew
```bash
# Install Homebrew if not already installed
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install Rust, Node LTS, and Ollama
brew install rustup-init node ollama
rustup-init -y
source "$HOME/.cargo/env"
```

#### 2. Start Ollama & Pull Models
```bash
# Start Ollama service (if not running in background)
ollama serve &

# Pull models
ollama pull qwen2.5-coder:14b-instruct-q8_0
ollama pull nomic-embed-text
```

#### 3. Install Dependencies & Launch
```bash
npm install
npm run tauri:dev
```

---

### Option D: Linux Setup (Ubuntu / Debian / Arch)

#### 1. Install System Dependencies (WebKitGTK & Build Essentials)
```bash
# Debian / Ubuntu
sudo apt update
sudo apt install -y build-essential curl wget file libssl-dev libgtk-3-dev \
    libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf

# Arch Linux
sudo pacman -S --needed base-devel curl wget openssl gtk3 webkit2gtk-4.1 libappindicator-gtk3 librsvg
```

#### 2. Install Rust, Node.js & Ollama
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Install Node.js LTS (via nvm or package manager)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
source ~/.bashrc
nvm install --lts

# Install Ollama
curl -fsSL https://ollama.com/install.sh | sh
```

#### 3. Pull Models & Launch
```bash
ollama pull qwen2.5-coder:14b-instruct-q8_0
ollama pull nomic-embed-text

npm install
npm run tauri:dev
```

---

## How the Editor-Grounded Coder Works

Kilroy avoids full-file rewriting hallucinations by leveraging **Aider-style SEARCH/REPLACE blocks** integrated directly into the Monaco editor workflow.

### 1. Active Buffer & Cursor Grounding
When you submit a prompt in `Code Agent` mode:
- Kilroy inspects the currently focused Monaco editor tab.
- The active file path, full buffer content, language mode, and cursor position are injected into prompt context.
- Any `@path/to/file` tags in your prompt are resolved against the project root and added as supplementary reference files.

### 2. Surgical SEARCH/REPLACE Blocks
Instead of streaming an entire 800-line file back over the wire, the model outputs targeted modification blocks:

````markdown
<<<<<<< SEARCH
def calculate_metrics(data):
    total = sum(data)
    return total / len(data)
=======
def calculate_metrics(data):
    if not data:
        return 0.0
    total = sum(data)
    return total / len(data)
>>>>>>>
````

### 3. Multi-Tier Fuzzy Match Engine
The Rust actuator (`src-tauri/src/actuator/search_replace.rs`) applies changes using a 4-tier match strategy:
1. **Exact Match**: Direct binary string match.
2. **Whitespace-Normalized Match**: Strips carriage returns (`\r\n` vs `\n`) and trailing whitespace while preserving syntax semantics.
3. **Indentation-Insensitive Match**: Normalizes variable indentation levels emitted by LLMs.
4. **Anchor-Based Fuzzy Matching**: Matches the first and last lines of the search block with Levenshtein-tolerant line scanning for the interior.

### 4. Interactive Monaco Side-by-Side Diff Review
- Changes do not silently overwrite your files.
- Kilroy renders an interactive `<DiffEditor />` inside Monaco, highlighting additions in green and deletions in red side-by-side with your original code.
- Simultaneously, an **ActionCard** appears in the chat stream with **Accept** and **Reject** controls.

### 5. Automated Compiler & Linter Gate
When you click **Accept**:
- The patched file is written to disk.
- Kilroy automatically identifies your project's toolchain:
  - **Rust**: `cargo check --message-format=json`
  - **TypeScript / JavaScript**: `npx tsc --noEmit`
  - **Go**: `go vet ./...`
  - **Python**: `ruff check`
- If compilation succeeds: a green confirmation indicator appears.
- If compilation fails: the exact compiler errors, line numbers, and diagnostics are sent to the model for an **automatic 1-shot repair pass**, generating a new corrective SEARCH/REPLACE diff for your review.

---

## Agent Modes

Select your desired execution mode from the top-left Agent selector or via Command Palette (`Ctrl+Shift+P`):

| Mode | Purpose | Mechanics |
| :--- | :--- | :--- |
| **Code Agent** *(Default)* | High-speed, precision coding | Grounded on active Monaco buffer + `@file` mentions. Generates SEARCH/REPLACE blocks with side-by-side Monaco diff review and compiler verification gate. |
| **Copilot** | Technical brainstorming & advisory | Streamed conversational assistance. Reads your project context and memory without generating intrusive file actions. |
| **Autonomous Engineer** | Complex multi-file features | Decomposes a macro objective into an executable DAG task graph (`runtime/planner.rs`), running tasks sequentially with live logs. |
| **Multi-Agent Org** | Role-specialized workflows | Dispatches distinct system roles (Planner, Software Architect, Developer, QA Engineer, Reviewer) across a structured pipeline. |
| **Governance** | Code auditing & risk assessment | Low-temperature analysis that produces formal structured findings, security risks, and architectural recommendations without editing code. |
| **Council** | Multi-perspective consensus | Evaluates competing engineering trade-offs through multi-agent deliberation. |
| **Debug** | Root-cause analysis | Focuses on stack traces, compiler errors, and regression triage. |
| **Test-First** | TDD engineering loop | Generates failing test specifications first, then develops implementation code until all tests pass. |

---

## Local AI Engine & Recommended Models

Kilroy connects directly to your local Ollama instance on `http://localhost:11434` (or any custom OpenAI-compatible endpoint like `llama.cpp` server).

Configure your endpoint and active models in **Settings** (`Ctrl+,`):

| Hardware / VRAM Tier | Recommended Chat Model | Embedding Model |
| :--- | :--- | :--- |
| **Sweet Spot (8GB – 12GB VRAM)** | `qwen2.5-coder:14b-instruct-q8_0` | `nomic-embed-text` |
| **Lightweight (4GB – 6GB VRAM)** | `qwen2.5-coder:7b-instruct-q8_0` | `nomic-embed-text` |
| **Desktop / Workstation (16GB+ VRAM)** | `qwen2.5-coder:32b-instruct-q4_K_M` or `deepseek-coder-v2:16b-lite` | `nomic-embed-text` |

```powershell
# Fast pull for the sweet-spot setup:
ollama pull qwen2.5-coder:14b-instruct-q8_0
ollama pull nomic-embed-text
```

---

## Execution Sandboxes & Security

When Kilroy runs shell commands or tests, you choose the isolation boundary:

| Sandbox Mode | Platform | Isolation Mechanism |
| :--- | :--- | :--- |
| **`WindowsSandbox`** | Windows 11 Pro/Enterprise | Spawns a disposable Hyper-V VM via `.wsb` config. The original project is isolated; command execution runs against a disposable copy with host networking disabled. |
| **`Docker`** | Cross-platform | Executes commands inside a disposable container (`debian:stable-slim`) with Linux capabilities and network access restricted. |
| **`Host`** | Cross-platform | Runs directly in your local environment. Fastest execution; intended for trusted local workflows. |

---

## Project Memory & Vector Store

Kilroy persists project intelligence locally in SQLite using the `sqlite-vec` vector extension:

```
<project-root>/
  .kilroy/
    memory.db          ← Embedded SQLite + WAL + sqlite-vec
```

- **Content-Hashed Code Chunks:** Slides across your codebase in 30-line windows, indexed with 768-dimensional embeddings.
- **Architectural Decisions Log:** Explicit decisions recorded via the Decision Composer modal are embedded and automatically retrieved during relevant chat turns.
- **Chat & Session History:** Complete local persistence of every prompt, context retrieval, and actuator event.
- **Indexing:** Press `Ctrl+Shift+I` (or click **Memory → Index Project**) to re-index your workspace.

---

## Complete Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl + N` | Create new untitled file |
| `Ctrl + O` | Open project folder |
| `Ctrl + S` | Save active file |
| `Ctrl + Shift + S` | Save all modified files |
| `Ctrl + W` | Close active editor tab |
| `Ctrl + B` | Toggle File Explorer sidebar |
| `` Ctrl + ` `` | Toggle Terminal panel |
| `` Ctrl + Shift + ` `` | Open new Terminal tab |
| `Ctrl + Shift + P` | Open Command Palette |
| `Ctrl + Shift + I` | Index Project into vector memory |
| `Ctrl + Shift + D` | Open Runtime Diagnostics panel |
| `Ctrl + ,` | Open Settings modal |

---

## Codebase Architecture

```
src/                                  # React 18 + TypeScript Frontend
├── components/
│   ├── layout/                       # Windows 11 TitleBar, MenuBar, StatusBar, IDELayout
│   ├── editor/
│   │   ├── MonacoPane.tsx            # Monaco editor & side-by-side <DiffEditor />
│   │   ├── EditorTabs.tsx            # Multi-file tab bar with dirty indicators
│   │   └── Watermark.tsx             # Tactical empty-state background
│   ├── chat/
│   │   ├── ChatPanel.tsx             # Agent chat interface & prompt input
│   │   ├── ActionCard.tsx            # Reviewable Accept/Reject diff & shell cards
│   │   └── ModeSelector.tsx          # Agent mode dropdown
│   ├── terminal/
│   │   └── TerminalPanel.tsx         # Multi-tab xterm.js PTY shell integration
│   ├── explorer/
│   │   └── FileTree.tsx              # Virtualized project file tree
│   └── modals/                       # Settings, Memory, Decisions, Diagnostics
├── lib/
│   ├── tauri.ts                      # Strongly-typed Tauri IPC invocations
│   └── syntaxValidator.ts            # SEARCH/REPLACE block parser & linter
└── store/
    ├── workspace.ts                  # Open tabs, active file, dirty buffers
    ├── agent.ts                      # Chat history, editor-grounded context, actions
    └── ui.ts                         # Layout sizes & panel collapse state

src-tauri/src/                        # Native Rust Backend (Tauri 2)
├── actuator/
│   ├── search_replace.rs             # 4-tier fuzzy SEARCH/REPLACE engine
│   ├── compiler.rs                   # Toolchain detection & automated compiler gate
│   ├── parser.rs                     # Action block extractor
│   └── sandbox.rs                    # Windows Sandbox / Docker / Host dispatcher
├── commands/
│   ├── agent_context.rs              # Active file grounding & @file mention resolution
│   ├── actions.rs                    # Action accept/reject + post-approval compiler check
│   ├── fs.rs                         # Native async file operations
│   ├── terminal.rs                   # portable-pty spawn/read/write/resize
│   └── memory.rs                     # Vector search & project indexing
├── db/                               # SQLite + sqlite-vec database migrations & KNN
├── runtime/
│   ├── coder.rs                      # Editor-Grounded Coder single-pass execution
│   ├── planner.rs                    # DAG task graph decomposition
│   └── executor.rs                   # Autonomous task execution engine
├── generation.rs                     # Ollama streaming chat client & JSON generator
└── platform.rs                       # OS detection & native shell resolution
```

---

## Developer Script Cheatsheet

```powershell
# Development & Testing
npm run tauri:dev               # Start Kilroy in dev mode with hot reload
npm run check                   # TypeScript build + unit tests
npm run check:all               # Full CI verification gate (Rustfmt, Clippy, Cargo tests, Vitest)
powershell scripts/check-all.ps1 # Run the complete test suite locally

# Clean & Maintenance
npm run cache:clear             # Clear Vite and temporary build caches
npm run doctor                  # Inspect local toolchains and report health
npm run icons:regen             # Re-rasterize application icons from SVG

# Production Packaging
npm run tauri:build             # Build local production binary
npm run build:release           # Build consumer-ready NSIS .exe with bundled dependencies
```

---

## Verification & Stability

The Kilroy engine maintains strict automated quality gates:
- **Rust Backend:** 97 unit and integration tests passing (`cargo test`), zero Clippy warnings (`cargo clippy --all-targets -- -D warnings`), formatted with `rustfmt`.
- **Frontend:** Strict TypeScript compilation (`tsc -b`), 13 unit tests passing (`vitest run`), Vite production build verification.
- Run the full verification suite anytime with:
  ```powershell
  npm run check:all
  ```

---

## License

Kilroy is open source software developed with high-performance, local-first engineering principles. Refer to `LICENSE` for details.
# kilroy
# kilroy
# kilroy
# kilroy
# kilroy
