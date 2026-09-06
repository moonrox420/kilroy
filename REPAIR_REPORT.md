# Kilroy repair and verification record

Date: September 3, 2026 (America/Chicago). Scope: this Kilroy checkout only.

Source files were changed and the Windows application was rebuilt at `src-tauri/target/debug/kilroy.exe`. This record separates implemented repairs, automated checks, live checks, and things not demonstrated by those checks.

## Execution and approvals

- The default Code Agent and SmartCoder panel's `ask` command now use the existing native Rust tool runtime. Model-generated Python is no longer automatically executed by those desktop paths before approval. Optional Python dataset/index commands and the standalone Python CLI remain available.
- Native agent investigation uses read-only file/search tools. File and shell proposals are persisted for explicit approval. Model responses cannot manufacture application-owned `file_change` actions.
- Shell commands receive a disposable source copy. Docker and Windows Sandbox never mount the original project. Successful commands produce separate reviewable additions, modifications, and deletions; failed commands import nothing.
- Staging excludes `.git`, `.kilroy`, virtualenvs, dependency/build caches, secret `.env` files, and links. Captured changes have application-computed before-hashes. Applying them checks the current file state, supports binary files/deletions, and rejects stale or protected targets.
- Follow-up approvals preserve their parent run/step linkage in SQLite. Migration tests verify existing action IDs and approval links survive the schema upgrade.
- Host and Docker output capture retains at most 1 MiB per stream while continuing to drain pipes. Windows Sandbox payloads now use PowerShell consistently and preserve Unicode; stdout is redirected rather than buffered in a PowerShell variable.
- Project switching is rejected while foreground agent, index, or approval operations hold a project lease. Failed folder opening no longer clears the previously confirmed frontend project.

Primary files: `src-tauri/src/commands/agent.rs`, `commands/smartcoder.rs`, `commands/actions.rs`, `runtime/agent.rs`, `actuator/staging.rs`, `actuator/sandbox.rs`, `db/migrations/005_staged_file_changes.sql`, `state.rs`.

Host mode remains intentionally unisolated: a staging working directory does not prevent host commands from using absolute paths. The standalone Python CLI's local executor is also not an OS sandbox. The desktop approval guarantees above do not apply to independently invoking that CLI. Interactive user terminals retain normal host privileges.

## Memory, settings, and model transport

- File indexing no longer advances a file hash or discards old chunks before embedding succeeds. Metadata, chunks, and vectors commit together. A failed vector insertion leaves the last valid index intact and retryable.
- Embedding responses must contain exactly one finite 768-dimensional vector per input. Each multi-request operation freezes its model/endpoint configuration.
- Database vector provenance prevents mixing different embedding models or servers. Explicit Index Project rebuilds derived vectors after a model change and re-embeds preserved decision text. Conversations and decision records are retained. Legacy indexes without provenance require this rebuild before semantic retrieval.
- Index fingerprints include chunk window/stride, so tuning chunking actually triggers reindexing. Stride cannot exceed window and leave source lines unindexed.
- Settings validate before saving or activation. Changed embedding configurations are probed before acceptance. Atomic settings writes and concurrent-update checks prevent partial persistence and lost updates. Invalid existing settings files are preserved rather than silently overwritten.
- Model readiness matches exact tags, only normalizing an omitted `:latest`; having a 7B model no longer makes a configured 14B model appear ready.
- Root `AGENTS.md` and file-form `.clinerules` are included as bounded native agent context. Recent conversation context no longer depends on embeddings succeeding. Nested project instructions remain path-scoped and must be read before edits there.
- Ollama streaming now rejects malformed/error/incomplete responses, handles a final line without a newline, preserves Unicode split across network chunks, and bounds accumulated frames/output. Model and endpoint are read from one consistent settings snapshot.
- Reading/listing skills no longer creates `.kilroy` directories in foreign projects. Skill names and resolved file targets are checked for traversal; oversized skill reads/writes are rejected.

Primary files: `src-tauri/src/commands/memory.rs`, `commands/agent_context.rs`, `commands/settings.rs`, `commands/skills.rs`, `db/files.rs`, `db/embedding_profile.rs`, `db/migrations/006_embedding_profile.sql`, `embeddings.rs`, `generation.rs`, `settings.rs`.

## Frontend and startup

- Async event registrations have per-mount ownership and are cleaned up even when registration finishes after unmount. Runtime, council, refactor, actions, and editor subscriptions use the shared tested helper.
- Approval IPC errors do not invent an action status. Old asynchronous action loads cannot repopulate a reset project's store. Approval cards use the same tested store path, including selected patch hunks and response-driven follow-up loading when an event is missed.
- Native Code Agent approvals without a task ID are displayed in chat. Cards distinguish a command finishing in staging from file changes actually being applied.
- Monaco and its workers are local assets and load only when an editor is opened. Settings, diagnostics, and activity panels are also deferred. External Google Fonts requests were removed.
- The inspected production build's initial JavaScript was about 946 KB before compression (main plus small shared chunks); the editor payload was absent from `index.html` module-preload links. The earlier build loaded a roughly 4.2 MB Monaco vendor chunk eagerly. This is a bundle-loading measurement, not a timed native UI launch benchmark. Debug builds are intentionally larger and include source maps.

Primary files: `src/lib/listenerScope.ts`, `src/lib/monacoSetup.ts`, `src/store/actions.ts`, `src/store/runtime.ts`, `src/store/council.ts`, `src/store/refactor.ts`, `src/store/workspace.ts`, `src/components/chat/ActionCard.tsx`, `src/components/layout/IDELayout.tsx`, `src/App.tsx`, `src/main.tsx`, `vite.config.ts`, `index.html`.

The React performance skill informed the lazy-loading boundaries and listener lifecycle repairs. No UI redesign was introduced.

## Dependencies, scripts, and release integrity

- Restored the missing npm lockfile and TypeScript installation without reverting the dependency versions already present in `package.json`.
- Generated hash-locked Windows/Python 3.12 core, development, and optional RAG requirements. Bootstrap and CI install from these locks; the package itself is then installed with `--no-deps`.
- GitHub Actions dependencies are pinned to commit SHAs. CI includes both Rust dependency audits alongside the existing cross-language gate.
- Updated the yanked `chacha20` lockfile entry to 0.10.2.
- Ollama downloads are pinned to an official release asset and SHA-256 in `scripts/ollama-release.json`. Verification occurs before extraction; archive traversal, alternate streams, duplicates, links, entry count, and expanded-size limits are checked. An installed bundle has a per-file hash manifest. A replaced bundle is retained as a recoverable backup.
- Artifact SHA-256 uses .NET directly, avoiding a `Get-FileHash` module-loading failure observed only through nested npm/PowerShell execution. The regression test uses the independent known SHA-256 of `abc`.
- Development reset/cache scripts validate their targets. They no longer kill every Node or WebView process on the machine; only a Kilroy application executable under this checkout is eligible. Rust cleanup targets the Kilroy package, not the entire build cache.
- README setup, sandbox, embedding, and release claims were updated to match these changes.

Primary files: `package-lock.json`, `smartcoder/requirements-*.lock`, `bootstrap.ps1`, `.github/workflows/ci.yml`, `scripts/fetch-ollama.ps1`, `scripts/artifact-integrity.ps1`, `scripts/test-artifact-integrity.ps1`, `scripts/dev-reset.ps1`, `scripts/check-all.ps1`, `README.md`.

## Verification actually run

| Check | Observed result |
| --- | --- |
| `npm run check:all` | Passed application/test typechecks, Vite build, Rust formatting/strict Clippy/tests, Python Ruff/pytest/compileall/pip check, and artifact integrity tests |
| Frontend regression tests | 13 passed |
| Application Rust unit tests | 81 passed |
| SmartCoder Rust-core tests | 2 passed |
| Python tests | 100 passed |
| Live Docker integration | Passed using the existing local `redis:7-alpine` image; successful output staged, originals unchanged; exit 7 imported nothing |
| Live Rust Ollama clients | Passed real streaming chat with installed `qwen2.5-coder-balanced:latest`, plus two 768-dimensional `nomic-embed-text` embeddings; no persisted settings changed |
| Native Windows build | `npx --no-install tauri build --debug --no-bundle` passed and produced `src-tauri/target/debug/kilroy.exe` |
| RustSec audit, both lockfiles | No blocking vulnerability advisories; application graph retains 17 upstream unmaintained/unsound warnings; Rust-core graph had none |
| npm production audit | Registry bulk-advisory requests timed out; no clean npm audit result is claimed |
| Scoped cleanup script | Dry-run verified the cache target; no development processes/build caches were removed by that check |

The two service-dependent Rust tests are excluded from the ordinary unit gate and were explicitly run, not counted as passing merely because they were skipped.

## Reproduce

From PowerShell at this checkout:

```powershell
Set-Location -LiteralPath C:\Users\droxa\kilroy
npm run check:all

$env:KILROY_DOCKER_IMAGE = 'redis:7-alpine' # or your already-installed toolchain image
cargo test --manifest-path src-tauri/Cargo.toml --locked live_docker_changes_are_staged_not_applied -- --ignored --nocapture

$env:KILROY_LIVE_CHAT_MODEL = 'qwen2.5-coder-balanced:latest' # installed local model
cargo test --manifest-path src-tauri/Cargo.toml --locked live_ollama_chat_and_embeddings -- --ignored --nocapture

npx --no-install tauri build --debug --no-bundle
```

## Remaining verification limits and prerequisites

- The configured default `qwen2.5-coder:14b-instruct-q8_0` is not installed on this machine. The live smoke used the installed balanced model with temporary test settings. Select an installed model in Settings or explicitly pull the desired model; this repair did not silently change your model choice or download a large model.
- `WindowsSandbox.exe` is absent here. Script/config generation has tests, but an actual Windows Sandbox VM was not exercised and the Windows feature was not enabled.
- The pinned Ollama archive is approximately 1.47 GB. Its full download and a signed NSIS consumer installer were not produced in this run. The rebuilt artifact is a debug application, not proof of a clean-machine release installation.
- Native UI interaction, a full model-driven coding task with a human approval click, and non-Windows builds were not exercised. The live chat/embedding and Docker tests verify their own paths, not the entire desktop workflow.
- Rust warnings are upstream GTK3/glib/unic/proc-macro-error maintenance and unsoundness notices. They were not hidden by changing audit policy. A framework/dependency migration would need separate compatibility testing.
- Staging capture has file/byte limits; these are not a disk quota on every possible command output. Host execution remains unsafe for untrusted commands. Retain backups/source control and review approvals.
- This directory has no Git repository metadata. No commit, remote CI run, or historical rollback point was fabricated. Existing project data and user model settings were not reset.
