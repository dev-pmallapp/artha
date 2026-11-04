# artha-tools: Low-Level Design

This is the companion to `HLD.md`. It describes code that does not exist
yet: `src/` holds only a placeholder `lib.rs`, and none of the files
below have been created.

Signatures from other crates come from these sources:
- `artha-core/docs/LLD.md` (§3 tool contract, §11 `PromptContributor`,
  §13 testing)
- `artha-data/docs/LLD.md` (§5 `Db`, migrations, `KvStore`)

The `artha-finance` items are **the contract this crate needs** (HLD §5),
pending that crate's own LLD.

Path conventions in code blocks:
- `core::` is `artha_core`.
- `data::` is `artha_data`.
- `llm::` is `artha_llm`.
- `fin::` is `artha_finance`.

## 1. Module layout

```
crates/artha-tools/
├── Cargo.toml
├── migrations/
│   ├── memory/0001_memory.sql         memory_entry, memory_entry_fts, sync triggers
│   ├── portfolio/0001_portfolio.sql   portfolio_account, portfolio_ledger, portfolio_watchlist(_item)
│   └── monitor/0001_monitor.sql       monitor_definition, monitor_state, monitor_event
├── src/
│   ├── lib.rs                 pub mods; builtin_tools(); MIGRATIONS; flat re-exports; #![deny(unsafe_code)]
│   ├── deps.rs                ToolDeps, ToolsConfig (+ WebConfig, PaperConfig, MemoryConfig)
│   ├── error.rs               ToolsError, From<ToolsError> for core::ToolError, map_finance(), map_data()
│   ├── names.rs               tool-name consts, read_only(), all()
│   ├── common/
│   │   ├── mod.rs
│   │   ├── traits.rs          traits_ro(), traits_local_write(), traits_file_write() (ToolTraits builders)
│   │   ├── db.rs              db_read() (spawn_blocking + Db::read), db_write() (= Db::call)
│   │   ├── symbol.rs          parse_symbols(), SYMBOL_RE rules → fin::Symbol
│   │   ├── money.rs           Decimal parse/format helpers, dec_str serde module
│   │   ├── time.rs            DateRange parsing ("6mo", "2024-01-01..2024-06-30"), Timestamp <-> jiff
│   │   ├── output.rs          json_output(), table(), with_as_of(), downsample policy
│   │   └── finance.rs         require_finance(&ToolDeps) -> Result<&Arc<dyn FinanceClient>, ToolsError>
│   ├── markets/
│   │   ├── mod.rs             tools()
│   │   ├── quote.rs           MarketQuote
│   │   ├── history.rs         MarketHistory
│   │   ├── fundamentals.rs    MarketFundamentals
│   │   ├── indicators.rs      MarketIndicators
│   │   └── search.rs          MarketSearch
│   ├── economy/
│   │   ├── mod.rs             tools()
│   │   ├── series.rs          MacroSeries
│   │   └── calendar.rs        MacroCalendar
│   ├── news/
│   │   ├── mod.rs             tools()
│   │   ├── search.rs          NewsSearch
│   │   ├── fetch.rs           WebFetch (cfg feature "web")
│   │   └── net_guard.rs       resolve_and_check(): SSRF guard (pure + DNS)
│   ├── research/
│   │   ├── mod.rs             tools()
│   │   ├── screen.rs          ResearchScreen
│   │   ├── filings.rs         ResearchFilings
│   │   ├── calc.rs            ResearchCalc + pure calc::{cagr, npv, irr, dcf, …}
│   │   └── report.rs          ReportRender (cfg feature "report")
│   ├── portfolio/
│   │   ├── mod.rs             tools(), PORTFOLIO_MIGRATIONS
│   │   ├── model.rs           AccountId, Account, AccountKind, LedgerEntry, EntryType, Watchlist…
│   │   ├── repo.rs            PortfolioRepo (SQL only)
│   │   ├── ledger.rs          pure: derive_positions(), cash_balances(), check_trade()
│   │   ├── valuation.rs       value_account(): positions + quotes + fx → Valuation
│   │   ├── fill.rs            pure: simulate_market_fill()
│   │   ├── view.rs            PortfolioView
│   │   ├── manage.rs          PortfolioManage
│   │   ├── trade.rs           PaperTrade
│   │   └── watchlist.rs       WatchlistShow, WatchlistEdit
│   ├── automation/
│   │   ├── mod.rs             tools(), MONITOR_MIGRATIONS, MonitorEvaluator re-export
│   │   ├── model.rs           MonitorId, MonitorDefinition, MonitorCondition, MonitorAction, Schedule, …
│   │   ├── repo.rs            MonitorRepo (SQL) + reconcile()
│   │   ├── scheduler.rs       MonitorScheduler port, ScheduleSpec, JobRef, SchedulerError
│   │   ├── condition.rs       pure: observe plan + satisfied(); EdgeState transitions
│   │   ├── evaluator.rs       MonitorEvaluator, Evaluation, FiredMonitor, DispatchOutcome
│   │   ├── create.rs          MonitorCreate
│   │   ├── list.rs            MonitorList
│   │   └── manage.rs          MonitorManage (pause | resume | delete)
│   ├── memory/
│   │   ├── mod.rs             tools(), contributor(), MEMORY_MIGRATIONS
│   │   ├── model.rs           MemoryId, MemoryKind, MemoryEntry, NewMemory, Origin
│   │   ├── normalize.rs       pure: normalize_content(), content_hash(), looks_like_tool_output()
│   │   ├── fts.rs             pure: build_match_query() (sanitizer), rank formula constants
│   │   ├── repo.rs            MemoryRepo (SQL incl. FTS)
│   │   ├── recall.rs          MemoryContributor: core::PromptContributor, RecallMode, render_section()
│   │   ├── admin.rs           MemoryAdmin: stats/import/tick/set_model/install
│   │   ├── save.rs            MemorySave
│   │   ├── search.rs          MemorySearch
│   │   └── forget.rs          MemoryForget
│   └── testing.rs             cfg(test | feature="test-util"): FakeFinance, FakeScheduler, TestDeps
└── tests/
    ├── schema.rs              prefix + idempotency + FTS-under-pragmas contract tests
    ├── specs.rs               insta snapshot of every ToolSpec + traits
    ├── markets.rs             finance-mapped tools against FakeFinance
    ├── portfolio.rs           ledger/trade/valuation flows
    ├── automation.rs          create/reconcile/evaluate edge-trigger flows
    ├── memory.rs              save/search/forget, contributor output, admin
    ├── web_fetch.rs           local HTTP server, guard, caps
    ├── e2e.rs                 core::testing::Harness with ScriptedProvider driving real tools
    └── snapshots/
```

Planned `Cargo.toml`. It has not been written yet, and the versions
still need to be verified:

```toml
[features]
default   = ["web", "report"]
web       = ["dep:reqwest", "dep:html2text"]
report    = ["dep:artha-report"]
test-util = []

[dependencies]
artha-core    = { path = "../artha-core", features = ["schemars"] }
artha-llm     = { path = "../artha-llm" }
artha-data    = { path = "../artha-data" }
artha-finance = { path = "../artha-finance" }
artha-report  = { path = "../artha-report", optional = true }
async-trait   = { workspace = true }
croner        = "2"
html2text     = { version = "0.x", optional = true }
jiff          = { version = "0.2", features = ["serde"] }
reqwest       = { version = "0.12", default-features = false, features = ["rustls-tls", "gzip", "stream"], optional = true }
rust_decimal  = { version = "1", features = ["serde-str"] }
schemars      = { version = "1", features = ["derive"] }
serde         = { workspace = true }
serde_json    = { workspace = true }
sha2          = "0.11"
thiserror     = { workspace = true }
tokio         = { workspace = true }   # rt, time, macros
tracing       = { workspace = true }
url           = "2.5"
uuid          = { version = "1", features = ["v7", "serde"] }

[dev-dependencies]
artha-core = { path = "../artha-core", features = ["schemars", "test-util"] }
artha-data = { path = "../artha-data", features = ["test-util"] }
artha-llm  = { path = "../artha-llm", features = ["test-util"] }
insta      = { version = "1", features = ["json"] }
proptest   = "1"
tokio      = { workspace = true }      # + test-util
```

## 2. Crate root, dependencies, configuration

### 2.1 `lib.rs`

```rust
pub mod automation; pub mod economy; pub mod markets; pub mod memory;
pub mod news; pub mod portfolio; pub mod research;
pub mod names;
mod common; mod deps; mod error;
#[cfg(any(test, feature = "test-util"))] pub mod testing;

pub use deps::{ToolDeps, ToolsConfig, WebConfig, PaperConfig, MemoryConfig};
pub use error::ToolsError;
pub use memory::{MEMORY_MIGRATIONS, MemoryRepo, MemoryContributor, RecallMode, admin::MemoryAdmin};
pub use portfolio::{PORTFOLIO_MIGRATIONS, PortfolioRepo};
pub use automation::{MONITOR_MIGRATIONS, MonitorRepo, MonitorEvaluator, Evaluation, FiredMonitor,
                     DispatchOutcome, MonitorScheduler, ScheduleSpec, JobRef, SchedulerError};

/// In the order artha-cli should pass them to Db::open (any order is valid: no cross-refs).
pub static MIGRATIONS: [&data::MigrationSet; 3] = [&MEMORY_MIGRATIONS, &PORTFOLIO_MIGRATIONS, &MONITOR_MIGRATIONS];

/// Every built-in tool, already wrapped in core::Typed. Order is irrelevant (ToolSet sorts).
pub fn builtin_tools(deps: &ToolDeps) -> core::Result<Vec<Arc<dyn core::Tool>>> {
    let mut v = Vec::new();
    v.extend(markets::tools(deps)?);
    v.extend(economy::tools(deps)?);
    v.extend(news::tools(deps)?);
    v.extend(research::tools(deps)?);
    v.extend(portfolio::tools(deps)?);
    v.extend(automation::tools(deps)?);
    v.extend(memory::tools(deps)?);
    Ok(v)
}
```

Composition in `artha-cli` (illustrative):

```rust
let db = data::Db::open(&paths.database_file(), opts,
    &[&artha_jobs::JOBS_MIGRATIONS, artha_tools::MIGRATIONS[0], artha_tools::MIGRATIONS[1], artha_tools::MIGRATIONS[2]])?;
let deps = ToolDeps {
    db: db.clone(),
    finance: artha_finance::client(&paths, &settings).ok().map(|c| Arc::new(c) as Arc<dyn fin::FinanceClient>),
    scheduler: Some(Arc::new(cli::adapters::JobsMonitorScheduler::new(jobs.clone()))),
    #[cfg(feature = "report")] report: Some(Arc::new(artha_report::Engine::new())),
    config: ToolsConfig::from_extra(settings.extra.get("tools"))?,
};
registry.register_all(artha_tools::builtin_tools(&deps)?)?;
let agent = Agent::builder(provider, registry, db, checkpoints, cfg)
    .prompt_contributor(artha_tools::memory::contributor(&deps))
    .build()?;
let _ = artha_tools::MonitorRepo::new(deps.db.clone()).reconcile(deps.scheduler.as_deref()).await; // best effort
jobs.register_handler("monitor", cli::adapters::MonitorHandler::new(MonitorEvaluator::new(&deps)));
```

### 2.2 `deps.rs`

```rust
#[derive(Clone)]
pub struct ToolDeps {
    pub db: data::Db,
    /// None = built without a finance client or construction failed; tools fail fast (HLD §8.2).
    pub finance: Option<Arc<dyn fin::FinanceClient>>,
    /// None = no jobs integration in this process; monitor_create fails, monitor_list still works.
    pub scheduler: Option<Arc<dyn MonitorScheduler>>,
    #[cfg(feature = "report")]
    pub report: Option<Arc<artha_report::Engine>>,
    pub config: ToolsConfig,
    /// Injected wall clock (epoch ms); `Clock::system()` by default, fixed in tests.
    pub clock: Clock,
}

#[derive(Clone)]
pub struct Clock(pub Arc<dyn Fn() -> data::Timestamp + Send + Sync>);
impl Clock { pub fn system() -> Self; pub fn fixed(t: data::Timestamp) -> Self; pub fn now(&self) -> data::Timestamp; }
impl Default for Clock { fn default() -> Self { Self::system() } }

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolsConfig {
    pub web: WebConfig,
    pub paper: PaperConfig,
    pub memory: MemoryConfig,
    /// Monitor minimum interval; floors user-supplied `every`.
    pub monitor_min_interval_secs: u64,           // 60
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub max_bytes: usize,        // 2 MiB
    pub max_chars: usize,        // 20_000 (text returned to the model)
    pub allow_private: bool,     // false; tests set true
    pub user_agent: String,      // "Artha/<ver> (+https://github.com/dev-pmallapp/artha)"
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PaperConfig {
    pub slippage_bps: u32,                       // 5
    #[serde(with = "rust_decimal::serde::str")]
    pub commission: Decimal,                     // 0
    pub default_base_ccy: String,                // "USD"
    #[serde(with = "rust_decimal::serde::str")]
    pub default_starting_cash: Decimal,          // 100000
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub recall: RecallMode,                      // Profile
    pub profile_budget_chars: usize,             // 2000
    pub relevant_k: usize,                       // 5 (ProfilePlusRelevant only)
    pub decay_after_days: Option<u32>,           // None = no importance decay in tick
    pub max_entries: u32,                        // 5000 (save refuses beyond; tells the model to forget)
}

impl ToolsConfig {
    /// Parses settings.extra["tools"] (artha-data keeps unknown keys); missing → defaults.
    pub fn from_extra(v: Option<&serde_json::Value>) -> Result<Self, ToolsError>;
}
```

### 2.3 `names.rs`

```rust
pub const MARKET_QUOTE: &str = "market_quote";           pub const MARKET_HISTORY: &str = "market_history";
pub const MARKET_FUNDAMENTALS: &str = "market_fundamentals"; pub const MARKET_INDICATORS: &str = "market_indicators";
pub const MARKET_SEARCH: &str = "market_search";
pub const MACRO_SERIES: &str = "macro_series";           pub const MACRO_CALENDAR: &str = "macro_calendar";
pub const NEWS_SEARCH: &str = "news_search";             pub const WEB_FETCH: &str = "web_fetch";
pub const RESEARCH_SCREEN: &str = "research_screen";     pub const RESEARCH_FILINGS: &str = "research_filings";
pub const RESEARCH_CALC: &str = "research_calc";         pub const REPORT_RENDER: &str = "report_render";
pub const PORTFOLIO_VIEW: &str = "portfolio_view";       pub const PORTFOLIO_MANAGE: &str = "portfolio_manage";
pub const PAPER_TRADE: &str = "paper_trade";
pub const WATCHLIST_SHOW: &str = "watchlist_show";       pub const WATCHLIST_EDIT: &str = "watchlist_edit";
pub const MONITOR_CREATE: &str = "monitor_create";       pub const MONITOR_LIST: &str = "monitor_list";
pub const MONITOR_MANAGE: &str = "monitor_manage";
pub const MEMORY_SAVE: &str = "memory_save";             pub const MEMORY_SEARCH: &str = "memory_search";
pub const MEMORY_FORGET: &str = "memory_forget";

/// ReadOnly built-ins, for artha-jobs least-privilege allow lists.
pub fn read_only() -> BTreeSet<String>;
pub fn all() -> BTreeSet<String>;
```

A unit test asserts that `read_only()` equals the set of registered tools
whose `traits().risk == ReadOnly`, so the list cannot drift.

## 3. Shared plumbing (`error.rs`, `common/`)

### 3.1 Errors

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ToolsError {
    /// Model-correctable: becomes ToolError::InvalidInput.
    #[error("{0}")]
    Invalid(String),
    /// A runtime dependency is absent in this build/session (finance, scheduler, report engine).
    #[error("{what} is not available: {hint}")]
    Unavailable { what: &'static str, hint: String },
    #[error(transparent)]
    Finance(#[from] fin::FinanceError),
    #[error(transparent)]
    Data(#[from] data::DataError),
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
    #[error("{0}")]
    Failed(String),                  // generic non-retryable
    #[error("cancelled")]
    Cancelled,
}

impl From<ToolsError> for core::ToolError {
    fn from(e: ToolsError) -> Self {
        use core::ToolError as T;
        match e {
            ToolsError::Invalid(m) => T::InvalidInput(m),
            ToolsError::Unavailable { what, hint } =>
                T::failed(format!("{what} is unavailable: {hint}. Tell the user; do not retry.")),
            ToolsError::Finance(f) => map_finance(&f),
            ToolsError::Data(d) => map_data(&d),
            ToolsError::Scheduler(s) => match s {
                SchedulerError::Busy => T::transient("The scheduler is busy; try again."),
                other => T::failed(format!("Could not schedule the monitor: {other}")),
            },
            ToolsError::Failed(m) => T::failed(m),
            ToolsError::Cancelled => T::Cancelled,
        }
    }
}

pub(crate) fn map_finance(e: &fin::FinanceError) -> core::ToolError {
    use fin::FinanceErrorKind as K;
    match e.kind() {
        K::NotFound          => core::ToolError::InvalidInput(format!("{e}. Use market_search to find the correct symbol or series id.")),
        K::InvalidRequest    => core::ToolError::InvalidInput(e.to_string()),
        K::Unsupported       => core::ToolError::failed(format!("{e}. This data source does not support that request.")),
        K::NotProvisioned    => core::ToolError::failed(format!(
                                    "Market data is unavailable: {e}. The user can run `artha doctor --fix`. Tell the user; do not retry.")),
        K::RateLimited { retry_after } => core::ToolError::transient(match retry_after {
                                    Some(d) => format!("Data source rate-limited; retry after ~{}s", d.as_secs()),
                                    None    => "Data source rate-limited".into() }),
        K::Timeout | K::Upstream => core::ToolError::transient(e.to_string()),
        K::Internal | _      => { tracing::error!(error = ?e, "finance internal error");
                                  core::ToolError::failed("The data source returned an unreadable response.") }
    }
}

pub(crate) fn map_data(e: &data::DataError) -> core::ToolError {
    if e.is_busy() { core::ToolError::transient("The local database is busy (another Artha process is writing); try again.") }
    else { tracing::error!(error = %e, "local storage error"); core::ToolError::failed(format!("Local storage error: {e}")) }
}
```

Every `TypedTool::run` body ends with `.map_err(Into::into)`, and the
`execute` path never uses `?` on a `ToolError` directly.

### 3.2 Helpers

```rust
// common/traits.rs: ToolTraits is #[non_exhaustive] (HLD §11 item 2), so mutate a default.
pub(crate) fn traits(risk: core::ToolRisk, concurrent: bool, timeout_s: u64, confirm: core::ConfirmOverride) -> core::ToolTraits {
    let mut t = core::ToolTraits::default();
    t.risk = risk; t.concurrency_safe = concurrent; t.timeout = Some(Duration::from_secs(timeout_s)); t.confirm = confirm;
    t
}

// common/db.rs
/// Read-only query off the reactor without taking the RESERVED lock (Db::call uses BEGIN IMMEDIATE).
pub(crate) async fn db_read<R: Send + 'static>(
    db: &data::Db, f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<R, ToolsError> + Send + 'static,
) -> Result<R, ToolsError> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || db.read(f))
        .await.map_err(|j| ToolsError::Data(data::DataError::TaskJoin(j.to_string())))?
}
pub(crate) async fn db_write<R: Send + 'static>(
    db: &data::Db, f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<R, ToolsError> + Send + 'static,
) -> Result<R, ToolsError> { db.call(f).await }

// common/finance.rs
pub(crate) fn require_finance(deps_fin: &Option<Arc<dyn fin::FinanceClient>>) -> Result<&Arc<dyn fin::FinanceClient>, ToolsError> {
    let f = deps_fin.as_ref().ok_or(ToolsError::Unavailable {
        what: "Market data", hint: "this build has no finance data source configured".into() })?;
    match f.availability() {
        fin::Availability::Ready | fin::Availability::Degraded { .. } => Ok(f),
        fin::Availability::NotProvisioned { hint } => Err(ToolsError::Unavailable { what: "Market data", hint }),
    }
}

// common/symbol.rs
/// Uppercases and trims, then validates: 1..=20 chars of [A-Z0-9.\-^=:/]; must contain a letter or '^'.
/// Accepts AAPL, BRK.B, ^GSPC, EURUSD=X, BTC-USD, NSE:RELIANCE. Dedupes and keeps order. Max 50 per call.
pub(crate) fn parse_symbols(raw: &[String]) -> Result<Vec<fin::Symbol>, ToolsError>;
pub(crate) fn parse_symbol(raw: &str) -> Result<fin::Symbol, ToolsError>;

// common/time.rs
/// "1d"|"5d"|"1mo"|"3mo"|"6mo"|"ytd"|"1y"|"5y"|"max" or "YYYY-MM-DD..YYYY-MM-DD" (either side optional).
pub(crate) fn parse_range(s: &str, today: jiff::civil::Date) -> Result<DateRange, ToolsError>;
pub struct DateRange { pub start: Option<jiff::civil::Date>, pub end: jiff::civil::Date }
pub(crate) fn ts_to_rfc3339(t: data::Timestamp) -> String;

// common/output.rs
pub(crate) fn json_output<T: Serialize>(v: &T) -> core::ToolOutput;   // ToolOutput::json(serde_json::to_value(v))
pub(crate) fn table(columns: &[&str], rows: Vec<Vec<String>>) -> core::ToolDisplay;
/// Bars over `max` are downsampled to the coarsest interval that fits (1d→1wk→1mo), and the choice is reported.
pub(crate) fn downsample_plan(n_bars: usize, interval: fin::Interval, max: usize) -> (fin::Interval, bool);
```

## 4. The `artha-finance` contract consumed here

These are the types and fields this crate reads. Each name is the one
`artha-finance` is expected to export. Anything still open is resolved
in that crate's LLD.

```rust
// expected in artha_finance
pub struct Symbol(String);                                   // normalized ticker; Display; as_str()
pub struct Currency(String);                                 // ISO 4217 or crypto code, uppercase
pub enum Availability { Ready, Degraded { reason: String }, NotProvisioned { hint: String } }

pub struct Quote {
    pub symbol: Symbol, pub name: Option<String>, pub currency: Currency,
    pub price: Decimal, pub change: Option<Decimal>, pub change_pct: Option<Decimal>,
    pub open: Option<Decimal>, pub high: Option<Decimal>, pub low: Option<Decimal>, pub prev_close: Option<Decimal>,
    pub volume: Option<u64>, pub avg_volume: Option<u64>, pub market_cap: Option<Decimal>,
    pub market_state: MarketState,          // Pre | Regular | Post | Closed | Unknown
    pub as_of: data::Timestamp, pub source: String,
}
pub enum Interval { M1, M5, M15, H1, D1, W1, Mo1 }
pub struct HistoryRequest { pub symbol: Symbol, pub start: Option<jiff::civil::Date>, pub end: jiff::civil::Date,
                            pub interval: Interval, pub adjusted: bool }
pub struct Bar { pub t: data::Timestamp, pub open: Decimal, pub high: Decimal, pub low: Decimal, pub close: Decimal, pub volume: Option<u64> }
pub struct PriceSeries { pub symbol: Symbol, pub currency: Currency, pub interval: Interval, pub bars: Vec<Bar>, pub source: String }

pub enum FundamentalsSection { Profile, KeyStats, Income, Balance, Cashflow, Earnings, Dividends }
pub struct FundamentalsRequest { pub symbol: Symbol, pub sections: Vec<FundamentalsSection>, pub period: Period /* Annual|Quarterly */, pub limit: u32 }
pub struct Fundamentals { pub symbol: Symbol, pub currency: Currency, pub sections: BTreeMap<String, serde_json::Value>, pub as_of: data::Timestamp, pub source: String }

pub enum Indicator { Sma { period: u32 }, Ema { period: u32 }, Rsi { period: u32 }, Macd { fast: u32, slow: u32, signal: u32 },
                     Bollinger { period: u32, k: Decimal }, Atr { period: u32 } }
pub struct IndicatorRequest { pub symbol: Symbol, pub indicators: Vec<Indicator>, pub interval: Interval, pub lookback: u32 }
pub struct IndicatorSeries { pub symbol: Symbol, pub columns: Vec<String>, pub rows: Vec<(data::Timestamp, Vec<Option<Decimal>>)>, pub source: String }

pub struct SymbolMatch { pub symbol: Symbol, pub name: String, pub exchange: Option<String>, pub kind: String /* equity|etf|index|fx|crypto|fund */ }
pub struct MacroRequest { pub series_id: String, pub start: Option<jiff::civil::Date>, pub end: jiff::civil::Date }
pub struct MacroSeries { pub id: String, pub title: String, pub units: String, pub frequency: String,
                         pub points: Vec<(jiff::civil::Date, Option<Decimal>)>, pub source: String }
pub struct CalendarRequest { pub from: jiff::civil::Date, pub to: jiff::civil::Date, pub countries: Vec<String>, pub min_importance: u8 }
pub struct CalendarEvent { pub at: data::Timestamp, pub country: String, pub event: String, pub importance: u8,
                           pub actual: Option<String>, pub forecast: Option<String>, pub previous: Option<String> }
pub struct NewsRequest { pub query: Option<String>, pub symbols: Vec<Symbol>, pub since: Option<data::Timestamp>, pub limit: u32 }
pub struct NewsItem { pub id: String /* stable, for dedupe */, pub title: String, pub url: String, pub publisher: String,
                      pub published_at: data::Timestamp, pub summary: Option<String>, pub symbols: Vec<Symbol> }
pub struct ScreenRequest { pub filters: Vec<ScreenFilter>, pub sort: Option<(String, bool)>, pub limit: u32 }
pub struct ScreenFilter { pub field: String, pub op: CmpOp, pub value: serde_json::Value }
pub struct ScreenRow { pub symbol: Symbol, pub fields: BTreeMap<String, serde_json::Value> }
pub struct FilingsRequest { pub symbol: Symbol, pub forms: Vec<String>, pub limit: u32 }
pub struct Filing { pub form: String, pub filed_at: jiff::civil::Date, pub title: String, pub url: String }
pub struct FxRate { pub from: Currency, pub to: Currency, pub rate: Decimal, pub as_of: data::Timestamp }

#[derive(Debug, thiserror::Error)] pub struct FinanceError { /* kind + message + source */ }
impl FinanceError { pub fn kind(&self) -> FinanceErrorKind; }
#[non_exhaustive]
pub enum FinanceErrorKind { NotProvisioned, NotFound, InvalidRequest, Unsupported,
                            RateLimited { retry_after: Option<Duration> }, Timeout, Upstream, Internal }

#[async_trait] pub trait FinanceClient: Send + Sync + 'static { /* methods as in HLD §5 */ }
```

The `FinanceClient` futures must be cancel-safe, because dropping one
must abort the sidecar request or the HTTP call. They must also enforce
their own internal deadlines below our 45 s tool timeout, so that a
`Timeout` kind can be reported as retryable instead of core's generic
`TimedOut`.

## 5. SQLite schema (DDL)

All of the conventions below follow artha-data LLD §5.5:
- every table is `STRICT`
- timestamps are epoch-ms `INTEGER`
- JSON is `TEXT CHECK (json_valid(...))`
- decimals are canonical decimal `TEXT`
- every object is prefixed with its namespace
- foreign keys point only within the namespace or into `core_*`

### 5.1 `memory` namespace

```sql
-- migrations/memory/0001_memory.sql
CREATE TABLE memory_entry (
    id             INTEGER PRIMARY KEY,
    kind           TEXT    NOT NULL CHECK (kind IN ('fact', 'preference', 'instruction')),
    content        TEXT    NOT NULL CHECK (length(content) BETWEEN 1 AND 2000),
    content_hash   TEXT    NOT NULL UNIQUE,                        -- hex sha256(normalize(content)), kind-independent
    tags           TEXT    NOT NULL DEFAULT '[]'
                           CHECK (json_valid(tags) AND json_type(tags) = 'array'),
    importance     INTEGER NOT NULL DEFAULT 1 CHECK (importance BETWEEN 0 AND 3),
    pinned         INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    origin         TEXT    NOT NULL DEFAULT 'model' CHECK (origin IN ('model', 'user', 'import')),
    source_session TEXT    REFERENCES core_session(id) ON DELETE SET NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    last_used_at   INTEGER,
    use_count      INTEGER NOT NULL DEFAULT 0 CHECK (use_count >= 0),
    expires_at     INTEGER                                         -- NULL = never
) STRICT;
CREATE INDEX memory_entry_profile ON memory_entry (pinned DESC, kind, importance DESC, id);
CREATE INDEX memory_entry_expiry  ON memory_entry (expires_at) WHERE expires_at IS NOT NULL;

CREATE VIRTUAL TABLE memory_entry_fts USING fts5(
    content, tags,
    content = 'memory_entry', content_rowid = 'id',
    tokenize = 'porter unicode61 remove_diacritics 2'
);

CREATE TRIGGER memory_entry_ai AFTER INSERT ON memory_entry BEGIN
    INSERT INTO memory_entry_fts (rowid, content, tags) VALUES (new.id, new.content, new.tags);
END;
CREATE TRIGGER memory_entry_ad AFTER DELETE ON memory_entry BEGIN
    INSERT INTO memory_entry_fts (memory_entry_fts, rowid, content, tags) VALUES ('delete', old.id, old.content, old.tags);
END;
-- Only content/tags changes touch the index; usage bumps (last_used_at, use_count) don't.
CREATE TRIGGER memory_entry_au AFTER UPDATE OF content, tags ON memory_entry BEGIN
    INSERT INTO memory_entry_fts (memory_entry_fts, rowid, content, tags) VALUES ('delete', old.id, old.content, old.tags);
    INSERT INTO memory_entry_fts (rowid, content, tags) VALUES (new.id, new.content, new.tags);
END;
```

FTS5 creates shadow tables (`memory_entry_fts_data`, `_idx`, `_docsize`,
`_config`). All of them carry the `memory_` prefix, so
`assert_objects_prefixed` passes. The watermarks and settings that are
not per-entry go in `core_kv` through `KvStore::new(db, "memory")`:
- `last_tick` (`Timestamp`)
- `embedding_model` (`String`)
- `stats_cache` (none in v1)

This avoids a one-row table.

**Fallback if trigger-based FTS fails under `trusted_schema=OFF`** (HLD
§11 item 4, which `tests/schema.rs` checks). If the triggers fail, they
are replaced *before 0001 ships*: the three triggers are dropped from the
migration, and `MemoryRepo` issues the same three FTS statements inside
the same `Db::write` transaction as the row change. That fallback has to
be chosen before release, because released migrations are checksummed.

### 5.2 `portfolio` namespace

```sql
-- migrations/portfolio/0001_portfolio.sql
CREATE TABLE portfolio_account (
    id          TEXT    PRIMARY KEY NOT NULL,                       -- UUIDv7
    name        TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    kind        TEXT    NOT NULL CHECK (kind IN ('paper', 'tracked')),
    base_ccy    TEXT    NOT NULL DEFAULT 'USD' CHECK (length(base_ccy) BETWEEN 3 AND 10),
    is_default  INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    archived    INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX portfolio_account_one_default ON portfolio_account (is_default) WHERE is_default = 1;

-- Append-only. Positions and cash are derived (ledger.rs). Corrections = compensating entries.
CREATE TABLE portfolio_ledger (
    id            INTEGER PRIMARY KEY,
    account_id    TEXT    NOT NULL REFERENCES portfolio_account(id) ON DELETE CASCADE,
    ts            INTEGER NOT NULL,
    entry_type    TEXT    NOT NULL CHECK (entry_type IN
                     ('buy', 'sell', 'deposit', 'withdraw', 'dividend', 'fee', 'split', 'adjust')),
    symbol        TEXT,                                  -- NULL for pure cash entries
    quantity      TEXT,                                  -- decimal; > 0 for buy/sell; ratio for split; signed for adjust
    price         TEXT,                                  -- decimal, in trade ccy (buy/sell/adjust cost basis)
    amount        TEXT    NOT NULL,                      -- signed cash delta in account base_ccy, decimal
    ccy           TEXT    NOT NULL,                      -- trade/quote currency
    fx_rate       TEXT,                                  -- ccy→base_ccy used; NULL when ccy = base_ccy
    fee           TEXT,                                  -- decimal, base_ccy, already included in amount
    quote_as_of   INTEGER,                               -- paper fills: timestamp of the quote used
    market_state  TEXT,                                  -- paper fills: 'regular'|'pre'|'post'|'closed'|'unknown'
    note          TEXT,
    session_id    TEXT    REFERENCES core_session(id) ON DELETE SET NULL,
    tool_call_id  TEXT,                                  -- idempotency key (model's tool_use id)
    CHECK (entry_type NOT IN ('buy', 'sell', 'split', 'adjust') OR (symbol IS NOT NULL AND quantity IS NOT NULL)),
    CHECK (entry_type NOT IN ('buy', 'sell') OR price IS NOT NULL),
    UNIQUE (account_id, tool_call_id)                    -- NULLs are distinct, so manual rows are unaffected
) STRICT;
CREATE INDEX portfolio_ledger_by_account ON portfolio_ledger (account_id, ts, id);
CREATE INDEX portfolio_ledger_by_symbol  ON portfolio_ledger (account_id, symbol) WHERE symbol IS NOT NULL;

CREATE TABLE portfolio_watchlist (
    id          TEXT    PRIMARY KEY NOT NULL,
    name        TEXT    NOT NULL UNIQUE COLLATE NOCASE,   -- "default" created lazily by the repo
    created_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE portfolio_watchlist_item (
    watchlist_id TEXT    NOT NULL REFERENCES portfolio_watchlist(id) ON DELETE CASCADE,
    symbol       TEXT    NOT NULL,
    note         TEXT,
    added_at     INTEGER NOT NULL,
    PRIMARY KEY (watchlist_id, symbol)
) STRICT, WITHOUT ROWID;
```

### 5.3 `monitor` namespace

```sql
-- migrations/monitor/0001_monitor.sql
CREATE TABLE monitor_definition (
    id             TEXT    PRIMARY KEY NOT NULL,                     -- UUIDv7
    name           TEXT    NOT NULL,
    condition      TEXT    NOT NULL CHECK (json_valid(condition)),   -- MonitorCondition (serde, tag "type")
    schedule       TEXT    NOT NULL CHECK (json_valid(schedule)),    -- Schedule
    action         TEXT    NOT NULL CHECK (json_valid(action)),      -- MonitorAction
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    status         TEXT    NOT NULL DEFAULT 'pending'
                           CHECK (status IN ('pending', 'scheduled', 'error', 'finished')),
    status_detail  TEXT,
    cooldown_ms    INTEGER NOT NULL DEFAULT 0 CHECK (cooldown_ms >= 0),
    max_fires      INTEGER CHECK (max_fires IS NULL OR max_fires >= 1),
    fire_count     INTEGER NOT NULL DEFAULT 0,
    expires_at     INTEGER,
    job_id         TEXT,                                             -- jobs_job id; no FK (sibling namespace)
    source_session TEXT    REFERENCES core_session(id) ON DELETE SET NULL,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
) STRICT;
CREATE INDEX monitor_definition_pending ON monitor_definition (status) WHERE status IN ('pending', 'error');
CREATE UNIQUE INDEX monitor_definition_job ON monitor_definition (job_id) WHERE job_id IS NOT NULL;

-- Edge-trigger and dedupe state; one row per monitor, written only by the evaluator.
CREATE TABLE monitor_state (
    monitor_id          TEXT    PRIMARY KEY NOT NULL REFERENCES monitor_definition(id) ON DELETE CASCADE,
    armed               INTEGER NOT NULL DEFAULT 1 CHECK (armed IN (0, 1)),  -- 1 = may fire on next satisfied
    last_checked_at     INTEGER,
    last_observed       TEXT    CHECK (last_observed IS NULL OR json_valid(last_observed)),
    last_fired_at       INTEGER,
    consecutive_errors  INTEGER NOT NULL DEFAULT 0,
    last_error          TEXT,
    seen                TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(seen))  -- news ids, capped at 500
) STRICT, WITHOUT ROWID;

CREATE TABLE monitor_event (
    id              INTEGER PRIMARY KEY,
    monitor_id      TEXT    NOT NULL REFERENCES monitor_definition(id) ON DELETE CASCADE,
    fired_at        INTEGER NOT NULL,
    observed        TEXT    NOT NULL CHECK (json_valid(observed)),
    message         TEXT    NOT NULL,                     -- human/model-readable one-liner
    dispatch_status TEXT    NOT NULL DEFAULT 'pending' CHECK (dispatch_status IN ('pending', 'delivered', 'failed')),
    dispatch_detail TEXT,
    acknowledged    INTEGER NOT NULL DEFAULT 0 CHECK (acknowledged IN (0, 1))
) STRICT;
CREATE INDEX monitor_event_by_monitor ON monitor_event (monitor_id, fired_at DESC);
```

### 5.4 Migration sets

```rust
// memory/mod.rs
pub static MEMORY_MIGRATIONS: data::MigrationSet = data::MigrationSet::new("memory", &[
    data::Migration { version: 1, name: "memory", sql: include_str!("../../migrations/memory/0001_memory.sql") },
]);
// portfolio/mod.rs
pub static PORTFOLIO_MIGRATIONS: data::MigrationSet = data::MigrationSet::new("portfolio", &[
    data::Migration { version: 1, name: "portfolio", sql: include_str!("../../migrations/portfolio/0001_portfolio.sql") },
]);
// automation/mod.rs
pub static MONITOR_MIGRATIONS: data::MigrationSet = data::MigrationSet::new("monitor", &[
    data::Migration { version: 1, name: "monitor", sql: include_str!("../../migrations/monitor/0001_monitor.sql") },
]);
```

## 6. Market, macro, news, and research tools

All of them share one pattern. The `deps` field is an
`Option<Arc<dyn FinanceClient>>` clone, and each `run` starts with
`require_finance(&self.finance)?`.

### 6.1 Markets

```rust
pub struct MarketQuote { finance: Option<Arc<dyn fin::FinanceClient>> }

/// Latest quotes for one or more symbols.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuoteInput {
    /// Tickers, e.g. ["AAPL", "MSFT", "^GSPC", "EURUSD=X"]. 1–50.
    pub symbols: Vec<String>,
}

#[async_trait]
impl core::TypedTool for MarketQuote {
    type Input = QuoteInput;
    const NAME: &'static str = names::MARKET_QUOTE;
    const DESCRIPTION: &'static str = "Get the latest price, daily change, volume and market state for up to 50 symbols. \
        Data may be delayed; check `as_of`.";
    fn traits(&self) -> core::ToolTraits { traits(ReadOnly, true, 45, ConfirmOverride::Default) }
    fn plan(&self, i: &QuoteInput, _: &core::PlanContext<'_>) -> core::ToolPlan {
        core::ToolPlan { summary: Some(format!("Quote {}", i.symbols.join(", "))), ..Default::default() }
    }
    async fn run(&self, i: QuoteInput, ctx: core::ToolContext) -> Result<core::ToolOutput, core::ToolError> {
        async {
            let syms = parse_symbols(&i.symbols)?;
            let fin = require_finance(&self.finance)?;
            let quotes = fin.quotes(&syms).await?;
            Ok(render::quotes(&syms, &quotes))      // missing symbols listed under "not_found", not an error
        }.await.map_err(Into::into)
    }
}
```

`TypedTool::plan` returns `ToolPlan`, not a `Result`, so syntactic
validation that must *reject* a call happens at the top of `run`. It
still runs before any I/O. Unknown fields are rejected by serde at the
`Typed` layer.

The output shape for `market_quote` is shown below. Decimals are
strings, and `ToolDisplay::Table` renders the same rows.

```json
{"quotes":[{"symbol":"AAPL","name":"Apple Inc.","price":"187.32","change_pct":"-0.84","currency":"USD",
            "volume":51234000,"market_state":"closed","as_of":"2026-11-03T21:00:00Z"}],
 "not_found":[],"source":"…"}
```

The other markets tools follow the same skeleton. Only their inputs and
caps differ:

```rust
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct HistoryInput {
    pub symbol: String,
    /// "1mo","6mo","ytd","1y","5y","max" or "YYYY-MM-DD..YYYY-MM-DD". Default "6mo".
    #[serde(default)] pub range: Option<String>,
    /// "1d" (default), "1wk", "1mo", or intraday "5m","15m","1h" (ranges ≤ 30 days).
    #[serde(default)] pub interval: Option<String>,
    /// Max bars returned (default 250, max 500); longer ranges are downsampled.
    #[serde(default)] pub max_bars: Option<u32>,
}
// output: {symbol, currency, interval, downsampled: Option<"1wk">, bars:[{t,o,h,l,c,v}], summary:{first,last,high,low,change_pct}}

#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct FundamentalsInput {
    pub symbol: String,
    /// Subset of profile|key_stats|income|balance|cashflow|earnings|dividends. Default [profile, key_stats].
    #[serde(default)] pub sections: Vec<FundamentalsSectionArg>,
    #[serde(default)] pub period: Option<PeriodArg>,       // annual (default) | quarterly
    /// Periods per statement (default 4, max 12).
    #[serde(default)] pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct IndicatorsInput {
    pub symbol: String,
    /// e.g. [{"kind":"sma","period":50},{"kind":"rsi","period":14}]. Max 6.
    pub indicators: Vec<IndicatorArg>,
    #[serde(default)] pub interval: Option<String>,        // default "1d"
    /// Most recent N rows (default 30, max 250).
    #[serde(default)] pub lookback: Option<u32>,
}
#[derive(Deserialize, JsonSchema)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndicatorArg { Sma { period: u32 }, Ema { period: u32 }, Rsi { period: u32 },
                        Macd { fast: Option<u32>, slow: Option<u32>, signal: Option<u32> },
                        Bollinger { period: Option<u32>, k: Option<String> }, Atr { period: u32 } }

#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct SearchInput { pub query: String, #[serde(default)] pub limit: Option<u32> /* 10, max 25 */ }
```

### 6.2 Economy

```rust
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct MacroSeriesInput {
    /// Series id, e.g. "CPIAUCSL", "UNRATE", "DGS10", "GDPC1".
    pub series_id: String,
    #[serde(default)] pub range: Option<String>,           // default "5y"
    /// Optional transform applied locally: none | yoy_pct | mom_pct | diff.
    #[serde(default)] pub transform: Option<Transform>,
    #[serde(default)] pub max_points: Option<u32>,         // default 120, max 500
}
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct CalendarInput {
    /// Default: today..today+7d.
    #[serde(default)] pub range: Option<String>,
    #[serde(default)] pub countries: Vec<String>,          // ISO-3166 alpha-2; default ["US"]
    #[serde(default)] pub min_importance: Option<u8>,      // 1..=3, default 2
}
pub struct MacroSeriesTool   { finance: Option<Arc<dyn fin::FinanceClient>> }   // NAME = "macro_series"
pub struct MacroCalendarTool { finance: Option<Arc<dyn fin::FinanceClient>> }   // NAME = "macro_calendar"
```

The transforms are pure functions in `economy/series.rs`
(`fn apply(points, Transform) -> Vec<(Date, Option<Decimal>)>`). They
live here, not in finance, because they are presentation choices.

### 6.3 News and web

```rust
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct NewsInput {
    #[serde(default)] pub query: Option<String>,
    #[serde(default)] pub symbols: Vec<String>,            // at least one of query/symbols required (checked in run)
    /// "1d","7d" (default),"30d".
    #[serde(default)] pub since: Option<String>,
    #[serde(default)] pub limit: Option<u32>,              // default 10, max 30
}
pub struct NewsSearch { finance: Option<Arc<dyn fin::FinanceClient>> }

#[cfg(feature = "web")]
pub struct WebFetch { http: reqwest::Client, cfg: WebConfig }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct WebFetchInput {
    /// Absolute http(s) URL.
    pub url: String,
    /// Characters of readable text to return (default 8000, max cfg.max_chars).
    #[serde(default)] pub max_chars: Option<u32>,
    /// Skip this many characters first (for paging long pages).
    #[serde(default)] pub offset: Option<u32>,
}
```

`WebFetch::run` takes these steps:
1. Parse the URL with `url::Url`. Only `http` and `https` are accepted,
   and anything else is `Invalid`.
2. Run `net_guard::resolve_and_check(host, port, cfg.allow_private)`,
   which calls `tokio::net::lookup_host` and rejects loopback,
   unspecified, link-local, RFC 1918, CGNAT 100.64/10, ULA fc00::/7, and
   169.254.169.254. The `reqwest::Client` is built with a custom resolver
   that pins the checked addresses, so that DNS rebinding between the
   check and the connect is impossible. Redirects go through
   `redirect::Policy::custom`, which re-runs the check on each hop
   (at most 5).
3. Run the request under `select!` with `ctx.cancel`. The body is read
   as a stream until `cfg.max_bytes`, then truncated with
   `truncated: true`.
4. Branch on content type:
   - HTML is converted with `html2text::from_read(width = 100)` on
     `spawn_blocking` if the body is over 256 KiB.
   - JSON and text pass through as they are.
   - Anything else is a `Failed` error: "unsupported content type".
5. Return
   `{url, final_url, status, title, text, offset, total_chars, truncated}`.
   A non-2xx status is still `Ok` with `status`, because the model can
   read a 404 page. A connection or TLS error is `Failed{retryable:true}`.

### 6.4 Research

```rust
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct ScreenInput {
    /// e.g. [{"field":"market_cap","op":"gt","value":10000000000},{"field":"sector","op":"eq","value":"Technology"}]
    pub filters: Vec<ScreenFilterArg>,
    #[serde(default)] pub sort_by: Option<String>,
    #[serde(default)] pub descending: Option<bool>,
    #[serde(default)] pub limit: Option<u32>,          // default 25, max 100
}
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct FilingsInput { pub symbol: String, #[serde(default)] pub forms: Vec<String>, #[serde(default)] pub limit: Option<u32> }

/// Deterministic financial math, no data source needed.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalcOp {
    Cagr        { start_value: String, end_value: String, years: String },
    PctChange   { from: String, to: String },
    Compound    { principal: String, rate_pct: String, years: String, periods_per_year: Option<u32>, contribution: Option<String> },
    Npv         { rate_pct: String, cashflows: Vec<String> },        // cashflows[0] at t=0
    Irr         { cashflows: Vec<String> },                          // Newton + bisection fallback, 1e-10, 200 iters
    Dcf         { fcf: String, growth_pct: String, years: u32, terminal_growth_pct: String, discount_pct: String,
                  net_debt: Option<String>, shares: Option<String> },
    PositionSize{ account_value: String, risk_pct: String, entry: String, stop: String },
}
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct CalcInput { #[serde(flatten)] pub op: CalcOp }    // root stays "type":"object" (HLD §4)

pub mod calc {   // pure; property-tested
    pub fn cagr(start: Decimal, end: Decimal, years: Decimal) -> Result<Decimal, CalcError>;
    pub fn npv(rate: Decimal, cfs: &[Decimal]) -> Decimal;
    pub fn irr(cfs: &[Decimal]) -> Result<Decimal, CalcError>;   // NoSignChange | NoConvergence
    pub fn dcf(p: &DcfParams) -> DcfResult;                      // per-year PV table + terminal + EV + per-share
    pub fn compound(p: &CompoundParams) -> Decimal;
    pub fn position_size(p: &PositionParams) -> Result<PositionResult, CalcError>;
}
```

Numeric inputs are strings, parsed with `Decimal::from_str`, so that
`0.1` stays exact. `Decimal` has no `powf`, so non-integer powers in
`cagr` and `irr` go through `f64`. The result is rounded to 8 dp and
flagged `"approx": true`. The `CalcOp` schema with a `flatten`ed tagged
enum is checked by the spec snapshot test, which confirms that schemars
emits a root object with `oneOf` under it.

```rust
#[cfg(feature = "report")]
pub struct ReportRender { engine: Option<Arc<artha_report::Engine>>, finance: Option<Arc<dyn fin::FinanceClient>>, portfolio: Arc<PortfolioRepo> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct ReportInput {
    pub title: String,
    /// Markdown body; may reference {{chart:SYMBOL:RANGE}} and {{portfolio:ACCOUNT}} blocks.
    pub body_markdown: String,
    /// Workdir-relative output path; default "artha-reports/<slug>-<YYYY-MM-DD>.pdf". Must end in .pdf.
    #[serde(default)] pub output: Option<String>,
    #[serde(default)] pub overwrite: bool,
}
impl core::TypedTool for ReportRender {
    // traits: FileWrite, serial, 120 s, Default
    fn plan(&self, i: &ReportInput, ctx: &core::PlanContext<'_>) -> core::ToolPlan {
        let path = resolve_output(ctx.workdir, i);       // pure; rejects nothing here (run re-validates)
        core::ToolPlan { summary: Some(format!("Render report \"{}\" to {}", i.title, path.display())),
                         writes: vec![path], preview: None }
    }
    // run:
    //  1. re-resolve the path and require it to be inside ctx.workdir with an .pdf extension,
    //     and !exists || overwrite (otherwise Invalid);
    //  2. gather data blocks (finance/portfolio) and render the PDF bytes on spawn_blocking;
    //  3. write to <path>.tmp, then rename.
    // Core already took the PreEdit snapshot from plan.writes; no ctx.edits() call.
}
```

## 7. Portfolio, watchlists, paper ledger

### 7.1 Model

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(transparent)]
pub struct AccountId(pub uuid::Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)] #[serde(rename_all = "snake_case")]
pub enum AccountKind { Paper, Tracked }

pub struct Account { pub id: AccountId, pub name: String, pub kind: AccountKind, pub base_ccy: fin::Currency,
                     pub is_default: bool, pub archived: bool, pub created_at: data::Timestamp }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType { Buy, Sell, Deposit, Withdraw, Dividend, Fee, Split, Adjust }   // as_str() matches the SQL CHECK

pub struct LedgerEntry {
    pub id: i64, pub account_id: AccountId, pub ts: data::Timestamp, pub entry_type: EntryType,
    pub symbol: Option<fin::Symbol>, pub quantity: Option<Decimal>, pub price: Option<Decimal>,
    pub amount: Decimal, pub ccy: fin::Currency, pub fx_rate: Option<Decimal>, pub fee: Option<Decimal>,
    pub quote_as_of: Option<data::Timestamp>, pub market_state: Option<String>,
    pub note: Option<String>, pub session_id: Option<data::SessionId>, pub tool_call_id: Option<String>,
}
pub struct NewLedgerEntry { /* same minus id */ }

pub struct Position {
    pub symbol: fin::Symbol, pub quantity: Decimal, pub ccy: fin::Currency,
    pub avg_cost: Decimal,            // in trade ccy, per unit, average-cost method
    pub cost_basis_base: Decimal,     // in account base ccy, at historical fx
    pub realized_pnl_base: Decimal,
}
pub struct Holdings { pub positions: Vec<Position>, pub cash_base: Decimal, pub realized_pnl_base: Decimal,
                      pub deposits_base: Decimal, pub dividends_base: Decimal, pub fees_base: Decimal }

pub struct Watchlist { pub id: String, pub name: String, pub items: Vec<WatchItem> }
pub struct WatchItem { pub symbol: fin::Symbol, pub note: Option<String>, pub added_at: data::Timestamp }
```

### 7.2 Pure ledger logic (`ledger.rs`, `fill.rs`)

```rust
/// Folds entries in (ts, id) order. Average cost: a buy updates avg_cost; a sell realizes
/// (price - avg_cost) * qty (converted at the sell's fx_rate) and leaves avg_cost unchanged; a split multiplies
/// qty and divides avg_cost by the ratio; adjust sets qty += q at the given price (tracked accounts).
/// Positions with |qty| < 1e-12 are dropped. Shorts are not representable in v1 (check_trade prevents them).
pub fn derive_holdings(entries: &[LedgerEntry]) -> Holdings;

pub enum TradeSide { Buy, Sell }
pub struct TradeCheck { pub ok: bool, pub reason: Option<String>, pub cash_after: Decimal, pub qty_after: Decimal }
/// Paper accounts: buy needs cash_base >= cost incl. fee (no margin); sell needs qty <= held.
/// Tracked accounts: only the sell-quantity rule applies.
pub fn check_trade(h: &Holdings, kind: AccountKind, side: TradeSide, symbol: &fin::Symbol,
                   qty: Decimal, est_cost_base: Decimal) -> TradeCheck;

pub struct Fill { pub price: Decimal, pub qty: Decimal, pub gross: Decimal, pub fee: Decimal,
                  pub amount_base: Decimal /* signed */, pub fx_rate: Option<Decimal> }
/// price = quote.price * (1 ± slippage_bps/10_000) (buy up, sell down), rounded to 4 dp;
/// gross = price*qty; fee = commission; amount_base = ∓(gross*fx) − fee.
pub fn simulate_market_fill(side: TradeSide, qty: Decimal, quote: &fin::Quote, fx: Option<Decimal>, cfg: &PaperConfig) -> Fill;
/// notional → qty: floor((notional − fee)/price, 6 dp); fractional shares are allowed in paper accounts.
pub fn qty_for_notional(notional: Decimal, price: Decimal, fee: Decimal) -> Decimal;
```

### 7.3 Repository

```rust
pub struct PortfolioRepo { db: data::Db }
impl PortfolioRepo {
    pub fn new(db: data::Db) -> Self;
    // accounts (all sync, called inside db_read/db_write closures via the `_tx` variants)
    pub fn create_account_tx(tx: &Transaction, name: &str, kind: AccountKind, base_ccy: &str, make_default: bool, now: Timestamp) -> Result<Account, ToolsError>;
    pub fn resolve_account_tx(tx: &Transaction, name_or_id: Option<&str>) -> Result<Account, ToolsError>;
        // None → default; no accounts at all → Invalid("No portfolio yet. Create one with portfolio_manage {action:create_account}.")
    pub fn list_accounts_tx(tx: &Transaction, include_archived: bool) -> Result<Vec<Account>, ToolsError>;
    // ledger
    pub fn entries_tx(tx: &Transaction, account: &AccountId) -> Result<Vec<LedgerEntry>, ToolsError>;
    /// INSERT; maps UNIQUE(account_id, tool_call_id) violation → Ok(Inserted::Duplicate(existing_id)).
    pub fn append_tx(tx: &Transaction, e: &NewLedgerEntry) -> Result<Inserted, ToolsError>;
    pub fn recent_entries_tx(tx: &Transaction, account: &AccountId, limit: u32) -> Result<Vec<LedgerEntry>, ToolsError>;
    // watchlists
    pub fn watchlist_tx(tx: &Transaction, name: &str, create: bool, now: Timestamp) -> Result<Option<Watchlist>, ToolsError>;
    pub fn watch_add_tx(tx: &Transaction, wl: &str, syms: &[fin::Symbol], note: Option<&str>, now: Timestamp) -> Result<Vec<fin::Symbol> /* newly added */, ToolsError>;
    pub fn watch_remove_tx(tx: &Transaction, wl: &str, syms: &[fin::Symbol]) -> Result<Vec<fin::Symbol> /* removed */, ToolsError>;
    pub fn db(&self) -> &data::Db;
}
pub enum Inserted { New(i64), Duplicate(i64) }
```

The `_tx` style lets one tool do read-check-write in **one** IMMEDIATE
transaction, for example "derive holdings, check cash, append fill". The
quote is fetched *before* the transaction opens, because network I/O
inside a `Db` closure is forbidden (artha-data LLD §5.2).

### 7.4 Valuation (`valuation.rs`)

```rust
pub struct Valuation {
    pub account: String, pub base_ccy: String, pub status: ValuationStatus,   // Live | Partial | Unavailable{reason}
    pub positions: Vec<PositionValue>, pub cash: Decimal, pub market_value: Option<Decimal>,
    pub cost_basis: Decimal, pub unrealized_pnl: Option<Decimal>, pub realized_pnl: Decimal,
    pub total_value: Option<Decimal>, pub as_of: Option<data::Timestamp> /* oldest quote */,
}
pub struct PositionValue { pub symbol: String, pub quantity: Decimal, pub avg_cost: Decimal, pub price: Option<Decimal>,
                           pub ccy: String, pub market_value_base: Option<Decimal>, pub unrealized_pnl_base: Option<Decimal>,
                           pub weight_pct: Option<Decimal>, pub day_change_pct: Option<Decimal>, pub quote_as_of: Option<data::Timestamp> }

/// One quotes() call for all symbols; one fx_rate() per distinct non-base ccy.
/// The finance client being absent or failing → Unavailable (still returns holdings); some symbols missing → Partial.
pub async fn value_account(h: &Holdings, acct: &Account, fin: Option<&Arc<dyn fin::FinanceClient>>) -> Valuation;
```

### 7.5 Tools

```rust
pub struct PortfolioView { repo: Arc<PortfolioRepo>, finance: Option<Arc<dyn fin::FinanceClient>> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct PortfolioViewInput {
    /// Account name or id; default account if omitted. Use "*" for all accounts.
    #[serde(default)] pub account: Option<String>,
    /// summary (default) | positions | history
    #[serde(default)] pub view: Option<ViewKind>,
    /// For view=history: number of recent ledger entries (default 20, max 200).
    #[serde(default)] pub limit: Option<u32>,
}
// traits: ReadOnly, concurrent, 30 s. run: db_read(entries) → derive_holdings → value_account → json + Table.

pub struct PortfolioManage { repo: Arc<PortfolioRepo>, cfg: PaperConfig }
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManageAction {
    CreateAccount { name: String, kind: AccountKind, base_ccy: Option<String>, starting_cash: Option<String>, make_default: Option<bool> },
    Deposit       { account: Option<String>, amount: String, note: Option<String> },
    Withdraw      { account: Option<String>, amount: String, note: Option<String> },
    /// Tracked accounts: record an existing holding at its cost basis.
    RecordHolding { account: Option<String>, symbol: String, quantity: String, avg_cost: String, ccy: Option<String> },
    SetDefault    { account: String },
    Archive       { account: String },
}
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct ManageInput { #[serde(flatten)] pub action: ManageAction }
// traits: LocalWrite, serial, 10 s, Default. plan summary e.g. "Deposit 5,000 USD into 'paper'".
// The first CreateAccount also becomes the default; paper accounts start with starting_cash (default cfg) as a deposit.

pub struct PaperTrade { repo: Arc<PortfolioRepo>, finance: Option<Arc<dyn fin::FinanceClient>>, cfg: PaperConfig }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct PaperTradeInput {
    #[serde(default)] pub account: Option<String>,
    pub symbol: String,
    pub side: TradeSideArg,                    // buy | sell
    /// Exactly one of quantity / notional (base currency).
    #[serde(default)] pub quantity: Option<String>,
    #[serde(default)] pub notional: Option<String>,
    #[serde(default)] pub note: Option<String>,
}
#[async_trait]
impl core::TypedTool for PaperTrade {
    type Input = PaperTradeInput;
    const NAME: &'static str = names::PAPER_TRADE;
    const DESCRIPTION: &'static str = "Simulate a market order in a local paper-trading account (no real money). \
        Fills at the latest quote plus configured slippage. Use portfolio_view to see the result.";
    fn traits(&self) -> core::ToolTraits { traits(LocalWrite, false, 30, ConfirmOverride::Default) }
    fn plan(&self, i: &PaperTradeInput, _: &core::PlanContext<'_>) -> core::ToolPlan {
        // "Paper BUY 10 AAPL at market in 'default'" (no I/O, so no price in the summary)
    }
    async fn run(&self, i: PaperTradeInput, ctx: core::ToolContext) -> Result<core::ToolOutput, core::ToolError> {
        // 1. validate: symbol, exactly one of qty/notional, > 0                     → Invalid
        // 2. acct = db_read(resolve_account); tracked → Invalid("paper_trade needs a paper account; use portfolio_manage record_holding")
        // 3. fin = require_finance()?; quote = fin.quotes(&[sym]).await? (missing → Invalid unknown symbol)
        // 4. fx = (quote.currency != base) ? Some(fin.fx_rate(..).await?.rate) : None
        // 5. fill = simulate_market_fill(..)
        // 6. db_write(move |tx| {
        //        let h = derive_holdings(&entries_tx(tx, &acct.id)?);
        //        let chk = check_trade(&h, Paper, side, &sym, fill.qty, -fill.amount_base);
        //        if !chk.ok { return Err(Invalid(chk.reason)) }          // "Insufficient cash: need 1,873.20 USD, have 950.00"
        //        append_tx(tx, &NewLedgerEntry{ tool_call_id: Some(ctx.call_id.0), session_id: Some(ctx.session_id), .. })
        //    })
        // 7. Duplicate(id) → Ok("This trade was already executed (ledger #id)"); New(id) → fill JSON
        //    {ledger_id, side, symbol, quantity, price, fee, amount, cash_after, quote_as_of, market_state,
        //     note: market_state != regular ? "market closed; filled at last price" : null}
    }
}

pub struct WatchlistShow { repo: Arc<PortfolioRepo>, finance: Option<Arc<dyn fin::FinanceClient>> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct WatchlistShowInput { #[serde(default)] pub watchlist: Option<String> /* default "default" */,
                                #[serde(default)] pub with_quotes: Option<bool> /* default true */ }

pub struct WatchlistEdit { repo: Arc<PortfolioRepo>, finance: Option<Arc<dyn fin::FinanceClient>> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct WatchlistEditInput {
    pub action: WatchAction,                  // add | remove
    pub symbols: Vec<String>,                 // 1..=50
    #[serde(default)] pub watchlist: Option<String>,
    #[serde(default)] pub note: Option<String>,
}
// traits: LocalWrite, serial, 10 s, ConfirmOverride::Never.
// add: if finance Ready, a best-effort search() per symbol with a 3 s budget; unknown → still added, listed in "unverified".
// Output: {"watchlist":"default","added":["AAPL"],"already_present":[],"unverified":[],"count":12}
```

## 8. Memory

### 8.1 Model and pure helpers

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(transparent)]
pub struct MemoryId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)] #[serde(rename_all = "snake_case")]
pub enum MemoryKind { Fact, Preference, Instruction }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum Origin { Model, User, Import }

#[derive(Debug, Clone, Serialize)]
pub struct MemoryEntry {
    pub id: MemoryId, pub kind: MemoryKind, pub content: String, pub tags: Vec<String>,
    pub importance: u8, pub pinned: bool, pub origin: Origin, pub source_session: Option<data::SessionId>,
    pub created_at: data::Timestamp, pub updated_at: data::Timestamp, pub last_used_at: Option<data::Timestamp>,
    pub use_count: u32, pub expires_at: Option<data::Timestamp>,
}
pub struct NewMemory { pub kind: MemoryKind, pub content: String, pub tags: Vec<String>, pub importance: u8,
                       pub pinned: bool, pub origin: Origin, pub source_session: Option<data::SessionId>,
                       pub expires_at: Option<data::Timestamp> }

// normalize.rs (pure)
/// NFC-less but stable: trim, collapse internal whitespace, strip trailing '.', lowercase for hashing only.
pub fn normalize_content(s: &str) -> String;           // the stored form keeps case; only whitespace is collapsed
pub fn content_hash(s: &str) -> String;                // hex sha256(lowercase(normalize_content(s)))
/// Tags: lowercase, [a-z0-9_-]{1,32}, max 8, deduped, sorted.
pub fn normalize_tags(t: &[String]) -> Result<Vec<String>, ToolsError>;
/// Rejects likely-pasted tool output: > 2000 chars, > 20 lines, JSON-looking blobs, or "[truncated:" markers.
pub fn looks_like_tool_output(s: &str) -> bool;
```

### 8.2 FTS query construction and ranking (`fts.rs`)

Model and user text must never reach `MATCH` raw, because FTS5 syntax
errors (an unbalanced `"`, a bare `AND`, `-`, or `:`) would turn into
SQL errors.

```rust
/// Tokenizes on Unicode word boundaries; keeps tokens of ≥ 2 chars; drops a small English stopword list;
/// at most 12 tokens; each becomes a quoted FTS5 string with internal '"' doubled, prefix-matched when ≥ 4 chars:
///   "dividend"* OR "tax" OR "roth"*
/// Returns None when nothing remains (the caller then falls back to the recency listing).
pub fn build_match_query(text: &str) -> Option<String>;

/// score = bm25(memory_entry_fts, 1.0, 0.5)        -- lower is better in SQLite; negated below
///       adjusted in SQL as:  -bm25 * (1 + 0.25*importance) * (pinned ? 1.5 : 1.0) * recency
///       recency = 1 / (1 + age_days/180)
pub const SEARCH_SQL: &str = r#"
SELECT e.id, e.kind, e.content, e.tags, e.importance, e.pinned, e.origin, e.source_session,
       e.created_at, e.updated_at, e.last_used_at, e.use_count, e.expires_at,
       (-bm25(memory_entry_fts, 1.0, 0.5))
         * (1.0 + 0.25 * e.importance)
         * (CASE e.pinned WHEN 1 THEN 1.5 ELSE 1.0 END)
         * (1.0 / (1.0 + ((?2 - e.updated_at) / 86400000.0) / 180.0)) AS score,
       snippet(memory_entry_fts, 0, '[', ']', '…', 16) AS snip
FROM memory_entry_fts JOIN memory_entry e ON e.id = memory_entry_fts.rowid
WHERE memory_entry_fts MATCH ?1
  AND (e.expires_at IS NULL OR e.expires_at > ?2)
  AND (?3 IS NULL OR e.kind = ?3)
ORDER BY score DESC, e.id DESC
LIMIT ?4"#;
```

The tag filter is applied in SQL with
`EXISTS (SELECT 1 FROM json_each(e.tags) WHERE value IN (…))` when the
caller asks for it.

### 8.3 Repository

```rust
pub struct MemoryRepo { db: data::Db }
pub enum SaveOutcome { Created(MemoryEntry), Duplicate(MemoryEntry), Updated(MemoryEntry) }
pub struct SearchHit { pub entry: MemoryEntry, pub score: f64, pub snippet: String }
pub struct MemoryFilter { pub kind: Option<MemoryKind>, pub tags: Vec<String>, pub limit: u32 }

impl MemoryRepo {
    pub fn new(db: data::Db) -> Self;
    /// One IMMEDIATE tx: enforce max_entries; INSERT … ON CONFLICT(content_hash) DO NOTHING; on conflict
    /// load and return Duplicate. If the duplicate has lower importance/pinned, raise them (never lower).
    pub async fn save(&self, m: NewMemory, now: Timestamp, max_entries: u32) -> Result<SaveOutcome, ToolsError>;
    /// Replace content/tags/kind/importance/pinned of an existing id (the model "corrects" a memory).
    /// A hash collision with another row → Invalid("identical to memory #N; forget one of them").
    pub async fn update(&self, id: MemoryId, patch: MemoryPatch, now: Timestamp) -> Result<SaveOutcome, ToolsError>;
    pub async fn search(&self, text: &str, f: &MemoryFilter, now: Timestamp) -> Result<Vec<SearchHit>, ToolsError>;   // db_read
    /// No query: most recently updated, honoring the filter.
    pub async fn recent(&self, f: &MemoryFilter, now: Timestamp) -> Result<Vec<MemoryEntry>, ToolsError>;
    /// Stable profile set: pinned, then instruction, then preference, then facts by importance DESC, id ASC;
    /// non-expired; stops when the cumulative content length exceeds budget_chars. Also returns the total count.
    pub async fn profile(&self, budget_chars: usize, now: Timestamp) -> Result<(Vec<MemoryEntry>, u32 /* total */), ToolsError>;
    pub async fn get(&self, ids: &[MemoryId]) -> Result<Vec<MemoryEntry>, ToolsError>;
    pub async fn delete(&self, ids: &[MemoryId]) -> Result<Vec<MemoryId> /* actually deleted */, ToolsError>;
    /// Best effort and fire-and-forget: UPDATE last_used_at, use_count+1 (does not touch FTS; see the AU trigger).
    pub fn touch_detached(&self, ids: Vec<MemoryId>, now: Timestamp);
    // admin support
    pub async fn stats(&self, now: Timestamp) -> Result<MemoryStats, ToolsError>;
    pub async fn import(&self, rows: Vec<NewMemory>, dry_run: bool, now: Timestamp) -> Result<ImportReport, ToolsError>;
    pub async fn maintenance(&self, now: Timestamp, decay_after_days: Option<u32>) -> Result<TickReport, ToolsError>;
}
```

### 8.4 Tools

```rust
pub struct MemorySave { repo: Arc<MemoryRepo>, cfg: MemoryConfig }
/// Save a durable fact, preference or standing instruction about the user, for future sessions.
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct MemorySaveInput {
    /// One self-contained statement, e.g. "User's base currency is INR." (≤ 2000 chars).
    pub content: String,
    /// fact (default) | preference | instruction ("always show prices in INR").
    #[serde(default)] pub kind: Option<MemoryKind>,
    #[serde(default)] pub tags: Vec<String>,
    /// 0–3 (default 1). Use 3 only for things the user called important.
    #[serde(default)] pub importance: Option<u8>,
    /// Pin to always include in context. Only when the user asks.
    #[serde(default)] pub pinned: Option<bool>,
    /// Replace an existing memory (from memory_search) instead of adding a new one.
    #[serde(default)] pub replaces: Option<i64>,
    /// Forget automatically after N days.
    #[serde(default)] pub ttl_days: Option<u32>,
}
// traits: LocalWrite, serial, 10 s, ConfirmOverride::Never.
// Output: {"id":42,"status":"created"|"duplicate"|"updated","kind":"preference"}

pub struct MemorySearch { repo: Arc<MemoryRepo> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct MemorySearchInput {
    /// Free text; omit to list the most recent memories.
    #[serde(default)] pub query: Option<String>,
    #[serde(default)] pub kind: Option<MemoryKind>,
    #[serde(default)] pub tags: Vec<String>,
    #[serde(default)] pub limit: Option<u32>,     // default 8, max 30
}
// traits: ReadOnly, concurrent, 10 s. After results: repo.touch_detached(hit ids).
// Output: {"results":[{"id":42,"kind":"preference","content":"…","tags":[…],"updated":"2026-10-02","score":3.1}],
//          "total_memories":118}

pub struct MemoryForget { repo: Arc<MemoryRepo> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct MemoryForgetInput { /** ids from memory_search; 1..=50 */ pub ids: Vec<i64> }
// traits: LocalWrite, serial, 10 s, Default.
// TypedTool::plan has no DB access, so the permission prompt shows the ids only. The ToolDisplay in the
// result shows the deleted contents. Output: {"deleted":[42],"not_found":[7]}
```

### 8.5 `PromptContributor` for recall (`recall.rs`)

This is the core trait it implements (core LLD §11):
- `fn id(&self) -> &str`
- `async fn contribute(&self, inputs: &PromptInputs<'_>) -> Result<Option<String>, String>`
- `PromptInputs { session_id, workdir, user_text, tool_names, today }`

It is called once per turn under a 500 ms timeout. A timeout or `Err`
drops the section and logs a warning.

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallMode {
    /// Stable profile only: cache-friendly (default).
    #[default] Profile,
    /// Profile + top-k FTS hits on this turn's user_text: changes the system prompt every turn (cache miss).
    ProfilePlusRelevant,
    /// Contribute nothing; memory_search only.
    Off,
}

pub struct MemoryContributor { repo: Arc<MemoryRepo>, cfg: MemoryConfig }
pub fn contributor(deps: &ToolDeps) -> Arc<dyn core::PromptContributor> {
    Arc::new(MemoryContributor { repo: Arc::new(MemoryRepo::new(deps.db.clone())), cfg: deps.config.memory.clone() })
}

#[async_trait]
impl core::PromptContributor for MemoryContributor {
    fn id(&self) -> &str { "memory" }

    async fn contribute(&self, inputs: &core::PromptInputs<'_>) -> Result<Option<String>, String> {
        if self.cfg.recall == RecallMode::Off { return Ok(None); }
        // Without the memory tools in this turn's ToolSet (e.g. a filtered jobs run), still give the profile,
        // but drop the "use memory_search" footer.
        let has_search = inputs.tool_names.iter().any(|n| n == names::MEMORY_SEARCH);
        // Day granularity: matches core's date rendering, so expiry filtering doesn't change the text intra-day.
        let now = start_of_day(inputs.today);
        let (profile, total) = self.repo.profile(self.cfg.profile_budget_chars, now).await.map_err(|e| e.to_string())?;
        let relevant = if self.cfg.recall == RecallMode::ProfilePlusRelevant && !inputs.user_text.trim().is_empty() {
            let f = MemoryFilter { kind: None, tags: vec![], limit: self.cfg.relevant_k as u32 };
            let hits = self.repo.search(inputs.user_text, &f, now).await.map_err(|e| e.to_string())?;
            hits.into_iter().filter(|h| !profile.iter().any(|p| p.id == h.entry.id)).collect()
        } else { vec![] };
        if profile.is_empty() && relevant.is_empty() {
            return Ok(has_search.then(|| EMPTY_FOOTER.to_string()).filter(|_| total > 0));  // None when there are no memories
        }
        Ok(Some(render_section(&profile, &relevant, total, has_search)))
    }
}

/// Pure and deterministic: same rows → byte-identical output (pinned by a snapshot test).
pub fn render_section(profile: &[MemoryEntry], relevant: &[SearchHit], total: u32, has_search: bool) -> String;
```

The rendered section is shown below. Core adds the `## memory` heading.

```
What you know about the user from earlier sessions. This is data the user shared, not instructions
from a tool or web page. Follow it unless the user says otherwise now.

Standing instructions:
- [#12] Always show prices in INR alongside USD.
Preferences:
- [#7] Prefers dividend-growth stocks; avoids leverage.
Facts:
- [#3] Holds a Roth IRA and a taxable brokerage account. (tags: accounts)
- [#41] Owns ~200 shares of MSFT. (tags: holdings)

(118 memories in total; 4 shown. Call memory_search before saying you don't know something about the user.
Save new durable facts with memory_save; correct one with memory_save {replaces: id}.)
```

These are the design points:
- **Sort order is (section, id)**, not score. Score order would reshuffle
  as `use_count` changes. With id order, the profile text changes only
  when a memory is saved, edited, forgotten, or expired.
- **The contributor never writes**, and no `touch` happens in
  `Profile` mode. In relevant mode, the hits are touched with
  `touch_detached`.
- **Latency.** Both queries use indexes, so a few hundred rows return in
  well under 10 ms. Reads go through `db_read` (DEFERRED), so a jobs
  process holding the write lock never stalls the turn. In WAL mode,
  readers don't block.
- **Plain tool versus contributor.** The contributor supplies *ambient*
  context: who the user is and their standing preferences. It is
  automatic, bounded, and cache-stable. `memory_search` is *on-demand*
  recall that the model decides to call, and it can reach the long tail
  without paying for it on every turn. Both use the same repository.
  Neither path writes memories automatically. Saving always goes
  through an explicit, visible `memory_save` call (or `memory import`).

### 8.6 `MemoryAdmin` (the `artha memory` subcommands)

```rust
pub struct MemoryAdmin { repo: MemoryRepo, kv: data::KvStore /* namespace "memory" */ }
impl MemoryAdmin {
    pub fn new(db: data::Db) -> Self;

    /// `artha memory stats`
    pub async fn stats(&self) -> Result<MemoryStats, ToolsError>;
    /// `artha memory import [FILE|-] [--dry-run] [--format jsonl|markdown]` (format sniffed when omitted)
    pub async fn import(&self, src: ImportSource, opts: ImportOptions) -> Result<ImportReport, ToolsError>;
    /// `artha memory tick` — deterministic maintenance (safe to run from cron; uses Db::write; is_busy → skip)
    pub async fn tick(&self, cfg: &MemoryConfig) -> Result<TickReport, ToolsError>;
    /// `artha memory model <name>` — (unconfirmed) records the embedding-model choice; no effect on recall in v1
    pub fn set_model(&self, name: &str) -> Result<(), ToolsError>;
    /// `artha memory install` — (unconfirmed) v1 returns Err(Unavailable{"Semantic memory recall", "not in this build"})
    pub async fn install(&self) -> Result<(), ToolsError>;
}

pub struct MemoryStats { pub total: u32, pub by_kind: BTreeMap<String, u32>, pub pinned: u32, pub bytes: u64,
                         pub fts_rows: u32, pub fts_consistent: bool /* fts 'integrity-check' */, pub oldest: Option<Timestamp>,
                         pub newest: Option<Timestamp>, pub top_tags: Vec<(String, u32)>, pub last_tick: Option<Timestamp>,
                         pub recall_mode: RecallMode, pub embedding_model: Option<String>, pub semantic_active: bool /* false */ }
pub enum ImportSource { File(PathBuf), Stdin(Box<dyn std::io::Read + Send>) }
pub struct ImportOptions { pub dry_run: bool, pub format: Option<ImportFormat> }
pub enum ImportFormat { JsonLines, Markdown }   // markdown: "- [pref] text #tag", "- [instr] …", "- text" (fact)
pub struct ImportReport { pub added: u32, pub duplicates: u32, pub rejected: Vec<(usize /* line */, String)> }
pub struct TickReport { pub expired_deleted: u32, pub decayed: u32, pub fts_optimized: bool, pub ran_at: Timestamp }
```

`tick` runs these steps:
1. `DELETE FROM memory_entry WHERE expires_at <= now`. The trigger
   cleans up the FTS rows.
2. If `decay_after_days` is set, lower `importance` by 1, down to a
   floor of 0, for unpinned `fact` rows that have
   `use_count = 0 AND updated_at < now - N days`. Nothing is ever
   deleted because of decay.
3. `INSERT INTO memory_entry_fts(memory_entry_fts) VALUES ('optimize')`.
4. Set `kv["last_tick"] = now`.

Each step runs in its own `Db::write`. If `is_busy()` is true, the
report comes back with `skipped: true`, and the CLI exits 0.

**A planned extension, outside v1:** `tick --extract` would distill
candidate memories from sessions updated since `last_tick`. It runs an
LLM, so the CLI would launch it as a headless agent turn through
`artha-jobs` with goal "review recent sessions and save durable facts",
`allow_tools = {memory_search, memory_save}`, and a read-only
session-transcript tool. That transcript tool doesn't exist yet. If
semantic recall is confirmed, an `Embedder` port
(`async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>`)
and a `memory_vector` table would arrive in migration
`memory/0002_vectors.sql`. The `0001` schema needs no change.

## 9. Automation (monitors)

### 9.1 Model

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(transparent)]
pub struct MonitorId(pub uuid::Uuid);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MonitorCondition {
    PriceAbove { symbol: String, #[serde(with = "dec_str")] price: Decimal },
    PriceBelow { symbol: String, #[serde(with = "dec_str")] price: Decimal },
    /// |change| ≥ pct over the window (day = vs. prev close; since = vs. reference price).
    PercentMove { symbol: String, #[serde(with = "dec_str")] pct: Decimal, window: MoveWindow, direction: Option<Direction> },
    /// Price crosses the moving average (fires on the cross, by edge semantics).
    IndicatorCross { symbol: String, ma: MovingAverage, direction: Direction },
    /// Today's volume ≥ multiple × average volume.
    VolumeSpike { symbol: String, #[serde(with = "dec_str")] multiple: Decimal },
    /// New articles matching the query/symbols since the last check (deduped by article id).
    NewsMention { query: Option<String>, symbols: Vec<String>, min_articles: Option<u32> },
    /// Paper/tracked account value drawdown from its running peak ≥ pct.
    PortfolioDrawdown { account: String, #[serde(with = "dec_str")] pct: Decimal },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum MoveWindow { Day, Since { #[serde(with = "dec_str")] reference: Decimal } }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)] #[serde(rename_all = "snake_case")]
pub enum Direction { Up, Down, Above, Below }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)] #[serde(tag = "kind", rename_all = "snake_case")]
pub enum MovingAverage { Sma { period: u32 }, Ema { period: u32 } }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Schedule {
    Every { seconds: u64, #[serde(default)] market_hours_only: bool },
    /// 5- or 6-field cron, evaluated in `tz` (IANA, default the local zone).
    Cron { expr: String, #[serde(default)] tz: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MonitorAction {
    /// Tell the user (notice on any open session plus jobs log). `message` may use {symbol} {value} {name}.
    Notify { message: Option<String> },
    /// Run an unattended agent turn with this goal; only `allow_tools` may write (read-only tools are always allowed).
    RunAgent { goal: String, #[serde(default)] allow_tools: Vec<String> },
}

pub struct MonitorDefinition {
    pub id: MonitorId, pub name: String, pub condition: MonitorCondition, pub schedule: Schedule,
    pub action: MonitorAction, pub enabled: bool, pub status: MonitorStatus, pub status_detail: Option<String>,
    pub cooldown: Duration, pub max_fires: Option<u32>, pub fire_count: u32, pub expires_at: Option<data::Timestamp>,
    pub job_id: Option<String>, pub source_session: Option<data::SessionId>,
    pub created_at: data::Timestamp, pub updated_at: data::Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum MonitorStatus { Pending, Scheduled, Error, Finished }

pub struct MonitorStateRow { pub armed: bool, pub last_checked_at: Option<Timestamp>, pub last_observed: Option<serde_json::Value>,
                             pub last_fired_at: Option<Timestamp>, pub consecutive_errors: u32, pub last_error: Option<String>,
                             pub seen: Vec<String> }
```

### 9.2 Scheduler port (`scheduler.rs`), implemented by an `artha-cli` adapter over `artha-jobs`

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScheduleSpec {
    /// The jobs_job.goal column: a human label, e.g. "Monitor: AAPL below 180".
    pub label: String,
    pub schedule: Schedule,
    /// Opaque to artha-tools' callers; artha-jobs stores it in jobs_job.config and dispatches on "handler".
    pub config: serde_json::Value,      // {"handler":"monitor","monitor_id":"<uuid>"}
    /// No runs after this; artha-jobs may mark the job done.
    pub until: Option<data::Timestamp>,
}
#[derive(Debug, Clone, PartialEq, Eq)] pub struct JobRef { pub job_id: String }

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SchedulerError {
    #[error("invalid schedule: {0}")] InvalidSchedule(String),
    #[error("job {0} not found")]     NotFound(String),
    #[error("scheduler busy")]        Busy,
    #[error("{0}")]                   Other(String),
}

#[async_trait]
pub trait MonitorScheduler: Send + Sync + 'static {
    async fn schedule(&self, spec: ScheduleSpec) -> Result<JobRef, SchedulerError>;
    async fn pause(&self, job: &JobRef) -> Result<(), SchedulerError>;
    async fn resume(&self, job: &JobRef) -> Result<(), SchedulerError>;
    /// Idempotent: NotFound is treated as success by callers.
    async fn cancel(&self, job: &JobRef) -> Result<(), SchedulerError>;
    /// Next planned fire time, for monitor_list (None if unknown).
    async fn next_run(&self, job: &JobRef) -> Result<Option<data::Timestamp>, SchedulerError>;
}
```

Implementations must not call back into a `Db` closure that is already
held. They run after our `db_write` has returned, never inside one.

### 9.3 Repository and reconciliation

```rust
pub struct MonitorRepo { db: data::Db }
impl MonitorRepo {
    pub fn new(db: data::Db) -> Self;
    pub async fn insert_pending(&self, def: &NewMonitor, now: Timestamp) -> Result<MonitorDefinition, ToolsError>; // + monitor_state row
    pub async fn set_scheduled(&self, id: &MonitorId, job: &JobRef, now: Timestamp) -> Result<(), ToolsError>;
    pub async fn set_status(&self, id: &MonitorId, st: MonitorStatus, detail: Option<String>, now: Timestamp) -> Result<(), ToolsError>;
    pub async fn set_enabled(&self, id: &MonitorId, enabled: bool, now: Timestamp) -> Result<(), ToolsError>;
    pub async fn delete(&self, id: &MonitorId) -> Result<bool, ToolsError>;
    pub async fn get(&self, id: &MonitorId) -> Result<Option<(MonitorDefinition, MonitorStateRow)>, ToolsError>;
    pub async fn resolve(&self, id_or_name: &str) -> Result<MonitorDefinition, ToolsError>;   // unique prefix or exact name
    pub async fn list(&self, include_finished: bool) -> Result<Vec<(MonitorDefinition, MonitorStateRow, Option<MonitorEventRow>)>, ToolsError>;
    pub async fn events(&self, id: &MonitorId, limit: u32) -> Result<Vec<MonitorEventRow>, ToolsError>;

    /// Heals step-2/3 crashes: for status IN ('pending','error') AND enabled, schedule() then set_scheduled().
    /// Returns (rescheduled, still_failing). A None scheduler → no-op.
    pub async fn reconcile(&self, scheduler: Option<&dyn MonitorScheduler>) -> Result<(u32, u32), ToolsError>;
}
```

### 9.4 Evaluator (the fire-time API used by `artha-jobs`)

```rust
#[derive(Clone)]
pub struct MonitorEvaluator { repo: Arc<MonitorRepo>, portfolio: Arc<PortfolioRepo>, finance: Option<Arc<dyn fin::FinanceClient>> }

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Evaluation {
    /// No such monitor: the adapter should cancel the job.
    Gone,
    /// Disabled, finished (max_fires reached) or expired: the adapter should pause or cancel the job.
    Inactive { reason: String, cancel_job: bool },
    /// market_hours_only and the market is closed (per quote.market_state): nothing recorded except last_checked_at.
    Skipped { reason: String },
    NotTriggered { observed: serde_json::Value },
    /// Condition true but not armed (already fired, waiting to re-arm) or inside the cooldown.
    Suppressed { observed: serde_json::Value, reason: String },
    Fired(FiredMonitor),
    /// Data-source failure; state.consecutive_errors incremented. After 10 consecutive errors, status='error'.
    Error { message: String, retryable: bool },
}

#[derive(Debug, Clone, Serialize)]
pub struct FiredMonitor {
    pub event_id: i64,
    pub monitor_id: MonitorId,
    pub name: String,
    pub observed: serde_json::Value,     // e.g. {"symbol":"AAPL","price":"179.40","as_of":"…"}
    pub message: String,                 // rendered: "AAPL fell below 180 (179.40)"
    pub action: MonitorAction,
    pub source_session: Option<data::SessionId>,
}

#[derive(Debug, Clone)]
pub enum DispatchOutcome { Delivered { detail: Option<String> }, Failed { error: String } }

impl MonitorEvaluator {
    pub fn new(deps: &ToolDeps) -> Self;
    /// Idempotent per (monitor, check): safe to call from multiple processes, since state updates run in one
    /// IMMEDIATE tx that re-reads the state. A concurrent duplicate sees armed=0 and yields Suppressed.
    pub async fn evaluate(&self, id: &MonitorId, now: data::Timestamp) -> Result<Evaluation, ToolsError>;
    /// Called by artha-jobs after carrying out the action.
    pub async fn record_dispatch(&self, event_id: i64, outcome: DispatchOutcome) -> Result<(), ToolsError>;
    /// Parses config.monitor_id from a jobs_job config value (adapter convenience).
    pub fn monitor_id_from_config(cfg: &serde_json::Value) -> Option<MonitorId>;
}
```

`evaluate` runs this algorithm:
1. Run `db_read` to get the `(def, state)`. If there is no row, the
   result is `Gone`. If the monitor is disabled, expired, or at
   `fire_count >= max_fires`, the result is `Inactive`. An expired or
   maxed-out monitor is also marked `finished`.
2. Run `condition::observe(def.condition, fin, portfolio, &state)`. This
   is async I/O *outside* any transaction, and it yields an
   `Observation { value: serde_json::Value, satisfied: bool, new_seen: Vec<String>, market_open: Option<bool> }`.
   - A finance error goes through `map_finance`'s classification, and
     the result is `Error`.
   - If `market_hours_only` is set and `market_open == Some(false)`,
     the result is `Skipped`.
3. In one `db_write`, re-read `state`, then apply the pure transition
   `condition::step(state, &obs, def.cooldown, now) -> (StateRow, Verdict)`:

   | `armed` | `satisfied` | in cooldown | → verdict | new `armed` |
   |---|---|---|---|---|
   | 1 | true | no | `Fire` | 0 |
   | 1 | true | yes | `Suppressed("cooldown")` | 1 |
   | 1 | false | – | `NotTriggered` | 1 |
   | 0 | true | – | `Suppressed("already fired; re-arms when the condition clears")` | 0 |
   | 0 | false | – | `NotTriggered` (re-armed) | 1 |

   `NewsMention` is special. Its `satisfied` value is "≥ min_articles
   *unseen* items", and it re-arms immediately, because each new article
   is a new edge. `seen` is the union of the existing and new ids,
   capped to the most recent 500.

   On `Fire`, the same transaction also runs
   `INSERT monitor_event(… dispatch_status='pending')`, `fire_count += 1`,
   and `last_fired_at = now`.
4. Return `Fired(FiredMonitor)` with the rendered message.

The **handoff contract** for `artha-jobs` (its side, carried out in the
`artha-cli` adapter) is:

```
on due job with config.handler == "monitor":
    id  = MonitorEvaluator::monitor_id_from_config(&job.config)  (None → mark job failed)
    match evaluator.evaluate(&id, now):
        Gone                         → jobs.cancel(job)
        Inactive{cancel_job: true}   → jobs.cancel(job);  Inactive{false} → jobs.pause(job)
        Skipped | NotTriggered | Suppressed → run succeeded, log at debug
        Error{retryable}             → run failed (jobs' retry/backoff policy), jobs_log the message
        Fired(f) → match f.action:
            Notify{..}  → for each in-process SessionHandle: notify(Notice::Info{ "[monitor] {f.message}" });
                          jobs_log(info, f.message); outcome = Delivered
            RunAgent{goal, allow_tools} →
                allow = artha_tools::names::read_only() ∪ allow_tools
                agent.run_turn_headless(SessionSelector::New, UserInput::text(
                    "[Monitor '{f.name}' fired] {f.message}\nObserved: {f.observed}\n\nTask: {goal}"),
                    …) with AgentConfig::unattended(allow)
                outcome = Delivered{detail: final text (truncated 4 KiB)} | Failed{error}
            evaluator.record_dispatch(f.event_id, outcome)
```

### 9.5 Tools

```rust
pub struct MonitorCreate { repo: Arc<MonitorRepo>, scheduler: Option<Arc<dyn MonitorScheduler>>, cfg: ToolsConfig }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct MonitorCreateInput {
    /// Short name, e.g. "AAPL under 180".
    pub name: String,
    pub condition: MonitorCondition,
    /// Default: {"kind":"every","seconds":300,"market_hours_only":true} for price conditions, 3600 for news.
    #[serde(default)] pub schedule: Option<Schedule>,
    /// Default: {"kind":"notify"}.
    #[serde(default)] pub action: Option<MonitorAction>,
    /// Minimum minutes between fires (default 60).
    #[serde(default)] pub cooldown_minutes: Option<u32>,
    #[serde(default)] pub max_fires: Option<u32>,
    #[serde(default)] pub expires_in_days: Option<u32>,
}
#[async_trait]
impl core::TypedTool for MonitorCreate {
    type Input = MonitorCreateInput;
    const NAME: &'static str = names::MONITOR_CREATE;
    const DESCRIPTION: &'static str = "Create a persistent market monitor that is checked on a schedule, even after \
        this session ends, and notifies the user or runs an agent task when its condition becomes true.";
    fn traits(&self) -> core::ToolTraits { traits(LocalWrite, false, 15, ConfirmOverride::Default) }
    fn plan(&self, i: &MonitorCreateInput, _: &core::PlanContext<'_>) -> core::ToolPlan {
        // summary: "Monitor 'AAPL under 180': price_below AAPL 180, every 5 min (market hours) → notify"
        // and, for RunAgent with allow_tools: "… → run agent (may use: paper_trade)", so the user consents to it explicitly.
    }
    async fn run(&self, i: MonitorCreateInput, ctx: core::ToolContext) -> Result<core::ToolOutput, core::ToolError> {
        // 1. validate (pure, condition.rs::validate): symbols parse; numbers > 0; Every.seconds ≥ cfg.monitor_min_interval_secs;
        //    Cron parses with croner (and is not more often than the minimum); allow_tools ⊆ names::all();
        //    a RunAgent goal is 1..=2000 chars                                          → Invalid
        // 2. scheduler = self.scheduler.as_ref().ok_or(Unavailable{"Monitors", "no job scheduler in this session"})?
        // 3. def = repo.insert_pending(..)                         (status 'pending', job_id NULL)
        // 4. match scheduler.schedule(spec(def)).await:
        //      Ok(job)  → repo.set_scheduled(&def.id, &job)
        //      Err(e)   → repo.delete(&def.id); return Err(Scheduler(e))
        // 5. next = scheduler.next_run(&job).await.ok().flatten()
        // Output: {"id","name","status":"scheduled","next_check":…,"summary":…}
    }
}

pub struct MonitorList { repo: Arc<MonitorRepo>, scheduler: Option<Arc<dyn MonitorScheduler>> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct MonitorListInput { #[serde(default)] pub include_finished: bool,
                              /** Show recent fire events for this monitor id or name. */ #[serde(default)] pub events_for: Option<String> }
// ReadOnly, concurrent, 15 s. It runs reconcile() first only when the scheduler is present and some row is pending.
// That one place writes from a ReadOnly tool. It is acceptable because it only repairs our own invariant, and it
// is idempotent and invisible to the user. The alternative (reconcile only at startup) is also wired in artha-cli.

pub struct MonitorManage { repo: Arc<MonitorRepo>, scheduler: Option<Arc<dyn MonitorScheduler>> }
#[derive(Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub struct MonitorManageInput { pub monitor: String /* id or name */, pub action: ManageMonitorAction /* pause|resume|delete|acknowledge */ }
// LocalWrite, serial, 15 s, Default.
// pause/resume: scheduler first, then set_enabled. delete: cancel (NotFound ok) → repo.delete. acknowledge: mark events acknowledged.
```

## 10. Testing strategy

All tests are deterministic. They use no network except the loopback
test server, no real home directory, and no real finance source.

### 10.1 Test doubles (`testing.rs`, feature `test-util`)

```rust
/// Scripted, in-memory FinanceClient. Unknown symbols → FinanceErrorKind::NotFound.
#[derive(Clone, Default)]
pub struct FakeFinance { inner: Arc<Mutex<FakeFinanceState>> }
struct FakeFinanceState {
    availability: fin::Availability,                      // default Ready
    quotes: HashMap<String, fin::Quote>,
    history: HashMap<String, fin::PriceSeries>,
    fx: HashMap<(String, String), Decimal>,
    news: Vec<fin::NewsItem>,
    macro_series: HashMap<String, fin::MacroSeries>,
    fail_next: VecDeque<fin::FinanceErrorKind>,           // injected failures, consumed one per call
    calls: Vec<String>,                                    // "quotes:AAPL,MSFT" for assertions (batching, no N+1)
    delay: Option<Duration>,                               // with tokio::time::pause() for timeout/cancel tests
}
impl FakeFinance {
    pub fn new() -> Self;
    pub fn quote(self, sym: &str, price: &str, ccy: &str) -> Self;   // builder; market_state Regular, as_of fixed
    pub fn set_price(&self, sym: &str, price: &str);                  // mutate between evaluator ticks
    pub fn set_market_state(&self, sym: &str, st: fin::MarketState);
    pub fn fx(self, from: &str, to: &str, rate: &str) -> Self;
    pub fn news(self, items: Vec<fin::NewsItem>) -> Self;
    pub fn unavailable(self, hint: &str) -> Self;
    pub fn fail_next(&self, k: fin::FinanceErrorKind);
    pub fn calls(&self) -> Vec<String>;
}

/// Records schedule/pause/resume/cancel; can be told to fail.
#[derive(Clone, Default)]
pub struct FakeScheduler { pub log: Arc<Mutex<Vec<String>>>, pub fail_schedule: Arc<AtomicBool>, next_id: Arc<AtomicU64> }

/// In-memory Db with core + all three of our migration sets, plus fakes, wired as ToolDeps.
pub struct TestDeps { pub deps: ToolDeps, pub finance: FakeFinance, pub scheduler: FakeScheduler }
impl TestDeps {
    pub fn new() -> Self;   // data::Db::open_in_memory(&MIGRATIONS) (core always runs first)
    pub fn without_finance(self) -> Self;
    pub fn without_scheduler(self) -> Self;
    /// Runs a tool with core::testing::tool_context() (TestEnv-bound EditRecorder, recording progress).
    pub async fn call(&self, tool: &dyn core::Tool, input: serde_json::Value) -> (Result<core::ToolOutput, core::ToolError>, core::testing::ContextProbe);
}
```

`FakeFinance` lives here rather than in `artha-finance`, because the
fake must model exactly the contract in §4. If `artha-finance` later
ships its own `test-util` fake, this one can delegate to it.

### 10.2 Test matrix

| File | Scenarios |
|---|---|
| `schema.rs` | `assert_objects_prefixed` and `assert_migrations_idempotent` for each set; FTS **contract test under artha-data's real pragmas** (`trusted_schema=OFF`, `foreign_keys=ON`): insert, update content, update `last_used_at` (FTS row count unchanged), delete, then `MATCH` finds or doesn't find accordingly; FTS `integrity-check` passes; every `CHECK` list matches the Rust `as_str()` sets (parsed from the SQL, as artha-data does for `Role`); `UNIQUE(account_id, tool_call_id)` allows multiple NULLs; deleting a `core_session` row nulls `source_session` and `session_id` |
| `specs.rs` | insta snapshot of every `ToolSpec` (name, description, schema) and `ToolTraits`; every schema root is `"type":"object"`; every name passes `ToolSpec::validate` and is < 64 characters; `names::read_only()` equals the registered ReadOnly set; `builtin_tools()` registers into a fresh `ToolRegistry` with no duplicates, with and without the `web`/`report` features |
| `markets.rs` | quote batching (one `quotes` call for N symbols); `not_found` listing; history downsampling and the `downsampled` flag; the bar cap; every `FinanceErrorKind` → expected `ToolError` variant and `retryable` (table-driven against `map_finance`); `finance: None` and `NotProvisioned` → `Failed{retryable:false}` with the doctor hint and **zero** fake calls; invalid symbol syntax → `InvalidInput` before any call; cancel during a delayed fake → future dropped cleanly (paused clock); macro transforms (yoy/mom/diff) golden values |
| `portfolio.rs` | `derive_holdings` property tests (proptest over random buy/sell/split/deposit sequences that respect `check_trade`): cash equals the sum of amounts; qty is never negative; realized plus unrealized at a constant price equals the change in total value; split invariance of position value. Fills: slippage direction, fee, fx conversion, notional-to-qty rounding. `paper_trade` insufficient cash → `InvalidInput` with the shortfall and no ledger row; the same `call_id` twice → one row plus the "already executed" text; tracked account → `InvalidInput`; `portfolio_view` with finance down → `valuation: unavailable` plus holdings (not an error); a multi-currency valuation; `watchlist_edit` add/remove idempotent; the default watchlist created lazily; `unverified` symbols when search fails |
| `automation.rs` | `condition::step` full truth table (pure); `NewsMention` dedupe and re-arm; `create` happy path: FakeScheduler log has `schedule`, row is `scheduled` with `job_id`; scheduler failure → row deleted plus a `Failed` error; simulated crash (insert_pending only) → `reconcile` schedules it; `evaluate` sequence on a price path 185→179→178→181→179 with cooldown 0 → fires at 179, suppressed at 178, re-arms at 181, fires at 179 again (two `monitor_event` rows); cooldown suppresses; `max_fires` → `Inactive{cancel_job:true}` and `finished`; deleted monitor → `Gone`; market closed plus `market_hours_only` → `Skipped`; 10 finance errors → `status='error'`; two concurrent `evaluate` calls on separate `Db` handles to one temp file → exactly one `Fired`; `record_dispatch` updates status; `validate` rejects a 10 s interval, a bad cron, and unknown `allow_tools` |
| `memory.rs` | save → search finds it (stemming: "dividends" matches "dividend"); duplicate content (case and whitespace variants) → `duplicate`, same id, importance raised not lowered; `replaces` updates and re-indexes; `forget` removes it from search; `build_match_query` property test (arbitrary Unicode, quotes, `AND`/`NEAR`/`-`/`:`) never causes an SQL error; `looks_like_tool_output` rejections; `max_entries` refusal; expiry hides entries and `tick` deletes them; decay only touches unpinned, unused facts; `import` JSONL and Markdown with line-numbered rejects and `--dry-run` writing nothing; `stats` counts and `fts_consistent`; `install` → `Unavailable` |
| `memory.rs` (contributor) | `render_section` snapshot; **byte-stability**: two calls with different `user_text` in `Profile` mode return identical strings; touching (`use_count`) does not change the output; `Off` → `None`; no memories → `None`; budget cap respected and the footer count correct; `ProfilePlusRelevant` includes a hit not already in the profile; memory tools absent from `tool_names` → no `memory_search` footer; latency on 5,000 rows < 50 ms (non-flaky budget, release-mode only via `#[ignore]` in debug) |
| `web_fetch.rs` | local `tokio` HTTP server on 127.0.0.1 with `allow_private: true`: HTML → text, JSON passthrough, `max_bytes` truncation, `offset` paging, 404 → `Ok` with `status`, unsupported content type → `Failed`, redirect chain > 5 → `Failed`; with `allow_private: false`: 127.0.0.1, `[::1]`, 10.0.0.1, 169.254.169.254, and a redirect to loopback are all refused (pure `net_guard` tests plus one live test); `ftp://` → `InvalidInput` |
| `e2e.rs` | `core::testing::Harness` with real tools and `ScriptedProvider`. (1) Turn 1: the model calls `memory_save` (no permission prompt, `ConfirmOverride::Never`). Turn 2: `provider.requests()[last].system` contains the saved fact under `## memory`. (2) The model calls `paper_trade`; the permission prompt summary contains "Paper BUY"; `Deny` → no ledger row. (3) `market_quote` with finance unavailable → the `is_error` text reaching the model contains "artha doctor". (4) `report_render` → `CheckpointRecorded(PreEdit)` for the planned path, the file exists, and rewind deletes it. (5) An unattended config with `allow = read_only()` → `memory_save` is denied without a prompt |

### 10.3 Conventions

- Time is injected as `now: Timestamp` into the repository and evaluator
  methods. Tools take it from `ToolDeps.clock` (§2.2). Tests use
  `Clock::fixed`.
- Decimal golden values are compared as strings, never as `f64`.
- Each test builds its own in-memory `Db`, so there is no shared state
  and tests run in parallel. File-backed tests (concurrent evaluate,
  pragmas) use `data::testing::TestEnv`.
- Snapshots normalize ids (UUIDv7, ledger ids) and timestamps.
