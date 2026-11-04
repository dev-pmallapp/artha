# artha-tools — High-Level Design

## 1. Purpose and scope

`artha-tools` holds Artha's **built-in tools**: the concrete `Tool`
implementations the agent can call. It covers the seven categories in
ARCHITECTURE §2: Markets, Macro and economy, News and web, Research, Your
data (portfolios, watchlists, a local paper-trading ledger), Automation
(monitors), and Memory. It also ships the SQLite tables for the state
those tools own, and one `PromptContributor` (memory recall).

In scope:

- **Tool implementations.** Each one is a `TypedTool` wrapped in
  `artha_core::Typed<T>`, so it plugs into `artha_core::ToolRegistry`
  unchanged.
- **Three storage namespaces** in the shared `artha.db`, following
  artha-data's sibling-table rules (artha-data HLD §5.4, LLD §5.6):
  `memory`, `portfolio`, and `monitor`. Each comes with a forward-only
  `MigrationSet` and a small repository struct over `artha_data::Db`.
- **Domain logic that is ours.** That means paper-ledger accounting
  (positions, cost basis, cash), portfolio valuation, monitor condition
  evaluation and its edge-trigger state, memory dedupe and FTS5 search,
  and deterministic financial math (`research_calc`).
- **The memory system-prompt section**, a `PromptContributor` with id
  `"memory"`.
- **Library entry points** behind the `artha memory <tick|stats|model|import|install>`
  subcommands. `artha-cli` parses the arguments and calls them (§7.4).
- **Two ports** that `artha-cli` wires at startup. One reaches the
  finance data layer (`artha-finance`). The other reaches the scheduler
  (`artha-jobs`).

Out of scope:

- **Fetching market, macro, news, or filings data.** That is
  `artha-finance`'s job, whether through the optional Python sidecar
  (ARCHITECTURE §5 option 1) or direct HTTP clients (option 2). This crate
  never talks to a market-data vendor. It calls `artha-finance` through
  one trait (§5).
- **Technical-indicator math** over price series. `artha-finance` owns it
  (ARCHITECTURE §3 crate table).
- **Cron scheduling, the durable job queue, job claiming, and running
  unattended agent turns.** These belong to `artha-jobs`. This crate
  persists monitor *definitions* and *evaluates* them on request.
  `artha-jobs` decides *when* to ask and carries out the resulting
  *action* (§6).
- **Generic cron and background-research tools** (`jobs add <goal>` as a
  tool call). The crate that owns `jobs_job` should own the tool that
  writes it, so these ship from `artha-jobs` (§11, item 6).
- **Report layout and typesetting.** These belong to `artha-report`. We
  only wrap it in one tool (`report_render`).
- **Real brokerage actions.** There is no tool with `ToolRisk::External`
  in v1, and the paper ledger never leaves the machine.
- **Permission policy, checkpointing, truncation, timeouts, and the
  rendering of `ToolError`s for the model.** These are artha-core
  policy. Tools only declare `ToolTraits` and `ToolPlan`.

## 2. Position in the workspace

```
                    artha-cli  (composition root)
     builds ToolDeps, registers builtin_tools(), adds memory contributor,
     wires FinanceClient → artha-finance and MonitorScheduler → artha-jobs
                         │
       ┌─────────────────┼─────────────────────────┐
       ▼                 ▼                         ▼
  artha-tools ──impl Tool / PromptContributor──▶ artha-core
       │  │  │                                     │
       │  │  └──▶ artha-report (feature "report")  ▼
       │  └─────▶ artha-finance (FinanceClient)  artha-llm, artha-data
       └────────▶ artha-data (Db, MigrationSet, KvStore)

  artha-jobs ──(no compile-time edge to artha-tools)──
     reaches MonitorEvaluator only through an adapter that artha-cli builds
```

Dependency rules:

- `artha-tools` depends on `artha-core` (the `schemars` feature),
  `artha-llm` (for `ToolSpec` and `ToolResultContent`, which appear in
  core's API), `artha-data`, and `artha-finance`. `artha-report` is
  optional.
- `artha-tools` **does not** depend on `artha-jobs`, and `artha-jobs`
  does not depend on `artha-tools`. The monitor handoff in both
  directions goes through small traits. Each trait is owned by the crate
  that *calls* it, and `artha-cli` supplies roughly 30 lines of adapter
  code (§6). This keeps `artha-jobs` free of the finance dependency tree,
  and keeps `artha-tools` free of `tokio-cron-scheduler`.
- Foreign keys from our tables point only into our own namespace or into
  `core_*`, per artha-data's migration rule. `monitor_definition.job_id`
  is a plain `TEXT` column with no FK, and the link is enforced in code.

## 3. Module-per-category layout

| Module | Category | Tools (model-facing names) | Owns tables | Runtime deps |
|---|---|---|---|---|
| `markets` | Markets | `market_quote`, `market_history`, `market_fundamentals`, `market_indicators`, `market_search` | — | `FinanceClient` |
| `economy` | Macro and economy | `macro_series`, `macro_calendar` | — | `FinanceClient` |
| `news` | News and web | `news_search`, `web_fetch` | — | `FinanceClient` (news), `reqwest` (fetch) |
| `research` | Research | `research_screen`, `research_filings`, `research_calc`, `report_render` | — | `FinanceClient`, `artha-report` (opt) |
| `portfolio` | Your data | `portfolio_view`, `portfolio_manage`, `paper_trade`, `watchlist_show`, `watchlist_edit` | `portfolio_*` | `Db`, `FinanceClient` (valuation, fills) |
| `automation` | Automation | `monitor_create`, `monitor_list`, `monitor_manage` | `monitor_*` | `Db`, `MonitorScheduler`, `FinanceClient` (evaluation) |
| `memory` | Memory | `memory_save`, `memory_search`, `memory_forget` | `memory_*` | `Db` |

(The module is named `economy` and not `macro`, because `macro` is a
reserved word in Rust.)

Each module exposes the same small surface:

```rust
pub fn tools(deps: &ToolDeps) -> Result<Vec<Arc<dyn Tool>>, CoreError>;  // every module
pub static MIGRATIONS: MigrationSet;   // portfolio, automation (ns "monitor"), memory only
pub struct <X>Repo { db: Db }          // portfolio, automation, memory only
```

The crate root assembles them:

```rust
let tools = artha_tools::builtin_tools(&deps)?;            // Vec<Arc<dyn Tool>>
registry.register_all(tools)?;                              // artha_core::ToolRegistry
let agent = Agent::builder(provider, registry, db, cps, cfg)
    .prompt_contributor(artha_tools::memory::contributor(&deps))
    .build()?;
// migrations, collected before Db::open:
Db::open(&path, opts, &[&JOBS_MIGRATIONS, &artha_tools::MEMORY_MIGRATIONS,
                         &artha_tools::PORTFOLIO_MIGRATIONS, &artha_tools::MONITOR_MIGRATIONS])?;
```

`artha_tools::MIGRATIONS` lists the three sets in the order the CLI
should pass them. None of them reference each other, so any order works.

### 3.1 Tool design rules (apply to every module)

1. **One risk level per tool.** `ToolTraits` are static (core LLD §3.1),
   so we never put a read action and a write action behind one tool
   name. That is why `watchlist_show` is separate from `watchlist_edit`,
   and `portfolio_view` from `portfolio_manage`. A user can then grant
   "always allow `watchlist_edit`" without also granting trades.
2. **Model-facing names** use the `<category>_<verb>` pattern, in
   snake_case, and are always fewer than 64 characters. They are stable
   API: changing one invalidates cached prompts and saved
   `NonInteractive` allow lists. `names.rs` defines them as constants,
   and an insta snapshot pins every `ToolSpec`.
3. **Structured output is `Json`.** Each tool returns a small, flat JSON
   object (core HLD §5.2), plus an optional `ToolDisplay::Table` for the
   UI. Number fields from `Decimal`s are serialized as strings
   (`"187.32"`) so precision survives. Every market-data result carries
   `as_of` and `source`, so the model can tell how fresh the data is.
4. **Budget-aware outputs.** Tools cap their own row counts
   (`market_history` returns at most 500 bars by default and downsamples
   longer ranges), so results come in well under core's 64 KiB
   truncation. They also say so (`"downsampled": "weekly"`). Core's
   truncation is a backstop. Tools shouldn't rely on it.
5. **`plan()` validates everything validatable without I/O**: symbol
   syntax, date ranges, enum combinations, and decimal parsing. It also
   writes the permission-prompt summary. An `Err` from `plan` skips
   execution and costs nothing.
6. **No reactor blocking.** All SQL goes through `Db::call` (writes) or
   the crate's `db_read` helper (spawn_blocking with `Db::read`; see §11
   item 3). All finance calls are async. CPU-heavy work, such as
   valuation over a large ledger or HTML-to-text conversion, runs on
   `spawn_blocking` once its input is over a size threshold.
7. **Cancel-safe.** Every `.await` is a point where the future may be
   dropped. Writes are a single `Db::call` (one transaction), so a
   cancelled tool never leaves half a ledger entry.

### 3.2 Risk and permission profile

| Tool | `risk` | `concurrency_safe` | `confirm` | `timeout` | Notes |
|---|---|---|---|---|---|
| `market_*`, `macro_*`, `news_search`, `research_screen`, `research_filings` | ReadOnly | yes | Default | 45 s | eligible for eager start |
| `web_fetch` | ReadOnly | yes | Default | 20 s | SSRF guard (§8.3) |
| `research_calc` | ReadOnly | yes | Default | 5 s | pure Rust, no I/O |
| `report_render` | FileWrite | no | Default | 120 s | writes declared in `ToolPlan.writes` |
| `portfolio_view`, `watchlist_show`, `monitor_list`, `memory_search` | ReadOnly | yes | Default | 30 s | |
| `watchlist_edit` | LocalWrite | no | **Never** | 10 s | low stakes, frequent |
| `memory_save` | LocalWrite | no | **Never** | 10 s | the core LLD's own example |
| `memory_forget` | LocalWrite | no | Default | 10 s | deleting asks |
| `portfolio_manage` | LocalWrite | no | Default | 10 s | create account, deposit, record holding |
| `paper_trade` | LocalWrite | no | Default | 30 s | the prompt shows side, qty, est. price |
| `monitor_create`, `monitor_manage` | LocalWrite | no | Default | 15 s | |

In unattended runs (`AgentConfig::unattended(allow)`), only tools in
`allow` may write. `artha_tools::names::read_only()` returns the
ReadOnly set, so `artha-jobs` can build a least-privilege allow list
without hard-coding names.

## 4. How tools plug into `artha-core`

- **Every tool is a `TypedTool`.** Its `Input` struct derives
  `Deserialize` and `JsonSchema`, and its doc comments become the field
  descriptions the model sees. `Typed::new` builds the `ToolSpec` once.
  Serde errors become `ToolError::InvalidInput` with the field path,
  which core renders as "Invalid arguments: … Check the tool's input
  schema" (core LLD §3.2). Root inputs are always structs, so the schema
  root is `"type": "object"`.
- **Tools hold `Arc`s to their dependencies**, such as `Arc<MemoryRepo>`
  or `Arc<dyn FinanceClient>`. They have no global state. Two `Agent`s in
  one process (a TUI session plus a jobs runner) share the same tool
  instances through the registry.
- **What a running tool uses from `ToolContext`:**
  - `session_id`: provenance for memory entries, ledger rows, and
    monitors.
  - `call_id`: the ledger idempotency key (§7.2).
  - `cancel`: watched by `web_fetch` and `report_render`.
  - `progress()`: used by `report_render`, `research_screen`, and
    `portfolio_view` when they value many positions.
  - `edits()`: **not used**. See §9.
  - `background()`: **not used in v1**. No built-in tool here runs
    longer than its timeout. Long agent-driven research is a job in
    `artha-jobs`, and monitors are durable by construction.
- **The memory `PromptContributor`** registers through
  `AgentBuilder::prompt_contributor`. It is described in §7.3.

## 5. Seam to `artha-finance`

ARCHITECTURE §5 leaves the finance data layer open: a sidecar now,
native HTTP later, and possibly paid APIs. Tools must not care which one
is used. The seam is **one object-safe async trait**, which we expect
`artha-finance` to export. `artha-tools` holds it as
`Option<Arc<dyn FinanceClient>>`:

```rust
#[async_trait]
pub trait FinanceClient: Send + Sync + 'static {
    fn availability(&self) -> Availability;              // cheap, cached; no I/O
    async fn quotes(&self, symbols: &[Symbol]) -> Result<Vec<Quote>, FinanceError>;
    async fn history(&self, req: &HistoryRequest) -> Result<PriceSeries, FinanceError>;
    async fn fundamentals(&self, req: &FundamentalsRequest) -> Result<Fundamentals, FinanceError>;
    async fn indicators(&self, req: &IndicatorRequest) -> Result<IndicatorSeries, FinanceError>;
    async fn search(&self, query: &str, limit: u32) -> Result<Vec<SymbolMatch>, FinanceError>;
    async fn macro_series(&self, req: &MacroRequest) -> Result<MacroSeries, FinanceError>;
    async fn calendar(&self, req: &CalendarRequest) -> Result<Vec<CalendarEvent>, FinanceError>;
    async fn news(&self, req: &NewsRequest) -> Result<Vec<NewsItem>, FinanceError>;
    async fn screen(&self, req: &ScreenRequest) -> Result<Vec<ScreenRow>, FinanceError>;
    async fn filings(&self, req: &FilingsRequest) -> Result<Vec<Filing>, FinanceError>;
    async fn fx_rate(&self, from: &Currency, to: &Currency) -> Result<FxRate, FinanceError>;
}
```

`artha-finance` has no design docs yet, so this is **the contract
`artha-tools` needs**, not a description of finished code. LLD §4 lists
the exact DTO fields we read. The ownership rules:

- **`artha-finance` owns the DTOs** (`Quote`, `PriceSeries`, …) and
  `FinanceError`, because `artha-report` and `artha-jobs` (through the
  evaluator) consume the same data. We read DTOs and never define
  parallel copies.
- **Money and quantities are `rust_decimal::Decimal`**, matching the
  decimal-`TEXT` rule in artha-data HLD §5.6.
- **`FinanceError::kind()`** gives a stable classification. All of our
  error mapping keys off that classification (§8), never off message
  text.
- **`availability()`** reports `Ready`, `NotProvisioned { hint }`
  (sidecar or Python missing), or `Degraded { reason }`. It must be cheap
  and must not block, because tools check it first and fail fast with a
  model-readable message (§8.2).
- **Tools are registered whether or not finance is available.** If they
  were not, the tool list (and with it the prompt-cache prefix) would
  change with the sidecar's state, and the model couldn't tell the user
  why it has no quotes. An unavailable source becomes a clear
  `ToolError::Failed { retryable: false }`.
- **No caching in `artha-tools`.** Quote caching and rate limiting
  belong to `artha-finance`, which can see every caller.

## 6. Seam to `artha-jobs`: monitor definition, scheduling, and firing

**Decision:** this crate **owns and evaluates** monitors. `artha-jobs`
**owns time and actions.** Neither crate reads the other's tables. The
handoff is two narrow traits, wired by `artha-cli`.

```
monitor_create (tool)                         artha-jobs runner / `artha jobs tick`
 1 validate (plan)                            ┌─────────────────────────────────────┐
 2 INSERT monitor_definition                  │ claims due jobs_job (BEGIN IMMEDIATE)│
   status='pending', job_id=NULL             │ config.handler == "monitor"          │
 3 MonitorScheduler::schedule(spec) ─────────▶│ → INSERT jobs_job(kind='cron', …)    │
                ◀── JobRef{job_id} ───────────│                                      │
 4 UPDATE job_id, status='scheduled'          │ at fire time:                        │
                                              │   MonitorEvaluator::evaluate(id) ────┼──▶ artha-tools
                                              │     (reads definition + state, calls │    (via cli adapter)
                                              │      FinanceClient, updates state,   │
                                              │      INSERT monitor_event if fired)  │
                                              │   ◀── Evaluation::Fired{event, action}
                                              │   carry out the action:              │
                                              │     Notify   → notice / jobs_log      │
                                              │     RunAgent → run_turn_headless      │
                                              │   MonitorEvaluator::record_dispatch() ┼──▶ artha-tools
                                              └─────────────────────────────────────┘
```

- **`MonitorScheduler`** is defined *here* because we call it. Its
  methods are `schedule`, `pause`, `resume`, and `cancel`. `artha-cli`
  implements it with `artha-jobs`' public API. The job it creates is an
  ordinary `jobs_job` row: `kind = 'cron'`, the cron expression or
  interval taken from the monitor, `goal` set to the monitor's name, and
  `config = {"handler":"monitor","monitor_id":"…"}`.
- **`MonitorEvaluator`** is a concrete struct exported by this crate.
  `artha-jobs` defines its own handler trait (for example `JobHandler`,
  keyed by `config.handler`). `artha-cli` registers an adapter whose
  `run` calls `MonitorEvaluator::evaluate`.
- **Evaluation is deterministic.** It never uses an LLM. It fetches the
  observation through `FinanceClient` (or the ledger, for portfolio
  conditions) and applies **edge-triggered** semantics: a monitor fires
  on the not-satisfied to satisfied transition. It re-arms only once the
  condition is false again, and it respects `cooldown`, `max_fires`, and
  `expires_at`. All state lives in `monitor_state`, so firing is correct
  across processes and restarts.
- **Actions are data, and executing them is `artha-jobs`' job.**
  - `Notify { message }` becomes a `Notice` on any open session in the
    same process (`SessionHandle::notify`) plus a `jobs_log` line, which
    `artha jobs logs` shows.
  - `RunAgent { goal, allow_tools }` becomes
    `Agent::run_turn_headless` with `AgentConfig::unattended(allow_tools)`
    and the observation prepended to the goal. An unattended agent may
    only `paper_trade` if the user explicitly listed it when creating
    the monitor. `monitor_create`'s permission prompt shows the list.
- **Failure and reconciliation.** Steps 2 to 4 cannot share one
  transaction. `Db` has a single connection behind a mutex, and the jobs
  API opens its own `Db::write`, so nesting it inside ours would
  deadlock. Instead:
  - If step 3 fails, the row from step 2 is deleted and the tool
    returns `Failed`.
  - A crash between steps leaves `status = 'pending'`.
    `MonitorRepo::reconcile` runs on every `monitor_list` and from
    `artha-cli` at startup. It reschedules pending rows.
  - An evaluator call for an unknown id returns `Evaluation::Gone`, and
    the jobs adapter then cancels that orphaned job.
  - Deleting a monitor cancels its job first, then deletes the row.
- **Why not have `artha-jobs` poll `monitor_definition` directly?** It
  would couple jobs to our schema, and would make jobs carry the finance
  client and the condition logic. **Why not a queue that `artha-jobs`
  owns?** `jobs_job` already *is* that durable queue. The monitor is
  simply one handler kind on it.

## 7. Key domain designs

### 7.1 Markets, macro, news, research

These are thin, validated wrappers over `FinanceClient`.
- **Inputs** are normalized in `plan`: symbols upper-cased, dates parsed,
  ranges bounded.
- **Outputs** are shaped for the model: compact JSON with `as_of`,
  `source`, and `currency`, plus a UI table.
- `research_calc` is pure Rust. It covers CAGR, NPV, IRR, a simple DCF,
  percent change, position sizing, and compounding, using `Decimal`. It
  exists because "runs the numbers" (ARCHITECTURE §1) should not depend
  on the model's arithmetic or on the sidecar.
- `web_fetch` is the one tool that does its own HTTP, through `reqwest`
  with rustls. It is generic web access, not market data. It returns
  readable text, capped at `max_chars`.
- `report_render` hands a typed report model to `artha-report` and
  writes the PDF to a path in the workdir that is known at plan time
  (§9).

### 7.2 Portfolio, watchlists, paper ledger

- **The ledger is the source of truth and is append-only.**
  `portfolio_ledger` rows (buy, sell, deposit, withdraw, dividend, fee,
  split, adjust) are never updated. Positions, average cost, realized
  P&L, and cash per currency are **derived in Rust** from the ledger. We
  use the average-cost method in v1, and the computation is pure (no
  I/O), so it is property-testable. Corrections are compensating
  entries.
- **There are two account kinds**, as in artha-data's proposal:
  - `paper`: trades are simulated, cash is enforced (no margin in v1),
    and fills are recorded.
  - `tracked`: the user mirrors a real account by hand ("I own 50 MSFT
    at 310"). Holdings are recorded as `adjust` entries, and cash is not
    enforced.
- **Paper fills.** v1 supports market orders only. A fill uses the
  `FinanceClient` quote at execution time, adjusted by `slippage_bps`
  (default 5) and with a flat `commission` (default 0). Both come from
  `ToolsConfig`. The quote's timestamp and market state are recorded on
  the fill. Limit and stop orders are left out of v1. The user can
  express them as a monitor with a `RunAgent` action that has
  `paper_trade` explicitly allowed.
- **Idempotency.** Each trade row stores the `tool_call_id`, with
  `UNIQUE(account_id, tool_call_id)`. A re-executed call (for example,
  after resume repair) reports "already executed" and does not buy
  twice.
- **"Report my portfolio's current value"** is `portfolio_view`:
  1. Derive the positions.
  2. Batch-quote them in one `quotes()` call.
  3. Convert to the base currency with `fx_rate`.
  4. Return market value, cost basis, unrealized and realized P&L,
     cash, total, and weights, with per-position `as_of`.

  If finance is unavailable, it still returns the positions, cash, and
  cost basis, with `valuation: "unavailable"` and the reason. Partial
  data the user owns is better than an error.
- **"Add AAPL to watchlist"** is `watchlist_edit { action: "add", symbols: ["AAPL"] }`.
  It is idempotent (`INSERT OR IGNORE`) and lazily creates the `default`
  watchlist. Symbols are validated syntactically in `plan`. Existence is
  optionally checked with `search` when finance is ready, and the tool
  warns rather than fails when it isn't.

### 7.3 Memory

- **Storage.** `memory_entry` holds `kind` (fact, preference, or
  instruction), the content (at most 2,000 characters), tags,
  `importance` (0 to 3), `pinned`, `origin`, provenance, usage counters,
  and an optional `expires_at`. It is indexed by an external-content
  FTS5 table (`memory_entry_fts`, porter + unicode61) that triggers keep
  in sync. The triggers fire only on `content` and `tags` updates, so
  usage bumps don't churn the index. A `content_hash` (normalized SHA-256)
  is `UNIQUE`, so saving the same fact twice returns the existing id.
- **Two recall paths, chosen deliberately:**
  1. **The `memory` system-prompt section** (`PromptContributor`).
     Core calls it once per turn, and the section lands in the
     **system prompt**, which is the head of the prompt-cache prefix.
     A section that varies with each turn's `user_text` would therefore
     invalidate the cached conversation on *every* turn. So in its
     default `RecallMode::Profile`, the contributor emits a **stable
     profile**: pinned entries, all `instruction` and `preference`
     entries, and the top facts by importance. They are ordered by id,
     capped at `profile_budget_chars` (default 2,000), and followed by
     one line saying how many other memories exist and that
     `memory_search` retrieves them. The text changes only when memory
     changes.
  2. **`memory_search`, an explicit tool** for relevance recall. It runs
     FTS5 BM25 with recency, importance, and pinned boosts, and the
     model calls it when the conversation needs it. The base prompt and
     the section's footer tell the model to search before claiming
     ignorance of the user's preferences.

  An opt-in `RecallMode::ProfilePlusRelevant` also adds the top-k BM25
  hits for this turn's `user_text` to the section. It is documented as
  costing a prompt-cache miss per turn, which is acceptable only for
  local models with no caching. §11 item 1 asks core for a better
  placement.
- **Prompt-injection hygiene.**
  - The section is framed as *data about the user*. Instruction-kind
    entries are listed under "User-stated standing instructions"; facts
    are quoted.
  - `memory_save` refuses content that looks like tool-output
    boilerplate or exceeds the length cap.
  - Memory writes are denied in unattended runs unless they are
    allowlisted (core's default for `LocalWrite`).
  - `origin` records whether the model, the user (through `/remember` in
    the TUI, if added), or `memory import` created an entry.
- **Forget means gone.** `memory_forget` hard-deletes the row, and the
  FTS index entry goes with it through the trigger. `memory tick` later
  runs FTS `optimize` and `PRAGMA incremental_vacuum` when it's enabled.
  `artha purge` remains the full reset.

### 7.4 `artha memory` subcommands: what is ours

`artha-cli` owns argument parsing and output formatting. The work is
done by `artha_tools::memory::admin::MemoryAdmin`.

| Subcommand | Owner of the logic | What it does |
|---|---|---|
| `stats` | **artha-tools** | Counts by kind, pinned count, total bytes, FTS row count and consistency check, oldest and newest, top tags, last `tick` time, current recall mode and model. Read-only. |
| `import [file\|-]` | **artha-tools** | Imports JSON Lines (`{"kind","content","tags",…}`) or a Markdown bullet list (`- [pref] …`). It normalizes, dedupes by `content_hash`, sets `origin='import'`, inserts in one transaction, and reports added, duplicate, and rejected rows. `--dry-run`. |
| `tick` | **artha-tools** (logic). `artha-jobs` may schedule it. | Deterministic maintenance: delete expired entries, decay `importance` of never-used, unpinned facts older than N days (config, off by default), FTS `optimize`, and record the `last_tick` watermark in `KvStore("memory")`. **No LLM in v1.** Distilling new memories from recent transcripts is a planned extension (`tick --extract`, LLD §8.6) that would run as a headless agent turn through `artha-jobs`, not inside this crate. |
| `model <name>` | **artha-tools** (setting) | **(unconfirmed)** We read this as selecting the embedding model for semantic recall. v1 recall is FTS5-only. The command records the choice in `KvStore("memory")["embedding_model"]`, and `stats` reports it as "configured, not active in this build". |
| `install` | **artha-tools** + `artha-finance`/doctor | **(unconfirmed)** We read this as provisioning that embedding model (download into `~/.artha/data/`). v1 returns a clear "semantic recall is not available in this build" error with exit code 2. The `Embedder` seam is sketched in LLD §8.6 so this can land without schema churn. |

The scheduling of `tick` (for example, daily) is an `artha-jobs` concern
using the same handler-adapter pattern as monitors (`handler: "memory_tick"`).

### 7.5 Automation: what a monitor is

A monitor is `condition + schedule + action + firing policy`, persisted
as validated JSON enums:
- **Conditions:** `price_above`, `price_below`, `percent_move`,
  `indicator_cross` (price versus SMA or EMA, via
  `FinanceClient::indicators`), `volume_spike`, `news_mention` (new
  articles matching a query, deduped by article id in
  `monitor_state.seen`), and `portfolio_drawdown` (our own ledger plus
  quotes).
- **Schedule:** `every` N seconds (minimum 60) or a cron expression,
  plus `market_hours_only`.
- **Actions:** `notify` or `run_agent` (§6).
- **Firing policy:** `cooldown`, `max_fires`, and `expires_at`.

## 8. Error-handling philosophy

The principle, inherited from core: **a tool problem is data for the
model.** Every failure becomes a `ToolError` whose text a model can act
on. It says what happened, whether retrying makes sense, and what to do
instead. Nothing in this crate panics on bad input or on external
failures. Panics are bugs, and core isolates them anyway.

### 8.1 Mapping

| Cause | `ToolError` | Model-facing message pattern |
|---|---|---|
| Serde failure on input (handled by `Typed`) | `InvalidInput` | the serde path and reason |
| `plan()` validation (bad symbol syntax, `end < start`, qty ≤ 0) | `InvalidInput` | the field plus a fix: "symbols must look like AAPL, BRK.B, ^GSPC" |
| `FinanceErrorKind::NotFound` (unknown symbol or series) | `InvalidInput` | "Unknown symbol 'APPL'. Use market_search to find the ticker." The model's argument is wrong, so it should correct it. |
| `FinanceErrorKind::InvalidRequest` / `Unsupported` | `InvalidInput` / `Failed{false}` | what isn't supported, such as "intraday history isn't available from this source" |
| `FinanceErrorKind::NotProvisioned` / `availability() == NotProvisioned` | `Failed{retryable:false}` | "Market data is unavailable: {hint} (run `artha doctor --fix`). Tell the user; don't retry." |
| `Timeout`, `Upstream` (5xx, network), `RateLimited` | `Failed{retryable:true}` | the cause plus a `retry_after` hint when there is one. Core appends "(transient; retrying once may succeed)". |
| `Internal` (sidecar protocol violation, decode error) | `Failed{false}` | a short cause. Full details go to `tracing`. |
| `DataError::is_busy()` | `Failed{true}` | "Local database is busy (another Artha process); try again." |
| Other `DataError` | `Failed{false}` | "Local storage error: {short}". This is logged at `error`. |
| Ledger rule (insufficient cash or shares, unknown account) | `InvalidInput` | the balance and the shortfall, so the model can resize |
| Missing optional dependency (scheduler or report engine not wired, `web` feature off) | `Failed{false}` | "Monitors are not available in this build/session: …" |
| `ctx.cancel` observed | `Cancelled` | (core text) |

`ToolError` is `#[non_exhaustive]`, and we only construct `InvalidInput`,
`Failed`, and `Cancelled`. Internally every module returns
`Result<_, ToolsError>`, a `thiserror` enum. A single
`impl From<ToolsError> for ToolError` does the mapping, which keeps the
table above in one tested function.

### 8.2 Optional runtime dependencies

`ToolDeps` carries `finance: Option<Arc<dyn FinanceClient>>` (which is
`None` when the CLI was built without finance or failed to construct the
client), `scheduler: Option<Arc<dyn MonitorScheduler>>`, and the optional
report engine. A tool whose dependency is `None`, or reports
`NotProvisioned`, fails fast in `execute`. It fails *before* any network
or DB work, with the message above. We always register these tools
(§5), so the model sees a stable toolset and can explain the gap.
Partial results win over failure where that is meaningful
(`portfolio_view`, `watchlist_show`, and `monitor_list`, which still
lists monitors when the scheduler is missing).

### 8.3 Network safety for `web_fetch`

- Only `http` and `https` are allowed.
- Redirects are limited to 5.
- The response is capped at `max_bytes` (2 MiB) before decoding.
- The tool refuses loopback, link-local, private (RFC 1918 / ULA), and
  metadata addresses after DNS resolution, unless
  `ToolsConfig.web.allow_private` is set. Tests set it.
- Content types other than text, HTML, JSON, and XML are refused with a
  clear message.

## 9. File-editing tools

**None of the v1 tools edit user files**, apart from one tool that
*creates* an output file: `report_render` (feature `report`).
- Its output path is an input argument. It defaults to
  `./artha-reports/<slug>-<date>.pdf` and is resolved against
  `ctx.workdir`.
- The path is known before execution, so it goes in **`ToolPlan.writes`**.
  Core then snapshots it before execution (PreEdit, recorded as absent
  for a new file, so `/undo` deletes it) and after.
- It does **not** call `ctx.edits().before_write()`. That API is for
  tools that discover paths while running, and no tool here does.

There is no `edit_file`, `write_file`, or shell tool in this crate. The
ARCHITECTURE scope doesn't call for one, and core's checkpoint machinery
doesn't need one to be useful. If one is added later, it follows the
same rules: declared writes in `plan()`, `FileWrite` risk, and
`ctx.edits()` only for dynamically discovered paths. It never touches
`CheckpointStore` directly (core HLD §7).

## 10. Dependencies

Versions are the intended major lines. They were **not** re-verified
against crates.io in this pass, so confirm them when writing
`Cargo.toml`.

| Crate | Version | Features | Why |
|---|---|---|---|
| `artha-core` | path | `schemars` | `Tool`, `TypedTool`/`Typed`, `ToolContext`, `PromptContributor`, `ToolRegistry`. |
| `artha-llm` | path | — | `ToolSpec`, `ToolResultContent` in core's public signatures. |
| `artha-data` | path | default (`async`) | `Db`, `MigrationSet`, `KvStore`, `Timestamp`, `SessionId`, re-exported `rusqlite`. |
| `artha-finance` | path | — | `FinanceClient`, DTOs, `FinanceError`. It is a required dependency for *types*. The sidecar is runtime-optional. |
| `artha-report` | path, optional | feature `report` | `report_render`. |
| `async-trait` | 0.1 (workspace) | — | `TypedTool`, `PromptContributor`, and our ports are object-safe async traits. |
| `schemars` | 1 | `derive` | Input schemas, via core's `Typed`. |
| `serde` / `serde_json` | 1 (workspace) | `derive` | Inputs, outputs, and JSON columns. |
| `rust_decimal` | 1 | `serde-str` | Money and quantities, serialized as strings and stored as decimal `TEXT`. |
| `jiff` | 0.2 | `serde` | Date parsing for tool inputs ("2024-01-31", "6mo"), market-hours checks, cron next-fire previews. `Timestamp` stays epoch ms at the storage boundary. |
| `uuid` | 1 | `v7` | Account, watchlist, and monitor ids (time-sortable, like `SessionId`). |
| `sha2` | 0.11 | — | Memory `content_hash`. It is already in the tree through artha-data. |
| `reqwest` | 0.12 | `rustls-tls`, `gzip`, `stream`, no default features; optional, feature `web` | `web_fetch` only. |
| `html2text` | 0.x, optional (`web`) | — | HTML to readable text for `web_fetch`. |
| `url` | 2.5 | — | URL validation, already in the tree. |
| `croner` | 2 | — | Parse and validate cron expressions in `monitor_create.plan` (the same syntax family as `tokio-cron-scheduler`). It doesn't schedule. |
| `thiserror` | workspace | — | `ToolsError`. |
| `tracing` | 0.1 (workspace) | — | A span per tool call, with error detail kept out of model text. |
| `tokio` | 1 (workspace) | `rt`, `time` | `spawn_blocking`, `select!` on cancel. |

Dev-only:
- `artha-core` with `test-util` (`tool_context`, `Harness`)
- `artha-data` with `test-util` (`TestEnv`, `assert_objects_prefixed`)
- `artha-llm` with `test-util`
- `insta` (spec and output snapshots)
- `proptest` (ledger math, FTS query sanitizer)
- `tokio` with `test-util`

Features:
- `default = ["web", "report"]`
- `web`
- `report`
- `test-util`, which exports `testing::{FakeFinance, FakeScheduler, deps_for_test}` for `artha-cli` and `artha-jobs` integration tests

Deliberately not used:
- **`sqlx` or an ORM.** artha-data standardizes on rusqlite.
- **An embedding or vector crate** (`fastembed`, `sqlite-vec`). Semantic
  recall is unconfirmed (§7.4).
- **A market-data client crate.** That is `artha-finance`'s business.
- **`chrono`.** `jiff` has a better time-zone story for market hours.
- **`anyhow`.**

## 11. Integration issues with sibling docs

These are raised here, not decided here.

1. **Prompt cache versus relevance recall (`artha-core`).**
   `PromptContributor` output can only go into the system prompt, and
   core builds that once per turn from `PromptInputs.user_text`. Any
   contributor that uses `user_text`, which `PromptInputs` invites with
   its "for relevance-based recall" comment, changes the head of the
   cache prefix on every turn. That is a cache miss on the whole
   conversation each turn. We sidestep this with a stable profile plus
   the `memory_search` tool (§7.3). The proper fix belongs to core: a
   contribution placement such as
   `Placement::System | Placement::UserPrefix`, where the user-prefix
   kind is folded into the user message the same way background notices
   are (core LLD §10). That would let per-turn recall stay cache-safe.
2. **`ToolTraits` is `#[non_exhaustive]` with no constructor
   (`artha-core`).** Code outside core can't use a struct literal or
   `..Default::default()` on it. It can only mutate the fields of
   `ToolTraits::default()`. That works, and `common/traits.rs` wraps it,
   but core should add builder methods (`ToolTraits::new(risk).serial().timeout(d).confirm(c)`)
   so every tool crate doesn't repeat the dance. The same applies to
   `ToolOutput`, which already has constructors, so it's fine. Also,
   the brief for this design described the traits as
   `{risk, timeout, parallel_safety}`. Core's LLD is authoritative, and
   its fields are `risk`, `concurrency_safe`, `timeout`, and `confirm`,
   which this crate follows.
3. **There is no async *read* on `Db` (`artha-data`).** `Db::call` always
   runs through `write` (`BEGIN IMMEDIATE`). Read-only tools and the
   per-turn memory contributor would therefore take SQLite's RESERVED
   lock and contend with `artha jobs tick` in another process. We use a
   local `db_read` helper (`spawn_blocking` plus `Db::read`), but a
   `Db::call_read` in artha-data would be the right home for it.
4. **FTS5 triggers under `trusted_schema=OFF` (`artha-data`).**
   artha-data sets `trusted_schema=OFF` and proposes trigger-synced
   external-content FTS5. With that pragma, a virtual table used from a
   trigger must be flagged innocuous. We believe FTS5 is, but this is
   unverified against the bundled SQLite version. LLD §8.2 adds a
   contract test. If it fails, the fallback (FTS maintenance in Rust
   inside the same `Db::write` transaction) is designed and keeps the
   same schema minus the triggers.
5. **Stale text in artha-data HLD §4** still says file-editing tools
   wrap writes in `checkpoints.begin_edit(…)`. Core HLD §11 already
   flagged this. This crate confirms the core contract: tools use
   `ToolPlan.writes` or `ctx.edits()`, and never `CheckpointStore`.
6. **Job tools and the handler discriminator (`artha-jobs`, no docs
   yet).**
   - The cron and background-research tools should ship from
     `artha-jobs`, the owner of `jobs_job`.
   - artha-data's proposed `jobs_job` has no handler column. We put
     `{"handler": "...", ...}` in `config` and need `artha-jobs` to
     dispatch on it. We also need it to accept a `goal` that is a
     descriptive label for handler jobs, not an agent prompt.
   - It must expose `schedule`, `pause`, `resume`, and `cancel` for
     `MonitorScheduler`, and a handler-registration hook for the
     evaluator adapter.
7. **artha-data's proposed monitor and memory schemas are extended**,
   not followed verbatim. This is allowed, because owners "start from
   them". On memory we add `pinned`, `content_hash`, `origin`,
   `use_count`, and `expires_at`. On monitors we add `schedule`, a
   `status` column, and a `monitor_state` table, and the condition JSON
   uses a `type` tag. LLD §5 has the final DDL.
8. **The `artha-finance` contract (§5) is a requirement on a crate that
   has no design yet.** Its LLD should confirm the `FinanceClient`
   shape, `FinanceError::kind()`, a non-blocking `availability()`, and
   `Decimal` money.
9. **Core lists "file editing" under `artha-tools`** (core HLD §1, out of
   scope). As §9 explains, v1 ships no general file-editing tool. The
   only `FileWrite` tool is `report_render`.
