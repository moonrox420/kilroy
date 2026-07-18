/**
 * Typed Tauri command surface.
 *
 * Every IPC call from React goes through this module. The shape of each
 * function matches the Rust `#[tauri::command]` signature so renaming a
 * command on either side will surface as a TS error here first.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ─── filesystem ──────────────────────────────────────────────────────────────

export interface DirEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

export interface FilterSpec {
  name: string;
  extensions: string[];
}

export const fs = {
  listDir: (path: string) => invoke<DirEntry[]>("list_dir", { path }),
  readFile: (path: string) => invoke<string>("read_file", { path }),
  writeFile: (path: string, contents: string) =>
    invoke<void>("write_file", { path, contents }),
  fileExists: (path: string) => invoke<boolean>("file_exists", { path }),
  pickFolder: () => invoke<string | null>("pick_folder"),
  pickSaveFile: (defaultName?: string) =>
    invoke<string | null>("pick_save_file", { defaultName }),
  pickOpenFile: (filters?: FilterSpec[]) =>
    invoke<string | null>("pick_open_file", { filters }),
};

// ─── terminal ────────────────────────────────────────────────────────────────

export interface TerminalSpawned {
  id: string;
  shell_label: string;
}

export interface TerminalAttached {
  chunks: string[];
  exited: boolean;
}

export interface ShellOption {
  id: string;       // "powershell" | "pwsh" | "cmd" | "gitbash" | "wsl" | "wsl:<dist>"
  label: string;    // human-readable
  path: string;     // resolved exe path (empty when not found)
  available: boolean;
}

export const term = {
  spawn: (opts: {
    cwd?: string;
    cols?: number;
    rows?: number;
    shell?: string;
  } = {}) => invoke<TerminalSpawned>("terminal_spawn", opts),
  /**
   * Attach handshake — call AFTER `onData`/`onExit` listeners are registered.
   * Returns the buffered shell output directly (banner, $PROFILE, prompt) so
   * React StrictMode cannot drain it into listeners that get torn down before
   * the surviving mount paints. Live output after attach still streams on
   * `onData` events.
   */
  attach: (id: string) => invoke<TerminalAttached>("terminal_attach", { id }),
  /** Force-flush any pending PTY writes (call right after attach). */
  flush: (id: string) => invoke<void>("terminal_flush", { id }),
  write: (id: string, data: string) =>
    invoke<void>("terminal_write", { id, data }),
  resize: (id: string, cols: number, rows: number) =>
    invoke<void>("terminal_resize", { id, cols, rows }),
  kill: (id: string) => invoke<void>("terminal_kill", { id }),
  listShells: () => invoke<ShellOption[]>("list_available_shells"),
  onData: (id: string, cb: (chunk: string) => void): Promise<UnlistenFn> =>
    listen<string>(`terminal://${id}/data`, (e) => cb(e.payload)),
  onExit: (id: string, cb: () => void): Promise<UnlistenFn> =>
    listen<void>(`terminal://${id}/exit`, () => cb()),
  onBytesExceeded: (id: string, cb: () => void): Promise<UnlistenFn> =>
    listen<void>(`terminal://${id}/bytes_exceeded`, () => cb()),
};

// ─── agent ───────────────────────────────────────────────────────────────────

export type AgentMode =
  | "copilot"
  | "autonomous"
  | "multi_agent"
  | "governance"
  | "council"
  | "debug"
  | "test_first"
  | "code_agent";

// ─── Council / Debug swarm events ────────────────────────────────────────────
//
// Both Council and Debug modes are 4-voice swarms with a synthesizer. They
// share the same event channels (`agent://council/*`) — the frontend
// discriminates them by VOICE ID, since each mode brings its own quartet:
//
//   Council:  velocity / maintainability / security / correctness
//   Debug:    error_reader / recent_changes / hypothesis / fix_author
//
// The store accumulates content keyed by voice ID; the live UI looks up
// the display label and emoji from a unified registry that knows both
// modes' voices.

export type CouncilVoice = "velocity" | "maintainability" | "security" | "correctness";
export type DebugVoice = "error_reader" | "recent_changes" | "hypothesis" | "fix_author";
export type SwarmVoice = CouncilVoice | DebugVoice;

export interface CouncilVoiceChunk {
  voice: SwarmVoice;
  delta: string;
}

export interface CouncilVoiceDone {
  voice: SwarmVoice;
  content: string;
}

export interface CouncilSynthesisChunk {
  delta: string;
}

export interface CouncilDone {
  synthesis: string;
}

export const council = {
  onVoiceChunk: (cb: (c: CouncilVoiceChunk) => void): Promise<UnlistenFn> =>
    listen<CouncilVoiceChunk>("agent://council/voice", (e) => cb(e.payload)),
  onVoiceDone: (cb: (c: CouncilVoiceDone) => void): Promise<UnlistenFn> =>
    listen<CouncilVoiceDone>("agent://council/voice_done", (e) => cb(e.payload)),
  onSynthesis: (cb: (c: CouncilSynthesisChunk) => void): Promise<UnlistenFn> =>
    listen<CouncilSynthesisChunk>("agent://council/synthesis", (e) => cb(e.payload)),
  onDone: (cb: (c: CouncilDone) => void): Promise<UnlistenFn> =>
    listen<CouncilDone>("agent://council/done", (e) => cb(e.payload)),
};

export interface ChunkHit {
  chunk_id: number;
  file_id: number;
  file_path: string;
  start_line: number;
  end_line: number;
  symbol: string | null;
  content: string;
  distance: number;
}

export interface DecisionHit {
  decision_id: number;
  title: string;
  summary: string;
  rationale: string | null;
  created_at: number;
  distance: number;
}

export interface AgentContext {
  chunks: ChunkHit[];
  decisions: DecisionHit[];
  recent_messages: number;
  ollama_used: boolean;
  note: string | null;
}

export interface TaskRow {
  id: number;
  type: string;
  agent: string;
  title: string;
  input: string;
  status: "pending" | "running" | "success" | "failed" | "cancelled";
  output_preview: string;
}

export interface AgentMessage {
  id: string;
  role: "user" | "agent" | "system";
  content: string;
  context: AgentContext;
  run_id: string | null;
  tasks: TaskRow[];
  plan_pending: boolean;
}

// ─── Runtime streaming events ────────────────────────────────────────────────

export interface StreamChunk {
  delta: string;
}

export interface RunStarted {
  run_id: string;
  session_id: number;
  mode: string;
  user_message: string;
}

export interface PlannedTask {
  task_id: number;
  type: string;
  agent: string;
  title: string;
  input: string;
}

export interface PlanReady {
  run_id: string;
  tasks: PlannedTask[];
}

export interface TaskStartedEvent {
  run_id: string;
  task_id: number;
}

export interface TaskChunkEvent {
  run_id: string;
  task_id: number;
  delta: string;
}

export interface TaskCompletedEvent {
  run_id: string;
  task_id: number;
  success: boolean;
  output_preview: string;
}

export interface RunCompletedEvent {
  run_id: string;
  success: boolean;
  summary: string;
}

export const runtime = {
  onStream: (cb: (c: StreamChunk) => void): Promise<UnlistenFn> =>
    listen<StreamChunk>("agent://stream", (e) => cb(e.payload)),
  onRunStarted: (cb: (e: RunStarted) => void): Promise<UnlistenFn> =>
    listen<RunStarted>("agent://run/started", (e) => cb(e.payload)),
  onPlanReady: (cb: (e: PlanReady) => void): Promise<UnlistenFn> =>
    listen<PlanReady>("agent://run/plan_ready", (e) => cb(e.payload)),
  onTaskStarted: (cb: (e: TaskStartedEvent) => void): Promise<UnlistenFn> =>
    listen<TaskStartedEvent>("agent://run/task_started", (e) => cb(e.payload)),
  onTaskChunk: (cb: (e: TaskChunkEvent) => void): Promise<UnlistenFn> =>
    listen<TaskChunkEvent>("agent://run/task_chunk", (e) => cb(e.payload)),
  onTaskCompleted: (cb: (e: TaskCompletedEvent) => void): Promise<UnlistenFn> =>
    listen<TaskCompletedEvent>("agent://run/task_completed", (e) => cb(e.payload)),
  onRunCompleted: (cb: (e: RunCompletedEvent) => void): Promise<UnlistenFn> =>
    listen<RunCompletedEvent>("agent://run/completed", (e) => cb(e.payload)),
};

// ─── Plan editor / executor ─────────────────────────────────────────────────

export interface ExecuteAck {
  run_id: string;
  task_count: number;
}

export const plan = {
  updateTask: (input: {
    task_id: number;
    title?: string;
    input?: string;
  }) => invoke<void>("update_plan_task", { payload: input }),
  deleteTask: (task_id: number) =>
    invoke<void>("delete_plan_task", { taskId: task_id }),
  insertTask: (input: {
    type: string;
    agent: string;
    title: string;
    input: string;
  }) => invoke<number>("insert_plan_task", { payload: input }),
  cancelPlan: (task_ids: number[]) =>
    invoke<void>("cancel_plan", { taskIds: task_ids }),
  executePlan: (run_id: string, task_ids: number[]) =>
    invoke<ExecuteAck>("execute_plan", { payload: { run_id, task_ids } }),
};

// ─── Actuator actions ───────────────────────────────────────────────────────

export type ActionKind = "file_write" | "file_patch" | "shell";
export type ActionStatus =
  | "pending"
  | "accepted"
  | "rejected"
  | "applied"
  | "failed";

export interface ActionView {
  id: number;
  session_id: number | null;
  task_id: number | null;
  kind: ActionKind;
  target: string | null;
  payload: any;
  diff: string | null;
  status: ActionStatus;
  error: string | null;
  created_at: number;
  resolved_at: number | null;
}

export interface ActionProposed {
  run_id: string;
  task_id: number;
  action_id: number;
  kind: ActionKind;
  target: string | null;
  has_diff: boolean;
}

export interface ActionResolved {
  action_id: number;
  status: ActionStatus;
  error: string | null;
}

export interface AcceptInput {
  action_id: number;
  /** For file_patch: subset of hunks the user picked. Omit to apply the whole diff. */
  override_diff?: string | null;
}

export const actions = {
  list: (limit?: number) => invoke<ActionView[]>("list_actions", { limit }),
  listPendingForTask: (task_id: number) =>
    invoke<ActionView[]>("list_pending_actions_for_task", { taskId: task_id }),
  accept: (input: AcceptInput) =>
    invoke<ActionResolved>("accept_action", { payload: input }),
  reject: (action_id: number) =>
    invoke<ActionResolved>("reject_action", { actionId: action_id }),
  onProposed: (cb: (e: ActionProposed) => void): Promise<UnlistenFn> =>
    listen<ActionProposed>("actuator://action_proposed", (e) => cb(e.payload)),
  onResolved: (cb: (e: ActionResolved) => void): Promise<UnlistenFn> =>
    listen<ActionResolved>("actuator://action_resolved", (e) => cb(e.payload)),
};

// ─── Activity feed ──────────────────────────────────────────────────────────

export interface ActivityView {
  id: number;
  session_id: number | null;
  kind: string;
  payload: any;
  created_at: number;
}

export const activity = {
  list: (opts?: { session_only?: boolean; limit?: number }) =>
    invoke<ActivityView[]>("list_activity", {
      sessionOnly: opts?.session_only,
      limit: opts?.limit,
    }),
};

// ─── Settings ───────────────────────────────────────────────────────────────

export type SandboxDefault = "host" | "windows_sandbox" | "docker";

export interface SettingsView {
  ollama_url: string;
  chat_model: string;
  embedding_model: string;
  default_sandbox: SandboxDefault;
  sandbox_timeout_secs: number;
  retrieval_chunks_k: number;
  retrieval_decisions_k: number;
  chunk_window: number;
  chunk_stride: number;
  embedding_dim: number;
  /** True until the first-run wizard completes. Drives the onboarding
   *  modal. Flipped to false by the wizard's Finish button. */
  first_run: boolean;
}

export interface SettingsPatch {
  ollama_url?: string;
  chat_model?: string;
  embedding_model?: string;
  default_sandbox?: SandboxDefault;
  sandbox_timeout_secs?: number;
  retrieval_chunks_k?: number;
  retrieval_decisions_k?: number;
  chunk_window?: number;
  chunk_stride?: number;
  first_run?: boolean;
}

export interface OllamaHealthFull {
  reachable: boolean;
  models: string[];
  chat_model: string;
  embedding_model: string;
  has_chat_model: boolean;
  has_embedding_model: boolean;
  error: string | null;
}

export const settings = {
  get: () => invoke<SettingsView>("get_settings"),
  update: (patch: SettingsPatch) =>
    invoke<SettingsView>("update_settings", { payload: patch }),
  ollamaHealth: () => invoke<OllamaHealthFull>("ollama_health"),
};

export interface AgentStatus {
  mode: AgentMode;
  active_agents: number;
  queued_tasks: number;
  model: string;
  ready: boolean;
}

export const agent = {
  /**
   * Send a chat turn. `images` are raw base64 strings (no
   * `data:image/...;base64,` prefix — Ollama rejects the data-URL
   * form). Vision-capable chat models (LLaVA, bakllava, llava-phi3,
   * llama3.2-vision, qwen2-vl, moondream, etc.) will read them;
   * non-vision models ignore them.
   */
  send: (message: string, images?: string[]) =>
    invoke<AgentMessage>("agent_send_message", {
      payload: { message, images },
    }),
  setMode: (mode: AgentMode) => invoke<void>("agent_set_mode", { mode }),
  status: () => invoke<AgentStatus>("agent_get_status"),
};

// ─── distillation corpus ─────────────────────────────────────────────────────

export interface CorpusAppendInput {
  user_message: string;
  agent_message: string;
  system_prompt?: string;
  tag?: string;
}

export interface CorpusStats {
  path: string;
  exists: boolean;
  count: number;
  size_bytes: number;
  /** Backend's "you have enough to train" threshold. UI compares
   *  `count >= train_threshold` to decide whether to surface the
   *  "Train a custom model" banner. */
  train_threshold: number;
}

export const corpus = {
  append: (input: CorpusAppendInput) =>
    invoke<CorpusStats>("corpus_append", { payload: input }),
  stats: () => invoke<CorpusStats>("corpus_stats"),
  openFolder: () => invoke<string>("corpus_open_folder"),
};

// ─── refactor (background-refactor swarm) ────────────────────────────────────

export interface RefactorCandidate {
  path: string;
  rel_path: string;
  size_bytes: number;
  loc: number;
  score: number;
  reason: string;
}

export type RefactorRisk = "low" | "medium" | "high";

export interface RefactorProposal {
  id: number;
  file_path: string;
  title: string;
  rationale: string;
  voice: string;
  impact_score: number;
  risk: RefactorRisk;
  diff: string;
  verification_status: "untested" | "verified_pass" | "verified_fail";
  verification_output: string | null;
  status: "pending" | "applied" | "dismissed";
  scan_run_id: string;
  created_at: number;
}

export interface RefactorScanStats {
  pending: number;
  applied: number;
  dismissed: number;
  last_scan_unix: number | null;
}

/** Voice IDs the refactor swarm emits over `agent://refactor/*`. */
export type RefactorVoice =
  | "duplicate"
  | "complexity"
  | "error_handling"
  | "modernizer";

export interface RefactorVoiceChunk {
  voice: RefactorVoice;
  delta: string;
}

export interface RefactorVoiceDone {
  voice: RefactorVoice;
  content: string;
}

export interface RefactorSynthesisChunk {
  delta: string;
}

export interface RefactorScanDone {
  scan_run_id: string;
  file_path: string;
  proposal: RefactorProposal | null;
}

export const refactor = {
  /** Heuristic file ranking — fast, no LLM. Returns top N candidates
   *  sorted by score descending. */
  scanCandidates: (limit?: number) =>
    invoke<RefactorCandidate[]>("refactor_scan_candidates", { limit }),
  /** Run the 4-voice refactor swarm on a single file. Resolves with
   *  the synthesized proposal (or null if "no proposal" was the
   *  honest outcome). Live progress flows over the `onVoice*` /
   *  `onSynthesis` event channels. */
  analyzeFile: (input: { file_path: string; scan_run_id?: string }) =>
    invoke<RefactorProposal | null>("refactor_analyze_file", { payload: input }),
  listProposals: (opts?: {
    include_dismissed?: boolean;
    include_applied?: boolean;
    limit?: number;
  }) =>
    invoke<RefactorProposal[]>("refactor_list_proposals", {
      includeDismissed: opts?.include_dismissed,
      includeApplied: opts?.include_applied,
      limit: opts?.limit,
    }),
  dismiss: (id: number) => invoke<void>("refactor_dismiss_proposal", { id }),
  /** Apply a proposal — routes the diff into the actuator as a pending
   *  file_patch action. Returns the new action_id. The user still has
   *  to Accept the action in the actuator UI for it to land on disk. */
  apply: (id: number) => invoke<number>("refactor_apply_proposal", { id }),
  stats: () => invoke<RefactorScanStats>("refactor_scan_run_stats"),
  onVoiceChunk: (cb: (c: RefactorVoiceChunk) => void): Promise<UnlistenFn> =>
    listen<RefactorVoiceChunk>("agent://refactor/voice", (e) => cb(e.payload)),
  onVoiceDone: (cb: (c: RefactorVoiceDone) => void): Promise<UnlistenFn> =>
    listen<RefactorVoiceDone>("agent://refactor/voice_done", (e) => cb(e.payload)),
  onSynthesis: (cb: (c: RefactorSynthesisChunk) => void): Promise<UnlistenFn> =>
    listen<RefactorSynthesisChunk>("agent://refactor/synthesis", (e) =>
      cb(e.payload),
    ),
  onScanDone: (cb: (c: RefactorScanDone) => void): Promise<UnlistenFn> =>
    listen<RefactorScanDone>("refactor://scan_done", (e) => cb(e.payload)),
};

// ─── memory ──────────────────────────────────────────────────────────────────

export interface Project {
  id: number;
  name: string;
  root_path: string;
  created_at: number;
  last_opened_at: number;
}

export interface Session {
  id: number;
  project_id: number;
  title: string | null;
  agent_mode: string;
  started_at: number;
  ended_at: number | null;
}

export interface StoredMessage {
  id: number;
  session_id: number;
  role: "user" | "agent" | "system" | "tool";
  content: string;
  metadata: string | null;
  parent_id: number | null;
  created_at: number;
}

export interface OllamaStatus {
  reachable: boolean;
  models: string[];
  embedding_model: string;
  has_embedding_model: boolean;
}

export interface SessionSwitched {
  session: Session;
  messages: StoredMessage[];
}

export interface ProjectOpened {
  project: Project;
  session: Session;
  messages: StoredMessage[];
  ollama_status: OllamaStatus;
}

export interface IndexProgress {
  phase: "walking" | "indexing" | "done";
  current: number;
  total: number;
  message: string;
}

export interface IndexResult {
  files_seen: number;
  files_indexed: number;
  chunks_inserted: number;
  skipped_too_large: number;
  skipped_binary: number;
  errors: number;
  duration_ms: number;
}

export interface ProjectIndexStatus {
  files_indexed: number;
  chunks_indexed: number;
  is_indexed: boolean;
}

export interface ClearIndexResult {
  files_removed: number;
  chunks_removed: number;
}

export interface SearchResult {
  chunks: ChunkHit[];
  decisions: DecisionHit[];
}

export interface Decision {
  id: number;
  project_id: number;
  title: string;
  summary: string;
  rationale: string | null;
  related_files: string | null;
  created_at: number;
}

export interface TaskRecord {
  id: number;
  session_id: number | null;
  parent_id: number | null;
  type: string;
  agent: string;
  status: "pending" | "running" | "success" | "failed" | "cancelled";
  input: string;
  output: string | null;
  retry_count: number;
  started_at: number | null;
  completed_at: number | null;
  created_at: number;
}

export const memory = {
  openProject: (path: string) =>
    invoke<ProjectOpened>("open_project", { path }),
  saveMessage: (
    role: "user" | "agent" | "system",
    content: string,
    metadata?: string,
  ) => invoke<StoredMessage>("save_message", { role, content, metadata }),
  listSessions: (limit?: number) =>
    invoke<Session[]>("list_sessions", { limit }),
  startSession: () => invoke<Session>("start_session"),
  switchSession: (sessionId: number) =>
    invoke<SessionSwitched>("switch_session", { sessionId }),
  indexProject: () => invoke<IndexResult>("index_project"),
  projectIndexStatus: () => invoke<ProjectIndexStatus>("project_index_status"),
  clearProjectIndex: () => invoke<ClearIndexResult>("clear_project_index"),
  searchMemory: (query: string, k?: number) =>
    invoke<SearchResult>("search_memory", { query, k }),
  logDecision: (input: {
    title: string;
    summary: string;
    rationale?: string;
    related_files?: string[];
  }) => invoke<number>("log_decision", { payload: input }),
  listDecisions: (limit?: number) =>
    invoke<Decision[]>("list_decisions", { limit }),
  listTasks: (limit?: number) =>
    invoke<TaskRecord[]>("list_tasks", { limit }),
  onIndexProgress: (cb: (p: IndexProgress) => void): Promise<UnlistenFn> =>
    listen<IndexProgress>("memory://index/progress", (e) => cb(e.payload)),
};

// ─── models (pull etc.) ──────────────────────────────────────────────────────

export interface PullProgress {
  tag: string;
  /** "starting" | "pulling manifest" | "downloading" | "verifying sha256 digest"
   *  | "writing manifest" | "success" | "error" | "complete" */
  status: string;
  /** Bytes downloaded for current layer. */
  completed: number;
  /** Bytes total for current layer (0 if unknown yet). */
  total: number;
  digest: string | null;
  error: string | null;
  done: boolean;
}

export const models = {
  /** Pull a model. Resolves when the pull finishes (success or error).
   *  Listen to `models.onPullProgress` for live status. */
  pull: (tag: string) => invoke<void>("pull_model", { tag }),
  onPullProgress: (cb: (p: PullProgress) => void): Promise<UnlistenFn> =>
    listen<PullProgress>("ollama://pull/progress", (e) => cb(e.payload)),
};

// ─── datasets (training-data ingestion + Modelfile composition) ──────────────

export type DatasetFormat =
  | "Alpaca"
  | "ShareGpt"
  | "OpenAiChat"
  | "PromptCompletion"
  | "Unknown";

export interface DatasetInspect {
  path: string;
  container: string;
  format: DatasetFormat;
  record_count: number;
  sampled_count: number;
  size_bytes: number;
  samples: string[];
  avg_input_chars: number;
  avg_output_chars: number;
  notes: string[];
}

export interface CreateModelInput {
  name: string;
  base: string;
  dataset_path: string;
  extra_system?: string;
  temperature?: number;
}

export interface ModelfileBuilt {
  name: string;
  modelfile_path: string;
  system_prompt: string;
  note: string;
}

export interface TrainingEnv {
  python_available: boolean;
  python_version: string | null;
  unsloth_installed: boolean;
  transformers_installed: boolean;
  gpu_visible: boolean;
  hint: string;
}

export interface CreateProgress {
  name: string;
  status: string;
  error: string | null;
  done: boolean;
}

export const datasets = {
  inspect: (path: string) => invoke<DatasetInspect>("dataset_inspect", { path }),
  createModelfile: (payload: CreateModelInput) =>
    invoke<ModelfileBuilt>("dataset_create_modelfile", { payload }),
  trainingEnvStatus: () => invoke<TrainingEnv>("training_env_status"),
  onCreateProgress: (cb: (p: CreateProgress) => void): Promise<UnlistenFn> =>
    listen<CreateProgress>("ollama://create/progress", (e) => cb(e.payload)),
};

// ─── skills ──────────────────────────────────────────────────────────────────

export interface Skill {
  name: string;
  title: string;
  summary: string;
  path: string;
  scope: "global" | "project";
  size_bytes: number;
  inline_eligible: boolean;
}

export const skills = {
  list: () => invoke<Skill[]>("list_skills"),
  read: (name: string, scope?: "global" | "project") =>
    invoke<string>("read_skill", { name, scope }),
  openFolder: (scope?: "global" | "project") =>
    invoke<string>("open_skills_folder", { scope }),
  /**
   * Create or overwrite a skill file.
   * - `name` must be a slug: letters, digits, '-' or '_' only.
   * - `scope` "global" writes to <app config>/skills/, "project" writes to
   *   <root>/.kilroy/skills/.
   * - `content` is the raw Markdown — first `# Heading` becomes the title,
   *   first paragraph after the title becomes the summary the model sees.
   * Returns the absolute path on disk.
   */
  write: (input: {
    name: string;
    scope: "global" | "project";
    content: string;
  }) => invoke<string>("write_skill", input),
};

// ─── app ─────────────────────────────────────────────────────────────────────

export interface AppInfo {
  name: string;
  version: string;
  commit: string | null;
}

export const app = {
  info: () => invoke<AppInfo>("app_info"),
};

// ─── platform (OS smart detector) ─────────────────────────────────────────────

export type OsId = "windows" | "macos" | "linux" | "other";

export interface PlatformInfo {
  os: OsId;
  arch: string;
  family: string; // "windows" | "unix"
  is_windows: boolean;
  is_macos: boolean;
  is_linux: boolean;
  /** Sandbox kinds selectable on THIS host (Windows Sandbox is Windows-only). */
  available_sandboxes: SandboxDefault[];
  default_sandbox: SandboxDefault;
  shell_kind: "windows" | "unix";
  /** "Cmd" on macOS, otherwise "Ctrl" — for keyboard-hint text. */
  modifier_key: string;
  path_sep: string;
}

export const platform = {
  info: () => invoke<PlatformInfo>("platform_info"),
};
