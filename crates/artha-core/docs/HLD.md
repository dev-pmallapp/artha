# artha-core — High-Level Design

## 1. Purpose and scope

`artha-core` is the agent. It turns "a user said something" into "the model
answered, possibly after calling tools many times", and it keeps the
session and the user's files consistent while doing so. Both front ends
(`artha-tui` and headless `artha ask`) and the background runner
(`artha-jobs`) drive the same engine. There is one chat loop in Artha, and
it lives here.

In scope:

- **The chat loop.** It sends a `ChatRequest`, consumes the
  `artha_llm::EventStream`, commits the assembled assistant message, and,
  when the turn stopped for tool use, dispatches the calls, appends the
  results and calls `chat` again. It owns the stop-reason policy
  (`ToolUse`, `PauseTurn`, `MaxTokens`, `Refusal`, …), the round limit,
  cancellation, and orchestration-level retries.
- **The `Tool` contract.** This covers the trait that `artha-tools`
  built-ins and `artha-mcp` bridged tools implement, the `ToolRegistry`
  they are registered in, the `ToolContext` they receive, and how a
  `ToolOutput` becomes an `artha_llm::ToolResult`.
- **Tool dispatch.** Resolution, argument checks, the **permission
  seam**, checkpointing around file edits, timeouts, panic isolation,
  parallelism, and result truncation.
- **The session lifecycle.** The in-memory `Session`, write-ahead
  persistence through `artha_data::SessionStore`, `--continue` and
  `--resume`, history repair after a crash, and rewind.
- **Checkpoint policy.** This crate decides *when* to snapshot (around
  tool calls that write files) and *what* to restore on rewind.
  `artha-data` provides only the git plumbing.
- **The outward event surface.** `CoreEvent`, a single typed stream that
  every UI renders and that `artha ask --json` can print as JSON Lines.
- **Background-work hand-off.** Tools start long work and return right
  away. The session later receives a `Notice` when the work finishes.

Out of scope:

- Wire formats, HTTP, provider retries before the first byte, and SSE
  assembly. Those belong to `artha-llm`, which this crate consumes through
  `Arc<dyn LlmProvider>`.
- Tool implementations: markets, macro, news, memory, portfolio, and
  file editing (`artha-tools`), and MCP transport and name mangling
  (`artha-mcp`).
- Rendering, key handling, and terminal prompts (`artha-tui`, `artha-cli`).
  Core never reads the terminal or writes to it.
- Cron scheduling and the durable job queue (`artha-jobs`). Core only
  exposes the hooks that `artha-jobs` needs to run headless turns and
  post completion notices.
- Storage mechanics: SQL, migrations, and git objects (`artha-data`).
- Settings loading and CLI-flag precedence (`artha-cli`). Core receives an
  already-resolved `AgentConfig`.

## 2. Position in the workspace

```
      artha-cli ─────────┬─────────────┬──────────────┐
   (composition root)    │             │              │
                         ▼             ▼              ▼
                    artha-tui     artha-jobs     (ask: headless
                         │             │           renderer in cli)
                         └──────┬──────┴──────────────┘
                                ▼
                           artha-core ◀──── implements Tool ──── artha-tools
                          ┌─────┴─────┐                          artha-mcp
                          ▼           ▼
                      artha-llm   artha-data
```

The dependency rules are strict:

- `artha-core` depends on `artha-llm` and `artha-data` only.
- `artha-tools`, `artha-mcp`, `artha-jobs`, and `artha-tui` depend on
  `artha-core`. Core never depends on them.
- `artha-cli` builds the concrete provider, registers the tools into a
  `ToolRegistry`, and hands both to `Agent::builder()`.

That is why the `Tool` trait is defined here and not in `artha-tools`.
The crate that *calls* tools owns their interface, and every tool source
(built-in, MCP, and later plugins) plugs into the same seam.

## 3. Public API surface

| Item | Kind | Used by |
|---|---|---|
| `Agent`, `AgentBuilder` | shared engine (provider, tools, stores, config); cheap `Clone` | `artha-cli`, `artha-jobs` |
| `AgentConfig`, `LoopLimits`, `DispatchConfig`, `PermissionMode` (re-export), `BackgroundFollowup` | configuration | `artha-cli` (from settings + flags), `artha-jobs` |
| `SessionSelector` (`New`, `Continue`, `Resume(id)`) | how to open a session | `artha-cli` (`-c`, `-r`) |
| `SessionHandle` | per-session control: `submit`, `cancel`, `retry`, `rewind`, `notify`, `close` | `artha-tui`, `artha ask`, `artha-jobs` |
| `EventReceiver` (`Stream<Item = CoreEvent>`) | outward event stream | `artha-tui`, `artha ask` |
| `CoreEvent` and its payload types (`TurnOutcome`, `ToolCallView`, `ToolOutcome`, `PermissionPrompt`, `Notice`, …) | event model (serde) | UIs, `ask --json` |
| `PermissionResponder`, `PermissionDecision` | answer to a permission prompt | UIs |
| `run_turn_headless` | one-shot convenience: open, submit, drain, close | `artha ask`, `artha-jobs` |
| `SessionInfo`, `list_sessions` | session picker data for `--resume` | `artha-tui`, `artha-cli` |
| `Tool`, `ToolTraits`, `ToolRisk`, `ToolPlan` | **tool contract** | `artha-tools`, `artha-mcp` |
| `ToolOutput`, `ToolError`, `ToolDisplay` | tool results | tool implementors |
| `ToolContext`, `EditRecorder`, `ProgressSink`, `BackgroundSpawner` | what a running tool may use | tool implementors |
| `ToolRegistry`, `ToolSet` | registration and per-turn snapshot | `artha-cli`, `artha-plugins` (enable/disable) |
| `TypedTool` (feature `schemars`) | serde-typed tool adapter | `artha-tools` |
| `PromptContributor` | system-prompt extension hook (for example, memory recall) | `artha-tools` (memory) |
| `RewindRequest`, `RewindScope`, `RewindReport` | rewind and undo | UIs |
| `CoreError`, `Result` | errors | everyone above |
| `testing::{FakeTool, ToolScript, EventLog, Harness}` | feature `test-util` | core tests, `artha-tui` and `artha-jobs` tests |

Typical use by a front end:

```rust
let agent = Agent::builder(provider, registry, db, checkpoints, config).build()?;
let (session, mut events) = agent.open(SessionSelector::Continue).await?;
session.submit(UserInput::text("How did NVDA close vs. its 50-day SMA?")).await?;
while let Some(ev) = events.next().await {
    match ev {
        CoreEvent::TextDelta { text, .. } => ui.append(&text),
        CoreEvent::PermissionRequested(p) => ui.ask(p),     // later: p.responder.respond(..)
        CoreEvent::TurnEnded { .. } => break,
        _ => ui.render(&ev),
    }
}
```

## 4. The chat loop

### 4.1 Contract inherited from `artha-llm`

- A tool call **ends the model's turn**. The stream finishes with
  `MessageEnd { stop_reason: ToolUse, message, usage }`. To continue, core
  calls `chat` again with the assistant message and one user-role
  tool-results message appended. Nothing is paused or resumed inside
  `artha-llm`.
- `chat` returns `Err` only after its own retries, which all happen
  before the first byte. An `Err` *inside* the stream is final for that
  stream and is never retried by `artha-llm`.
- Tool arguments arrive fully assembled in `ToolCallEnd`. Malformed JSON
  is delivered as a `ToolCall` with `malformed` set. It is not a stream
  error.
- `validate_request` rejects histories that break tool-use/tool-result
  pairing. Core must therefore never send a history with an unanswered
  `ToolUse`, even after a crash or a cancel.
- Dropping the stream aborts generation. That *is* the cancel mechanism.

### 4.2 Vocabulary

- A **turn** starts with one user submission and ends when the model
  stops without asking for tools, or on cancel, failure, or a limit. It
  has a `TurnId`.
- A **round** is one `chat` call plus its stream, followed by the
  dispatch of the tools it requested. A turn has 1..=`max_rounds` rounds.
- A **call** is one `ToolUse` block, identified by the model's
  `tool_use` id (`CallId`).

### 4.3 Shape of one turn

```
submit(user) ─▶ persist user msg ─▶ build system prompt + snapshot ToolSet
   ┌──────────────────────────────────────────────────────────────────┐
   ▼                                                                  │
 Requesting ──chat()──▶ Streaming ──MessageEnd──▶ Committing          │
   │ ctx-length: compact once, retry          │ persist assistant msg │
   │                                          ▼                       │
   │                stop_reason = ToolUse ─▶ Dispatching ─────────────┤
   │                PauseTurn ────────────────────────────────────────┘
   │                EndTurn / StopSequence ─▶ Finished(Completed)
   │                MaxTokens (no calls) ───▶ Finished(MaxTokens)
   │                Refusal / ContentFilter ▶ Finished(Refused)
   └─ any state ──cancel──▶ Cancelling ─▶ repair + persist ─▶ Finished(Cancelled)
```

`Dispatching` persists the tool-results message and loops back to
`Requesting` unless the round limit has been reached. The exact states,
events, and transitions are specified in LLD §5.

### 4.4 Key loop decisions

- **Write-ahead persistence at message granularity.** The user message
  is persisted on submit. The assistant message is persisted at
  `MessageEnd`, *before* any tool runs, and the tool-results message is
  persisted after all of the round's calls have finished. This gives the
  assistant message a real `message_seq` before tools run, so checkpoint
  links can point at it, and a crash loses at most the in-flight round.
  The history that results from a crash or cancel always has the shape
  "assistant with tool_use, no results yet". That is detected and
  repaired on load (§6.4).
- **Stop-reason policy lives here.** `PauseTurn` re-requests at once and
  counts as a round. `MaxTokens` with tool calls dispatches them; the
  truncated ones have `malformed` set and come back as `is_error` results,
  which lets the model retry with shorter arguments. `MaxTokens` without
  calls ends the turn so the UI can offer "continue".
- **Bounded autonomy.** `max_rounds` (default 24) caps model calls per
  turn. On the limit the turn ends with `RoundLimit` after the last
  round's results are persisted, so history stays valid and the user can
  say "keep going".
- **Stable request prefix.** The system prompt and the `ToolSet` are
  computed once per turn and reused for every round. Tools registered
  mid-turn (an MCP server finishing its handshake) appear in the next
  turn. This keeps `artha-llm`'s prompt-cache breakpoints hitting across
  tool round trips.
- **No history copy per round.** `Session` stores `Vec<Message>`
  contiguously, so `ChatRequest::new(&session.messages)` borrows it
  directly. That matches `artha-llm`'s borrowed-request design.

## 5. The `Tool` contract

### 5.1 Trait

```rust
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    fn spec(&self) -> &ToolSpec;                       // name, description, JSON Schema
    fn traits(&self) -> ToolTraits { ToolTraits::default() }
    fn plan(&self, input: &Value, ctx: &PlanContext<'_>) -> Result<ToolPlan, ToolError> { Ok(ToolPlan::default()) }
    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError>;
}
```

- **`spec`** reuses `artha_llm::ToolSpec` unchanged, so registration
  goes straight into `ChatRequest::tools` with no conversion.
  `ToolRegistry::register` runs `ToolSpec::validate` and rejects
  duplicate names. Name-legality mapping for MCP tools stays in
  `artha-mcp`.
- **`traits`** is static metadata that core needs for policy:
  - `risk`: `ReadOnly`, `LocalWrite`, `FileWrite`, `External`, or
    `Execute`. This is the input to the permission policy.
  - `concurrency_safe`: whether the tool may run in parallel with other
    calls.
  - `timeout`: the per-call deadline.
  - `confirm`: an override that forces a prompt or skips one.
- **`plan`** is synchronous, cheap, and pure. It runs *before*
  permission. It returns the human-readable summary for the prompt
  ("Write 42 lines to `strategy.py`"), and the files the call intends to
  write, which core snapshots before execution. Tools that only discover
  their paths while running can also declare them dynamically through
  `ctx.edits()`.
- **`execute`** gets owned JSON input and an owned `ToolContext`, so the
  future is `'static` and can be spawned. It returns either a
  `ToolOutput` (content for the model plus an optional richer display
  for the UI) or a `ToolError`.

The detailed contract (each type, defaults, and the MCP mapping) is in
LLD §3.

### 5.2 Result serialization

`ToolOutput` maps onto `artha_llm::ToolResult` like this:

| Tool returned | `ToolResult` sent in the next `chat` |
|---|---|
| `Ok(ToolOutput { content, .. })` | `{ tool_use_id, content, is_error: false }` |
| `Err(ToolError::InvalidInput(msg))` | `is_error: true`, text `"Invalid arguments: {msg}"` |
| `Err(ToolError::Failed { message, .. })` | `is_error: true`, text `message` |
| `Err(ToolError::Denied { .. })` (produced by core) | `is_error: true`, text `"The user declined this tool call. {feedback}"` |
| `Err(ToolError::Cancelled)` | `is_error: true`, text `"Cancelled by the user before completion."` |
| timeout, panic, unknown tool, malformed args (produced by core) | `is_error: true`, with a specific message |

Rules:

- `content` is `Vec<ToolResultContent>`, so `Text` or `Json`. Structured
  finance data should be returned as `Json`. The codecs render it as
  compact JSON for both wire formats.
- Results are capped at `max_result_bytes` (64 KiB by default) **per
  call**. Anything longer is cut on a UTF-8 boundary and ends with a
  `"[truncated: N bytes omitted]"` note. Without the cap, one runaway
  tool could blow the context window. Large data belongs in a file or a
  report, which the tool can mention by path.
- `ToolDisplay` (tables, diffs, charts-as-text) goes only to the UI
  through `CoreEvent::ToolFinished`. It is never sent to the model and is
  not persisted.
- All results of a round go into **one** user-role message, in the order
  the calls appeared (`Message::tool_results`). That holds even when the
  calls completed out of order.

### 5.3 Failure isolation

A tool can never take down the session:
- Each `execute` runs in its own `tokio::spawn` task, so a panic becomes
  a `JoinError` and then an `is_error` result.
- Each runs under `tokio::time::timeout(traits.timeout)`.
- Each gets a `CancellationToken` that is a child of the turn's token.

A tool that blocks a reactor thread is a bug in that tool. The contract
says to use `spawn_blocking` or `artha_data::Db::call`.

## 6. Session model

### 6.1 In memory

A `Session` is owned by exactly one `SessionActor` task. It holds:
- the identity and metadata that mirror `core_session`: id, title,
  workdir, mode, provider, model, and parent;
- the history as parallel vectors, `messages: Vec<Message>` and
  `entries: Vec<EntryMeta>`, where `EntryMeta` holds the seq, the
  interrupted and synthetic flags, and usage;
- the session's permission grants, kept in memory only;
- pending background notices;
- running usage totals.

### 6.2 Persistence mapping (onto `artha-data`)

| Core concept | `artha-data` call | Notes |
|---|---|---|
| first submit in a new session | `SessionStore::create(NewSession{ workdir, mode, provider: info.format.as_str(), model, title: None, .. })` | **Lazy.** Launching and quitting leaves no empty session row. |
| title | `SessionStore::rename` after the first turn | The first user line, trimmed to 60 characters. |
| user message | `append(id, [user])` | `Role::User`. |
| assistant message | `append(id, [assistant])` with `input_tokens`/`output_tokens` from `MessageEnd.usage` | `Role::Assistant`. |
| tool results | `append(id, [results])` | `Role::Tool` (because `is_tool_results()` is true). |
| message content | `serde_json::to_value(PersistedMessage{ message, meta })` | `meta` is flattened under an `"artha"` key and omitted when empty. The JSON stays readable as a plain `artha_llm::Message`. |
| `--continue` | `most_recent(workdir)` | |
| `--resume` (picker) | `list(SessionFilter{ workdir: Some(cwd), .. })` | |
| `--resume <id>` | `get(id)` then `messages(id)` | |
| rewind conversation | `truncate_after(id, N-1)` | |
| fork ("branch from here") | `fork(id, through)` | |

All `SessionStore` calls are synchronous. Core runs them on
`spawn_blocking` through a small `Persister`, never on the reactor.

### 6.3 Resume semantics

- `-c/--continue` uses `SessionSelector::Continue`, which picks the most
  recent non-archived session for the canonical cwd. If there is none,
  core starts a new session and emits
  `Notice::Info("no previous session in this directory")`. That is not
  an error, because `artha -c` in a new directory should just work.
- `-r/--resume` with no id is a front-end concern. The UI calls
  `list_sessions` and shows a picker, then calls
  `open(Resume(id))`. `artha ask -r` without an id is an error, since
  there is nothing to pick with.
- `-r <id>` loads exactly that session. If its workdir differs from the
  cwd, the conversation loads, but file rewind is disabled for the
  session (checkpoints are per-workdir) and a `Notice::Warning` explains
  this.
- **Provider and model may change on resume.** The history is
  provider-neutral by `artha-llm`'s design. The session row keeps its
  creation-time provider; see the open item in §11.

### 6.4 Repair on load

The loader walks the history once:
1. It decodes each `content` into `PersistedMessage`. A message that
   fails to decode is a hard `CoreError::CorruptHistory { seq }`. We
   never skip it silently, because dropping a `tool_use` would break
   pairing anyway.
2. An assistant message whose `tool_use` ids have no `ToolResult` in the
   next message is a crashed or cancelled round. Core appends a
   synthetic tool-results message ("not executed: Artha exited before
   this tool call completed"), flagged `synthetic`. It persists that
   message so the database also becomes valid, then continues.
3. The result is run through the same pairing rules `artha-llm`
   enforces. A test pins that every reachable crash point produces a
   history that passes `validate_request`.

## 7. Checkpointing

`artha-data` provides `CheckpointStore::record(ctx, kind, files)`,
`restore`, `list`, `get`, and `CheckpointLinkStore`. **The policy is
here.**

- **When to snapshot.** Only around a tool call that will write files,
  and only when `preferences.checkpoints_enabled` is true. A tool
  announces its paths through `ToolPlan.writes` (known before execution)
  and/or through `ctx.edits().before_write(&paths).await` (discovered
  during execution). Core performs:
  1. `record(PreEdit, paths)` before the write, plus a
     `CheckpointLink { session, message_seq: <assistant seq>, tool_call_id, kind: PreEdit }`.
  2. `record(PostEdit, all_paths_touched_by_the_call)` after `execute`
     returns, whether it succeeded or not, plus its link.

  Nothing is recorded at startup, on idle, or for read-only tools,
  following ARCHITECTURE §6.
- **Why core uses `record` and not `EditGuard`.** `EditGuard<'a>`
  borrows the store, so it can't be held across the `'static` spawned
  tool task. The two `record` calls are exactly what the guard does
  internally. The guard is still the right API for synchronous callers.
  Tools never touch `CheckpointStore` directly. They go through
  `EditRecorder`, so every snapshot is linked to its session and message.
- **Fail closed.** If the pre-edit snapshot fails (disk full, or a path
  outside the workdir), the call is not executed and the model receives
  `is_error: "could not snapshot <path> before editing: …"`. An edit we
  can't undo is worse than an edit that didn't happen. A failed
  *post*-edit snapshot is only logged and reported as a `Notice::Warning`,
  because the pre-edit snapshot already guarantees undo.
- **What triggers a restore.** Only an explicit user action:
  `SessionHandle::rewind(RewindRequest { to_user_seq, scope })`, which
  the TUI exposes as `/rewind` and `/undo`. Core never restores
  automatically, not even after a failed tool. A partly applied edit is
  still visible to the user and undoable.
- **Restore algorithm (per path, not per commit).** The checkpoint tree
  is cumulative, so the earliest pre-edit commit after message N does not
  contain files that were first touched later. Restoring it with
  `RestoreScope::All` would therefore *delete* those files. For each path
  touched since N, core restores from that path's **own first pre-edit
  checkpoint at or after N**, grouping paths by checkpoint into
  `RestoreScope::Files`. LLD §9.3 gives the algorithm. The `PreRestore`
  safety snapshot from `artha-data` makes the rewind itself undoable.

## 8. Permission seam

- **Policy input.** It combines:
  - `PermissionMode` from settings (`Ask` or `Skip`, where `Skip` is set
    by `--skip-permissions` / `--yolo`),
  - the tool's `ToolRisk` and its `confirm` override,
  - the session's in-memory grants ("always allow `edit_file` this
    session"),
  - for unattended runs, an explicit `NonInteractive { allow }` list.
- **Default policy in `Ask` mode:**
  - `ReadOnly` runs without a prompt.
  - `LocalWrite`, `FileWrite`, `External`, and `Execute` prompt, unless
    the tool sets `confirm: Some(false)`. Memory saves, for example, are
    low-stakes and frequent.
  - MCP tools map their `readOnlyHint` and `destructiveHint` annotations.
    With no hints, they are `External`.
- **`Skip` mode** allows everything without a prompt and logs each
  auto-approval at `info`.
- **How a prompt reaches the user.** When the policy says *Ask*, core
  emits `CoreEvent::PermissionRequested(PermissionPrompt)` and waits on
  the oneshot inside `PermissionResponder`. The prompt carries the tool,
  risk, summary, input, and planned files. The UI renders it and calls
  `responder.respond(decision)`. The possible decisions are `AllowOnce`,
  `AllowForSession`, `Deny { feedback }`, and `DenyAndStop`.
  - If the responder is dropped unanswered (the UI went away, or a
    headless consumer ignores prompts), the result is `Deny`.
  - Cancelling the turn while a prompt is open also resolves to `Deny`.
- **Headless.** `artha ask` gets the same event. `artha-cli` decides
  how to handle it:
  - a stderr y/n prompt when stdin is a TTY,
  - an automatic deny with a hint to use `--yolo` otherwise.

  `artha-jobs` uses `NonInteractive`, so the policy denies without ever
  emitting a prompt.
- **Ordering.** Prompts are asked one at a time, in call order, before
  any non-eager call runs. The user never sees two dialogs at once.
- **Denied calls** still get a `ToolResult` (`is_error`, carrying the
  user's feedback), and the loop continues. The model can adapt ("OK, I
  won't place that trade"). `DenyAndStop` gives every remaining call a
  "not run" result, persists, and ends the turn with `Stopped`.

## 9. Streaming to the UI: `CoreEvent`

The decision is a **single ordered stream of owned, serde-serializable
events per session**, delivered through a bounded
`tokio::sync::mpsc` channel and exposed as `EventReceiver: Stream<Item = CoreEvent>`.
We chose it over the alternatives for these reasons:
- **A callback trait** (`trait UiSink { fn on_text(..) }`) would run UI
  code on the agent task, force `Send + Sync` UIs, and make headless and
  TUI rendering diverge. A stream lets the TUI `select!` over terminal
  input and core events in one loop.
- **A broadcast channel** drops events for slow receivers. Text deltas
  must not be lost, and there is exactly one renderer per session.
- **Per-turn streams** (`submit()` returning a stream) can't carry
  between-turn events: background notices, session load, rewind results.

Variants, grouped (full definitions in LLD §4):
- **Session:** `SessionOpened`, `SessionPersisted`, `Notice`,
  `RewindCompleted`.
- **Turn lifecycle:** `TurnStarted`, `RoundStarted`, `RoundRetrying`,
  `TurnEnded { outcome, usage }`.
- **Model output:** `TextDelta`, `ThinkingDelta`, `ToolCallStreaming`,
  `ToolCallReady`, `AssistantCommitted`, `Usage`.
- **Tools:** `PermissionRequested`, `ToolStarted`, `ToolProgress`,
  `ToolFinished`, `CheckpointRecorded`.

Rules:
- Every event carries its `TurnId` (and `round` where relevant). UIs can
  ignore stale events after a cancel.
- The channel is bounded at 1024. A UI that stops reading eventually
  applies backpressure all the way to the model stream, and through it to
  TCP. That is intentional: nothing buffers without bound.
- `RoundRetrying { discard_partial: true }` tells the UI to erase text it
  showed from a failed stream before the retry streams fresh text.
- `artha ask --json` serializes each event as one JSON line. The
  responder is skipped.

## 10. Concurrency model

- **One actor task per open session.** `Agent::open` spawns a
  `SessionActor` that owns the `Session` and runs turns sequentially.
  `SessionHandle` is a cheap `Clone` sender of `Command`s: `Submit`,
  `Cancel`, `Retry`, `Rewind`, `Notify`, and `Close`. There is no lock
  around the history, because exactly one task mutates it.
- **Inside a turn**, the actor is a single async `TurnDriver::run`. It
  `select!`s over the model stream, the turn's `CancellationToken`, and
  the command channel. A `Submit` that arrives mid-turn is queued (FIFO)
  and runs after the current turn ends. `Cancel` takes effect at the next
  `select!` point, so within one poll.
- **Tools run in spawned tasks.**
  - Calls whose tools are all `concurrency_safe` and allowed without a
    prompt run in parallel, up to `max_parallel_tools` (4).
  - Everything else runs sequentially in call order. Writes are never
    interleaved.
  - **Eager start.** An auto-allowed, read-only, concurrency-safe call
    starts as soon as its `ToolCallEnd` arrives, while the model is still
    streaming. If the round is then retried or cancelled, eager results
    are discarded, which is safe because the tool is read-only.
- **Long-running work never blocks a turn.** A tool that kicks off
  something slow (a backtest, a research job, a monitor) returns
  promptly with a `ToolOutput` that says it started the work, plus a
  `BackgroundTicket`. Two mechanisms exist:
  - **In-process, session-scoped work** uses
    `ctx.background().spawn(label, future)`. Core owns the task set, ties
    its cancellation to the session (not the turn), and turns completion
    into `Notice::BackgroundFinished`.
  - **Durable work** that must survive exit goes through `artha-jobs`
    (the tool inserts a job). When a job finishes and the session is
    still open in the same process, `artha-jobs` calls
    `SessionHandle::notify`.

  A notice is shown at once (`CoreEvent::Notice`). It is also handed to
  the model on the **next** user turn, as a short text block prepended to
  that user message. `BackgroundFollowup::AutoTurn` (opt-in) starts a
  turn on its own instead.
- **Blocking I/O** (SQLite and libgit2 through `artha-data`) always runs
  on `spawn_blocking`. A checkpoint `record` for a large file never
  stalls TUI rendering.
- **Multiple sessions** in one process (a TUI plus background jobs) are
  just multiple actors that share one `Agent`: `Arc` provider, registry,
  and stores. `artha-llm`'s per-provider concurrency cap serializes them
  in front of a local GPU.
- **Cross-process.** Two Artha processes on the same session (a TUI and
  `artha ask -c` in another terminal) are not prevented in v1. Appends
  stay safe because of `artha-data`'s IMMEDIATE transactions and the
  `UNIQUE(session_id, seq)` constraint. On reload, each process sees the
  merged history. A session lease is deferred; see §11.

## 11. Error handling and retries (orchestration level)

Principle: **tool problems are data for the model; model and storage
problems are events for the user; nothing loses the user's input.**

| Failure | Handling |
|---|---|
| Tool returns `Err`, times out, panics, or is unknown; malformed arguments | An `is_error` `ToolResult` with a precise message, and the loop continues. The model usually self-corrects. Bounded by `max_rounds`. |
| Same tool fails with an identical error `repeat_error_limit` (3) times in a row within a turn | The result text gains "This has failed repeatedly; stop retrying and tell the user." That is a nudge, not a hard stop. |
| `chat()` returns `Err` | `artha-llm` has already retried. `ContextLengthExceeded` is compacted once (old tool-result bodies elided in the *request view*, never in storage) and re-requested. Anything else ends the turn with `Failed(err)`. The user message stays persisted, and `SessionHandle::retry()` re-runs the turn without re-typing. |
| Mid-stream `Err`, retryable (`Truncated`, `StreamIdle`, overloaded) | Core retries the round up to `stream_retries` (2) times with a short backoff. It emits `RoundRetrying { discard_partial: true }` first, and discards eager results. |
| Mid-stream `Err`, not retryable | `Failed(err)`. The partial text is discarded and not persisted. |
| `RateLimited { retry_after }` / `Auth { reauth_required }` | Surfaced in `TurnOutcome::Failed`. `LlmError::kind()` lets the UI show "retry in 3 min" or "run `artha login`". Core never sleeps for minutes itself. |
| Persistence failure (`DataError::is_busy()` after `busy_timeout`) | Retried 3 times with backoff. If it still fails, the turn continues in memory, the session is marked `unsynced`, the flush is retried at the next round boundary, and `TurnEnded.persisted = false` warns the UI. Corruption-class `DataError`s end the turn with `Failed`. |
| Corrupt stored message on load | `CoreError::CorruptHistory { session, seq }`. Hard fail. |
| Event receiver dropped | The session keeps running to the end of the current turn so persistence stays consistent, then shuts down. Permission prompts resolve to `Deny`. |

`CoreError` is a single `thiserror` enum, `#[non_exhaustive]`. It wraps
`LlmError` and `DataError` with `#[from]` and keeps their `kind()` and
`is_busy()` reachable. `ToolError` is separate and never escapes as a
`CoreError`. There is no `anyhow` in this crate.

**Open items for sibling crates** (raised here, not decided here):
1. `artha-data` has no `SessionStore` call to update `provider`, `model`,
   or `metadata` after creation. Core would use one for provider changes
   on resume and for usage totals. Until it exists, those live only in
   memory.
2. Rewinding with `RestoreScope::All` from `first_pre_edit_from` is
   unsafe for files first touched after that checkpoint (§7). Core avoids
   it, but `artha-data`'s HLD §4 describes the four-step rewind as if it
   were sufficient. That text should be updated to match.
3. The artha-data HLD names the tool method `Tool::run`. The method is
   `execute`, and file-editing tools call `ctx.edits()` rather than
   `begin_edit` directly.
4. A cross-process session lease (for example, a `core_kv` row with a
   heartbeat) to warn about two writers on one session.

## 12. Dependencies

Versions are the intended major lines. They were **not** re-verified
against crates.io in this design pass, so confirm them when writing
`Cargo.toml`.

| Crate | Version | Features | Why |
|---|---|---|---|
| `artha-llm` | path | default (`settings`) | `LlmProvider`, `Message`, `ToolSpec`, `ChatEvent`, `ToolResult`, `LlmError`. |
| `artha-data` | path | default (`async`) | `SessionStore`, `CheckpointStore`, `CheckpointLinkStore`, `Db`, `PermissionMode`, `SessionId`. |
| `tokio` | 1 (workspace) | `rt`, `sync`, `time`, `macros` | Actor tasks, mpsc/oneshot, `spawn_blocking`, timeouts, `JoinSet`. |
| `tokio-util` | 0.7 | `rt` | `CancellationToken` with child tokens. That is the one thing tokio itself lacks. |
| `futures-util` | 0.3 | `std` | `Stream`/`StreamExt`, `FuturesUnordered` for parallel calls. |
| `async-trait` | 0.1 (workspace) | — | `Tool` must be object-safe (`Arc<dyn Tool>`), same reasoning as artha-llm HLD §4.1. |
| `serde` / `serde_json` | 1 (workspace) | `derive` | `PersistedMessage`, `CoreEvent`, tool input. |
| `thiserror` | workspace (`1`) | — | `CoreError`, `ToolError`. |
| `tracing` | 0.1 (workspace) | — | Spans per turn, round, and call. |
| `schemars` | 1 | optional (feature `schemars`) | `TypedTool`: derive a tool's JSON Schema from its input struct. |

Dev-only: `artha-llm` with `test-util` (`ScriptedProvider`), `artha-data`
with `test-util` (`TestEnv`), `tokio` with `test-util` (paused clock),
and `insta` (event-sequence snapshots).

Deliberately not used:
- **`jsonschema`.** We don't validate inputs against the schema in core.
  Typed tools get serde errors, which become `InvalidInput`. MCP servers
  validate their own inputs. Generic validation would add a large
  dependency to re-check what both sides already check.
- **Agent frameworks** (`rig`, `langchain-rust`, and the like). The loop
  is about 600 lines of policy specific to Artha's persistence,
  checkpoint, and permission rules. A framework would own exactly the
  part we need to control.
- **`tokio::sync::broadcast` for events** (see §9) and **`tokio-stream`**
  (a 15-line `Stream` impl over `mpsc::Receiver` is enough).
- **`uuid` directly.** `SessionId` comes from `artha-data`. `TurnId` is a
  per-session `u64` counter.
