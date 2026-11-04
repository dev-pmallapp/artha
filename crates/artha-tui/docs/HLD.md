# artha-tui — High-Level Design

## 1. Purpose and scope

`artha-tui` is Artha's interactive front end: the full-screen terminal chat
that `artha [message]`, `artha -c`, and `artha -r [id]` open. It renders
one `artha-core` session as a scrolling transcript. That covers the user's
messages, the assistant's streaming markdown, thinking, tool calls and their
results, background notices, and permission prompts. It also turns
keystrokes into `SessionHandle` commands.

It is pure Rust, built on `ratatui` and `crossterm`. There is no native
rendering addon, no JS runtime, and no subprocess.

In scope:

- **Terminal lifecycle**: raw mode, alternate screen, bracketed paste,
  mouse capture, keyboard-enhancement flags, panic-safe restore, suspend
  (Ctrl-Z) and resume, and resize.
- **Chat view**: a virtualized transcript, streaming markdown with
  incremental re-parse, syntax-highlighted code blocks (through
  `artha-highlight`), inline tool cells, a multi-line input editor, and a
  status bar.
- **Overlays**: the permission modal, session picker (bare `-r` and
  `/resume`), plugin browser, jobs list, rewind picker, model picker, and
  help.
- **Slash commands**: parsing, completion, and dispatch.
- **Theming**: light/dark detection or override, and a small set of color
  tokens.
- **The sync-render/async-core bridge**: one UI loop that draws on a single
  thread, fed by async tasks.

Out of scope:

- The chat loop, tool dispatch, permission *policy*, persistence, resume,
  and rewind. `artha-core` owns all of these. The TUI only renders
  `CoreEvent`s and sends `SessionHandle` commands.
- Building the `Agent`, loading settings, and applying CLI flags. That is
  `artha-cli`'s job.
- Highlighting grammars and the tree-sitter machinery (`artha-highlight`).
- Plugin and job *management*. The browser and jobs overlays call
  narrow service traits that `artha-cli` implements over `artha-plugins`
  and `artha-jobs` (§6). `artha-tui` doesn't depend on those crates.
- Headless output (`artha ask`, `--json`). That renderer lives in
  `artha-cli`.

## 2. Position in the workspace

```
                artha-cli  (owns main(), the tokio runtime, settings, Agent)
                    │  artha_tui::run(agent, TuiOptions, TuiServices).await
                    ▼
                artha-tui ──────────────► artha-highlight
                    │                     (byte ranges + classes; no ratatui)
                    ▼
                artha-core  (Agent, SessionHandle, EventReceiver, CoreEvent,
                             SessionInfo, PermissionResponder)
```

Dependency rules:

- `artha-tui` depends on `artha-core` and `artha-highlight`. It also
  depends on `artha-llm` **only for re-exported value types** that appear
  inside `CoreEvent` (`Usage`, `StopReason`, `BlockIndex`). If core
  re-exports them, the direct dependency goes away.
- It does **not** depend on `artha-data`. Session-picker rows are
  `artha_core::SessionInfo`, which core already builds from
  `artha_data::SessionSummary`. Settings that the TUI needs, such as the
  theme, arrive already resolved in `TuiOptions`.
- It does **not** depend on `artha-jobs` or `artha-plugins`. Those features
  come in through trait objects in `TuiServices`.

### 2.1 Who owns `main()`

`artha-cli` owns `main()`, the tokio runtime, logging, and the `Agent`.
`artha-tui` exposes one async entry point and owns everything *inside* the
terminal session:

```rust
pub async fn run(agent: Agent, opts: TuiOptions, services: TuiServices) -> Result<TuiExit, TuiError>;
```

- **Why not `run(session_handle)`?** The TUI has to open and switch
  sessions itself. Bare `-r` shows a picker *before* any session exists.
  `/resume` and `/new` replace the current session mid-run. Rewind's
  "branch from here" opens a `SessionSelector::Fork`. All of these need
  `Agent::open` and `Agent::list_sessions`, so the TUI takes the cheap,
  `Clone` `Agent` plus a `StartupSelector` (`New`, `Continue`,
  `Resume(id)`, or `Pick`).
- **Why not let `artha-tui` own the runtime?** `artha-cli` already needs a
  runtime for `ask`, `jobs`, and `doctor`. The same process also runs
  in-process `artha-jobs` workers that call `SessionHandle::notify`. One
  runtime owned by the composition root keeps a single reactor and a
  single `Agent`.
- `artha-cli` must route `tracing` output to a file (or nowhere) while the
  TUI runs. `run` returns `TuiError::StdoutLogging` early if the caller
  says that stderr logging is active (`TuiOptions::stderr_is_log_sink`),
  because log lines written into an alternate screen corrupt it.
- `TuiExit` carries the final `SessionId`, if any. `artha-cli` prints
  `Resume this session with: artha -r <id>` after the terminal is
  restored.

## 3. Screen model: one layout plus an overlay stack

```
┌────────────────────────────────────────────────────────────────────┐
│ transcript (virtualized, scrollable)                               │
│                                                                    │
│ › How did NVDA close vs. its 50-day SMA?                           │
│                                                                    │
│ ● get_price_history  NVDA · 1y · daily                ✓ 0.8s       │
│   ┌────────────┬─────────┬─────────┐                               │
│   │ date       │   close │  sma_50 │                               │
│   │ 2026-11-03 │  141.22 │  133.90 │                               │
│   │ … 247 more rows · ctrl-o to expand                             │
│                                                                    │
│ NVDA closed at **141.22**, about 5.5% above its 50-day SMA…▌       │
│                                                                    │
├────────────────────────────────────────────────────────────────────┤
│ > ask anything · / for commands                                    │  input (1–10 rows)
├────────────────────────────────────────────────────────────────────┤
│ ⠙ responding · sonnet-x · ASK · 12.4k tok · 1 bg · "NVDA vs SMA"   │  status bar
└────────────────────────────────────────────────────────────────────┘
```

**Decision: a single fixed chat layout with a stack of overlays, not a
router of full-screen modes.**

Overlays are drawn on top of the chat with `Clear` plus a bordered block.
The large ones (plugins, jobs, session picker) use about 90% of the
screen. The small ones (permission, rewind, model, help) are centred.
The top overlay receives keyboard focus. The permission modal sits in its
own slot above the stack and always takes focus.

Reasons:

1. **The session keeps running underneath.** A user who opens `/jobs`
   while the assistant is streaming must still see the stream progress,
   and a permission prompt must be able to appear *over* the jobs list.
   A mode router would either stop rendering the chat or need a second
   code path to show "chat is busy" inside every mode.
2. **Permission prompts preempt everything.** Core waits indefinitely on
   the responder, so a prompt must never be hidden behind another view.
   A dedicated top slot makes "always visible, always focused" structural
   rather than a rule each mode has to remember.
3. **One draw function and one focus rule.** `draw()` paints the chat,
   then each overlay bottom-up, then the permission modal. Key routing
   goes to the permission modal if present, else the top overlay, else
   the chat. There are no per-mode event loops.
4. **The startup picker fits the same model.** With bare `-r`, the TUI
   starts with an empty chat and the picker overlay on top. Choosing a
   session opens it and pops the overlay. Pressing Esc either starts a
   new session or quits, depending on `TuiOptions`.

What we give up: a mode router would allow a jobs view with its own
layout and status bar. In v1, overlays are list-plus-detail panes, and
that is enough.

### 3.1 Views implied by the CLI surface

| View | Opened by | Data source |
|---|---|---|
| Session picker | bare `-r`/`--resume`, `/resume` | `Agent::list_sessions(SessionListFilter{ this_workdir_only, .. })` → `Vec<SessionInfo>` |
| Plugin browser | `/plugins` | `TuiServices::plugins: Option<Arc<dyn PluginsSource>>` (list, toggle enable, install from a marketplace entry) |
| Jobs list | `/jobs` | `TuiServices::jobs: Option<Arc<dyn JobsSource>>` (list, logs, pause/resume, run now), plus in-session background tickets seen through `CoreEvent` |
| Rewind picker | `/rewind`, `/undo` | the transcript's user cells and their `user_seq` → `SessionHandle::rewind` |
| Model picker | `/model` | `TuiServices::models` (needs a core API; see §10) |
| Help | `/help`, `?` with an empty input | static keymap plus the slash-command registry |
| Permission modal | `CoreEvent::PermissionRequested` | the prompt itself |

If a service is `None`, its slash command reports "not available in this
build or mode" and doesn't open an empty view.

## 4. Rendering pipeline

### 4.1 Markdown: `pulldown-cmark` and our own span renderer

The architecture doc names `pulldown-cmark` or `comrak`. We choose
**`pulldown-cmark`**:

- It is a pull parser over `&str` with no AST allocation. Re-parsing the
  growing tail of a streaming message is cheap, which is the operation we
  do most.
- It is pure Rust with no C dependencies, and it supports the GFM
  extensions we need (tables, strikethrough, task lists) behind
  `Options`.
- We don't need `comrak`'s full AST, source-position mapping, or HTML
  fidelity.

We write the renderer from pulldown events to ratatui lines ourselves
(about 600 lines in `markdown/`) instead of using a ready-made
markdown-to-ratatui crate. The available ones bring their own
highlighting engine (which conflicts with `artha-highlight`), don't
expose the block boundaries that incremental streaming needs, and tend to
lag ratatui releases. The renderer produces a width-independent
`Vec<RenderedBlock>` (styled logical lines plus a block kind). Wrapping
to the terminal width is a separate, cached step (`wrap.rs`), so a resize
re-wraps but doesn't re-parse.

Supported in terminal form:

- headings (bold and accent; H1/H2 underlined with a rule)
- paragraphs, emphasis, strong, and strikethrough
- inline code (code token style)
- links (text, plus a dimmed URL in parentheses; OSC 8 hyperlinks when
  the terminal supports them)
- bullet, ordered, and task lists with nesting indentation
- block quotes (a `│` gutter)
- fenced and indented code blocks (highlighted, with a language label)
- GFM tables (box-drawn, numeric columns right-aligned, cells truncated
  with `…` to fit)
- horizontal rules

Raw HTML is shown as literal text. Images become `[image: alt]`.

### 4.2 The `artha-highlight` seam: byte ranges and classes, not spans

**Decision: `artha-highlight` returns sorted, non-overlapping byte ranges
tagged with a small closed `HighlightClass` enum. `artha-tui` maps each
class to a theme token and builds ratatui `Span`s.**

```rust
// what artha-tui needs from artha-highlight (contract proposed here; artha-highlight's own docs finalize it)
pub fn language_for(fence_info: &str) -> Option<Language>;              // "py", "python", "ts", "bash", "sh", "zig", …
impl Highlighter {
    pub fn highlight(&self, lang: Language, src: &str) -> Result<Vec<HighlightRange>, HighlightError>;
}
pub struct HighlightRange { pub bytes: std::ops::Range<usize>, pub class: HighlightClass }
```

Why ranges rather than styled spans:

- **No ratatui in `artha-highlight`.** `artha-report` may want highlighted
  code in Typst output, and any future GUI would want it too. A UI-crate
  type in the highlighter would tie every consumer to ratatui's version.
- **The theme belongs to the UI.** Light/dark switching and 16-color
  fallback just remap classes to styles. The highlighter never re-runs
  for a theme change.
- **Caching is simple.** Ranges are plain data, keyed by a hash of
  (language, source), and independent of the theme or width.

Why a closed class enum rather than raw tree-sitter capture names:
capture names vary between grammars (`@function.method.call`,
`@keyword.control.import`, …). `artha-highlight` folds them into about 14
classes (keyword, string, number, comment, function, type, constant,
variable, property, operator, punctuation, attribute, escape, and
heading/emphasis for markdown-in-code). The TUI then needs exactly one
token per class.

Budget: code blocks over 256 KiB or 4,000 lines are shown unhighlighted.
Highlighting runs synchronously on the UI thread, behind a cache; a
tree-sitter parse of a few hundred lines takes well under a millisecond.
A *streaming* code block is re-highlighted at most every 150 ms and once
when its fence closes. Lines that arrive between highlights are drawn in
plain style.

### 4.3 Streaming without re-parsing everything

ratatui redraws a full frame into a back buffer every draw, then diffs it
against the previous buffer and writes only the changed cells. Terminal
**flicker** is therefore not the problem. The two real costs are CPU time
to *build* the frame and how often we draw. We address both:

1. **Coalesced draws.** Core events mark the state `dirty`. A
   `FrameScheduler` draws at most once per 33 ms (about 30 fps) while
   events keep arriving, and draws immediately (if at least 8 ms have
   passed) after a key press, so typing never feels throttled. When idle,
   there are zero wake-ups. Animation ticks (spinners) run only while
   something is animating.
2. **Batch-drain before drawing.** On each wake, the loop drains every
   ready `AppEvent` up to a budget (512 events or 4 ms) before it draws.
   A burst of 300 deltas becomes one frame.
3. **Incremental markdown: a stable prefix and a live tail.** Each
   streaming text cell keeps the raw `String`, a `stable_upto` byte
   offset, the rendered blocks for `raw[..stable_upto]`, and a re-rendered
   tail. On a frame where the cell changed, a cheap line scanner advances
   `stable_upto` to the last *safe block boundary*: a blank line outside
   any fenced code block, not inside a list or table continuation. Only
   `raw[stable_upto..]` is re-parsed. That tail is usually one paragraph
   or one code block, so the work per frame stays O(tail), not
   O(message). Prefix blocks are never re-parsed. Their wrapped lines are
   cached per width.
4. **Virtualized transcript.** Each cell caches its wrapped height for
   the current width. A prefix-sum over the heights maps the scroll
   offset to (cell, line). Only lines that intersect the viewport are
   cloned into the frame. A 2,000-message session costs the same to draw
   as a 20-message one.
5. **Synchronized output.** Each draw is wrapped in crossterm's
   `BeginSynchronizedUpdate`/`EndSynchronizedUpdate`. Supporting terminals
   present the frame atomically, which removes any remaining tearing on
   large repaints. Other terminals ignore the sequence.

On `TurnEnded` or `AssistantCommitted`, the cell is *finalized*: the whole
text gets one last full parse. That pass corrects any boundary
heuristic's misjudgment, such as a reference-style link defined later.
The cell then becomes immutable.

`RoundRetrying { discard_partial: true }` removes the text, thinking, and
tool-streaming cells of that turn and round before the retry streams.

### 4.4 Tool-call visibility

Each tool call is one **tool cell** in the transcript, keyed by `CallId`,
and it moves through these states:

```
ToolCallStreaming ─▶ Pending (args streaming)  "● get_quote …"
ToolCallReady     ─▶ Ready   (args shown compactly: summary of input)
PermissionRequested ▶ AwaitingPermission  (cell shows "⏸ waiting for approval"; modal opens)
ToolStarted       ─▶ Running  (spinner, plan summary, elapsed timer, eager badge)
ToolProgress      ─▶ Running  (latest message + optional progress gauge)
CheckpointRecorded ▶ adds "⎌ checkpoint" badge (undoable)
ToolFinished      ─▶ Done(✓ | ✗ error | ⊘ denied | ⊘ cancelled | – skipped) + elapsed + display body
```

The result **body** comes from `ToolFinished.display` (`ToolDisplay`):

- `Table` → our table renderer. It right-aligns numeric columns, colors a
  leading sign with the `gain`/`loss` tokens, and truncates cells.
- `Markdown` → the markdown pipeline.
- `Code` → highlighted code.
- `Diff` → a `+`/`-` colored unified diff with the path as a header.
- `None` → only the status line. For `Error`, the status line also shows
  the first line of the message.

Large results are **collapsed by default** to 8 body lines with a
`… N more rows · ctrl-o to expand` footer. Ctrl-O toggles the most recent
tool cell, and in scroll-select mode (§5) it toggles the selected cell. An
expanded body is still virtualized like any other lines, so a 5,000-row
price history doesn't slow the frame. `ToolDisplay` is UI-only and not
persisted, so tool cells rebuilt from a resumed history show only a
status line (§10, item 1).

A `ToolOutcome::Ok { background: Some(ticket) }` renders a `⧗ started
in background: <label>` line. The later
`Notice::BackgroundFinished`/`Failed` renders as its own notice cell. That
cell links back by ticket id and includes the result's display body if it
has one. The status bar counts open tickets.

### 4.5 Permission prompts: a modal over a live UI

`CoreEvent::PermissionRequested(prompt)` puts the prompt, including its
`PermissionResponder`, into `AppState::permission`. The UI loop **never
blocks** on it. The modal is just state that `draw()` paints and the
keymap routes to, so spinners, streaming text from parallel eager calls,
background notices, and resizes all keep working underneath.

The modal shows:
- the tool name and risk badge (color by `ToolRisk`)
- the summary
- the "2 of 3" position
- planned writes
- the preview (a `ToolDisplay`, typically a diff)
- a collapsible pretty-printed JSON input

Keys:

| Key | Decision |
|---|---|
| `y` / Enter | `AllowOnce` |
| `a` | `AllowForSession` |
| `n` | `Deny { feedback: None }` |
| `f` | opens a one-line feedback field; Enter sends `Deny { feedback: Some(..) }` |
| `Esc` / `s` | `DenyAndStop` |
| Ctrl-C | `SessionHandle::cancel` (core resolves the prompt as `Deny`, then ends the turn as `Cancelled`) |

Safety properties:

- **Dropping means deny.** The responder lives only in `AppState`. If the
  TUI exits, panics, or switches sessions with a prompt open, dropping the
  state drops the responder, and core treats that as `Deny`. We never
  hold a clone anywhere else.
- **Stale prompts close themselves.** When `TurnEnded` arrives for the
  prompt's turn, or `respond()` returns `false`, the modal closes with a
  notice instead of waiting for a key that would do nothing.
- **No accidental approval.** Key presses that arrive within 250 ms of
  the modal opening are ignored. This stops a user who is mid-typing
  from approving with Enter or `y` by accident.
- In `--yolo` (Skip) mode, core never asks, and the status bar shows a
  red `YOLO` badge for the whole session.

## 5. Input handling

- **Editor.** A custom multi-line `InputBuffer` (a `Vec<String>` of lines
  with a grapheme-aware cursor), rather than `tui-textarea`. That crate
  ties us to its ratatui version, and we need hooks for slash completion,
  history, and paste handling that its API doesn't expose cleanly. The
  editor is about 400 lines and fully unit-testable.
- **Submit vs. newline.**
  - Enter submits.
  - Shift-Enter inserts a newline. This needs keyboard-enhancement
    (kitty protocol) flags, which we push when the terminal supports
    them.
  - Alt-Enter and Ctrl-J always insert a newline.
  - A line ending in `\` followed by Enter becomes a newline. This is
    the portable fallback.
  - Bracketed paste inserts text verbatim, never submits, and folds
    pastes over 1,000 lines into a placeholder chip that is expanded on
    submit.
- **History.** Up on the first row or Down on the last row walks the
  history, which is filtered by the current prefix. The history is seeded
  from the session's user messages, and from the optional `HistoryStore`
  service across sessions. The in-progress draft is preserved while
  walking.
- **Slash commands.** A `/` at column 0 of the first row turns on command
  mode. A completion popup lists matching commands, and Tab or Enter
  completes. Unknown commands are rejected locally with a hint and are
  never sent to the model. Text beginning with `//` is sent literally,
  minus one slash.
- **Sending.** On submit, the trimmed text is echoed immediately as a
  user cell in the *pending* state. A spawned task then calls
  `SessionHandle::submit(UserInput::text(..))`. The cell is bound to its
  turn when `TurnStarted { user_seq, queued_behind }` arrives. The user
  may submit while a turn runs. Core queues it FIFO, and the cell shows
  `queued` until it starts.
- **Scroll and select.**
  - PgUp/PgDn and the mouse wheel scroll the transcript.
  - End, or typing anything, jumps back to *follow* mode. New output then
    auto-scrolls only while following.
  - Alt-↑/Alt-↓ step a selection cursor across cells, used for
    expand/collapse and copy.
- **Interrupts.**
  - Esc during a turn cancels it (`cancel(drop_queue: false)`).
  - Ctrl-C cancels a running turn. When idle, it clears the input; when
    idle with an empty input, a second press within 1.5 s quits.
  - Ctrl-D on an empty input quits.

### 5.1 Slash commands

Derived from the CLI surface and the core docs:

| Command | Effect |
|---|---|
| `/help` | help overlay |
| `/new` | close the current session and open `SessionSelector::New` |
| `/resume [id]` | picker, or open `Resume(id)` directly |
| `/rename <title>` | `SessionHandle::rename` |
| `/retry` | `SessionHandle::retry()` (offered after `Failed`) |
| `/continue` | submits "continue" (offered after `MaxTokens` / `RoundLimit`) |
| `/rewind` | rewind picker → `RewindRequest { to_user_seq, scope }`; the result's `restored_prompt` refills the input |
| `/undo` | the last edit only: `RewindScope::FilesOnly` at the last user seq (core LLD §9.3) |
| `/model [name]` | model picker or direct switch. **Blocked on a core API** (§10) |
| `/jobs` | jobs overlay |
| `/plugins` | plugin browser |
| `/theme [auto\|dark\|light]` | switch the theme for this run; persisting it is `artha-cli`'s job through `TuiServices::settings` |
| `/copy` | copy the last assistant message through OSC 52 |
| `/quit`, `/exit` | quit |

The registry is a static table (`name`, aliases, argument hint,
`availability(&AppState)`, handler), so `/help` and completion are
generated from it. Plugin-contributed slash commands are deferred. The
table is designed to accept dynamic entries later.

## 6. Service seams for non-core features

```rust
pub struct TuiServices {
    pub jobs: Option<Arc<dyn JobsSource>>,          // artha-cli adapts artha-jobs
    pub plugins: Option<Arc<dyn PluginsSource>>,    // artha-cli adapts artha-plugins
    pub history: Option<Arc<dyn HistoryStore>>,     // artha-cli adapts artha-data KvStore("tui")
    pub settings: Option<Arc<dyn SettingsWriter>>,  // persist /theme (and later /model)
    pub models: Option<Arc<dyn ModelCatalog>>,      // names for /model; switching needs core (§10)
}
```

These are small `async_trait` traits with view-model return types defined
in `artha-tui` (`JobRow`, `PluginRow`, …). They keep `artha-tui`'s
dependency set to core and highlight, let tests use in-memory fakes, and
let `artha-cli` decide what is available. For example, `artha-cli` passes
no jobs source in builds without `artha-jobs`.

## 7. Concurrency model: a single-threaded renderer over async sources

```
              tokio runtime (owned by artha-cli; multi-thread)
 ┌─────────────────────────────────────────────────────────────────────┐
 │ input task: crossterm::EventStream ──► mpsc<InputEvent> (bounded 256)│
 │ core bridge task (one per open session, tagged with SessionGen):     │
 │     EventReceiver.next() ──► mpsc<(SessionGen, CoreEvent)> (bounded) │
 │ effect tasks: submit/cancel/rewind/list_sessions/service calls       │
 │     ──► mpsc<EffectResult> (unbounded; one message per effect)       │
 │                                                                     │
 │ UI loop  (= the `run` future, block_on'd on the main thread)         │
 │   select!{ biased; input, effect results, core, frame deadline }     │
 │   → App::update(AppEvent) -> Vec<Effect>   (pure, sync)              │
 │   → spawn effects                                                    │
 │   → if scheduler says so: terminal.draw(|f| view::draw(f, &state))   │
 └─────────────────────────────────────────────────────────────────────┘
```

- **Draws stay on one thread.** `artha-cli`'s `#[tokio::main]` uses
  `block_on(run(..))`, which polls the top-level future on the main
  thread. Every `terminal.draw` happens inside that future, so there is
  no cross-thread `Terminal`. A draw takes a few milliseconds at most
  (virtualized lines and cached highlights), so running it on the
  runtime's main thread doesn't starve the worker threads that run core.
- **Elm-style core.** `App::update(&mut self, AppEvent) -> Vec<Effect>` is
  synchronous and does no I/O. The runtime runs the async `Effect`s
  (`Submit`, `Cancel`, `Rewind`, `OpenSession`, `ListSessions`,
  `Respond`, service calls) on spawned tasks and feeds their results back
  as `AppEvent::Effect(..)`. This keeps `SessionHandle` and `Agent` out of
  `AppState`, which makes `update` testable without a runtime.
  `PermissionResponder::respond` is synchronous, but it still goes
  through an effect so the reducer stays pure.
- **Backpressure is preserved.** The core bridge `await`s on a bounded
  channel. If the UI ever falls behind, core's own bounded channel fills,
  and the backpressure reaches the model stream, as core HLD §9 intends.
  **Input is selected first** (`biased`), so keystrokes are never stuck
  behind a delta flood.
- **Session switching.** Every opened session gets a fresh `SessionGen`.
  The bridge for the old session is aborted, and any of its events still
  in flight are dropped by generation. Then the old handle's `close()` is
  awaited in the background. `close` lets the current turn finish, as
  core specifies, and a dropped old receiver makes core auto-deny any
  further prompts.
- **Terminal I/O on suspend.** Ctrl-Z (on Unix) restores the terminal,
  raises `SIGTSTP`, and re-initializes on `SIGCONT`. The input task is
  paused during this. Windows doesn't get Ctrl-Z suspend.
- **Panics.** A panic hook installed by `TerminalGuard` restores the
  terminal before the default hook prints. That covers both a panicking
  UI and a panicking effect task. The guard's `Drop` restores the terminal
  on normal exit and on `?` errors.

## 8. Theming

- **Mode.**
  - `TuiOptions::theme` is `Auto`, `Dark`, or `Light`. `artha-cli`
    resolves it from settings and flags.
  - `Auto` queries the terminal background with OSC 11, with a 100 ms
    timeout, *before* the input stream starts. A luminance above 0.5
    means light.
  - The fallback is the `COLORFGBG` environment variable, and then dark.
- **Color depth.** Truecolor if `COLORTERM` is `truecolor` or `24bit`;
  else 256 colors if `TERM` contains `256color`; else 16 colors.
  Palettes are written in RGB and downsampled once at startup.
  `NO_COLOR` turns on a monochrome theme that uses only
  bold/dim/reverse/underline.
- **Tokens.** There are 20 UI tokens and 14 syntax tokens (one per
  `HighlightClass`).
  - Base text: `text`, `text_muted`, `text_dim`.
  - Accents: `accent`, `user_prefix`.
  - Borders: `border`, `border_focus`, `selection`.
  - Status: `success`, `warning`, `error`, `info`.
  - Finance: `gain`, `loss`.
  - Code and diffs: `code_inline`, `code_block_bg`, `diff_add`,
    `diff_del`.
  - Badges: `badge_yolo`, `link`.

  Each token is a ratatui `Style`. We use no background color except
  `code_block_bg` and `selection`, so the terminal's own background shows
  through. That is why the same palette works under most user themes.
  There are no user-defined themes in v1.

## 9. Dependencies

Versions are the intended major lines and were **not** re-verified
against crates.io in this pass. Confirm them when writing `Cargo.toml`.

| Crate | Version | Why |
|---|---|---|
| `artha-core` | path | `Agent`, `SessionHandle`, `EventReceiver`, `CoreEvent`, `SessionInfo`, `PermissionResponder`, `RewindRequest`; dev: feature `test-util` (`Harness`) |
| `artha-highlight` | path | `Highlighter`, `Language`, `HighlightRange`, `HighlightClass` |
| `artha-llm` | path | only value types inside `CoreEvent` (`Usage`, `StopReason`); drop the dependency if core re-exports them |
| `ratatui` | 0.29 | layout, widgets, `Buffer` diffing, `TestBackend` |
| `crossterm` | the version ratatui re-exports, used via `ratatui::crossterm` | backend, `EventStream` (feature `event-stream`), bracketed paste, keyboard enhancement, synchronized update; avoids two crossterm versions |
| `pulldown-cmark` | 0.12 | markdown pull parser (§4.1); `default-features = false` (no HTML renderer) |
| `unicode-width` | 0.2 | display width for wrapping and table layout |
| `unicode-segmentation` | 1 | grapheme-aware cursor and wrapping |
| `tokio` | workspace | `select!`, `mpsc`, `time::Instant`/`sleep_until`, `spawn` |
| `futures-util` | 0.3 | `StreamExt` over `EventStream` and `EventReceiver` |
| `async-trait` | workspace | object-safe service traits |
| `thiserror` | workspace | `TuiError` |
| `tracing` | workspace | diagnostics (to the file sink only) |
| `base64` | 0.22 | OSC 52 `/copy` |

Dev-only: `insta` (buffer snapshots), `tokio` with `test-util` (paused
clock for throttling tests), `artha-core` with `test-util`, and
`artha-llm` with `test-util`.

Deliberately not used:
- `tui-textarea`, for the reasons in §5.
- Ready-made markdown-to-ratatui crates, for the reasons in §4.1.
- `syntect`, because highlighting is `artha-highlight`'s job and a second
  engine would mean two grammar sets.
- A terminal-background-detection crate: OSC 11 plus `COLORFGBG` is
  about 60 lines in `theme/detect.rs`. Revisit if terminal quirks pile up.
- `textwrap`, which wraps strings, not styled spans. Our wrapper works on
  spans directly.
- A clipboard crate, since OSC 52 avoids X11/Wayland/Win32 bindings.
- `bitflags` and `lru`. The dirty flags and the 256-entry highlight cache
  are small enough to hand-roll. The LLD names these crates only as
  possible drop-in replacements.

## 10. Integration issues with sibling designs

Raised here, decided in the owning crate:

1. **There is no way to get a resumed session's transcript (artha-core).**
   `CoreEvent::SessionOpened` carries only `history: usize`, and
   `SessionHandle` has no read accessor. On `-c` or `-r`, the TUI cannot
   draw the earlier conversation, and it can't offer `/rewind` targets
   from before this run, because it never learns their `user_seq`s.
   Proposed fix: a `SessionHandle::transcript() -> Result<Vec<TranscriptEntry>>`
   (served by the actor from memory), or a `CoreEvent::HistoryLoaded {
   entries }` right after `SessionOpened`. `TranscriptEntry` would hold
   the role, seq, `artha_llm::Message`, and the interrupted/synthetic
   flags. Until then, the TUI shows "Resumed session · N earlier messages
   (not shown)".
2. **`/model` has no core command (artha-core vs artha-data).**
   `artha-data`'s LLD now provides `SessionStore::update_provider`, which
   cites a user `/model` command. However, `artha-core` has no
   `SessionHandle::set_model` and no `Command` variant for it. The
   provider is fixed per `Agent`. Core HLD §11 item 1 still lists the
   missing update call as open, which is now out of date. `/model` is
   shown as unavailable until core adds a per-session model override.
3. **Some session metadata has no event (artha-core).** Core auto-renames
   the session after the first turn (`derive_title`), but it emits no
   event. `SessionOpened` also doesn't carry the title, model, or
   permission mode. The status bar wants all of these. Proposed fix: add
   `title`, `model`, and `permission_mode` to `SessionOpened`, plus a
   `SessionRenamed { title }` event. Until then, `artha-cli` passes the
   model and mode in `TuiOptions`, and the TUI shows the first user line
   as the title.
4. **There is no way to cancel or list in-process background tasks
   (artha-core).** `BackgroundSpawner` documents that "the user cancels
   the task", but `SessionHandle` has no `cancel_background(ticket_id)`.
   The jobs overlay can list durable jobs through `JobsSource`, but it can
   only *display* in-process tickets, built from `ToolFinished` and
   `Notice` events.
5. **The theme has no settings field (artha-data, minor).**
   `Preferences` has no theme field. Proposed fix: add
   `preferences.theme: "auto" | "dark" | "light"`. Until then,
   `artha-cli` reads it from `Settings.extra`.
