# artha-tui: Low-Level Design

This document refines [HLD.md](HLD.md). Section numbers such as "HLD §4.3"
refer to that document. Signatures of `artha-core` types are taken from
`crates/artha-core/docs/LLD.md` §4, §6.5, and §8.

## 1. Module layout

All of these files are planned and none of them has been written yet.

```
crates/artha-tui/
├── Cargo.toml
├── src/
│   ├── lib.rs                 pub fn run, TuiOptions, StartupSelector, ThemeChoice, TuiExit; re-exports
│   ├── error.rs               TuiError
│   ├── services.rs            TuiServices; JobsSource, PluginsSource, HistoryStore, SettingsWriter,
│   │                          ModelCatalog traits; JobRow, PluginRow, … view models
│   ├── terminal.rs            TerminalGuard: raw mode, alt screen, paste, mouse, kbd flags,
│   │                          panic hook, suspend/resume, capability probe (TermCaps)
│   ├── runtime/
│   │   ├── mod.rs             run_loop(): the select! loop, effect spawning, draw calls
│   │   ├── event.rs           AppEvent, InputEvent, EffectResult, SessionGen
│   │   ├── input_task.rs      crossterm EventStream → mpsc<InputEvent>
│   │   ├── core_bridge.rs     EventReceiver → mpsc<(SessionGen, CoreEvent)>; abort handle
│   │   ├── effects.rs         Effect enum + EffectRunner (owns Agent, SessionHandle, services)
│   │   └── frame.rs           FrameScheduler: dirty flags, throttle, animation deadlines
│   ├── app/
│   │   ├── mod.rs             App { state }, App::update(AppEvent) -> Vec<Effect>
│   │   ├── state.rs           AppState, SessionView, TurnPhase, Focus, Overlay, Toast
│   │   ├── core_events.rs     apply_core_event(): CoreEvent → transcript/state mutations
│   │   ├── keymap.rs          KeyEvent + Focus → Action
│   │   ├── action.rs          Action enum and its handlers
│   │   └── commands.rs        slash-command registry, parser, completion source
│   ├── transcript/
│   │   ├── mod.rs             Transcript: Vec<Cell>, indices by CallId / (turn, round, block)
│   │   ├── cell.rs            Cell, CellKind, UserCell, TextCell, ThinkingCell, ToolCell, NoticeCell,
│   │   │                      TurnFooterCell
│   │   ├── stream.rs          StreamingMarkdown: stable prefix + live tail (HLD §4.3)
│   │   └── layout.rs          per-width height cache, prefix sums, Viewport, ScrollState
│   ├── markdown/
│   │   ├── mod.rs             render(src) -> Vec<RenderedBlock>; RenderedBlock, BlockKind, LogicalLine
│   │   ├── boundary.rs        safe-block-boundary scanner (fences, lists, tables)
│   │   ├── walker.rs          pulldown-cmark event walker → RenderedBlock
│   │   ├── table.rs           GFM table layout (also used for ToolDisplay::Table)
│   │   └── code.rs            fenced code → highlighted LogicalLines via highlight.rs
│   ├── highlight.rs           HighlightCache; HighlightRange → Span conversion by theme
│   ├── wrap.rs                span-aware word wrapping with unicode-width
│   ├── input/
│   │   ├── mod.rs             InputBuffer (lines, cursor, edit ops, paste chips)
│   │   ├── history.rs         InputHistory (prefix-filtered walk, draft preservation)
│   │   └── completion.rs      CompletionState for slash commands and /resume ids
│   ├── view/
│   │   ├── mod.rs             draw(frame, &AppState, &mut RenderCtx): layout split + overlay pass
│   │   ├── chat.rs            transcript viewport painter
│   │   ├── cells.rs           per-CellKind line producers (user, text, thinking, notice, footer)
│   │   ├── tool_cell.rs       tool cell header/progress/body; collapse logic
│   │   ├── display.rs         ToolDisplay → lines (Table, Markdown, Code, Diff)
│   │   ├── input.rs           input box + completion popup + cursor placement
│   │   ├── status.rs          status bar
│   │   └── overlay/
│   │       ├── mod.rs         Overlay painting, centred_rect, focus ring
│   │       ├── permission.rs  permission modal
│   │       ├── session_picker.rs
│   │       ├── plugins.rs
│   │       ├── jobs.rs
│   │       ├── rewind.rs
│   │       ├── model.rs
│   │       └── help.rs
│   └── theme/
│       ├── mod.rs             Theme, Token, SyntaxToken, Theme::style(Token)
│       ├── palette.rs         dark/light/mono RGB palettes; downsample to 256/16
│       └── detect.rs          OSC 11 query, COLORFGBG, COLORTERM/TERM/NO_COLOR → TermCaps
└── tests/
    ├── render_snapshots.rs    TestBackend + insta snapshots of whole frames
    ├── streaming.rs           incremental markdown equivalence + frame-rate tests
    ├── permission.rs          modal flow against Harness-driven core
    ├── input.rs               editor/history/slash state machine
    ├── e2e_harness.rs         full run_loop against artha-core Harness with scripted input
    └── support/mod.rs         FakeServices, scripted key streams, frame-to-string helpers
```

## 2. Public API (`lib.rs`, `error.rs`, `services.rs`)

```rust
pub async fn run(agent: Agent, opts: TuiOptions, services: TuiServices) -> Result<TuiExit, TuiError>;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TuiOptions {
    pub startup: StartupSelector,
    /// `artha [message]`: submitted automatically after SessionOpened.
    pub initial_prompt: Option<String>,
    pub theme: ThemeChoice,
    /// Shown in the status bar until core can report it (HLD §10.3).
    pub model_label: Option<String>,
    pub permission_mode: PermissionMode,     // artha_core re-export of artha_data::PermissionMode
    /// true if tracing currently writes to stderr/stdout → run() refuses (HLD §2.1).
    pub stderr_is_log_sink: bool,
    pub mouse_capture: bool,                 // default true; Shift-drag still selects natively
    pub picker_esc_starts_new: bool,         // bare -r: Esc in the startup picker → New (true) or quit
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupSelector { New, Continue, Resume(SessionId), Pick }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice { #[default] Auto, Dark, Light }

#[derive(Debug)]
pub struct TuiExit { pub session: Option<SessionId>, pub reason: ExitReason }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason { UserQuit, PickerDismissed, CoreClosed }

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TuiError {
    #[error("not a terminal: {0}")]
    NotATty(&'static str),
    #[error("terminal I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Core(#[from] artha_core::CoreError),
    #[error("logging writes to the terminal; route tracing to a file before starting the TUI")]
    StdoutLogging,
}
```

```rust
#[derive(Default, Clone)]
pub struct TuiServices {
    pub jobs: Option<Arc<dyn JobsSource>>,
    pub plugins: Option<Arc<dyn PluginsSource>>,
    pub history: Option<Arc<dyn HistoryStore>>,
    pub settings: Option<Arc<dyn SettingsWriter>>,
    pub models: Option<Arc<dyn ModelCatalog>>,
}

#[async_trait]
pub trait JobsSource: Send + Sync {
    async fn list(&self) -> Result<Vec<JobRow>, ServiceError>;
    async fn logs(&self, id: &str, tail: usize) -> Result<Vec<String>, ServiceError>;
    async fn pause(&self, id: &str) -> Result<(), ServiceError>;
    async fn resume(&self, id: &str) -> Result<(), ServiceError>;
    async fn run_now(&self, id: &str) -> Result<(), ServiceError>;
}
pub struct JobRow { pub id: String, pub goal: String, pub schedule: Option<String>,
                    pub state: JobState, pub last_run: Option<SystemTime>, pub next_run: Option<SystemTime> }
pub enum JobState { Scheduled, Running, Paused, Failed { message: String }, Done }

#[async_trait]
pub trait PluginsSource: Send + Sync {
    async fn installed(&self) -> Result<Vec<PluginRow>, ServiceError>;
    async fn marketplace(&self) -> Result<Vec<MarketplaceRow>, ServiceError>;  // all configured marketplaces
    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), ServiceError>;
    async fn install(&self, spec: &str) -> Result<PluginRow, ServiceError>;
    async fn uninstall(&self, name: &str) -> Result<(), ServiceError>;
}
pub struct PluginRow { pub name: String, pub version: String, pub enabled: bool, pub source: String,
                       pub tools: Vec<String>, pub error: Option<String> }
pub struct MarketplaceRow { pub marketplace: String, pub name: String, pub description: String,
                            pub spec: String, pub installed: bool }

#[async_trait]
pub trait HistoryStore: Send + Sync {
    async fn load(&self, workdir: &Path, limit: usize) -> Vec<String>;   // newest last; errors → empty
    async fn push(&self, workdir: &Path, entry: &str);
}
#[async_trait]
pub trait SettingsWriter: Send + Sync {
    async fn set_theme(&self, theme: ThemeChoice) -> Result<(), ServiceError>;
}
#[async_trait]
pub trait ModelCatalog: Send + Sync {
    async fn models(&self) -> Result<Vec<String>, ServiceError>;
}
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct ServiceError(pub String);
```

## 3. Events and effects (`runtime/event.rs`, `runtime/effects.rs`)

```rust
/// Increments on every Agent::open. Core events tagged with an older gen are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionGen(pub u32);

/// Everything the reducer consumes. Owned, no references into the runtime.
#[derive(Debug)]
pub enum AppEvent {
    Input(InputEvent),
    Core { gen: SessionGen, event: CoreEvent },
    /// The core bridge saw EventReceiver end (the actor stopped).
    CoreClosed { gen: SessionGen },
    Effect(EffectResult),
    /// From FrameScheduler: an animation deadline passed (spinner/elapsed timers).
    AnimationTick(Instant),
}

#[derive(Debug, Clone)]
pub enum InputEvent {
    Key(crossterm::event::KeyEvent),   // only KeyEventKind::Press/Repeat are forwarded
    Paste(String),
    Mouse(crossterm::event::MouseEvent),
    Resize { cols: u16, rows: u16 },
    FocusGained,
    FocusLost,
    /// The input stream failed (stdin closed). Treated as quit.
    Eof,
}

/// Results of async effects, routed back into update().
#[derive(Debug)]
pub enum EffectResult {
    SessionOpened { gen: SessionGen, result: Result<(), String> },   // the handle stays in EffectRunner
    Submitted { local_id: LocalId, result: Result<TurnId, String> },
    Retried(Result<TurnId, String>),
    Rewound(Result<RewindReport, String>),
    Renamed(Result<(), String>),
    Sessions(Result<Vec<SessionInfo>, String>),
    Jobs(Result<Vec<JobRow>, String>),
    JobLogs { id: String, result: Result<Vec<String>, String> },
    Plugins { installed: Result<Vec<PluginRow>, String>, market: Option<Result<Vec<MarketplaceRow>, String>> },
    ServiceDone { what: &'static str, result: Result<(), String> },
    History(Vec<String>),
    Models(Result<Vec<String>, String>),
    PermissionAnswered { call_id: CallId, accepted: bool },          // responder.respond() return value
}

/// Correlates an optimistic user cell with its later TurnStarted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u64);

/// Side effects requested by update(). Executed by EffectRunner, never by the reducer.
#[derive(Debug)]
pub enum Effect {
    OpenSession { gen: SessionGen, selector: SessionSelector },     // closes the previous handle first
    Submit { local_id: LocalId, text: String },
    Cancel { drop_queue: bool },
    Retry,
    Rewind(RewindRequest),
    Rename(String),
    ListSessions(SessionListFilter),
    Respond { responder: PermissionResponder, call_id: CallId, decision: PermissionDecision },
    LoadHistory, PushHistory(String),
    Jobs(JobsOp), Plugins(PluginsOp),
    SetTheme(ThemeChoice),           // persists via SettingsWriter; the state already switched
    LoadModels,
    CopyToClipboard(String),          // OSC 52 written on the UI thread before the next draw
    Suspend,                          // Unix Ctrl-Z
    Quit(ExitReason),
}
pub enum JobsOp { List, Logs(String), Pause(String), Resume(String), RunNow(String) }
pub enum PluginsOp { List { with_market: bool }, SetEnabled(String, bool), Install(String), Uninstall(String) }
```

`EffectRunner` owns `Agent`, `Option<(SessionGen, SessionHandle, AbortHandle /* bridge */)>`, and
`TuiServices`. Each async effect is a `tokio::spawn` that sends one `EffectResult` on an
unbounded `mpsc`. "Unbounded" is safe here because it carries at most one message per effect,
and effects come from user actions. `Respond`, `CopyToClipboard`, `Suspend`, and `Quit` are
handled inline on the UI loop because they are synchronous.

`OpenSession` sequence:

```
1. if let Some((_, old, bridge)) = current.take():
       bridge.abort();                     // stop forwarding; old receiver dropped → core auto-denies prompts
       spawn(async move { let _ = old.close().await; });
2. spawn: match agent.open(selector).await {
       Ok((handle, rx)) => { install (gen, handle); spawn core_bridge(gen, rx, core_tx); send SessionOpened{gen, Ok} }
       Err(e)           => send SessionOpened{gen, Err(e.to_string())}
   }
```

Installing the handle requires `&mut EffectRunner` from a spawned task. To allow that, the
runner keeps `current` behind an `Arc<tokio::sync::Mutex<..>>`. Effects that need the handle,
such as `Submit`, lock it, clone the `SessionHandle` (which is cheap), and release the lock
before awaiting core. A `Submit` that arrives before the open completes waits on the same
mutex, so its ordering is preserved.

## 4. State (`app/state.rs`)

```rust
pub struct AppState {
    pub session: SessionView,
    pub transcript: Transcript,
    pub input: InputState,
    pub overlays: Vec<Overlay>,                 // bottom → top; top has focus unless `permission` is Some
    pub permission: Option<PermissionModal>,    // always topmost; owns the responder (HLD §4.5)
    pub permission_queue: VecDeque<PermissionPrompt>, // defensive: core asks one at a time
    pub scroll: ScrollState,
    pub theme: Theme,
    pub caps: TermCaps,
    pub toasts: VecDeque<Toast>,                // transient one-line messages above the input
    pub quit_armed_until: Option<Instant>,      // Ctrl-C twice to quit
    pub size: (u16, u16),
    pub now: Instant,                           // set from each event; views never call Instant::now()
    pub next_local_id: u64,
    pub dirty: Dirty,
}

pub struct SessionView {
    pub gen: SessionGen,
    pub id: Option<SessionId>,
    pub status: SessionStatus,
    pub resumed: bool,
    pub history_hidden: usize,                  // SessionOpened.history until a transcript API exists (HLD §10.1)
    pub workdir: PathBuf,
    pub title: Option<String>,
    pub model_label: Option<String>,
    pub permission_mode: PermissionMode,
    pub phase: TurnPhase,
    pub queued: usize,                           // submits accepted but not yet started
    pub usage: Usage,                            // session total (sum of TurnEnded.usage)
    pub turn_usage: Option<Usage>,               // live, from CoreEvent::Usage
    pub background: BTreeMap<String, BackgroundTicket>, // open tickets by id
    pub last_outcome: Option<TurnOutcome>,       // drives /retry and /continue availability
    pub pending_initial_prompt: Option<String>,
}

pub enum SessionStatus { Opening, Open, Closed, Failed(String) }

#[derive(Debug, Clone, PartialEq)]
pub enum TurnPhase {
    Idle,
    /// Submitted; waiting for TurnStarted.
    Submitting,
    Running { turn: TurnId, round: RoundIndex, since: Instant, activity: Activity },
    Cancelling { turn: TurnId, since: Instant },
}
#[derive(Debug, Clone, PartialEq)]
pub enum Activity { Requesting, Streaming, Thinking, Tools { running: usize }, AwaitingPermission, Retrying { attempt: u32 } }

pub enum Overlay {
    SessionPicker(PickerState<SessionInfo>),
    Plugins(PluginsState),
    Jobs(JobsState),
    Rewind(PickerState<RewindTarget>),
    Model(PickerState<String>),
    Help { scroll: u16 },
}

pub struct PickerState<T> {
    pub items: Loadable<Vec<T>>,
    pub query: String,                     // fuzzy filter typed into the overlay
    pub filtered: Vec<usize>,              // indices into items, ranked
    pub selected: usize,
    pub startup: bool,                     // the bare -r picker: Esc → New or quit
}
pub enum Loadable<T> { Loading { since: Instant }, Ready(T), Failed(String) }

pub struct RewindTarget { pub user_seq: MessageSeq, pub preview: String, pub scope: RewindScope }

pub struct PermissionModal {
    pub prompt: PermissionPrompt,          // holds the responder; dropping AppState drops it → Deny
    pub opened_at: Instant,                // 250 ms key guard
    pub feedback: Option<InputBuffer>,     // Some while typing deny feedback
    pub show_input_json: bool,
    pub scroll: u16,                       // for long previews
}

pub struct InputState {
    pub buffer: InputBuffer,
    pub mode: InputMode,
    pub history: InputHistory,
    pub completion: Option<CompletionState>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode { Normal, Command, HistoryBrowse, Disabled /* session opening/closed */ }

pub struct ScrollState {
    /// Lines from the bottom of the transcript to the bottom of the viewport. 0 = following.
    pub offset_from_bottom: usize,
    pub follow: bool,
    pub selected_cell: Option<CellId>,     // Alt-↑/↓ cell cursor
    pub unseen_below: bool,                // "↓ new output" pill when not following
}

pub struct Toast { pub level: ToastLevel, pub text: String, pub until: Instant }
pub enum ToastLevel { Info, Warning, Error }

bitflags::bitflags! { // or a small hand-rolled u8 newtype, to avoid the dependency
    pub struct Dirty: u8 { const CONTENT = 1; const INPUT = 2; const LAYOUT = 4; const ANIMATION = 8; }
}
```

### 4.1 Transcript (`transcript/`)

```rust
pub struct Transcript {
    cells: Vec<Cell>,
    by_call: HashMap<CallId, CellId>,
    by_block: HashMap<(TurnId, RoundIndex, BlockIndex), CellId>,
    by_local: HashMap<LocalId, CellId>,
    layout: LayoutCache,                       // transcript/layout.rs
    next_id: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId(pub u64);

pub struct Cell {
    pub id: CellId,
    pub turn: Option<TurnId>,
    pub round: Option<RoundIndex>,
    pub kind: CellKind,
    /// Bumped on any content change; the layout cache keys on (id, rev, width).
    pub rev: u32,
}

pub enum CellKind {
    User(UserCell),
    Text(TextCell),
    Thinking(ThinkingCell),
    Tool(ToolCell),
    Notice(NoticeCell),
    TurnFooter(TurnFooterCell),     // only for non-Completed outcomes, plus a usage line on demand
    HiddenHistory { count: usize }, // placeholder for resumed history (HLD §10.1)
}

pub struct UserCell { pub text: String, pub local: Option<LocalId>, pub user_seq: Option<MessageSeq>,
                      pub state: UserCellState }
pub enum UserCellState { Pending, Queued { behind: usize }, Started, Rejected(String) }

pub struct TextCell { pub md: StreamingMarkdown, pub finalized: bool }
pub struct ThinkingCell { pub text: String, pub expanded: bool, pub finalized: bool }

pub struct ToolCell {
    pub call_id: CallId,
    pub name: String,
    pub input: Option<serde_json::Value>,
    pub malformed: Option<String>,
    pub summary: Option<String>,          // ToolStarted.summary (plan summary)
    pub state: ToolCellState,
    pub eager: bool,
    pub progress: Option<(String, Option<f32>)>,
    pub checkpoints: Vec<CheckpointKindView>,
    pub display: Option<RenderedDisplay>, // ToolDisplay pre-rendered to width-independent blocks
    pub expanded: bool,
}
pub enum ToolCellState {
    Streaming, Ready, AwaitingPermission,
    Running { since: Instant },
    Done { outcome: ToolOutcome, elapsed_ms: u64 },
}

pub struct NoticeCell { pub notice: Notice, pub display: Option<RenderedDisplay> }
pub struct TurnFooterCell { pub outcome: TurnOutcome, pub usage: Usage, pub rounds: u32, pub persisted: bool }
```

## 5. Reducer: `CoreEvent` to state (`app/core_events.rs`)

`App::update(AppEvent::Core { gen, event })` returns immediately if
`gen != state.session.gen`. Otherwise it applies the following table.
Every arm sets `dirty |= CONTENT` unless noted.

| `CoreEvent` | Mutation |
|---|---|
| `SessionOpened { session, resumed, history, workdir }` | `status = Open`; set id, workdir, resumed; if `history > 0`, push `HiddenHistory{history}`; enable input; if `pending_initial_prompt`, emit `Submit` |
| `SessionPersisted { session }` | `session.id = Some(session)` (status bar only) |
| `Notice(n)` | push `NoticeCell`. `BackgroundFinished`/`Failed` remove the ticket from `session.background`. `Warning` also raises a toast. |
| `RewindCompleted(r)` | toast "rewound: N messages, M files"; remove the cells after the target user cell; `input.buffer.set(r.restored_prompt)` if the input is empty |
| `TurnStarted { turn, user_seq, queued_behind }` | bind the oldest `Pending`/`Queued` user cell (FIFO) → `Started`, `user_seq`, `turn`; `phase = Running{Requesting}`; `queued = queued.saturating_sub(1)` |
| `RoundStarted { turn, round }` | update the phase round; `Activity::Requesting` |
| `RoundRetrying { turn, round, attempt, discard_partial, .. }` | if `discard_partial`, remove every Text/Thinking/Tool cell with `(turn, round)` and their index entries; `Activity::Retrying{attempt}` |
| `TextDelta { turn, round, index, text }` | find or create a `TextCell` at `(turn, round, index)`; `md.push(&text)`; `Activity::Streaming`; if following, keep the scroll pinned |
| `ThinkingDelta { .. }` | the same with a `ThinkingCell` (collapsed: "✻ thinking… N words") |
| `ToolCallStreaming { call_id, name, .. }` | create a `ToolCell{Streaming}` |
| `ToolCallReady { call }` | set input/malformed; `Ready` (create the cell if Streaming was missed) |
| `AssistantCommitted { turn, round, .. }` | finalize every Text/Thinking cell of `(turn, round)` (one full re-parse) |
| `Usage { usage, .. }` | `turn_usage = Some(usage)`; `dirty |= LAYOUT` only (status bar) |
| `PermissionRequested(p)` | the tool cell → `AwaitingPermission`; `Activity::AwaitingPermission`; open the modal if none is open, else push to `permission_queue` |
| `ToolStarted { call_id, summary, eager, .. }` | `Running{since: now}`; set summary/eager |
| `ToolProgress { message, fraction, .. }` | `progress = Some(..)` |
| `CheckpointRecorded { kind, .. }` | `checkpoints.push(kind)` |
| `ToolFinished { outcome, elapsed_ms, display, .. }` | `Done`; `display = display.map(render_display)`; if `Ok{background: Some(t)}`, insert `t` into `session.background` |
| `TurnEnded { turn, outcome, usage, rounds, persisted }` | finalize the turn's cells; `session.usage += usage`; `turn_usage = None`; `last_outcome = Some(outcome)`; push `TurnFooter` unless `Completed && persisted`; if the modal belongs to `turn`, close it (§7); `phase = Idle`, or `Submitting` if `queued > 0` |
| unknown (`#[non_exhaustive]`) | ignored, `tracing::debug!` |

`AppEvent::CoreClosed { gen }` for the current generation sets
`status = Closed`, disables the input, and shows an error toast that
suggests `/new`.

## 6. Run loop (`runtime/mod.rs`, `runtime/frame.rs`)

```rust
pub(crate) async fn run_loop(agent: Agent, opts: TuiOptions, services: TuiServices) -> Result<TuiExit, TuiError> {
    if opts.stderr_is_log_sink { return Err(TuiError::StdoutLogging); }
    let caps  = theme::detect::probe(opts.theme)?;         // OSC 11 *before* EventStream exists
    let mut guard = TerminalGuard::enter(&opts, &caps)?;    // raw, alt screen, paste, mouse, kbd flags, panic hook
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let (input_tx, mut input_rx)   = mpsc::channel::<InputEvent>(256);
    let (core_tx, mut core_rx)     = mpsc::channel::<(SessionGen, Option<CoreEvent>)>(1024); // None = closed
    let (fx_tx, mut fx_rx)         = mpsc::unbounded_channel::<EffectResult>();
    let input_task = spawn(input_task::run(input_tx));
    let mut runner = EffectRunner::new(agent, services, core_tx, fx_tx);
    let mut app    = App::new(&opts, caps, terminal.size()?);
    let mut sched  = FrameScheduler::new();

    for fx in app.startup_effects(&opts) { runner.run(fx, &mut app, &mut guard); } // OpenSession or ListSessions+picker, LoadHistory
    draw(&mut terminal, &mut app)?;                         // first frame immediately

    loop {
        let deadline = sched.next_deadline(&app.state);     // None when idle and nothing animates
        let ev = tokio::select! {
            biased;
            Some(i) = input_rx.recv()              => AppEvent::Input(i),
            Some(r) = fx_rx.recv()                 => AppEvent::Effect(r),
            Some((g, e)) = core_rx.recv()          => core_to_app(g, e),
            _ = sleep_until_opt(deadline)          => AppEvent::AnimationTick(Instant::now()),
        };
        let mut effects = app.update(ev);

        // Batch-drain: coalesce bursts before drawing (HLD §4.3 point 2).
        let budget_end = Instant::now() + Duration::from_millis(4);
        let mut n = 0;
        while n < 512 && Instant::now() < budget_end {
            let next = if let Ok(i) = input_rx.try_recv() { AppEvent::Input(i) }
                       else if let Ok(r) = fx_rx.try_recv() { AppEvent::Effect(r) }
                       else if let Ok((g, e)) = core_rx.try_recv() { core_to_app(g, e) }
                       else { break };
            effects.extend(app.update(next));
            n += 1;
        }

        for fx in effects {
            if let ControlFlow::Break(reason) = runner.run(fx, &mut app, &mut guard) {
                return finish(guard, terminal, runner, reason).await;  // restores the terminal, then closes the session
            }
        }
        if sched.should_draw(&app.state, Instant::now()) {
            draw(&mut terminal, &mut app)?;                 // wrapped in Begin/EndSynchronizedUpdate
            sched.drew(Instant::now());
            app.state.dirty = Dirty::empty();
        }
    }
}
```

`FrameScheduler`:

```rust
pub(crate) struct FrameScheduler { last_draw: Instant, min_interactive: Duration /* 8ms */, min_stream: Duration /* 33ms */ }
impl FrameScheduler {
    /// INPUT|LAYOUT dirty → draw if ≥ 8 ms since the last draw; CONTENT dirty → if ≥ 33 ms; else no.
    pub fn should_draw(&self, s: &AppState, now: Instant) -> bool;
    /// The earliest of: a pending throttled draw (last_draw + 33ms when CONTENT is dirty),
    /// the next spinner frame (100 ms, only if something animates), toast expiry, the quit-arm expiry.
    pub fn next_deadline(&self, s: &AppState) -> Option<Instant>;
}
```

Something "animates" when the turn phase isn't `Idle`, when any tool cell
is `Running`, when the session is `Opening`, or when any overlay is
`Loading`. An idle TUI therefore sleeps until the next input.

`core_bridge::run(gen, rx, tx)` is a loop of `while let Some(ev) =
rx.next().await { if tx.send((gen, Some(ev))).await.is_err() { break } }`
followed by `tx.send((gen, None))`. The awaited `send` passes the UI's
backpressure on to core.

`input_task::run` reads `crossterm::event::EventStream`. It forwards
`Press` and `Repeat` key events and drops `Release` events, which arrive
only when keyboard enhancement is on. During `Suspend`, the runner stops
reading by sending the task a pause signal over a `watch` channel. On
`SIGCONT`, it re-enters raw mode and the alternate screen, and forces a
full redraw with `terminal.clear()`.

### 6.1 `draw` and the view layer (`view/mod.rs`)

```rust
pub fn draw(f: &mut Frame, s: &AppState, cx: &mut RenderCtx) {
    let input_h = s.input.buffer.visual_height(f.area().width - 4).clamp(1, 10) + 2;  // +border
    let [chat, toast, input, status] = Layout::vertical([
        Constraint::Min(3), Constraint::Length(s.toasts.len().min(2) as u16),
        Constraint::Length(input_h), Constraint::Length(1),
    ]).areas(f.area());
    chat::draw(f, chat, s, cx);
    toasts::draw(f, toast, s);
    input::draw(f, input, s);            // places the cursor via f.set_cursor_position unless an overlay has focus
    status::draw(f, status, s);
    for o in &s.overlays { overlay::draw(f, o, s, cx); }
    if let Some(m) = &s.permission { overlay::permission::draw(f, m, s, cx); }
}
```

`RenderCtx` holds the caches that views fill lazily: the `HighlightCache`
and the per-width wrap cache that sits inside `Transcript.layout`. It is
`&mut` so that `draw` can memoize. `AppState` itself stays immutable
during a draw.

`chat::draw` virtualizes the transcript. It works like this:

```
width = area.width - 2 (gutter)
layout.ensure(width, &cells)             // recompute heights only for cells whose (rev, width) changed
total = layout.total_height()
bottom = total - scroll.offset_from_bottom
top = bottom.saturating_sub(area.height)
for (cell, line_range) in layout.cells_in(top..bottom):   // binary search on prefix sums
    lines = cells::wrapped_lines(cell, width, cx)          // cached Vec<Line<'static>>; clone only the range
    push lines[line_range] into a Paragraph (no Wrap: already wrapped)
render the Paragraph; overlay a "↓ new output" pill if unseen_below; draw a scrollbar if total > height
```

`LayoutCache` stores `Vec<(CellId, u32 rev, u16 width, usize height)>` plus
a prefix-sum `Vec<usize>`. The prefix sums are rebuilt from the first
changed index onward. A streaming tail cell is always last, so rebuilding
costs O(1) on most frames.

## 7. Markdown and highlight pipeline

### 7.1 Data model (`markdown/mod.rs`)

```rust
/// Width-independent rendering. Wrapping happens in wrap.rs.
pub struct RenderedBlock { pub kind: BlockKind, pub lines: Vec<LogicalLine> }
pub enum BlockKind { Paragraph, Heading(u8), Code { lang: Option<String> }, List, Quote, Table, Rule, Html }
pub struct LogicalLine {
    pub spans: Vec<StyledSpan>,
    /// Prefix repeated on every wrapped continuation line ("  " for list bodies, "│ " for quotes).
    pub hang: Vec<StyledSpan>,
    /// Code and tables never word-wrap: they are hard-truncated with "…" (code may soft-wrap; see 7.4).
    pub wrap: WrapMode,
}
pub struct StyledSpan { pub text: CompactString /* or String */, pub token: Token, pub mods: Modifier, pub link: Option<Arc<str>> }
pub enum WrapMode { Word, Char, Truncate }

pub fn render(src: &str, hl: &mut HighlightCache) -> Vec<RenderedBlock>;
```

Spans store **theme tokens, not colors**. `Token` to `Style` is resolved at
wrap time, so a `/theme` switch only invalidates the wrap cache and
nothing is re-parsed.

### 7.2 Walker (`markdown/walker.rs`)

Parser setup: `pulldown_cmark::Parser::new_ext(src, Options::ENABLE_TABLES
| ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS)`.

The walker keeps the following stacks:
- `inline_style: Vec<(Token, Modifier)>`
- `list: Vec<ListCtx { ordered: Option<u64>, depth }>`
- `quote_depth`
- the current `code: Option<(lang, String)>`
- the current `table: Option<TableBuilder>`

| Event | Action |
|---|---|
| `Start(Heading{level})` | new block `Heading(level)`; push `(Token::Accent, BOLD)` |
| `End(Heading)` | for H1/H2, append a rule line of `─` (drawn to the width at wrap time; `WrapMode::Truncate`) |
| `Start(Paragraph)` / `End` | new `Paragraph` block (inside a list item, continue the item's hang) |
| `Text(t)` | if inside code, append to the code buffer; if in a table, append to the cell; else push a span with the current style |
| `Code(t)` | span `Token::CodeInline` |
| `Start(Emphasis/Strong/Strikethrough)` | push `ITALIC` / `BOLD` / `CROSSED_OUT` |
| `Start(Link{dest})` | push `Token::Link`, `UNDERLINED`, with `link: Some(dest)`; at `End`, if the text differs from dest, append a dim ` (dest)` |
| `Start(List(n))` / `Start(Item)` | bullet `•`/`◦`/`▪` by depth, or `{n}.`; hang = the indent width |
| `TaskListMarker(b)` | `☑`/`☐` |
| `Start(BlockQuote)` | `quote_depth += 1`; the prefix `│ ` × depth goes into the hang |
| `Start(CodeBlock(kind))` | start the code buffer; `lang = info.split_whitespace().next()` |
| `End(CodeBlock)` | `code::render(lang, buf, hl)` → a `Code` block (§7.4) |
| `Start(Table(aligns))` … `End(Table)` | `TableBuilder`; on end → `table::layout` (§7.5) |
| `SoftBreak` | a space; `HardBreak` → a new logical line |
| `Rule` | a `Rule` block |
| `Html(t)` / `InlineHtml(t)` | literal text, `Token::TextDim` |
| `FootnoteReference`, `Image` | `[^n]`, `[image: alt]` |

### 7.3 Streaming (`transcript/stream.rs`, `markdown/boundary.rs`)

```rust
pub struct StreamingMarkdown {
    raw: String,
    stable_upto: usize,                 // byte offset; raw[..stable_upto] is parsed once and frozen
    stable: Vec<RenderedBlock>,
    tail: Vec<RenderedBlock>,
    tail_dirty: bool,
    scan: BoundaryScanner,              // incremental line state, resumes at its last position
    last_tail_highlight: Option<Instant>,
}

impl StreamingMarkdown {
    pub fn push(&mut self, delta: &str) { self.raw.push_str(delta); self.tail_dirty = true; }

    /// Called from wrapped_lines() at draw time only, so it runs at most once per frame.
    pub fn refresh(&mut self, hl: &mut HighlightCache, now: Instant) {
        if !self.tail_dirty { return; }
        if let Some(b) = self.scan.advance(&self.raw) {          // the newest safe boundary > stable_upto
            self.stable.extend(markdown::render(&self.raw[self.stable_upto..b], hl));
            self.stable_upto = b;
        }
        let tail_src = &self.raw[self.stable_upto..];
        let allow_hl = self.last_tail_highlight.map_or(true, |t| now - t >= Duration::from_millis(150));
        self.tail = markdown::render_with(tail_src, hl, RenderOpts { highlight_open_fence: allow_hl });
        if allow_hl { self.last_tail_highlight = Some(now); }
        self.tail_dirty = false;
    }

    /// On AssistantCommitted / TurnEnded: one full parse, so the result is exactly the non-streaming render.
    pub fn finalize(&mut self, hl: &mut HighlightCache) {
        self.stable = markdown::render(&self.raw, hl); self.tail.clear(); self.stable_upto = self.raw.len();
    }
    pub fn blocks(&self) -> impl Iterator<Item = &RenderedBlock> { self.stable.iter().chain(&self.tail) }
}
```

`BoundaryScanner` tracks state line by line over *complete* lines (those
ending in `\n`). It keeps this state:
- `in_fence: Option<(char /* ` or ~ */, usize /* run length */)>`
- `in_table: bool` (the previous line started with `|`)
- `list_open: bool` (the previous non-blank line was a list item or an
  indented continuation)
- `last_boundary`

A blank line is a boundary only when all of these hold:
- `in_fence` is `None`;
- the next non-blank line isn't yet known to continue a list. The
  scanner can only know that after the *next* line arrives, so a
  boundary is confirmed one line late. A list is held open while the
  following line is indented or starts with a list marker.
- `in_table` is false.

A closing fence line counts as a boundary right after it. That freezes a
finished code block, including its final highlight, as soon as it
closes. Reference-link definitions that appear later can change the
rendering of earlier text. Those cases are rare, and `finalize` corrects
them.

Correctness invariant, which the tests check: for any delta split,
`finalize(push*(chunks))` equals `render(full)`. Streaming frames may
differ from the final render only inside the tail.

### 7.4 Code blocks (`markdown/code.rs`, `highlight.rs`)

```rust
pub struct HighlightCache {
    hl: artha_highlight::Highlighter,
    map: lru::LruCache<u64 /* hash(lang, src) */, Arc<[HighlightRange]>>,  // 256 entries; or a small hand-rolled LRU
}
impl HighlightCache {
    pub fn ranges(&mut self, lang: Language, src: &str) -> Option<Arc<[HighlightRange]>>;  // None over budget or on error
}

pub fn render_code(lang: Option<&str>, src: &str, hl: &mut HighlightCache, highlight: bool) -> RenderedBlock {
    let language = lang.and_then(artha_highlight::language_for);
    let ranges = match (highlight, language) { (true, Some(l)) => hl.ranges(l, src), _ => None };
    // Walk src line by line with a cursor into `ranges` (sorted, non-overlapping):
    //   each line's spans = the gaps (Token::CodeText) + the ranges clipped to the line (Token::Syntax(class)).
    // A range that crosses '\n' is split across lines. Tabs are expanded to 4 spaces after styling.
    // Line 0 is a header: dim language label, right-aligned. Every line has the code_block_bg token
    //   and a 1-space left pad. WrapMode::Char (wraps at the width with a dim "↪" hang).
}
```

`Token::Syntax(HighlightClass)` maps one-to-one onto the theme's 14
syntax tokens. An unknown future `HighlightClass` variant (the enum is
`#[non_exhaustive]`) falls back to `CodeText`. While a code block is
still open in the streaming tail, it is highlighted against the partial
source. Tree-sitter's error recovery keeps the already-complete lines
correct, and the 150 ms throttle bounds the cost.

### 7.5 Tables (`markdown/table.rs`), shared with `ToolDisplay::Table`

```rust
pub struct TableModel { pub header: Vec<String>, pub rows: Vec<Vec<String>>, pub align: Vec<Align> }
pub fn layout(t: &TableModel, max_width: u16, max_rows: Option<usize>) -> Vec<LogicalLine>;
```

The table is laid out once per width, during the wrap step:
- Column widths start at `max(display_width)` per column.
- If the total exceeds the space, the widest column shrinks first, down
  to a minimum of 6 cells. Any further overflow drops the rightmost
  columns and adds a `+k cols` marker.
- A column is treated as numeric, and right-aligned, when at least 80%
  of its non-empty cells parse as numbers after stripping `,`, `%`, `$`,
  `₹`, and a sign.
- A leading `+` or `-` in a numeric cell gets the `gain` or `loss` token.
- Borders are `┌┬┐│├┼┤└┴┘` with the `border` token, or ASCII `+-|`
  when `TermCaps::unicode` is false.
- `max_rows` implements tool-cell collapsing (8 rows), and the footer
  line is added by `tool_cell.rs`.

### 7.6 `ToolDisplay` rendering (`view/display.rs`)

```rust
pub struct RenderedDisplay { pub blocks: Vec<RenderedBlock>, pub total_rows: usize }
pub fn render_display(d: &ToolDisplay, hl: &mut HighlightCache) -> RenderedDisplay {
    match d {
        ToolDisplay::Markdown { text }            => markdown::render(text, hl).into(),
        ToolDisplay::Table { columns, rows }       => table block (deferred layout: needs the width),
        ToolDisplay::Code { language, text }       => code::render_code(language.as_deref(), text, hl, true).into(),
        ToolDisplay::Diff { path, unified }        => header(path) + lines: '+' DiffAdd, '-' DiffDel, '@@' Info, else TextMuted,
        _ /* non_exhaustive */                     => one dim line "[unsupported display]",
    }
}
```

## 8. Permission-modal flow (`view/overlay/permission.rs`, `app/action.rs`)

```
CoreEvent::PermissionRequested(p)
   │  tool cell → AwaitingPermission; Activity::AwaitingPermission
   ├─ state.permission is None → state.permission = Some(PermissionModal{ prompt: p, opened_at: now, .. })
   └─ else                     → permission_queue.push_back(p)      (should not happen; core serializes)

Key while permission.is_some()  (keymap routes here before any overlay or the input box):
   now - opened_at < 250ms           → ignored (no accidental approval)
   feedback.is_some():               typing goes to the feedback buffer; Enter → decide(Deny{Some(text)}); Esc → feedback = None
   'y' | Enter → decide(AllowOnce)   'a' → decide(AllowForSession)
   'n'         → decide(Deny{None})  'f' → feedback = Some(empty)
   Esc | 's'   → decide(DenyAndStop) 'i' → toggle show_input_json
   PgUp/PgDn   → scroll the preview
   Ctrl-C      → Effect::Cancel{drop_queue:false}   (core resolves the prompt as Deny; the modal stays until TurnEnded)

decide(d):
   let m = state.permission.take().unwrap();
   effects.push(Effect::Respond{ responder: m.prompt.responder, call_id: m.prompt.call_id, decision: d });
   tool cell → Ready (core will send ToolStarted or ToolFinished{Denied})
   if d == AllowForSession: toast "allowing <tool> for this session"
   state.permission = permission_queue.pop_front().map(PermissionModal::new)

EffectRunner::run(Respond{..}) — inline, synchronous:
   let accepted = responder.respond(decision);     // false = already answered or the turn moved on
   app.update(AppEvent::Effect(EffectResult::PermissionAnswered{ call_id, accepted }))
       !accepted → toast(Warning, "that prompt had already expired")
   // the responder is consumed here; no clone survives anywhere

TurnEnded{turn} with state.permission.prompt.turn == turn:
   drop the modal (drops the responder; core already resolved it) and clear queued prompts of that turn
   toast(Info, "permission prompt closed: turn ended")

Session switch / quit / panic:
   AppState (and so the responder) is dropped → oneshot closed → core: Deny   (core LLD §6.5)
```

Layout:
- The modal is centred, 70% wide, and at most 80% high.
- **Title row**: `⚠ <tool>` plus a risk badge. `ReadOnly` uses `info`,
  `LocalWrite` uses `accent`, `FileWrite` uses `warning`, and `External`
  and `Execute` use `error`. The position "2 of 3" is right-aligned.
- **Body**:
  - the summary;
  - `writes:` with paths relative to the workdir;
  - the preview through `render_display`, scrollable;
  - the JSON input when toggled, pretty-printed and highlighted as
    JSON (if `artha-highlight` supports it) or plain.
- **Footer**: the key legend.

The chat underneath keeps redrawing, because the modal is just the last
painter in `draw`.

## 9. Input state machine (`input/`, `app/keymap.rs`)

```rust
pub struct InputBuffer { lines: Vec<String>, row: usize, col: usize /* byte idx on a grapheme boundary */,
                         chips: Vec<PasteChip>, undo: Vec<Snapshot>, redo: Vec<Snapshot> }
pub struct PasteChip { pub id: u32, pub text: String }   // shown as "[pasted 1,240 lines #1]"; expanded on submit
impl InputBuffer {
    pub fn insert_str(&mut self, s: &str); pub fn newline(&mut self);
    pub fn backspace(&mut self); pub fn delete(&mut self);
    pub fn move_(&mut self, m: Motion);          // Left/Right/WordLeft/WordRight/Home/End/Up/Down/BufStart/BufEnd
    pub fn kill(&mut self, k: Kill);             // Ctrl-U (to line start), Ctrl-K (to end), Ctrl-W (word)
    pub fn undo(&mut self); pub fn redo(&mut self);
    pub fn text(&self) -> String;                // with chips expanded
    pub fn is_empty(&self) -> bool;
    pub fn on_first_row(&self) -> bool; pub fn on_last_row(&self) -> bool;
    pub fn visual_height(&self, width: u16) -> u16;
    pub fn set(&mut self, s: Option<String>); pub fn clear(&mut self);
}

pub struct InputHistory { entries: Vec<String>, cursor: Option<usize>, draft: Option<String>, prefix: String }
impl InputHistory {
    pub fn prev(&mut self, current: &str) -> Option<&str>;   // the first call saves the draft and prefix
    pub fn next(&mut self) -> Option<&str>;                  // past the newest → returns the draft
    pub fn reset(&mut self);
    pub fn push(&mut self, s: String);                       // dedups consecutive entries; cap 1,000
}
```

States are `InputState::mode`. The keymap applies only when no permission
modal or overlay has focus.

```
            ┌──────────── '/' typed at row 0 col 0 ────────────┐
            ▼                                                   │
 ┌───────────────┐  Up on first row     ┌────────────────┐      │
 │    Normal     │ ───────────────────▶ │ HistoryBrowse  │      │
 │ (editing)     │ ◀─ any edit key /    │ (buffer shows  │      │
 └──┬──────▲─────┘    Down past newest  │  an entry)     │      │
    │      │                            └───────┬────────┘      │
    │      │ Esc / buffer no longer starts '/'  │ Enter = submit that entry
    │      │                                    ▼               │
    │   ┌──┴─────────────┐                   (Submit)           │
    │   │   Command      │ ◀──────────────────────────────────────┘
    │   │ completion     │  Tab: complete · ↑/↓: move in popup · Enter: run command
    │   └────────────────┘
    │
    └─ session Opening/Closed → Disabled (keys other than quit/overlay keys are ignored; drafts stay)
```

How the main keys behave in each mode:

- **Enter**
  - *Normal*: if the text is empty, do nothing. If it is a slash
    command, dispatch it (`/`-prefixed and not `//`). Otherwise
    `submit(text)`.
  - *Command*: accept the popup selection and dispatch.
  - *HistoryBrowse*: `submit(entry)`.
- **Shift-Enter, Alt-Enter, Ctrl-J**: `newline()` in *Normal* and
  *HistoryBrowse*. In *HistoryBrowse*, this first converts the entry
  into an editable draft.
- **Enter after a trailing `\`**: replace the `\` with a newline.
- **Up / Down**: on the first row, step to the previous history entry;
  on the last row, step to the next one. Otherwise move the cursor. In
  *Command*, move the popup selection.
- **Esc**:
  - with a turn running: `Cancel{drop_queue: false}`;
  - in *Command*: close the popup;
  - in *HistoryBrowse*: restore the draft.
- **Tab**: in *Command*, complete. In *Normal*, insert 4 spaces.
- **Ctrl-C**:
  - with a turn running: `Cancel`;
  - else with a non-empty buffer: `clear`;
  - else: arm quit for 1.5 s; a second press sends `Quit(UserQuit)`.
- **Ctrl-D**: `Quit` if the buffer is empty.
- **Ctrl-L**: force a full redraw.
- **Ctrl-O**: expand or collapse the selected cell, or else the most
  recent tool cell.
- **Ctrl-T**: expand or collapse thinking.
- **PgUp / PgDn**: scroll by `viewport - 2`.
- **Alt-↑ / Alt-↓**: move the cell selection.
- **End**: follow the bottom (when the buffer is empty).
- **Ctrl-Z**: `Suspend`.
- **`?`**: opens Help when the buffer is empty.

`submit(text)`:

```
local = LocalId(next_local_id++); transcript.push(User{ text, local: Some(local), state: Pending })
history.push(text); effects += [PushHistory(text), Submit{ local_id: local, text }]
buffer.clear(); scroll.follow = true
phase = if Idle { Submitting } else { phase };  queued += (phase was not Idle) as usize
// EffectResult::Submitted{local, Err(e)} → the cell becomes Rejected(e); text goes back into the buffer if it's empty
// EffectResult::Submitted{local, Ok(turn)} → cell.turn = Some(turn); Queued{behind} if it isn't yet Started
```

Slash-command registry (`app/commands.rs`):

```rust
pub struct Command {
    pub name: &'static str, pub aliases: &'static [&'static str], pub args: &'static str, pub help: &'static str,
    pub available: fn(&AppState) -> Result<(), &'static str>,       // e.g. /retry needs last_outcome == Failed
    pub run: fn(&mut AppState, args: &str) -> Vec<Effect>,
}
pub static COMMANDS: &[Command] = &[ /* help, new, resume, rename, retry, continue, rewind, undo, model,
                                       jobs, plugins, theme, copy, quit (alias exit) */ ];
pub fn parse(line: &str) -> Option<(&Command, &str)>;          // exact name or alias; else None → "unknown command" toast
pub fn complete(prefix: &str, s: &AppState) -> Vec<(&'static Command, bool /* available */)>;
```

`/rewind` builds `RewindTarget`s from the `User` cells that have a
`user_seq`, newest first. It offers the scopes `Both`,
`ConversationOnly`, and `FilesOnly`, the last only if any
`CheckpointRecorded` was seen. `/undo` sends `RewindRequest{ to_user_seq:
last user seq, scope: FilesOnly }`, following core LLD §9.3. `/model`'s
`available` returns `Err("needs artha-core support (HLD §10.2)")` until
core gains an API.

## 10. Theme (`theme/`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    Text, TextMuted, TextDim, Accent, UserPrefix, Border, BorderFocus, Selection,
    Success, Warning, Error, Info, Gain, Loss, CodeInline, CodeBlockBg, CodeText, DiffAdd, DiffDel,
    BadgeYolo, Link, Syntax(HighlightClass),
}
pub struct Theme { pub kind: ThemeKind, styles: HashMap<Token, Style> /* or a fixed array */ }
pub enum ThemeKind { Dark, Light, Mono }
impl Theme {
    pub fn new(kind: ThemeKind, depth: ColorDepth) -> Self;   // palette.rs RGB → downsampled
    pub fn style(&self, t: Token) -> Style;
}
pub struct TermCaps { pub depth: ColorDepth, pub unicode: bool, pub kbd_enhancement: bool,
                      pub sync_update: bool, pub osc8: bool, pub osc52: bool, pub background: Option<(u8,u8,u8)> }
pub enum ColorDepth { TrueColor, Ansi256, Ansi16, None }
```

`detect::probe(choice)` steps:
1. `NO_COLOR` set → `Mono`.
2. `choice != Auto` → use the choice.
3. Write `ESC ] 11 ; ? BEL`, then read raw stdin for up to 100 ms and
   parse `rgb:RRRR/GGGG/BBBB`. A luminance (Rec. 709) above 0.5 → `Light`.
4. Otherwise, `COLORFGBG` has the form `fg;bg`, where `bg` is a palette
   index: 7 or 15 → `Light`.
5. Otherwise → `Dark`.

The color depth is detected separately. Probing for kbd-enhancement
support uses `crossterm::terminal::supports_keyboard_enhancement()`.
Capabilities that can't be probed, such as OSC 8 and OSC 52 support, are
taken from `TERM_PROGRAM` allowlists, and the default is off.

## 11. Testing strategy

All tests are deterministic. They use no real TTY, no network, and a
fixed `AppState.now` (or `tokio::time::pause()`).

### 11.1 Pure unit tests (in-module)

- **`markdown::walker`**: each construct in §7.2 is checked against
  expected `RenderedBlock`s. This covers nested lists inside quotes, a
  code block inside a list, a task list, raw HTML, and a link whose text
  equals its destination.
- **`markdown::boundary`**:
  - fences with ``` and ~~~;
  - fences longer than three characters, and a closing fence that is
    shorter than the opening one (which doesn't close it);
  - a list held open across a blank line;
  - a table followed by a paragraph;
  - CRLF line endings.
- **Streaming equivalence**, as a property test. For random markdown
  documents from a small grammar (headings, lists, fences, tables,
  inline styles), and for every split of the source into 1 to 20
  chunks, including splits inside a multi-byte character (deltas are
  `String`s, so they are valid UTF-8, but the split points vary):
  - `finalize(push*(chunks)) == render(full)`;
  - every intermediate `refresh` is free of panics;
  - the stable prefix never changes once frozen.
- **`wrap`**:
  - CJK and emoji widths;
  - a word longer than the width;
  - the hang prefix on continuation lines;
  - `Truncate` adds `…` at exactly the width.
- **`table::layout`**:
  - numeric detection;
  - right alignment;
  - shrinking, then dropping columns with a `+k cols` marker;
  - the ASCII fallback;
  - `gain`/`loss` tokens.
- **`highlight`**:
  - a range that crosses a newline is split correctly;
  - gaps get `CodeText`;
  - the budget cutoff returns `None`;
  - cache hits for the same (lang, src).
- **`InputBuffer`**:
  - grapheme-correct backspace over ZWJ emoji and combining marks;
  - word motions;
  - undo and redo;
  - paste chips expand on `text()`.
- **`InputHistory`**:
  - prefix filtering;
  - the draft is restored after walking past the newest entry;
  - consecutive duplicates are removed.
- **`commands::parse` and `complete`**: aliases, `//` escaping,
  availability gating.
- **`FrameScheduler`**:
  - with CONTENT dirty, consecutive draws are at least 33 ms apart;
  - INPUT draws within 8 ms;
  - when idle, `next_deadline` is `None`;
  - with a spinner running, the deadline is about 100 ms away.

### 11.2 Reducer tests (`app/`, no tokio)

These feed hand-built `AppEvent` sequences into `App::update` and assert
on both the state and the returned `Vec<Effect>`:
- `TextDelta` creates or appends to the cell for its `(turn, round, index)`.
- Two text blocks in one round become two cells.
- `RoundRetrying{discard_partial: true}` removes that round's cells and
  nothing else.
- `ToolCallReady` without a prior `ToolCallStreaming` still creates a
  cell.
- `TurnStarted` binds the oldest pending user cell (FIFO) and handles
  `queued_behind`.
- An event with a stale `SessionGen` is a no-op.
- `TurnEnded` closes an open permission modal for that turn and drops
  its responder. The test checks this by holding the oneshot receiver
  and asserting `Err(RecvError)`.
- `Submitted(Err)` puts the text back into the buffer.

### 11.3 Render snapshots (`tests/render_snapshots.rs`)

These use `ratatui::backend::TestBackend` with fixed sizes: 80×24,
120×40, and a narrow 40×20. Each test builds an `AppState`, calls
`terminal.draw(|f| view::draw(f, &state, &mut cx))`, and snapshots the
buffer with `insta`. There are two forms:

- `assert_snapshot!(buffer_to_string(&buf))` checks the text layout.
- A second, styled form, `buffer_to_annotated(&buf)`, prints every cell
  that has a non-default style as `[token]text[/]`. Theme tokens are
  recovered by reverse lookup in the active `Theme`. This lets the
  snapshots pin colors by *meaning* instead of by RGB value, so
  palette tweaks don't churn every snapshot.

Scenes to snapshot:
- empty session (dark, light, and mono);
- the startup session picker, both loading and populated;
- a user message plus a finished assistant message with H1/H2, a list,
  a quote, inline code, a link, a highlighted Python block, and a GFM
  table;
- a streaming tail cut mid-fence;
- tool cells in each state: streaming, running with progress, a
  collapsed 250-row `Table`, the same table expanded and scrolled,
  error, denied, background ticket;
- the permission modal over a streaming chat with a `Diff` preview, plus
  the feedback field;
- the jobs and plugins overlays with a permission modal on top
  (focus/priority);
- a status bar in YOLO mode with queued and background counts;
- multi-line input of 10 or more lines (capped height and scrolling),
  and the completion popup;
- the narrow width: tables drop columns and code wraps with `↪`.

The tests use a deterministic `artha-highlight` (the real one, since it
is native and fast). If grammar output churns, these snapshots are
re-reviewed together with `artha-highlight`'s own snapshot tests.

### 11.4 Integration with real core events (`tests/e2e_harness.rs`, `tests/permission.rs`)

These use `artha_core::testing::Harness` (feature `test-util`) and
`artha_llm::testing::ScriptedProvider`, and they run `run_loop` with
three test hooks:
- an injected `TestBackend` terminal, so no `TerminalGuard` is involved
  (the runtime takes a `Backend` generic, and `run` instantiates it with
  `CrosstermBackend`);
- a scripted `InputEvent` stream instead of `input_task`;
- `tokio::time::pause()`.

The test cases are:

- **Streaming turn.** A scripted text-only turn with 200 small deltas
  produces at most `ceil(duration / 33ms) + 2` draws, counted through a
  `CountingBackend` wrapper. The final frame matches the snapshot of the
  same text rendered non-streamed.
- **Tool round trip.** A `FakeTool` returns `ToolOutput::json(..)`
  `.with_display(Table)`. The tool cell walks from Streaming through
  Running to Done with a table body. Ctrl-O expands it.
- **Permission flow.**
  - A `FakeTool` with `risk(FileWrite)` and a `ToolPlan` (summary,
    writes, and a `Diff` preview) opens the modal.
  - Scripted `y` → the tool runs. `n` → `ToolFinished{Denied}`, and the
    model sees the denial. `f` with text and Enter → the feedback
    reaches the model, which the test asserts through
    `ScriptedProvider::requests()`. `Esc` → `TurnEnded(Stopped)`.
    `a` → no second prompt for the same tool in the same session.
  - A key within 250 ms is ignored. This is tested with the virtual
    clock.
  - Ctrl-C while the modal is open → `TurnEnded(Cancelled)` and the
    modal closes.
  - While the modal is open, the spinner frame index keeps advancing
    across draws. This proves the UI isn't blocked.
- **Quit with an open prompt.** `Quit` → `run_loop` returns. The
  Harness's core logs `Deny` for the call (responder dropped), and the
  stored history passes pairing validation.
- **Queueing.** Submit twice during a running turn. The second user cell
  shows `Queued`, and then `Started` after the first `TurnEnded`.
- **Session switching.** `/new` mid-stream. Old-generation events stop
  rendering, and the old session's turn still completes and persists,
  which the test checks through `Harness::stored`.
- **Startup `Pick`.** `list_sessions` returns two `SessionInfo`s.
  Selecting one opens `Resume(id)`, `SessionOpened{resumed: true}`
  arrives, and the `HiddenHistory` placeholder renders.
- **Background notice.** `ToolScript::Background` → a ticket line, the
  status bar shows `1 bg`, and after the virtual delay a
  `BackgroundFinished` notice cell appears and the count drops to 0.
- **Initial prompt.** `TuiOptions::initial_prompt` is submitted exactly
  once, after `SessionOpened`.

### 11.5 Terminal-guard checks (`terminal.rs`, manual + CI smoke)

`TerminalGuard` can't be exercised without a PTY. A CI smoke test runs
the binary under a pseudo-terminal: `artha` starts, gets `/quit`, and
exits 0. It asserts that the output contains the alternate-screen enter
and leave sequences, and that a forced panic (`ARTHA_TUI_TEST_PANIC=1`,
compiled only with `cfg(debug_assertions)`) still emits the leave
sequence. The PTY test lives in `artha-cli`'s test suite because it needs
the binary.
