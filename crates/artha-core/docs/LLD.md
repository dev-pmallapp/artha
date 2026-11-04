# artha-core: Low-Level Design

This is the companion to `HLD.md`. It describes code that does not exist
yet: `src/` holds only a placeholder `lib.rs`, and none of the files
below have been created. External signatures are taken from
`artha-llm/docs/LLD.md` and `artha-data/docs/LLD.md`. Where this crate
needs something those docs don't provide, it is called out as an
**open item**.

Path conventions in code blocks:
- `llm::` is `artha_llm`.
- `data::` is `artha_data`.

## 1. Module layout

```
crates/artha-core/
├── Cargo.toml
├── src/
│   ├── lib.rs                pub mods, flat re-exports, crate docs; #![deny(unsafe_code)]
│   ├── error.rs              CoreError, Result
│   ├── ids.rs                TurnId, RoundIndex, CallId
│   ├── config.rs             AgentConfig, LoopLimits, DispatchConfig, BackgroundFollowup, PermissionPolicyConfig
│   ├── agent.rs              Agent, AgentBuilder, Agent::open / list_sessions / run_turn_headless
│   ├── handle.rs             SessionHandle, Command, UserInput, EventReceiver (Stream impl)
│   ├── actor.rs              SessionActor: command loop, turn queue, background task set
│   ├── event.rs              CoreEvent and payloads (TurnOutcome, ToolCallView, ToolOutcome, Notice, …), EventTx
│   ├── prompt.rs             SystemPromptBuilder, PromptContributor, PromptInputs
│   ├── turn/
│   │   ├── mod.rs            TurnDriver::run: the state machine (§5)
│   │   ├── state.rs          TurnState, StepResult, transition helpers (pure)
│   │   ├── round.rs          RoundConsumer: drive one EventStream, forward deltas, collect ToolCallEnd
│   │   ├── stop.rs           classify(StopReason, &Message) -> NextStep (pure)
│   │   ├── retry.rs          RoundRetryPolicy (mid-stream), backoff (pure)
│   │   └── compact.rs        RequestView, Compactor (elide old tool-result bodies)
│   ├── tool/
│   │   ├── mod.rs            Tool trait, ToolTraits, ToolRisk, ConfirmOverride, ToolPlan, PlanContext
│   │   ├── output.rs         ToolOutput, ToolDisplay, ToolError, BackgroundTicket, into_tool_result(), truncate
│   │   ├── context.rs        ToolContext, EditRecorder, ProgressSink, BackgroundSpawner
│   │   ├── registry.rs       ToolRegistry, ToolSet (Arc snapshot, sorted specs)
│   │   └── typed.rs          cfg(feature="schemars"): TypedTool trait + blanket adapter
│   ├── dispatch/
│   │   ├── mod.rs            Dispatcher::run_round: resolve → plan → gate → checkpoint → exec → results
│   │   ├── permission.rs     PermissionPolicy, PolicyVerdict, PermissionPrompt, PermissionResponder,
│   │   │                     PermissionDecision, Grants
│   │   ├── exec.rs           spawn + timeout + panic capture + cancel; EagerSet
│   │   └── guard.rs          RepeatErrorTracker
│   ├── session/
│   │   ├── mod.rs            Session, SessionMeta, EntryMeta, UsageTotals
│   │   ├── persist.rs        PersistedMessage, MessageMeta, role mapping, Persister (spawn_blocking wrapper)
│   │   ├── resume.rs         SessionSelector, load(), repair()
│   │   └── title.rs          derive_title()
│   ├── checkpoint.rs         CheckpointManager (pre/post record + links), RewindPlan, rewind()
│   ├── background.rs         BackgroundTasks (JoinSet + session token), Notice routing, notice → text block
│   └── testing.rs            cfg(test | feature="test-util"): FakeTool, ToolScript, EventLog, Harness
└── tests/
    ├── loop_basic.rs         text turn, single tool round trip, parallel calls, PauseTurn, round limit
    ├── loop_errors.rs        tool errors/timeouts/panics, malformed args, unknown tool, llm errors, stream retry
    ├── cancel.rs             cancel at every event index; history validity property
    ├── permission.rs         policy matrix, prompt ordering, deny/feedback/stop, dropped responder
    ├── persistence.rs        write-ahead order, resume, repair, --continue/--resume selection
    ├── checkpoint.rs         pre/post records, fail-closed, rewind per-path algorithm (TestEnv)
    ├── background.rs         tickets, notices, next-turn injection, AutoTurn
    └── snapshots/            insta CoreEvent sequences
```

Cargo features:
- `default = []`
- `schemars`: enables `dep:schemars` and `tool/typed.rs`.
- `test-util`: exposes `testing`, and enables `artha-llm/test-util` and
  `artha-data/test-util`.

Planned `Cargo.toml` (not written yet, versions to verify):

```toml
[dependencies]
artha-llm    = { path = "../artha-llm" }
artha-data   = { path = "../artha-data" }
async-trait  = { workspace = true }
futures-util = { version = "0.3", default-features = false, features = ["std"] }
schemars     = { version = "1", optional = true }
serde        = { workspace = true }
serde_json   = { workspace = true }
thiserror    = { workspace = true }
tokio        = { workspace = true }        # rt, sync, time, macros used
tokio-util   = { version = "0.7", features = ["rt"] }
tracing      = { workspace = true }

[dev-dependencies]
artha-llm  = { path = "../artha-llm", features = ["test-util"] }
artha-data = { path = "../artha-data", features = ["test-util"] }
insta      = { version = "1", features = ["json"] }
tokio      = { workspace = true }          # + test-util for paused time
```

## 2. Errors and ids (`error.rs`, `ids.rs`)

```rust
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    #[error(transparent)]
    Llm(#[from] llm::LlmError),
    #[error(transparent)]
    Data(#[from] data::DataError),
    #[error("session {0} not found")]
    SessionNotFound(data::SessionId),
    #[error("stored message {seq} in session {session} could not be decoded: {reason}")]
    CorruptHistory { session: data::SessionId, seq: data::MessageSeq, reason: String },
    #[error("invalid tool registration: {0}")]
    ToolRegistration(String),
    #[error("nothing to retry: the last turn completed")]
    NothingToRetry,
    #[error("rewind target {0} is not a user message")]
    BadRewindTarget(data::MessageSeq),
    #[error("file rewind unavailable: {0}")]
    RewindUnavailable(String),
    #[error("invalid configuration: {0}")]
    Config(String),
    /// The session actor has shut down (closed or panicked).
    #[error("session is closed")]
    Closed,
    #[error("background task join failed: {0}")]
    TaskJoin(String),
}

impl CoreError {
    /// Convenience for UIs: `Some(kind)` when the root cause is an LLM error.
    pub fn llm_kind(&self) -> Option<llm::ErrorKind>;
    pub fn is_busy(&self) -> bool;  // Data(e) if e.is_busy()
}
```

```rust
/// Per-session, strictly increasing, starting at 1. Not persisted; it only correlates events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TurnId(pub u64);

/// 0-based index of a `chat` call within a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RoundIndex(pub u32);

/// The model's tool_use id, verbatim (artha-llm guarantees non-empty, synthesizing if needed).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallId(pub String);
```

## 3. The tool contract (`tool/`)

### 3.1 Trait and metadata (`tool/mod.rs`)

```rust
#[async_trait::async_trait]
pub trait Tool: Send + Sync + 'static {
    /// Name (validated by ToolSpec::validate), description, JSON Schema (root type "object").
    /// Must be stable for the tool's lifetime, since it is cached in the registry.
    fn spec(&self) -> &llm::ToolSpec;

    /// Static policy metadata. Defaults: ReadOnly, concurrency_safe = true, timeout = 120 s, confirm = Default.
    fn traits(&self) -> ToolTraits { ToolTraits::default() }

    /// Cheap, synchronous, no I/O beyond stat-ing paths. Runs before the permission decision.
    /// Errors here are InvalidInput-type (e.g. a missing required field) and skip execution.
    fn plan(&self, input: &serde_json::Value, ctx: &PlanContext<'_>) -> Result<ToolPlan, ToolError> {
        let _ = (input, ctx);
        Ok(ToolPlan::default())
    }

    /// Performs the call. `input` is the model's arguments (always a JSON object).
    /// Must be cancel-safe at await points: core drops the future on cancel or timeout.
    /// Must not block the reactor: use spawn_blocking or Db::call for sync work.
    async fn execute(&self, input: serde_json::Value, ctx: ToolContext) -> Result<ToolOutput, ToolError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolTraits {
    pub risk: ToolRisk,
    /// May run in parallel with other concurrency-safe calls in the same round.
    pub concurrency_safe: bool,
    /// None = no deadline (discouraged; use the background pattern instead).
    pub timeout: Option<Duration>,
    pub confirm: ConfirmOverride,
}
impl Default for ToolTraits { /* ReadOnly, true, Some(120s), ConfirmOverride::Default */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    /// No persistent side effects (quotes, fundamentals, news, reading local files).
    ReadOnly,
    /// Writes Artha's own state: memory, watchlists, paper-trading ledger, monitors, jobs.
    LocalWrite,
    /// Writes user files under the workdir. Snapshotted via checkpoints.
    FileWrite,
    /// Side effects outside this machine (sending messages, real brokerage actions, unknown MCP tools).
    External,
    /// Runs code or subprocesses (a backtest in the Python sidecar, a shell command).
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmOverride {
    #[default] Default,
    /// Always prompt in Ask mode, even for ReadOnly (e.g. a tool that reads secrets).
    Always,
    /// Never prompt in Ask mode (e.g. memory_save). Skip mode and NonInteractive lists still apply.
    Never,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolPlan {
    /// One line for the permission prompt and the ToolStarted event. None = "{name}({compact args})".
    pub summary: Option<String>,
    /// Files this call will create, modify, or delete. Absolute or workdir-relative.
    /// Snapshotted (PreEdit) before execute. Non-empty implies risk >= FileWrite for policy purposes.
    pub writes: Vec<PathBuf>,
    /// Optional extra detail for the prompt (e.g. a unified diff preview). Not sent to the model.
    pub preview: Option<ToolDisplay>,
}

pub struct PlanContext<'a> {
    pub workdir: &'a Path,
    pub session_id: Option<data::SessionId>,
}
```

**MCP mapping (implemented in `artha-mcp`, specified here):**

| MCP tool field | `ToolTraits` |
|---|---|
| `annotations.readOnlyHint == true` | `risk: ReadOnly` |
| `annotations.destructiveHint == true` | `risk: External`, `confirm: Always` |
| no annotations | `risk: External` |
| `concurrency_safe` | `readOnlyHint` |
| `timeout` | from plugin config, else 120 s |

### 3.2 Output and errors (`tool/output.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ToolOutput {
    /// Sent to the model. Prefer Json for structured data.
    pub content: Vec<llm::ToolResultContent>,
    /// UI-only rendering (never sent to the model, never persisted).
    pub display: Option<ToolDisplay>,
    /// Set when the tool started work that will finish later (§10).
    pub background: Option<BackgroundTicket>,
}
impl ToolOutput {
    pub fn text(s: impl Into<String>) -> Self;
    pub fn json(v: serde_json::Value) -> Self;
    pub fn with_display(self, d: ToolDisplay) -> Self;
    pub fn with_background(self, t: BackgroundTicket) -> Self;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolDisplay {
    Markdown { text: String },
    Table { columns: Vec<String>, rows: Vec<Vec<String>> },
    Diff { path: PathBuf, unified: String },
    Code { language: Option<String>, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTicket {
    pub id: String,          // e.g. "bg-3" (in-process) or a jobs_job UUID (durable)
    pub label: String,       // "Backtest SMA-cross on SPY 2010–2024"
    pub durable: bool,       // true = artha-jobs owns it and it survives exit
}

#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ToolError {
    /// The model's fault: wrong or missing arguments. The message is shown to the model.
    #[error("invalid arguments: {0}")]
    InvalidInput(String),
    /// The tool ran and failed (data source down, symbol not found, sidecar crashed).
    #[error("{message}")]
    Failed { message: String, retryable: bool },
    /// Produced by core only.
    #[error("denied by user")]
    Denied { feedback: Option<String> },
    /// Produced by core, or by a tool that observed ctx.cancel.
    #[error("cancelled")]
    Cancelled,
    /// Produced by core only.
    #[error("timed out after {0:?}")]
    TimedOut(Duration),
    /// Produced by core only: JoinError::is_panic.
    #[error("tool crashed: {0}")]
    Panicked(String),
    /// Produced by core only: the pre-edit checkpoint failed (fail-closed).
    #[error("could not snapshot {path} before editing: {reason}")]
    CheckpointFailed { path: PathBuf, reason: String },
}
impl ToolError {
    pub fn failed(msg: impl Into<String>) -> Self;     // retryable: false
    pub fn transient(msg: impl Into<String>) -> Self;  // retryable: true (the hint is shown to the model)
}
```

Serialization into the next request (`into_tool_result`):

```rust
pub(crate) fn into_tool_result(
    call: &CallId,
    outcome: &Result<ToolOutput, ToolError>,
    max_bytes: usize,
    repeat_note: Option<&str>,
) -> llm::ToolResult;
```

| Outcome | `ToolResult.content` (one `Text` unless noted) | `is_error` |
|---|---|---|
| `Ok(out)` | `out.content`, truncated (below). Plus, if `background`, an appended `Text`: `"[background task {id} started; you will be told when it finishes]"` | false |
| `InvalidInput(m)` | `"Invalid arguments: {m}. Check the tool's input schema and try again."` | true |
| `Failed{m, retryable:true}` | `"{m} (transient; retrying once may succeed)"` | true |
| `Failed{m, retryable:false}` | `m` | true |
| `Denied{None}` | `"The user declined to run this tool call."` | true |
| `Denied{Some(f)}` | `"The user declined to run this tool call and said: {f}"` | true |
| `Cancelled` | `"Cancelled by the user before completion."` | true |
| `TimedOut(d)` | `"The tool did not finish within {d}s and was stopped."` | true |
| `Panicked(_)` | `"The tool crashed due to an internal error. Do not retry it with the same arguments."` (the panic text goes only to `tracing`) | true |
| `CheckpointFailed{p,r}` | `"Not executed: could not snapshot {p} before editing ({r}). No files were changed."` | true |
| unknown tool `n` (no `ToolError`, built directly) | `"Unknown tool '{n}'. Available tools: a, b, c."` | true |
| `ToolCall.malformed` (built directly) | the `Err` string from `ToolCall::input_checked()` | true |

Truncation is applied to the *total* text length of `content`. `Json`
values are measured as compact JSON. When a `Json` value is over budget,
it becomes a `Text` holding a truncated pretty print. The cut lands on a
char boundary, followed by
`"\n[truncated: {n} bytes omitted; ask for a narrower query]"`.
`repeat_note` (§6.6) is appended last.

### 3.3 Context handed to `execute` (`tool/context.rs`)

```rust
/// Owned, Clone, 'static, so it can move into spawned work.
#[derive(Clone)]
pub struct ToolContext {
    pub session_id: data::SessionId,
    pub turn: TurnId,
    pub call_id: CallId,
    pub tool_name: Arc<str>,
    pub workdir: Arc<Path>,
    /// Child of the turn token; also fires on timeout. Long tools should select! on it.
    pub cancel: CancellationToken,
    progress: ProgressSink,
    edits: EditRecorder,
    background: BackgroundSpawner,
}
impl ToolContext {
    pub fn progress(&self) -> &ProgressSink;
    pub fn edits(&self) -> &EditRecorder;
    pub fn background(&self) -> &BackgroundSpawner;
}

/// Emits CoreEvent::ToolProgress. Never blocks: uses try_send and drops on a full channel,
/// so progress is best-effort, unlike deltas.
#[derive(Clone)]
pub struct ProgressSink { /* EventTx, turn, call_id */ }
impl ProgressSink {
    pub fn report(&self, message: impl Into<String>);
    pub fn fraction(&self, done: f32, message: impl Into<String>);
}

/// Dynamic checkpoint declaration for tools that discover paths at runtime.
/// Paths already snapshotted for this call (from ToolPlan.writes) are skipped.
#[derive(Clone)]
pub struct EditRecorder { /* Arc<CheckpointManager>, CallScope (Arc<Mutex<BTreeSet<PathBuf>>>) */ }
impl EditRecorder {
    /// Records a PreEdit snapshot of `paths`. MUST be awaited before writing them.
    /// Err => the tool must not write (it should return the error, which converts to CheckpointFailed).
    /// Ok when checkpoints are disabled (no-op).
    pub async fn before_write(&self, paths: &[PathBuf]) -> Result<(), ToolError>;
    pub fn is_enabled(&self) -> bool;
}

/// Session-scoped (not turn-scoped) task spawning for "I'll let you know" work.
#[derive(Clone)]
pub struct BackgroundSpawner { /* mpsc::Sender<actor::Command>, counter */ }
impl BackgroundSpawner {
    /// Spawns `fut` on the session's task set and returns a ticket immediately.
    /// The future receives a token that fires when the session closes or the user cancels the task.
    /// Its Ok/Err result becomes Notice::BackgroundFinished / BackgroundFailed.
    pub fn spawn<F, Fut>(&self, label: impl Into<String>, f: F) -> BackgroundTicket
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = Result<BackgroundResult, String>> + Send + 'static;
    /// For durable work registered elsewhere (artha-jobs): build a ticket only; completion
    /// arrives later through SessionHandle::notify.
    pub fn durable_ticket(&self, job_id: impl Into<String>, label: impl Into<String>) -> BackgroundTicket;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundResult {
    /// Short text the model will see on the next turn (capped at 4 KiB).
    pub summary: String,
    pub display: Option<ToolDisplay>,
}
```

### 3.4 Registry (`tool/registry.rs`)

```rust
/// Thread-safe, mutable registry. Shared by Agent and artha-plugins (enable/disable at runtime).
pub struct ToolRegistry { inner: std::sync::RwLock<Arc<ToolSet>> }

impl ToolRegistry {
    pub fn new() -> Self;
    /// Validates the spec (ToolSpec::validate) and rejects duplicate names.
    pub fn register(&self, tool: Arc<dyn Tool>) -> Result<()>;
    pub fn register_all(&self, tools: impl IntoIterator<Item = Arc<dyn Tool>>) -> Result<()>;
    pub fn unregister(&self, name: &str) -> bool;
    /// Atomically replaces every tool whose name starts with `prefix` (e.g. "mcp__weather__").
    pub fn replace_prefix(&self, prefix: &str, tools: Vec<Arc<dyn Tool>>) -> Result<()>;
    /// O(1): clones the current Arc. Taken once per turn.
    pub fn snapshot(&self) -> Arc<ToolSet>;
}

/// Immutable view used for one turn.
pub struct ToolSet {
    tools: BTreeMap<String, Arc<dyn Tool>>,   // sorted ⇒ deterministic spec order ⇒ cache-friendly
    specs: Vec<llm::ToolSpec>,               // precomputed, same order
}
impl ToolSet {
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>>;
    pub fn specs(&self) -> &[llm::ToolSpec];
    pub fn names(&self) -> impl Iterator<Item = &str>;
    pub fn len(&self) -> usize;
    /// Restricts the set to `allow` (used by artha-jobs for least-privilege runs).
    pub fn filtered(&self, allow: &BTreeSet<String>) -> ToolSet;
}
```

Writers take the write lock only to build a new `ToolSet` and swap the
`Arc`, so readers never wait on tool construction. A `std` lock is fine
because the critical section never awaits.

### 3.5 Typed adapter (`tool/typed.rs`, feature `schemars`)

```rust
#[async_trait::async_trait]
pub trait TypedTool: Send + Sync + 'static {
    type Input: serde::de::DeserializeOwned + schemars::JsonSchema + Send;
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    fn traits(&self) -> ToolTraits { ToolTraits::default() }
    fn plan(&self, input: &Self::Input, ctx: &PlanContext<'_>) -> ToolPlan { ToolPlan::default() }
    async fn run(&self, input: Self::Input, ctx: ToolContext) -> Result<ToolOutput, ToolError>;
}

/// Wraps a TypedTool as a dyn Tool. Builds the spec once (schema_for!, root forced to "type":"object").
/// serde_json::from_value errors become ToolError::InvalidInput with the serde path in the message.
pub struct Typed<T: TypedTool> { inner: T, spec: llm::ToolSpec }
impl<T: TypedTool> Typed<T> { pub fn new(inner: T) -> Result<Self>; }
impl<T: TypedTool> Tool for Typed<T> { /* … */ }
```

## 4. Outward events (`event.rs`)

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CoreEvent {
    // ── session ───────────────────────────────────────────────────────────
    /// First event on every receiver. `history` = number of messages loaded (0 for new).
    SessionOpened { session: Option<data::SessionId>, resumed: bool, history: usize, workdir: PathBuf },
    /// The DB row was created lazily on the first submit.
    SessionPersisted { session: data::SessionId },
    Notice(Notice),
    RewindCompleted(RewindReport),

    // ── turn lifecycle ────────────────────────────────────────────────────
    TurnStarted { turn: TurnId, user_seq: Option<data::MessageSeq>, queued_behind: usize },
    RoundStarted { turn: TurnId, round: RoundIndex },
    /// A mid-stream failure will be retried. UIs must discard text shown for this round.
    RoundRetrying { turn: TurnId, round: RoundIndex, attempt: u32, reason: String, discard_partial: bool },
    TurnEnded { turn: TurnId, outcome: TurnOutcome, usage: llm::Usage, rounds: u32, persisted: bool },

    // ── model output (mirrors llm::ChatEvent, tagged with turn/round) ─────
    TextDelta { turn: TurnId, round: RoundIndex, index: llm::BlockIndex, text: String },
    ThinkingDelta { turn: TurnId, round: RoundIndex, index: llm::BlockIndex, text: String },
    /// From ToolCallStart: the model began emitting a call (args still streaming).
    ToolCallStreaming { turn: TurnId, round: RoundIndex, call_id: CallId, name: String },
    /// From ToolCallEnd: arguments complete.
    ToolCallReady { turn: TurnId, round: RoundIndex, call: ToolCallView },
    /// The assistant message was committed to history (and to the DB if persisted).
    AssistantCommitted { turn: TurnId, round: RoundIndex, seq: Option<data::MessageSeq>, stop_reason: llm::StopReason },
    /// Running totals for the turn (merged across rounds).
    Usage { turn: TurnId, round: RoundIndex, usage: llm::Usage },

    // ── tools ─────────────────────────────────────────────────────────────
    PermissionRequested(PermissionPrompt),
    ToolStarted { turn: TurnId, call_id: CallId, name: String, summary: String, eager: bool },
    ToolProgress { turn: TurnId, call_id: CallId, message: String, fraction: Option<f32> },
    ToolFinished { turn: TurnId, call_id: CallId, name: String, outcome: ToolOutcome,
                   elapsed_ms: u64, display: Option<ToolDisplay> },
    CheckpointRecorded { turn: TurnId, call_id: CallId, checkpoint: String /* 40-hex */,
                         kind: CheckpointKindView, files: Vec<PathBuf> },
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallView {
    pub id: CallId,
    pub name: String,
    pub input: serde_json::Value,
    pub malformed: Option<String>,   // error text when the args failed to parse
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolOutcome {
    Ok { background: Option<BackgroundTicket> },
    Error { message: String },             // any is_error result; message = text sent to the model
    Denied { feedback: Option<String> },
    Cancelled,
    Skipped { reason: String },            // DenyAndStop / round aborted / eager result discarded
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointKindView { PreEdit, PostEdit }

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnOutcome {
    Completed,
    /// MaxTokens with no tool calls. The UI may offer "continue".
    MaxTokens,
    Refused { reason: RefusalKind },
    RoundLimit { limit: u32 },
    /// DenyAndStop chosen at a permission prompt.
    Stopped,
    Cancelled,
    /// The user message is kept; SessionHandle::retry() re-runs the turn.
    Failed { error: ErrorView },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalKind { Refusal, ContentFilter }

/// Serializable snapshot of a CoreError / LlmError for UIs and --json output.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorView {
    pub message: String,
    pub llm_kind: Option<String>,            // ErrorKind as snake_case
    pub retry_after_ms: Option<u64>,
    pub reauth_required: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Notice {
    Info { message: String },
    Warning { message: String },
    BackgroundFinished { ticket: BackgroundTicket, summary: String, display: Option<ToolDisplay> },
    BackgroundFailed { ticket: BackgroundTicket, error: String },
    /// A history repair happened on load.
    HistoryRepaired { seqs: Vec<data::MessageSeq> },
}
```

The sender side:

```rust
/// Bounded (1024) mpsc sender with two send flavors.
#[derive(Clone)]
pub(crate) struct EventTx { tx: mpsc::Sender<CoreEvent>, closed: Arc<AtomicBool> }
impl EventTx {
    /// Lossless; awaits capacity (backpressure). Used for everything except progress.
    pub(crate) async fn send(&self, ev: CoreEvent);
    /// Lossy; used for ToolProgress only.
    pub(crate) fn try_send(&self, ev: CoreEvent);
    /// True once the receiver is dropped. The loop keeps running; prompts auto-deny.
    pub(crate) fn is_closed(&self) -> bool;
}
```

`send` never returns an error to callers. When the receiver is gone,
events are discarded and `closed` is set.

**Event-order guarantees** (asserted by `testing::EventLog::assert_well_formed`):
1. `SessionOpened` comes first, exactly once.
2. Per turn: `TurnStarted`, then 1 or more rounds, then `TurnEnded`,
   exactly once. Turns never interleave.
3. Per round: `RoundStarted`, then deltas and `ToolCall*`, then either
   `AssistantCommitted` or `RoundRetrying` (in which case the round
   restarts with the same `round` index and a new attempt). After a
   commit come zero or more `PermissionRequested` / `ToolStarted` /
   `ToolProgress` / `CheckpointRecorded` / `ToolFinished`.
4. Every `ToolCallReady` in a committed round gets exactly one
   `ToolFinished`, even when the call was denied, cancelled, or skipped.
5. A call's `ToolStarted` comes before its `ToolProgress`, which comes
   before its `ToolFinished`. Its `CheckpointRecorded(PreEdit)` comes
   before `ToolStarted`'s side effects. Its
   `CheckpointRecorded(PostEdit)` comes before `ToolFinished`.
6. `Notice` events may appear anywhere, including between turns.

## 5. The chat-loop state machine (`turn/`)

### 5.1 Driver and state

```rust
pub(crate) struct TurnDriver<'s> {
    agent: &'s AgentShared,           // provider, config, persister, checkpoints
    session: &'s mut Session,
    tools: Arc<ToolSet>,              // snapshot for the whole turn
    system: String,                   // built once for the whole turn
    turn: TurnId,
    cancel: CancellationToken,        // turn-scoped; child of the session token
    events: EventTx,
    cmd_rx: &'s mut mpsc::Receiver<Command>,  // to react to Cancel while running; Submit is queued
    usage: llm::Usage,                // merged across rounds
}

impl TurnDriver<'_> {
    pub(crate) async fn run(self, input: TurnInput) -> TurnOutcome;
}

pub(crate) enum TurnInput {
    /// A new user message (notices already folded in by the actor).
    User(llm::Message),
    /// SessionHandle::retry(): re-run from the existing trailing user/tool-results message.
    Retry,
    /// BackgroundFollowup::AutoTurn: a synthetic user message from notices.
    Followup(llm::Message),
}

#[derive(Debug)]
pub(crate) enum TurnState {
    Preparing,
    Requesting   { round: RoundIndex, attempt: u32, compacted: bool },
    Streaming    { round: RoundIndex, attempt: u32, stream: llm::EventStream, acc: RoundConsumer },
    Committing   { round: RoundIndex, message: llm::Message, stop: llm::StopReason, usage: llm::Usage, eager: EagerSet },
    Dispatching  { round: RoundIndex, seq: Option<data::MessageSeq>, calls: Vec<llm::ToolCall>, eager: EagerSet },
    Cancelling   { at: CancelPoint },
    Finished(TurnOutcome),
}

/// What was in flight when the cancel arrived; decides the repair.
pub(crate) enum CancelPoint {
    BeforeRequest,
    Streaming { partial_text: String },       // assembled from TextDeltas so far (text blocks only)
    Dispatching { seq: Option<data::MessageSeq>, calls: Vec<llm::ToolCall>, done: Vec<(CallId, llm::ToolResult)> },
}
```

The driver is a `loop { state = match state { … } }`. Each arm is one
async step that returns the next state. Each `.await` that can take a
long time is wrapped in
`select! { biased; _ = cancel.cancelled() => Cancelling{..}, r = fut => … }`.

### 5.2 Transition table

| # | From | Trigger / condition | Action | To |
|---|---|---|---|---|
| 1 | `Preparing` | `User(m)` / `Followup(m)` | `session.push(m)`; `persister.append(User)` (lazily `create` the session row first; on create emit `SessionPersisted`); emit `TurnStarted` | `Requesting{0,0,false}` |
| 2 | `Preparing` | `Retry` and the last message is User or tool results | emit `TurnStarted{user_seq: None}` | `Requesting{0,0,false}` |
| 3 | `Preparing` | `Retry` and the last message is a completed assistant message | — | `Finished(Failed(NothingToRetry))`; the handle returns the error before the turn starts, so this is defensive |
| 4 | `Requesting` | always | emit `RoundStarted` (on attempt 0 only); build `RequestView` (§5.4); `provider.chat(&req)` under `select!` | (5–8) |
| 5 | `Requesting` | `Ok(stream)` | `acc = RoundConsumer::new()` | `Streaming` |
| 6 | `Requesting` | `Err(ContextLengthExceeded)` and `!compacted` | `Compactor::compact(view)`; emit `Notice::Info("conversation is long; older tool outputs were shortened")` | `Requesting{round, attempt, compacted: true}` |
| 7 | `Requesting` | `Err(e)` otherwise | — | `Finished(Failed(e))` |
| 8 | `Requesting` | cancel | — | `Cancelling{BeforeRequest}` |
| 9 | `Streaming` | `Ok(TextDelta/ThinkingDelta)` | forward as a `CoreEvent`; `acc` records text | `Streaming` |
| 10 | `Streaming` | `Ok(ToolCallStart)` | emit `ToolCallStreaming` | `Streaming` |
| 11 | `Streaming` | `Ok(ToolCallDelta)` | ignored (not forwarded in v1; UIs show a spinner on `ToolCallStreaming`) | `Streaming` |
| 12 | `Streaming` | `Ok(ToolCallEnd{call})` | emit `ToolCallReady`; if `eager_eligible(call)` (§6.4), start it in `acc.eager` | `Streaming` |
| 13 | `Streaming` | `Ok(Usage(u))` | `usage.merge(u)`; emit `Usage` | `Streaming` |
| 14 | `Streaming` | `Ok(MessageEnd{stop, usage, message})` | drop the stream | `Committing` |
| 15 | `Streaming` | `Err(e)`, `e.is_retryable()`, `attempt < stream_retries` | abort `acc.eager`; emit `RoundRetrying{discard_partial: true}`; `sleep(backoff(attempt))` under `select!` | `Requesting{round, attempt+1, compacted}` |
| 16 | `Streaming` | `Err(e)` otherwise | abort `acc.eager`; partial output discarded | `Finished(Failed(e))` |
| 17 | `Streaming` | `None` (no `MessageEnd`) | treated as `Err(Protocol("stream ended without MessageEnd"))` → row 16. This should be impossible given artha-llm's guarantees. | `Finished(Failed)` |
| 18 | `Streaming` | cancel | drop the stream (aborts the HTTP request); abort `acc.eager` | `Cancelling{Streaming{partial_text}}` |
| 19 | `Committing` | always | `session.push(message)`; `seq = persister.append(Assistant, usage)`; emit `AssistantCommitted`; `next = stop::classify(stop, &message)` | (20–24) |
| 20 | `Committing` | `next = Dispatch` (`ToolUse`, or `MaxTokens` with tool uses) | — | `Dispatching{round, seq, calls, eager}` |
| 21 | `Committing` | `next = Continue` (`PauseTurn`) | if `round + 1 >= max_rounds`, go to `Finished(RoundLimit)` | `Requesting{round+1, 0, compacted}` |
| 22 | `Committing` | `next = Done` (`EndTurn`, `StopSequence`, `Other`) | `Other(s)` logs a `warn` | `Finished(Completed)` |
| 23 | `Committing` | `next = Truncated` (`MaxTokens`, no tool uses) | — | `Finished(MaxTokens)` |
| 24 | `Committing` | `next = Refused(k)` | — | `Finished(Refused{k})` |
| 25 | `Dispatching` | always | `results = Dispatcher::run_round(..)` (§6) under cancel | (26–28) |
| 26 | `Dispatching` | `RoundResult::Done(results)` | `session.push(Message::tool_results(results))`; `persister.append(Tool)`; if `round + 1 >= max_rounds`, go to `Finished(RoundLimit)` | `Requesting{round+1, 0, compacted}` |
| 27 | `Dispatching` | `RoundResult::Stopped(results)` (DenyAndStop) | push and persist the results (the remaining calls get "not run") | `Finished(Stopped)` |
| 28 | `Dispatching` | cancel | in-flight calls: cancel, wait up to `cancel_grace` (2 s), then abort | `Cancelling{Dispatching{seq, calls, done}}` |
| 29 | `Cancelling{BeforeRequest}` | — | nothing to repair (history ends with User or tool results) | `Finished(Cancelled)` |
| 30 | `Cancelling{Streaming{t}}` | `t.trim()` non-empty | push and persist `Message::assistant_text(t)` with `meta.interrupted = true` | `Finished(Cancelled)` |
| 31 | `Cancelling{Streaming{t}}` | `t` empty | nothing (a following User message is merged by the codecs) | `Finished(Cancelled)` |
| 32 | `Cancelling{Dispatching{..}}` | — | results = `done` plus `Cancelled` for every other call, in call order; push and persist `tool_results` | `Finished(Cancelled)` |
| 33 | `Finished(o)` | — | emit `TurnEnded{o, usage, rounds, persisted: !session.unsynced}` | return `o` |

A second `Cancel` during `Cancelling` is ignored. The repair is short
and must complete. If the persist in row 30 or 32 fails, the session is
marked `unsynced`, and the actor retries the flush before the next turn.

**Invariant (property-tested in `cancel.rs`, §13.2):** at every `Finished`, both
`session.messages` and the database's message list pass
`llm::validate_request` pairing rules (rules 5 and 6 in artha-llm LLD
§4.1), given one trailing user message appended.

### 5.3 Stop-reason classification (`turn/stop.rs`)

```rust
pub(crate) enum NextStep { Dispatch, Continue, Done, Truncated, Refused(RefusalKind) }

pub(crate) fn classify(stop: &llm::StopReason, msg: &llm::Message) -> NextStep {
    use llm::StopReason::*;
    match stop {
        ToolUse                              => NextStep::Dispatch,
        MaxTokens if msg.has_tool_uses()     => NextStep::Dispatch,  // truncated calls arrive malformed
        MaxTokens                            => NextStep::Truncated,
        PauseTurn                            => NextStep::Continue,
        Refusal                              => NextStep::Refused(RefusalKind::Refusal),
        ContentFilter                        => NextStep::Refused(RefusalKind::ContentFilter),
        EndTurn | StopSequence(_) | Other(_) => if msg.has_tool_uses() { NextStep::Dispatch } else { NextStep::Done },
        _                                    => NextStep::Done,  // #[non_exhaustive]
    }
}
```

The `EndTurn` with tool uses case is already normalized by artha-llm's
assembler. Core re-checks it because an unanswered `tool_use` in history
would make the *next* request invalid.

### 5.4 Request building and compaction (`turn/compact.rs`)

```rust
/// Borrowed by default; owned only after compaction.
pub(crate) enum RequestView<'a> {
    Borrowed(&'a [llm::Message]),
    Compacted(Vec<llm::Message>),
}
impl RequestView<'_> { pub(crate) fn messages(&self) -> &[llm::Message]; }

pub(crate) struct Compactor { pub keep_recent_rounds: usize /* 4 */, pub elided_max_chars: usize /* 200 */ }
impl Compactor {
    /// Replaces the content of ToolResult blocks older than the last `keep_recent_rounds` tool-result
    /// messages with a single Text "[elided: {n} bytes of earlier tool output; re-run the tool if needed]"
    /// (first `elided_max_chars` kept). Never removes messages, so tool_use/tool_result pairing is preserved.
    pub(crate) fn compact(&self, history: &[llm::Message]) -> Vec<llm::Message>;
}
```

```rust
// Per round, inside Requesting:
let view = if compacted { RequestView::Compacted(self.compactor.compact(&self.session.messages)) }
           else { RequestView::Borrowed(&self.session.messages) };
let req = llm::ChatRequest::new(view.messages())
    .system(&self.system)
    .tools(self.tools.specs())
    .tool_choice(&self.agent.config.tool_choice)
    .options(&self.agent.config.generation);
let stream = select! { biased; _ = cancel.cancelled() => …, r = self.agent.provider.chat(&req) => r };
```

Compaction is per turn. Once a turn compacts, its later rounds reuse
compacted views, recomputed each round so new results are included.
Persisted history is never altered. If a compacted request still
exceeds the window, row 7 ends the turn with `Failed`, and the UI
suggests starting a new session. LLM-driven summarization (`/compact`)
is a later addition. It would be a new `TurnInput` variant and would not
change the state machine.

### 5.5 Round consumer (`turn/round.rs`)

```rust
pub(crate) struct RoundConsumer {
    partial_text: String,          // TextDelta concat, for cancel repair only
    ready: Vec<llm::ToolCall>,     // ToolCallEnd order
    eager: EagerSet,
}
impl RoundConsumer {
    pub(crate) fn new() -> Self;
    /// Applies one ChatEvent: forwards UI events, records state. Returns Some(..) on MessageEnd.
    pub(crate) async fn on_event(&mut self, ev: llm::ChatEvent, cx: &RoundCx<'_>) -> Option<Committed>;
}
pub(crate) struct Committed { pub message: llm::Message, pub stop: llm::StopReason, pub usage: llm::Usage }
```

The authoritative list of calls is `message.tool_uses()` from
`MessageEnd`, not `ready`. `ready` exists only for eager starts. If the
two disagree, the message wins. Eager tasks whose id is not in the
message are aborted and reported as `Skipped`.

### 5.6 Mid-stream retry (`turn/retry.rs`)

```rust
pub(crate) struct RoundRetryPolicy { pub max: u32 /* 2 */, pub base: Duration /* 1s */, pub cap: Duration /* 8s */ }
impl RoundRetryPolicy {
    /// Pure. Full jitter like artha-llm's policy; `None` once attempts are exhausted.
    pub(crate) fn delay(&self, attempt: u32, jitter: f64) -> Option<Duration>;
}
```

The retry applies only to errors that `LlmError::is_retryable()` accepts
*inside* a stream. Errors from `chat()` itself were already retried by
artha-llm and are not retried again (row 7). That avoids multiplying
the two budgets.

## 6. Tool dispatch (`dispatch/`)

### 6.1 Entry point

```rust
pub(crate) struct Dispatcher<'a> {
    tools: &'a ToolSet,
    policy: &'a PermissionPolicy,
    grants: &'a mut Grants,
    checkpoints: &'a CheckpointManager,
    events: &'a EventTx,
    cfg: &'a DispatchConfig,
    repeat: &'a mut RepeatErrorTracker,
    cx: DispatchCx,   // session_id, turn, round, assistant seq, workdir, cancel token, background spawner
}

pub(crate) enum RoundResult {
    Done(Vec<llm::ToolResult>),     // one per call, in call order
    Stopped(Vec<llm::ToolResult>),  // DenyAndStop
}

impl Dispatcher<'_> {
    pub(crate) async fn run_round(&mut self, calls: Vec<llm::ToolCall>, eager: EagerSet) -> RoundResult;
}
```

### 6.2 Per-call pipeline

```
for each call c (in message order), phase A, which is sequential and cheap:
  A1 resolve       tools.get(c.name)          ─ none ─▶ result = Unknown(name)          → Resolved::Done
  A2 eager?        eager.contains(c.id)       ─ yes ─▶ Resolved::Eager(handle)           (skip A3–A5)
  A3 args          c.input_checked()          ─ err ─▶ result = Malformed(msg)          → Resolved::Done
  A4 plan          tool.plan(&input, &pcx)    ─ err ─▶ result = Err(e)                  → Resolved::Done
  A5 gate          policy.verdict(tool, &plan, grants)
                     Allow                    → Resolved::Run(c, tool, plan)
                     Deny(reason)             → result = Denied{reason}                → Resolved::Done
                     Ask                      → emit PermissionRequested, await responder (under cancel)
                         AllowOnce            → Run
                         AllowForSession      → grants.insert(name); Run
                         Deny{feedback}       → Denied{feedback}
                         DenyAndStop          → Denied{None}; mark every later call Skipped("stopped by user");
                                                stop = true; break
                         (responder dropped)  → Denied{None}

phase B, execution:
  group the Run calls: P = calls where traits.concurrency_safe && plan.writes.is_empty();
                       S = the rest
  run P concurrently (FuturesUnordered, ≤ max_parallel_tools) together with the Eager handles,
  then run S one at a time in call order.
  each Run call goes through exec_one (§6.3).

phase C: assemble results in call order → into_tool_result(.., repeat.note(name, &outcome)),
         emit ToolFinished for each call that has not emitted one yet → RoundResult.
```

Why permissions come before any execution: the user answers every
prompt for the round up front, and only then does anything run. A
dialog never pops up halfway through a sequence of writes. It also
makes `DenyAndStop` clean, because nothing from the round has run yet
except eager read-only calls.

Why S runs after P: read-only lookups ("get the quote, then write the
report") are usually inputs to the writes. Within S, the call order is
the model's order.

### 6.3 Executing one call (`dispatch/exec.rs`)

```rust
pub(crate) async fn exec_one(
    tool: Arc<dyn Tool>,
    call: &llm::ToolCall,
    plan: &ToolPlan,
    cx: &DispatchCx,
    checkpoints: &CheckpointManager,
    events: &EventTx,
) -> Result<ToolOutput, ToolError> {
    // 1. Pre-edit snapshot (fail closed). Skipped when checkpoints are disabled or plan.writes is empty.
    let scope = checkpoints.begin_call(cx.call_scope(call)).await;          // CallScope
    if !plan.writes.is_empty() {
        scope.pre_edit(&plan.writes).await?;                                 // Err => ToolError::CheckpointFailed
    }
    // 2. Started.
    events.send(CoreEvent::ToolStarted { /* summary from plan */ eager: false, .. }).await;
    // 3. Spawn with a child token, under a timeout.
    let token = cx.cancel.child_token();
    let ctx = ToolContext::new(cx, call, token.clone(), scope.recorder(), events.clone());
    let input = call.input.clone();
    let mut task = tokio::spawn({ let tool = tool.clone(); async move { tool.execute(input, ctx).await } });
    let outcome = select! {
        biased;
        _ = cx.cancel.cancelled() => { token.cancel(); grace_then_abort(&mut task, cx.cancel_grace).await; Err(ToolError::Cancelled) }
        r = timeout_opt(tool.traits().timeout, &mut task) => match r {
            Err(_elapsed)                     => { token.cancel(); grace_then_abort(&mut task, GRACE).await; Err(ToolError::TimedOut(..)) }
            Ok(Err(join)) if join.is_panic()  => Err(ToolError::Panicked(panic_message(join))),
            Ok(Err(join))                     => Err(ToolError::Failed { message: join.to_string(), retryable: false }),
            Ok(Ok(res))                       => res,
        },
    };
    // 4. Post-edit snapshot over every path declared (plan + dynamic), on success OR failure.
    scope.post_edit().await;                                                 // errors → Notice::Warning only
    outcome
}
```

- `grace_then_abort` waits up to the grace period for the task to
  observe its token, then calls `JoinHandle::abort()`. Work inside the
  tool's own `spawn_blocking` can't be aborted. It runs to completion
  and its result is ignored. This is documented on the trait.
- The panic message is logged at `error` with the tool name and call
  id. The model only sees the generic text from §3.2.

### 6.4 Eager start

```rust
pub(crate) struct EagerSet { tasks: HashMap<CallId, EagerTask> }
struct EagerTask { handle: JoinHandle<Result<ToolOutput, ToolError>>, token: CancellationToken, started: Instant }

fn eager_eligible(call: &llm::ToolCall, tools: &ToolSet, policy: &PermissionPolicy, grants: &Grants, cfg: &DispatchConfig) -> bool {
    cfg.eager_start
        && call.malformed.is_none()
        && tools.get(&call.name).is_some_and(|t| {
            let tr = t.traits();
            tr.risk == ToolRisk::ReadOnly && tr.concurrency_safe
                && matches!(policy.verdict_static(&call.name, &tr, grants), PolicyVerdict::Allow)
                && t.plan(&call.input, &pcx).is_ok_and(|p| p.writes.is_empty())
        })
}
```

Eager tasks go through the same spawn, timeout, and panic path, minus
the checkpoint steps. They are aborted on a round retry or a cancel.
`EagerSet` aborts any remaining tasks on drop, so a task is never
leaked.

### 6.5 Permission policy (`dispatch/permission.rs`)

```rust
#[derive(Debug, Clone)]
pub enum PermissionPolicyConfig {
    /// settings.preferences.permission_mode, after the CLI applied --skip-permissions/--yolo.
    Interactive(data::PermissionMode),
    /// Unattended (artha-jobs): never prompt. Tools in `allow` run; everything non-ReadOnly else is denied.
    NonInteractive { allow: BTreeSet<String>, allow_read_only: bool },
}

pub(crate) struct PermissionPolicy { cfg: PermissionPolicyConfig }

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PolicyVerdict { Allow, Ask, Deny(String) }

impl PermissionPolicy {
    /// `plan.writes` non-empty lifts the effective risk to at least FileWrite.
    pub(crate) fn verdict(&self, name: &str, traits: &ToolTraits, plan: &ToolPlan, grants: &Grants) -> PolicyVerdict;
    pub(crate) fn verdict_static(&self, name: &str, traits: &ToolTraits, grants: &Grants) -> PolicyVerdict;
}
```

Decision matrix (effective risk across, configuration down):

| | `ReadOnly` | `LocalWrite` | `FileWrite` | `External` | `Execute` |
|---|---|---|---|---|---|
| `Interactive(Ask)`, `confirm: Default` | Allow | Ask | Ask | Ask | Ask |
| `Interactive(Ask)`, `confirm: Never` | Allow | Allow | Allow | Allow | Allow |
| `Interactive(Ask)`, `confirm: Always` | Ask | Ask | Ask | Ask | Ask |
| `Interactive(Ask)`, name in `grants` | Allow | Allow | Allow | Allow | Allow |
| `Interactive(Skip)` (yolo) | Allow | Allow | Allow | Allow | Allow (logged `info`) |
| `NonInteractive`, name in `allow` | Allow | Allow | Allow | Allow | Allow |
| `NonInteractive`, not in `allow` | Allow if `allow_read_only` else Deny | Deny | Deny | Deny | Deny |

The grant check comes before `confirm: Always`. A session grant is an
explicit user answer, so it wins over the tool's default caution.

```rust
#[derive(Debug, Clone, Serialize)]
pub struct PermissionPrompt {
    pub turn: TurnId,
    pub call_id: CallId,
    pub tool: String,
    pub risk: ToolRisk,
    pub summary: String,
    pub input: serde_json::Value,
    pub writes: Vec<PathBuf>,
    pub preview: Option<ToolDisplay>,
    /// Position in this round's prompt queue, e.g. "2 of 3".
    pub position: (usize, usize),
    #[serde(skip)]
    pub responder: PermissionResponder,
}

/// Clone-able so the event can be cloned; the first respond() wins and later ones are no-ops.
#[derive(Clone)]
pub struct PermissionResponder { tx: Arc<std::sync::Mutex<Option<oneshot::Sender<PermissionDecision>>>> }
impl PermissionResponder {
    pub fn respond(&self, d: PermissionDecision) -> bool;   // false if already answered or the turn moved on
}
impl fmt::Debug for PermissionResponder { /* "PermissionResponder{..}" */ }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    /// Allow this tool name for the rest of this session (in memory; reset on resume).
    AllowForSession,
    Deny { feedback: Option<String> },
    /// Deny this call, skip the rest of the round, and end the turn.
    DenyAndStop,
}

/// In-memory session grants. Deliberately not persisted (HLD §8).
#[derive(Debug, Default, Clone)]
pub(crate) struct Grants { tools: BTreeSet<String> }
```

The await on a prompt:

```rust
let (tx, rx) = oneshot::channel();
events.send(CoreEvent::PermissionRequested(prompt_with(PermissionResponder::new(tx)))).await;
if events.is_closed() { return PermissionDecision::Deny { feedback: None }; }
select! {
    biased;
    _ = cancel.cancelled() => PermissionDecision::Deny { feedback: None },  // then the Cancelling path
    r = rx => r.unwrap_or(PermissionDecision::Deny { feedback: None }),       // Err = responder dropped
}
```

There is no timeout on interactive prompts. The user may step away.
Unattended runs never reach this code because `NonInteractive` never
returns `Ask`.

### 6.6 Repeat-error nudge (`dispatch/guard.rs`)

```rust
pub(crate) struct RepeatErrorTracker { last: HashMap<String, (u64 /* hash of error text */, u32 /* count */)>, limit: u32 /* 3 */ }
impl RepeatErrorTracker {
    /// Updates the tracker with this outcome; returns a note to append when `count >= limit`.
    pub(crate) fn note(&mut self, tool: &str, outcome: &Result<ToolOutput, ToolError>) -> Option<&'static str>;
}
```

The tracker lives for one turn. The note text is "This tool has failed
the same way {n} times; stop retrying it and explain the problem to the
user." A success resets that tool's count.

## 7. Session model (`session/`)

### 7.1 In-memory types

```rust
pub(crate) struct Session {
    pub meta: SessionMeta,
    /// Contiguous so ChatRequest can borrow it. messages[i] ↔ entries[i].
    pub messages: Vec<llm::Message>,
    pub entries: Vec<EntryMeta>,
    pub grants: Grants,
    pub pending_notices: Vec<Notice>,          // BackgroundFinished/Failed awaiting the next user turn
    pub usage: UsageTotals,
    pub unsynced: Vec<usize>,                  // indices into messages not yet persisted
    pub next_turn: u64,
    pub file_rewind: FileRewind,               // Enabled | Disabled(reason) (workdir mismatch / checkpoints off)
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: Option<data::SessionId>,           // None until the lazy create on first submit
    pub title: Option<String>,
    pub workdir: PathBuf,                      // canonical
    pub mode: data::Mode,
    pub provider: String,                      // info().format.as_str() at creation
    pub model: String,
    pub parent: Option<data::SessionId>,
    pub created_at: Option<data::Timestamp>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EntryMeta {
    pub seq: Option<data::MessageSeq>,         // None while unsynced
    pub meta: MessageMeta,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct UsageTotals { pub input: u64, pub output: u64, pub cache_read: u64, pub turns: u64 }

impl Session {
    pub(crate) fn push(&mut self, m: llm::Message, meta: MessageMeta) -> usize;
    pub(crate) fn last_user_seq(&self) -> Option<data::MessageSeq>;
    /// Truncates in-memory history to messages with seq <= keep_through.
    pub(crate) fn truncate_after(&mut self, keep_through: data::MessageSeq);
}
```

### 7.2 Persisted shape (`session/persist.rs`)

```rust
/// What goes into core_message.content. The flattened Message keeps the JSON readable as a
/// plain llm::Message (serde ignores the extra "artha" key), per artha-llm LLD §3.2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedMessage {
    #[serde(flatten)]
    pub message: llm::Message,
    #[serde(default, rename = "artha", skip_serializing_if = "MessageMeta::is_empty")]
    pub meta: MessageMeta,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct MessageMeta {
    /// The assistant text was cut short by a cancel.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interrupted: bool,
    /// Written by core (repair, cancel, DenyAndStop), not produced by a tool or the model.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub synthetic: bool,
    /// For tool-results messages: per-call elapsed ms and outcome tag, for `--resume` rendering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<CallRecord>,
    /// For user messages: background tickets whose notices were folded into this message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
}
impl MessageMeta { pub(crate) fn is_empty(&self) -> bool; }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CallRecord { pub id: CallId, pub elapsed_ms: u64, pub status: String /* ok|error|denied|cancelled|skipped */ }

pub(crate) fn to_data_role(m: &llm::Message) -> data::Role {
    match m.role {
        llm::Role::Assistant                     => data::Role::Assistant,
        llm::Role::User if m.is_tool_results()   => data::Role::Tool,
        llm::Role::User                          => data::Role::User,
    }
}
```

```rust
/// Async facade over the synchronous artha-data stores. All calls go through spawn_blocking.
#[derive(Clone)]
pub(crate) struct Persister { sessions: Arc<data::SessionStore>, retry: PersistRetry /* 3 attempts, 100ms→400ms, only is_busy() */ }

impl Persister {
    pub(crate) async fn create(&self, new: data::NewSession) -> Result<data::Session>;
    /// Appends one message; returns its seq. Retries is_busy() errors.
    pub(crate) async fn append(&self, id: data::SessionId, m: &llm::Message, meta: &MessageMeta,
                               usage: Option<&llm::Usage>) -> Result<data::MessageSeq>;
    pub(crate) async fn load(&self, id: data::SessionId) -> Result<(data::Session, Vec<data::StoredMessage>)>;
    pub(crate) async fn most_recent(&self, workdir: PathBuf) -> Result<Option<data::Session>>;
    pub(crate) async fn list(&self, f: data::SessionFilter) -> Result<Vec<data::SessionSummary>>;
    pub(crate) async fn rename(&self, id: data::SessionId, title: String) -> Result<()>;
    pub(crate) async fn truncate_after(&self, id: data::SessionId, keep: data::MessageSeq) -> Result<u64>;
}
```

Write order within a turn (write-ahead):

```
submit ─▶ [create row if new] ─▶ append(User)                         seq u
round  ─▶ stream … MessageEnd ─▶ append(Assistant, usage)               seq a   ← checkpoint links use a
       ─▶ tools run (PreEdit/PostEdit links: message_seq = a)
       ─▶ append(Tool results)                                          seq a+1
next round …
end    ─▶ rename(title) on the session's first completed turn
```

Each `append` is one IMMEDIATE transaction in artha-data. Core uses
one message per call rather than batching the round, so the assistant
seq exists before tools run.

Unsynced recovery: when `append` fails after its retries, the index is
pushed onto `session.unsynced`. Before each later append, the actor
first flushes `unsynced` in order. Message order in the database
therefore always matches memory. If the session is closed while still
unsynced, `close()` returns `Err(Data(..))`, and the UI warns that the
last messages were not saved.

### 7.3 Selection and load (`session/resume.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSelector {
    New,
    /// -c/--continue: most recent non-archived session for the canonical cwd; New if none.
    Continue,
    /// -r <id>; the picker for bare -r is the UI's job (Agent::list_sessions).
    Resume(data::SessionId),
    /// "branch from here": artha-data fork() through `through`, then open the fork.
    Fork { from: data::SessionId, through: data::MessageSeq },
}

pub(crate) async fn load(p: &Persister, sel: &SessionSelector, cwd: &Path, info: &llm::ProviderInfo)
    -> Result<(Session, Vec<Notice>)>;
pub(crate) fn repair(messages: &mut Vec<llm::Message>, entries: &mut Vec<EntryMeta>) -> Vec<RepairAction>;

pub(crate) enum RepairAction {
    /// Append a synthetic tool-results message after `after_index`, answering `ids`.
    AnswerDangling { after_index: usize, ids: Vec<CallId> },
    /// A tool-results message answered ids that the preceding assistant message did not ask for.
    DropOrphanResults { index: usize, ids: Vec<CallId> },
}
```

Load sequence:

```
1. resolve:
     New               → Session::fresh(cwd, info), id = None
     Continue          → most_recent(cwd) ?: Session::fresh + Notice::Info("no previous session…")
     Resume(id)        → get(id) ?: Err(SessionNotFound)
     Fork{from,thr}    → fork(from, thr) then as Resume(new id)
2. messages(id) → for each StoredMessage:
     serde_json::from_value::<PersistedMessage>(content)
       Err(e) → Err(CorruptHistory{ session, seq, reason: e })
3. repair():
     walk i = 0..n: if messages[i] is Assistant with tool_use ids U:
        if i+1 == n or messages[i+1] is not tool-results       → AnswerDangling{i, U}
        else let R = ids in messages[i+1]:
           U \ R non-empty → append synthetic ToolResult blocks for those ids into messages[i+1] (in memory)
                             and record AnswerDangling (to persist)  // see note
           R \ U non-empty → DropOrphanResults (in memory only; logged warn)
4. apply AnswerDangling at the end of the history only (the only place a crash can leave one) by
   append(synthetic results). An AnswerDangling in the middle can't come from Artha; it is
   repaired in memory only and Notice::Warning is emitted.
5. workdir check: session.workdir != canonical(cwd) → file_rewind = Disabled("session belongs to <dir>"),
   plus Notice::Warning
6. provider change: info.format/model differ from the row → Notice::Info("continuing with <model>"). The row is
   not updated (open item, HLD §11.1).
7. emit Notice::HistoryRepaired{seqs} if any repairs happened.
```

Note on step 3: Artha itself only ever produces dangling calls at the
tail, because of the write-ahead order. Mid-history mismatches indicate
an external edit or a bug, so core repairs them in memory, warns, and
doesn't rewrite the database.

### 7.4 Titles (`session/title.rs`)

`derive_title(first_user_text) -> String` takes the first line,
collapses whitespace, and trims to 60 characters on a word boundary with
`…`. It is applied with `rename` after the first completed turn. The UI
can rename later through `SessionHandle::rename`.

## 8. Agent, handle, and actor (`agent.rs`, `handle.rs`, `actor.rs`)

```rust
#[derive(Clone)]
pub struct Agent { shared: Arc<AgentShared> }

pub(crate) struct AgentShared {
    pub provider: Arc<dyn llm::LlmProvider>,
    pub registry: Arc<ToolRegistry>,
    pub persister: Persister,
    pub checkpoints: Arc<CheckpointManager>,
    pub prompt: SystemPromptBuilder,
    pub config: AgentConfig,
    pub cwd: PathBuf,                          // canonical launch dir
}

pub struct AgentBuilder { /* … */ }
impl Agent {
    pub fn builder(
        provider: Arc<dyn llm::LlmProvider>,
        registry: Arc<ToolRegistry>,
        db: data::Db,
        checkpoints: Arc<data::CheckpointStore>,  // built by artha-cli with CheckpointStore::new(&paths, &cwd)
        config: AgentConfig,
    ) -> AgentBuilder;
    /// Opens (or lazily prepares) a session and spawns its actor. The receiver's first event is SessionOpened.
    pub async fn open(&self, sel: SessionSelector) -> Result<(SessionHandle, EventReceiver)>;
    pub async fn list_sessions(&self, filter: SessionListFilter) -> Result<Vec<SessionInfo>>;
    /// Headless convenience for `artha ask` and artha-jobs: open, submit, forward every event to `on_event`,
    /// close. Returns the outcome and the final assistant text.
    pub async fn run_turn_headless(
        &self,
        sel: SessionSelector,
        input: UserInput,
        on_event: impl FnMut(&CoreEvent) + Send,
    ) -> Result<HeadlessResult>;
}
impl AgentBuilder {
    pub fn prompt_contributor(self, c: Arc<dyn PromptContributor>) -> Self;
    pub fn cwd(self, cwd: PathBuf) -> Self;       // default: checkpoints.workdir()
    pub fn build(self) -> Result<Agent>;          // validates config (max_rounds ≥ 1, etc.)
}

pub struct HeadlessResult { pub session: Option<data::SessionId>, pub outcome: TurnOutcome, pub text: String, pub usage: llm::Usage }

pub struct SessionListFilter { pub this_workdir_only: bool /* true */, pub include_archived: bool, pub limit: u32 }
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo { pub id: data::SessionId, pub title: Option<String>, pub workdir: PathBuf,
                         pub model: Option<String>, pub message_count: u32, pub preview: Option<String>,
                         pub updated_at: data::Timestamp }
```

```rust
#[derive(Clone)]
pub struct SessionHandle { tx: mpsc::Sender<Command>, id: Arc<OnceLock<data::SessionId>> }

impl SessionHandle {
    /// Queues a turn (FIFO). Returns once the actor has accepted it; events report progress.
    pub async fn submit(&self, input: UserInput) -> Result<TurnId>;
    /// Cancels the running turn (no-op when idle). Queued submits are kept unless `drop_queue`.
    pub async fn cancel(&self, drop_queue: bool) -> Result<()>;
    /// Re-runs the last turn from its trailing user/tool-results message. Err(NothingToRetry) if it completed.
    pub async fn retry(&self) -> Result<TurnId>;
    pub async fn rewind(&self, req: RewindRequest) -> Result<RewindReport>;
    /// Background completion from outside (artha-jobs) or from core's own task set.
    pub async fn notify(&self, notice: Notice) -> Result<()>;
    pub async fn rename(&self, title: String) -> Result<()>;
    /// Finishes the current turn (it is not cancelled), flushes unsynced messages, cancels background
    /// tasks, and stops the actor.
    pub async fn close(self) -> Result<()>;
    pub fn session_id(&self) -> Option<data::SessionId>;
}

#[derive(Debug, Clone)]
pub struct UserInput { pub text: String /* future: attachments */ }
impl UserInput { pub fn text(s: impl Into<String>) -> Self; }

pub(crate) enum Command {
    Submit   { input: UserInput, reply: oneshot::Sender<Result<TurnId>> },
    Cancel   { drop_queue: bool },
    Retry    { reply: oneshot::Sender<Result<TurnId>> },
    Rewind   { req: RewindRequest, reply: oneshot::Sender<Result<RewindReport>> },
    Notify   { notice: Notice },
    Rename   { title: String, reply: oneshot::Sender<Result<()>> },
    Background { ticket: BackgroundTicket, fut: BoxFuture<'static, Result<BackgroundResult, String>>,
                 token: CancellationToken },
    Close    { reply: oneshot::Sender<Result<()>> },
}

/// Single consumer. Implements Stream by polling the inner mpsc::Receiver.
pub struct EventReceiver { rx: mpsc::Receiver<CoreEvent> }
impl futures_util::Stream for EventReceiver { type Item = CoreEvent; /* poll_recv */ }
impl EventReceiver { pub async fn recv(&mut self) -> Option<CoreEvent>; }
```

Actor loop:

```text
SessionActor::run(mut self):
  emit SessionOpened (+ load notices)
  loop {
    select! {
      biased;
      Some(res) = self.background.join_next()  → route to on_notice(BackgroundFinished|Failed)
      cmd = self.rx.recv() → match cmd {
        None | Close         → break (after draining queue? no: queued submits are dropped, with Notice::Info)
        Submit{input,reply}  → id = next_turn(); queue.push_back((id, User(fold_notices(input)))); reply.send(Ok(id))
        Retry{reply}         → check retryable; queue.push_back((id, Retry))
        Cancel{..}           → (idle) no-op; drop_queue → queue.clear()
        Notify{n}            → on_notice(n)
        Rewind{req,reply}    → reply.send(self.rewind(req).await)        // idle only; running ⇒ queued like a turn
        Rename{..}           → persister.rename
        Background{..}       → self.background.spawn(ticket, fut, token)
      }
    }
    while let Some((id, input)) = queue.pop_front() {
      flush_unsynced().await
      let cancel = session_token.child_token()
      TurnDriver{ .., cmd_rx: &mut self.rx, cancel }.run(input).await
      //   while running, the driver's select! handles Cancel directly, pushes Submit/Retry onto `queue`
      //   (shared through &mut), applies Notify/Rename/Background immediately, and defers Rewind/Close
      //   until after TurnEnded.
    }
  }
  shutdown: session_token.cancel(); background.shutdown().await (2s grace, then abort); flush_unsynced()
```

`on_notice(n)`:
- Always emits `CoreEvent::Notice(n)`.
- For `BackgroundFinished` and `BackgroundFailed`, pushes the notice to
  `pending_notices`.
- If `config.background_followup == AutoTurn` and the actor is idle,
  enqueues `Followup(fold_notices(empty))`.

`fold_notices(input)` builds the user `Message` with one `Text` block
per pending notice, *before* the user's text. Each block reads
`"[Background task {id} \"{label}\" finished]\n{summary}"`. The
pending list is then cleared, and the ticket ids are recorded in
`MessageMeta.notices`.

## 9. Checkpoint integration (`checkpoint.rs`)

### 9.1 Manager

```rust
pub(crate) struct CheckpointManager {
    store: Arc<data::CheckpointStore>,
    links: data::CheckpointLinkStore,
    enabled: bool,                             // settings.preferences.checkpoints_enabled
}

/// Per-call state: which paths were snapshotted (PreEdit) and must be PostEdit-recorded.
#[derive(Clone)]
pub(crate) struct CallScope {
    mgr: Arc<CheckpointManager>,
    ctx: data::EditContext,                    // session_id, tool_call_id, tool_name, label = plan summary
    message_seq: Option<data::MessageSeq>,     // the assistant message's seq
    touched: Arc<tokio::sync::Mutex<BTreeSet<PathBuf>>>,
    events: EventTx, turn: TurnId, call_id: CallId,
}

impl CheckpointManager {
    pub(crate) fn begin_call(self: &Arc<Self>, ctx: data::EditContext, seq: Option<data::MessageSeq>, ..) -> CallScope;
    pub(crate) async fn rewind(&self, session: data::SessionId, to_user_seq: data::MessageSeq, scope: &RewindScope)
        -> Result<RewindReport>;
}

impl CallScope {
    /// spawn_blocking(store.record(&ctx, PreEdit, new_paths)) → link insert → CheckpointRecorded event.
    /// Ok(()) and no work when !enabled or every path is already in `touched`.
    pub(crate) async fn pre_edit(&self, paths: &[PathBuf]) -> Result<(), ToolError>;
    /// spawn_blocking(store.record(&ctx, PostEdit, touched)) → link insert → event. Errors → Notice::Warning.
    pub(crate) async fn post_edit(&self);
    pub(crate) fn recorder(&self) -> EditRecorder;
}
```

Per artha-data's contract, `record` returns `Ok(None)` when nothing
changed. Core then inserts no link and emits no event. `pre_edit`
resolves relative paths against the workdir before calling `record`.
artha-data rejects paths outside the workdir, and that error becomes
`ToolError::CheckpointFailed`.

`post_edit` runs even when `execute` failed or was cancelled, because a
half-written file is exactly the case where undo matters.

Link written for each recorded checkpoint:

```rust
data::CheckpointLink {
    checkpoint: cp.id, workdir_hash: store.workdir_hash().clone(),
    session_id: Some(session), message_seq: seq /* assistant seq */,
    tool_call_id: Some(call_id), kind: cp.kind, created_at: now,
}
```

### 9.2 Rewind API

```rust
#[derive(Debug, Clone)]
pub struct RewindRequest {
    /// Seq of a *user* message. History is truncated to before it, and files are restored to
    /// their state just before the first agent edit at or after it.
    pub to_user_seq: data::MessageSeq,
    pub scope: RewindScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewindScope { Both, ConversationOnly, FilesOnly }

#[derive(Debug, Clone, Serialize)]
pub struct RewindReport {
    pub truncated_messages: u64,
    pub restored: Vec<PathBuf>,
    pub deleted: Vec<PathBuf>,
    /// PreRestore safety checkpoints (one per restore group), for undoing the rewind.
    pub safety_checkpoints: Vec<String>,
    /// The text of the rewound-to user message, so the UI can put it back into the input box.
    pub restored_prompt: Option<String>,
}
```

### 9.3 Rewind algorithm (per path)

```
rewind(session, N, scope):
  require messages[seq == N].role == User (not tool results)        else BadRewindTarget(N)
  if scope != ConversationOnly:
     require file_rewind == Enabled                                    else RewindUnavailable(reason)
     links = CheckpointLinkStore::for_session(session)
               .filter(kind == PreEdit && message_seq >= N)
               .sort_by(message_seq, created_at)
     first_for: BTreeMap<PathBuf, CheckpointId> = {}
     for link in links:
        cp = CheckpointStore::get(link.checkpoint)?                     // Checkpoint.files = paths in this record
        for f in cp.files: first_for.entry(f).or_insert(cp.id)
     groups = invert(first_for): CheckpointId → Vec<PathBuf>
     for (id, files) in groups (ascending creation order):
        report += CheckpointStore::restore(&id, &RestoreScope::Files(files))?   // each takes its own PreRestore
  if scope != FilesOnly:
     Persister::truncate_after(session, N - 1)
     session.truncate_after(N - 1)
  emit RewindCompleted(report)
```

All store calls run in one `spawn_blocking` closure, so a rewind is a
single blocking unit. If a restore group fails midway, the files
already restored stay restored. The error returns with the partial
report, and the `PreRestore` checkpoints taken so far are listed, so the
user can undo the partial restore. The conversation is truncated only
after every file restore has succeeded.

Why per path: `CheckpointStore` trees are cumulative from the tip, so
the tree of the earliest pre-edit commit after N lacks every file that
was first touched later. `RestoreScope::All` from that commit would
delete those files, even if they existed before Artha touched them.
Each path's own first `PreEdit` at or after N holds that path's
pre-agent content, or its absence, which correctly means "the agent
created it, so delete it". This is HLD §11 open item 2.

`/undo` (the last edit only) is
`rewind`-like, using `CheckpointStore::list(filter{session, limit})` to
find the most recent `PreEdit`, then `restore(Files(cp.files))`. It is
exposed as `RewindScope::FilesOnly` with `to_user_seq` set to the last
user message.

## 10. Background work (`background.rs`)

```rust
pub(crate) struct BackgroundTasks {
    set: tokio::task::JoinSet<(BackgroundTicket, Result<BackgroundResult, String>)>,
    session_token: CancellationToken,
    by_id: HashMap<String, CancellationToken>,   // cancel a single task (future: /tasks cancel <id>)
    next: u64,
}
impl BackgroundTasks {
    pub(crate) fn spawn(&mut self, t: BackgroundTicket, fut: BoxFuture<'static, Result<BackgroundResult, String>>, token: CancellationToken);
    pub(crate) async fn join_next(&mut self) -> Option<(BackgroundTicket, Result<BackgroundResult, String>)>;
    pub(crate) async fn shutdown(&mut self, grace: Duration);
}
```

Sequence for "run a backtest and tell me later":

```
model ─ToolUse(run_backtest)─▶ Dispatcher ─exec─▶ tool.execute(input, ctx)
                                                  ticket = ctx.background().spawn("Backtest …", |tok| async move { … })
                                                  return Ok(ToolOutput::text("Started backtest bg-3.").with_background(ticket))
       ◀── ToolResult("Started backtest bg-3. [background task bg-3 started; …]") ── loop continues, turn ends normally
       … user keeps chatting …
JoinSet completes ─▶ actor.on_notice(BackgroundFinished{bg-3, summary}) ─▶ CoreEvent::Notice (UI toast)
next submit ─▶ fold_notices: user Message = [Text("[Background task bg-3 … finished]\n<summary>"), Text(user text)]
```

Durable variant: the tool inserts a `jobs_job` through the artha-jobs
API and returns `ctx.background().durable_ticket(job_id, label)`. If
the job's runner is in the same process (the TUI hosts a jobs runner),
it calls `SessionHandle::notify(BackgroundFinished{..})` on completion.
Otherwise the result is visible through `artha jobs logs <id>`. The
summary cap (4 KiB) keeps a chatty job from flooding the next request.

## 11. System prompt (`prompt.rs`)

```rust
#[async_trait::async_trait]
pub trait PromptContributor: Send + Sync + 'static {
    /// Stable id for ordering and logs, e.g. "memory".
    fn id(&self) -> &str;
    /// Called once per turn (not per round). Return None to contribute nothing. Must be fast (<100 ms);
    /// it runs under a 500 ms timeout, and a timeout or Err only drops the section (warn log).
    async fn contribute(&self, inputs: &PromptInputs<'_>) -> Result<Option<String>, String>;
}

pub struct PromptInputs<'a> {
    pub session_id: Option<data::SessionId>,
    pub workdir: &'a Path,
    pub user_text: &'a str,          // this turn's user text, for relevance-based recall
    pub tool_names: &'a [String],
    pub today: data::Timestamp,
}

pub(crate) struct SystemPromptBuilder { base: Cow<'static, str>, contributors: Vec<Arc<dyn PromptContributor>> }
impl SystemPromptBuilder {
    /// base + "\n\n## Environment\n- Date: …\n- Working directory: …\n- Mode: …"
    ///      + one "## {id}" section per contributor (sorted by id, for a stable prefix).
    pub(crate) async fn build(&self, inputs: &PromptInputs<'_>) -> String;
}
```

The date is rendered at day granularity, so the prompt-cache prefix is
stable within a day. The base prompt is compiled in
(`include_str!("prompt/base.md")`; the file is created in the
implementation pass) and states Artha's finance-assistant role, its
tool-use etiquette, and the "not financial advice" framing.

## 12. Configuration (`config.rs`)

```rust
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentConfig {
    pub generation: llm::GenerationOptions,     // max_tokens, thinking, …
    pub tool_choice: llm::ToolChoice,           // Auto
    pub limits: LoopLimits,
    pub dispatch: DispatchConfig,
    pub permissions: PermissionPolicyConfig,
    pub checkpoints_enabled: bool,              // settings.preferences.checkpoints_enabled
    pub background_followup: BackgroundFollowup,
    pub event_buffer: usize,                    // 1024
}

#[derive(Debug, Clone, Copy)]
pub struct LoopLimits {
    pub max_rounds: u32,          // 24
    pub stream_retries: u32,      // 2
    pub cancel_grace: Duration,   // 2s
}

#[derive(Debug, Clone, Copy)]
pub struct DispatchConfig {
    pub max_parallel_tools: usize,  // 4
    pub eager_start: bool,          // true
    pub max_result_bytes: usize,    // 64 KiB
    pub repeat_error_limit: u32,    // 3
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BackgroundFollowup { #[default] NextTurn, AutoTurn }

impl AgentConfig {
    /// Interactive defaults from settings (permission mode, checkpoints flag).
    pub fn from_settings(s: &data::Settings) -> Self;
    /// artha-jobs: NonInteractive permissions, max_rounds 12, eager on, AutoTurn off.
    pub fn unattended(allow: BTreeSet<String>) -> Self;
}
```

`artha-cli` applies `--skip-permissions`/`--yolo` by setting
`permissions = Interactive(PermissionMode::Skip)` after
`from_settings`.

## 13. Testing strategy

All loop tests are deterministic. They use no network, no real clock
(`tokio::time::pause()`), and no real home directory.

### 13.1 Test doubles (`testing.rs`, feature `test-util`)

```rust
/// Scripted tool: each call pops the next step. Records every (input, ctx-ids) it saw.
pub struct FakeTool { spec: llm::ToolSpec, traits: ToolTraits, plan: Option<ToolPlan>,
                      script: Arc<Mutex<VecDeque<ToolScript>>>, calls: Arc<Mutex<Vec<serde_json::Value>>> }

pub enum ToolScript {
    Ok(ToolOutput),
    Err(ToolError),
    /// Sleep (virtual time), then the inner step. Observes ctx.cancel → Err(Cancelled).
    Delay(Duration, Box<ToolScript>),
    /// Ignores cancellation (exercises grace_then_abort).
    Stubborn(Duration),
    Panic(&'static str),
    /// Calls ctx.edits().before_write(paths) then writes `contents` to each (file tools).
    WriteFiles(Vec<(PathBuf, String)>),
    /// ctx.background().spawn(..) completing after `after` with `summary`.
    Background { after: Duration, summary: String },
    Progress(Vec<String>, Box<ToolScript>),
}

impl FakeTool {
    pub fn new(name: &str) -> Self;                        // ReadOnly, {"type":"object"} schema
    pub fn risk(self, r: ToolRisk) -> Self;
    pub fn serial(self) -> Self;                           // concurrency_safe = false
    pub fn timeout(self, d: Duration) -> Self;
    pub fn plan(self, p: ToolPlan) -> Self;
    pub fn then(self, s: ToolScript) -> Self;
    pub fn calls(&self) -> Vec<serde_json::Value>;
}

/// Collects events; helpers for assertions and insta snapshots (responder skipped; ids normalized).
pub struct EventLog { pub events: Vec<CoreEvent> }
impl EventLog {
    pub fn assert_well_formed(&self);                      // §4 order guarantees
    pub fn text(&self, turn: TurnId) -> String;            // concatenated TextDeltas (minus discarded rounds)
    pub fn outcome(&self, turn: TurnId) -> &TurnOutcome;
    pub fn tool_outcomes(&self) -> Vec<(String, ToolOutcome)>;
}

/// Agent + in-memory Db + TestEnv checkpoint store + ScriptedProvider, wired together.
pub struct Harness {
    pub agent: Agent,
    pub provider: llm::testing::ScriptedProvider,
    pub env: data::testing::TestEnv,
    pub db: data::Db,
}
impl Harness {
    pub fn new(turns: impl IntoIterator<Item = llm::testing::TurnScript>, tools: Vec<Arc<dyn Tool>>) -> Self;
    pub fn with_config(self, f: impl FnOnce(&mut AgentConfig)) -> Self;
    /// Opens a session; a background task drains events into an EventLog. Permission prompts are answered
    /// by `answers` in order (default: AllowOnce for each).
    pub async fn open(&self, sel: SessionSelector, answers: Vec<PermissionDecision>) -> (SessionHandle, LogHandle);
    /// submit + wait for TurnEnded.
    pub async fn turn(&self, h: &SessionHandle, log: &LogHandle, text: &str) -> TurnOutcome;
    pub fn stored(&self, id: data::SessionId) -> Vec<PersistedMessage>;  // decoded core_message rows
}

/// Standalone ToolContext for unit-testing a tool without the loop: recording ProgressSink,
/// EditRecorder bound to `env`'s checkpoint store, a fresh CancellationToken, a fake SessionId.
pub fn tool_context(env: &data::testing::TestEnv, tool_name: &str) -> (ToolContext, ContextProbe);
/// Inspects what the tool did through its context: progress messages, declared edits, tickets.
pub struct ContextProbe { /* … */ }
```

`ScriptedProvider` (artha-llm `test-util`) runs the real
`validate_request`, so every loop test also checks history pairing on
each request. `ScriptedProvider::requests()` lets tests assert exactly
what was sent: the tool-results order, compacted bodies, and the
system prompt reused across rounds.

### 13.2 Scenario matrix

| File | Scenarios |
|---|---|
| `loop_basic.rs` | text-only turn (1 round, `Completed`); single tool round trip (2 requests; second contains `tool_use` plus `tool_result`); 3 parallel read-only calls finish out of order, results in call order; mixed P and S calls (P before S, S in order); `PauseTurn` → extra round; `max_rounds = 2` with the model always calling tools → `RoundLimit`, history valid; `MaxTokens` without calls → `MaxTokens`; `MaxTokens` with a truncated call → `is_error` result plus another round; `Refusal` → `Refused`; the system prompt and tool specs are byte-identical across rounds of one turn |
| `loop_errors.rs` | `ToolError::Failed` → `is_error` text and the loop continues; `InvalidInput` wording; unknown tool lists available names; malformed args are not executed (`FakeTool.calls()` empty); timeout (virtual clock) → `TimedOut`; `Stubborn` tool aborted after grace; panic → generic text, session still usable on the next turn; repeat-error note on the 3rd identical failure; `chat()` `Err(Auth)` → `Failed` with `reauth_required`, user message persisted, `retry()` re-runs with no duplicate user message; `ContextLengthExceeded` → compaction → success (request 2 has elided bodies, DB unchanged); a second `ContextLengthExceeded` → `Failed`; `FailAfter(Truncated)` → `RoundRetrying{discard_partial}` → success; `EventLog::text` excludes the discarded text; non-retryable mid-stream → `Failed`, no assistant row; result truncation at `max_result_bytes` with a multi-byte boundary |
| `cancel.rs` | **property test.** For a 3-round script with tools, cancel at event index k for every k in 0..len. Assert: `TurnEnded(Cancelled)`; every `ToolCallReady` of a committed round has a `ToolFinished`; DB history decodes, and with one extra user message appended it passes `validate_request` (via a fresh `ScriptedProvider` request); no tasks leaked (`FakeTool` side-effect counters stop). Plus: cancel during a permission prompt → `Deny` then `Cancelled`; partial text persisted with `interrupted`; eager tasks aborted on cancel |
| `permission.rs` | the full §6.5 matrix (risk × mode × confirm × grants); prompts asked in call order before any non-eager execution (`FakeTool` timestamps); `AllowForSession` suppresses the 2nd prompt in the same session but not after re-open; `Deny{feedback}` text reaches the model; `DenyAndStop` → remaining `Skipped`, `Stopped`, history valid; a dropped `EventReceiver` → `Deny`; `NonInteractive` emits no `PermissionRequested`; `plan.writes` non-empty lifts `ReadOnly` to `FileWrite` |
| `persistence.rs` | write order (user, assistant, tool) with seqs; assistant usage columns filled; lazy create (open + close leaves 0 rows); title set after the first turn; `--continue` picks the most recent in the same workdir, ignores other workdirs and archived; `Continue` with none → new + `Notice::Info`; `Resume(bad)` → `SessionNotFound`; crash simulation (stop the actor after the assistant append, before results) → reload repairs with a synthetic result, persists it, `HistoryRepaired`; corrupt content row → `CorruptHistory{seq}`; `PersistedMessage` golden JSON parses as a plain `llm::Message`; `is_busy` injected (two Db handles, one holding IMMEDIATE) → retried, then `unsynced`, flushed before the next turn |
| `checkpoint.rs` (TestEnv temp dir) | a `FileWrite` tool with `plan.writes` → `PreEdit` before execute (file content in commit equals original), `PostEdit` after, two links with `message_seq` = assistant seq; dynamic `before_write` path; checkpoints disabled → no repo created on disk; pre-edit failure (path outside workdir) → tool not executed, `CheckpointFailed` text; post-edit after a tool error still recorded; **rewind per-path regression:** edit `a.txt` in turn 1, edit pre-existing `b.txt` in turn 2, rewind to turn 1 → `a` and `b` restored to their originals and `b` **not deleted**; a file created by the agent is deleted on rewind; `ConversationOnly` leaves files; workdir-mismatch resume → `RewindUnavailable`; the restored prompt is returned |
| `background.rs` | `Background` script → immediate `Ok` with ticket text; `Notice::BackgroundFinished` emitted later (virtual time); the next user message starts with the notice block and `MessageMeta.notices` is recorded; `AutoTurn` starts a turn by itself when idle and waits when busy; close cancels background tasks within grace; external `notify` from a fake jobs runner |

### 13.3 Pure unit tests (in-module)

- `stop::classify`: every `StopReason` × has-tool-uses.
- `into_tool_result`: every row of §3.2, plus truncation edge cases (an
  exact boundary, a multi-byte char at the cut, a `Json` over budget).
- `PermissionPolicy::verdict`: the matrix as a table-driven test.
- `RoundRetryPolicy::delay`: ceilings, floor, exhaustion.
- `Compactor::compact`: keeps pairing, only touches `ToolResult`
  content, keeps the last N rounds intact, is idempotent.
- `repair`: tail dangling, partially answered, orphan results, already
  valid (no-op).
- `rewind` grouping (`first_for` / `invert`) on synthetic link lists.
- `derive_title`: unicode, long first lines, empty input.
- `fold_notices`: ordering, 4 KiB cap.
- `EventLog::assert_well_formed`: self-tests that it rejects hand-built
  bad sequences.

### 13.4 Snapshots

Each `loop_basic` scenario also snapshots its normalized `EventLog` as
JSON with `insta`. Call ids, checkpoint ids, and timings are
normalized. This pins the event contract that `artha-tui` and
`artha ask --json` rely on. Changing a snapshot means changing the UI
contract, and the change is reviewed as such.

### 13.5 Downstream contract

- `artha-tui` tests use `Harness` (feature `test-util`) to drive the UI
  from real `CoreEvent` sequences.
- `artha-jobs` tests use `Agent::run_turn_headless` with
  `AgentConfig::unattended` and assert that no `PermissionRequested` is
  ever produced.
- `artha-tools` uses `testing::tool_context()`, a standalone
  `ToolContext` with a recording `ProgressSink` and an `EditRecorder`
  bound to a `TestEnv` checkpoint store, to unit-test tools without the
  loop.
