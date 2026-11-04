# artha-data: Low-Level Design

This document is the companion to `HLD.md`. Signatures below match the
skeleton in `src/`. Function bodies there are still `todo!()`.

## 1. Module layout

```
crates/artha-data/
├── Cargo.toml
├── migrations/core/
│   ├── 0001_sessions.sql           core_session, core_message, core_kv
│   └── 0002_checkpoint_links.sql   core_checkpoint_link
└── src/
    ├── lib.rs              pub mods, flat re-exports, `pub use rusqlite`
    ├── error.rs            DataError, MigrationErrorKind, Result
    ├── types.rs            Timestamp (epoch ms) + ToSql/FromSql
    ├── paths.rs            ArthaPaths (layout, ARTHA_HOME, ensure_dirs, purge), PurgeReport
    ├── fsio.rs             atomic_write, FileLock, FileMode
    ├── json_store.rs       VersionedDocument trait, JsonStore<T>
    ├── settings/
    │   ├── mod.rs          re-exports
    │   ├── model.rs        Settings, Mode, LlmConnector, ConnectorKind, CloudSession,
    │   │                   Preferences, PermissionMode, Secret
    │   └── store.rs        SettingsStore
    ├── db/
    │   ├── mod.rs          Db, DbOptions (read/write/with_conn/call, health_check)
    │   └── pragmas.rs      per-connection PRAGMA setup
    ├── migrate/
    │   ├── mod.rs          Migration, MigrationSet, AppliedMigration, MigrationReport,
    │   │                   Migrator (crate-private), LEDGER_DDL, checksum()
    │   └── builtin.rs      CORE_MIGRATIONS (include_str! of migrations/core/*)
    ├── store/
    │   ├── mod.rs          re-exports
    │   ├── session.rs      SessionId, Role, Session, NewSession, SessionSummary, SessionFilter,
    │   │                   StoredMessage, NewMessage, MessageSeq, SessionStore
    │   ├── kv.rs           KvStore
    │   └── checkpoint_link.rs  CheckpointLink, CheckpointLinkStore
    ├── checkpoint/
    │   ├── mod.rs          CheckpointStore, EditGuard
    │   ├── model.rs        CheckpointId, CheckpointKind, EditContext, Checkpoint, CheckpointFilter,
    │   │                   RestoreScope, RestoreReport, CheckpointDiff, FileChange, ChangeStatus
    │   ├── hash.rs         WorkdirHash
    │   ├── repo.rs         private git2 plumbing (tree building, commit, diff, write-back)
    │   └── trailers.rs     commit-message trailer encode/decode (pure)
    └── testing.rs          cfg(test | feature="test-util"): TestEnv, schema assertions
```

Cargo features:
- `default = ["async"]`
- `async`: enables `dep:tokio` and `Db::call`.
- `test-util`: exposes the `testing` module.

The crate root carries a temporary `#![allow(unused_variables, dead_code,
unused_imports)]` for the skeleton phase. Remove it once the bodies exist.

## 2. Errors (`error.rs`)

```rust
pub type Result<T, E = DataError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DataError {
    Io { path: PathBuf, source: std::io::Error },
    Json { path: PathBuf, source: serde_json::Error },
    UnsupportedDocumentVersion { path: PathBuf, found: u32, supported: u32 },
    InvalidSettings(String),
    Sqlite(#[from] rusqlite::Error),
    Migration { namespace: String, version: u32, kind: MigrationErrorKind },
    Git(#[from] git2::Error),
    Checkpoint(String),
    NoHomeDir,
    NotFound(String),
    Locked(PathBuf),
    TaskJoin(String),
}

#[non_exhaustive]
pub enum MigrationErrorKind {
    ChecksumMismatch { recorded: String, compiled: String },
    UnknownApplied,
    NonContiguous,
    BadNamespace,
    Sql(rusqlite::Error),
}

impl DataError {
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> DataError; // for map_err
    pub fn is_busy(&self) -> bool;  // SQLITE_BUSY / SQLITE_LOCKED after busy_timeout
}
```

## 3. Paths, file primitives, and JSON documents

```rust
pub struct ArthaPaths { root: PathBuf }
impl ArthaPaths {
    pub const ENV_HOME: &'static str = "ARTHA_HOME";
    pub fn discover() -> Result<Self>;                 // $ARTHA_HOME else ~/.artha
    pub fn with_root(root: impl Into<PathBuf>) -> Self; // no I/O
    pub fn root(&self) -> &Path;
    pub fn settings_file(&self) -> PathBuf;            // <root>/settings.json
    pub fn data_dir(&self) -> PathBuf;                 // <root>/data
    pub fn database_file(&self) -> PathBuf;            // <root>/data/artha.db
    pub fn sidecars_dir(&self) -> PathBuf;             // <root>/data/sidecars
    pub fn snapshot_root(&self) -> PathBuf;            // <root>/data/snapshot
    pub fn snapshot_dir(&self, h: &WorkdirHash) -> PathBuf;
    pub fn plugins_dir(&self) -> PathBuf;
    pub fn plugin_state_file(&self) -> PathBuf;        // <root>/plugins/state.json
    pub fn ensure_dirs(&self) -> Result<()>;           // root, data, plugins; 0700
    pub fn purge(&self) -> Result<PurgeReport>;        // refuses "/", $HOME, non-Artha dirs
}

pub enum FileMode { Private /*0600*/, Default }
pub fn atomic_write(path: &Path, bytes: &[u8], mode: FileMode) -> Result<()>;

pub struct FileLock { file: File, path: PathBuf }     // unlocks on Drop
impl FileLock {
    pub fn lock_path_for(target: &Path) -> PathBuf;    // "<target>.lock"
    pub fn acquire(target: &Path, timeout: Duration) -> Result<Self>; // Err(Locked) on timeout
    pub fn try_acquire(target: &Path) -> Result<Option<Self>>;
    pub fn path(&self) -> &Path;
}

pub trait VersionedDocument: Serialize + DeserializeOwned + Default {
    const CURRENT_VERSION: u32;
    const FILE_MODE: FileMode = FileMode::Default;
    fn upgrade(from_version: u32, raw: serde_json::Value) -> Result<serde_json::Value> { Ok(raw) }
    fn validate(&self) -> Result<()> { Ok(()) }
}

pub struct JsonStore<T> { path: PathBuf, lock_timeout: Duration, _doc: PhantomData<fn() -> T> }
impl<T: VersionedDocument> JsonStore<T> {
    pub const DEFAULT_LOCK_TIMEOUT: Duration;           // 5s
    pub fn new(path: impl Into<PathBuf>) -> Self;
    pub fn with_lock_timeout(self, t: Duration) -> Self;
    pub fn path(&self) -> &Path;
    pub fn load(&self) -> Result<T>;                    // missing => T::default()
    pub fn load_if_exists(&self) -> Result<Option<T>>;
    pub fn save(&self, doc: &T) -> Result<()>;
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R>; // lock+load+mutate+save
}
```

`atomic_write` works in five steps:
1. Create `NamedTempFile::new_in(parent)`.
2. `write_all`, then `sync_all`.
3. Apply the permissions.
4. `persist(path)`, which does a rename. On Windows this is
   `MoveFileEx(REPLACE_EXISTING)`, which tempfile handles.
5. On Unix, fsync the parent directory.

`load` works as follows:
1. Read the file.
2. Parse it into a `serde_json::Value`.
3. Read `version`. A missing value counts as 1.
4. If the version is greater than `CURRENT_VERSION`, return
   `UnsupportedDocumentVersion`.
5. If it is lower, run `upgrade`.
6. Run `from_value::<T>`, then `validate`.

`save` stamps the current version on every write.

**Plugin state.** `artha-plugins` declares the schema itself:

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct PluginState { pub version: u32, pub plugins: BTreeMap<String, PluginEntry>, pub marketplaces: Vec<MarketplaceSource> }
impl artha_data::VersionedDocument for PluginState { const CURRENT_VERSION: u32 = 1; }
let store = artha_data::JsonStore::<PluginState>::new(paths.plugin_state_file());
```

## 4. Settings (`settings/`)

### 4.1 Serde shape

```json
{
  "version": 1,
  "mode": "local",
  "llm": {
    "id": "anthropic-messages",
    "model": "my-local-model",
    "base_url": "http://127.0.0.1:8080",
    "api_key": "sk-..."
  },
  "session": {
    "token": "…",
    "email": "user@example.com",
    "expires_at": 1793750400000,
    "api_base": "https://api.example.io"
  },
  "preferences": {
    "log_level": "info",
    "permission_mode": "ask",
    "checkpoints_enabled": true
  }
}
```

Key behaviors:
- `llm` and `session` are omitted when `None`.
- All `preferences` fields default when absent.
- Unknown top-level keys are kept in `extra` (`#[serde(flatten)]`) and
  written back unchanged.
- The file is written with mode 0600 (`FILE_MODE = Private`).

### 4.2 Types

```rust
pub struct Settings {
    #[serde(default = "Settings::current_version")] pub version: u32,
    #[serde(default)] pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub llm: Option<LlmConnector>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub session: Option<CloudSession>,
    #[serde(default)] pub preferences: Preferences,
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>,
}
impl Settings { pub fn local_connector(&self) -> Option<&LlmConnector>; }
impl VersionedDocument for Settings { const CURRENT_VERSION: u32 = 1; const FILE_MODE: FileMode = FileMode::Private; fn validate(&self) -> Result<()>; }

#[serde(rename_all = "snake_case")] pub enum Mode { #[default] Local, Cloud }

pub struct LlmConnector {
    pub id: ConnectorKind,
    pub model: String,
    pub base_url: url::Url,
    #[serde(default)] pub api_key: Secret,
}
pub enum ConnectorKind {
    #[serde(rename = "anthropic-messages")] AnthropicMessages,
    #[serde(rename = "openai-chat")]        OpenAiChat,
}

pub struct CloudSession {
    pub token: Secret,
    pub email: Option<String>,
    pub expires_at: Option<Timestamp>,
    pub api_base: Option<url::Url>,
}

#[serde(default)]
pub struct Preferences { pub log_level: Option<String>, pub permission_mode: PermissionMode, pub checkpoints_enabled: bool /* default true */ }
#[serde(rename_all = "snake_case")] pub enum PermissionMode { #[default] Ask, Skip }

#[serde(transparent)] pub struct Secret(String);   // Debug => "Secret(***)"
impl Secret { pub fn new(v: impl Into<String>) -> Self; pub fn expose(&self) -> &str; pub fn is_empty(&self) -> bool; }

pub struct SettingsStore { inner: JsonStore<Settings> }
impl SettingsStore {
    pub fn new(paths: &ArthaPaths) -> Self;
    pub fn load(&self) -> Result<Settings>;
    pub fn save(&self, s: &Settings) -> Result<()>;
    pub fn update<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Result<R>;
    pub fn set_local_connector(&self, c: LlmConnector) -> Result<()>;  // also sets mode=Local
    pub fn set_cloud_session(&self, s: CloudSession) -> Result<()>;
    pub fn clear_cloud_session(&self) -> Result<()>;
    pub fn json_store(&self) -> &JsonStore<Settings>;
}
```

`validate` checks two things:
- `llm.model` is not empty.
- `base_url` uses http or https.

A missing `llm` in local mode is **valid**: it is the first-run state.
`artha-cli` reports it as "not configured", not as a corrupt file.

## 5. Database

### 5.1 `Db` (`db/mod.rs`)

```rust
pub struct DbOptions {
    pub busy_timeout: Duration,      // default 5s
    pub read_only: bool,             // default false; verifies migrations, never applies
    pub backup_before_migrate: bool, // default true
}

#[derive(Clone)] pub struct Db { inner: Arc<DbInner> }
struct DbInner { conn: Mutex<rusqlite::Connection>, path: Option<PathBuf> }

impl Db {
    pub fn open(path: &Path, opts: DbOptions, schemas: &[&'static MigrationSet]) -> Result<Self>;
    pub fn open_in_memory(schemas: &[&'static MigrationSet]) -> Result<Self>;
    pub fn path(&self) -> Option<&Path>;
    pub fn read <R, E: From<DataError>>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<R, E>) -> Result<R, E>; // DEFERRED
    pub fn write<R, E: From<DataError>>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<R, E>) -> Result<R, E>; // IMMEDIATE
    pub fn with_conn<R, E: From<DataError>>(&self, f: impl FnOnce(&mut Connection) -> Result<R, E>) -> Result<R, E>;
    #[cfg(feature = "async")]
    pub async fn call<R: Send + 'static, E: From<DataError> + Send + 'static>(
        &self, f: impl FnOnce(&Transaction<'_>) -> Result<R, E> + Send + 'static) -> Result<R, E>; // spawn_blocking(write)
    pub fn applied_migrations(&self) -> Result<Vec<AppliedMigration>>;
    pub fn health_check(&self) -> Result<Vec<String>>;  // integrity_check + foreign_key_check
    pub fn migrate_additional(&self, schemas: &[&'static MigrationSet]) -> Result<MigrationReport>;
}
```

### 5.2 Locking and concurrency details

- **One connection per process.** `std::sync::Mutex<Connection>` inside
  an `Arc`. Each `read`, `write`, or `call` holds the mutex for the length
  of one closure, so closures must not do network I/O or other slow work.
- **A poisoned mutex is recovered** with
  `PoisonError::into_inner`. The panicking closure's `Transaction` has
  already rolled back on drop.
- **`write` uses `TransactionBehavior::Immediate`.** It takes SQLite's
  RESERVED lock up front, so read-then-write logic (job claiming, dense
  `seq` assignment in `SessionStore::append`) is race-free across
  processes.
- **`read` uses `Deferred`.** In WAL mode this gives a consistent snapshot
  without blocking writers.
- **Cross-process waits are bounded by `busy_timeout`.** When it expires,
  the caller gets `DataError::Sqlite(SQLITE_BUSY)` and `is_busy()` returns
  true.
- **Async callers** use `call`. It clones the `Arc` into
  `spawn_blocking`. A `JoinError` becomes `DataError::TaskJoin`.
- **Why not a pool:** see HLD §5.5. If read latency under a long-running
  writer ever matters, the upgrade path is to add N read-only connections
  behind `read` and keep the single writer. The public API doesn't change.

### 5.3 Pragmas (`db/pragmas.rs`)

`journal_mode=WAL` (file-backed only), `synchronous=NORMAL`,
`foreign_keys=ON`, `busy_timeout=<opts>`, `temp_store=MEMORY`,
`trusted_schema=OFF`.

### 5.4 Migrations (`migrate/`)

```rust
pub struct Migration { pub version: u32, pub name: &'static str, pub sql: &'static str }
pub struct MigrationSet { pub namespace: &'static str, pub migrations: &'static [Migration] }
impl MigrationSet {
    pub const fn new(namespace: &'static str, migrations: &'static [Migration]) -> Self;
    pub fn latest_version(&self) -> u32;
    pub fn validate(&self) -> Result<()>;  // ns =~ [a-z][a-z0-9_]*, versions 1..=n contiguous
}
pub struct AppliedMigration { pub namespace: String, pub version: u32, pub name: String, pub checksum: String, pub applied_at: Timestamp }
pub struct MigrationReport { pub applied: Vec<(String, u32)>, pub backup: Option<PathBuf> }
pub static CORE_MIGRATIONS: MigrationSet;   // "core": 1 sessions, 2 checkpoint_links

pub(crate) struct Migrator<'a> { sets: Vec<&'a MigrationSet> }   // core first
impl<'a> Migrator<'a> {
    pub(crate) fn new(extra: &[&'a MigrationSet]) -> Result<Self>;
    pub(crate) fn pending(&self, conn: &Connection) -> Result<Vec<(&'a MigrationSet, &'a Migration)>>;
    pub(crate) fn run(&self, conn: &mut Connection, backup_to: Option<&Path>) -> Result<MigrationReport>;
    pub(crate) fn verify(&self, conn: &Connection) -> Result<()>;
}
pub(crate) fn checksum(sql: &str) -> String;  // lowercase hex SHA-256
```

**Numbering scheme.** Each namespace keeps its own counter, starting at 1,
with no gaps. Files are named
`crates/<owner>/migrations/<namespace>/NNNN_<name>.sql` (four digits).
The `name` field must equal the file's `<name>` part. A ledger row's
identity is `(namespace, version)`.

**The algorithm in `run`:**
1. `execute_batch(LEDGER_DDL)`.
2. Load the ledger.
3. For every ledger row, check that it is known to this binary
   (otherwise return `UnknownApplied`) and that its checksum matches
   (otherwise return `ChecksumMismatch`).
4. Compute the pending list.
5. If the list is empty, return.
6. If `backup_to` is set, run the SQLite online backup to
   `artha.db.bak-<ms>`.
7. For each pending migration:
   1. `BEGIN IMMEDIATE`.
   2. Re-check the ledger row. If another process already applied it,
      skip.
   3. `execute_batch(sql)`.
   4. Insert the ledger row.
   5. `COMMIT`.

**Ledger table:**

```sql
CREATE TABLE IF NOT EXISTS _artha_migrations (
    namespace   TEXT    NOT NULL,
    version     INTEGER NOT NULL,
    name        TEXT    NOT NULL,
    checksum    TEXT    NOT NULL,
    applied_at  INTEGER NOT NULL,
    PRIMARY KEY (namespace, version)
) STRICT;
```

### 5.5 Schema: `core` namespace (owned and shipped by this crate)

Tables are `STRICT`, timestamps are epoch-ms `INTEGER`, and JSON columns
are `TEXT CHECK (json_valid(...))`.

```sql
-- migrations/core/0001_sessions.sql
CREATE TABLE core_session (
    id          TEXT    PRIMARY KEY NOT NULL,              -- UUIDv7
    title       TEXT,
    workdir     TEXT    NOT NULL,
    mode        TEXT    NOT NULL CHECK (mode IN ('local', 'cloud')),
    provider    TEXT,
    model       TEXT,
    parent_id   TEXT    REFERENCES core_session(id) ON DELETE SET NULL,
    archived    INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    metadata    TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(metadata)),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;
CREATE INDEX core_session_by_workdir ON core_session (workdir, archived, updated_at DESC);

CREATE TABLE core_message (
    id             INTEGER PRIMARY KEY,
    session_id     TEXT    NOT NULL REFERENCES core_session(id) ON DELETE CASCADE,
    seq            INTEGER NOT NULL CHECK (seq >= 0),
    role           TEXT    NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    content        TEXT    NOT NULL CHECK (json_valid(content)),
    input_tokens   INTEGER,
    output_tokens  INTEGER,
    created_at     INTEGER NOT NULL,
    UNIQUE (session_id, seq)
) STRICT;

CREATE TABLE core_kv (
    namespace   TEXT    NOT NULL,
    key         TEXT    NOT NULL,
    value       TEXT    NOT NULL CHECK (json_valid(value)),
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (namespace, key)
) STRICT, WITHOUT ROWID;

-- migrations/core/0002_checkpoint_links.sql
CREATE TABLE core_checkpoint_link (
    commit_oid    TEXT    PRIMARY KEY NOT NULL,
    workdir_hash  TEXT    NOT NULL,
    session_id    TEXT    REFERENCES core_session(id) ON DELETE SET NULL,
    message_seq   INTEGER,
    tool_call_id  TEXT,
    kind          TEXT    NOT NULL CHECK (kind IN ('pre_edit', 'post_edit', 'pre_restore')),
    created_at    INTEGER NOT NULL
) STRICT;
CREATE INDEX core_checkpoint_link_by_session ON core_checkpoint_link (session_id, message_seq);
```

### 5.6 Proposed sibling schemas (owned and shipped by the sibling crates)

These are **not** compiled into `artha-data`. They show how the generic
mechanism covers the required domains, and each owner crate should start
from them. Every one follows the namespace prefix rule, and foreign keys
point only to their own namespace or to `core_*`.

```sql
-- artha-jobs: migrations/jobs/0001_jobs.sql
CREATE TABLE jobs_job (
    id            TEXT    PRIMARY KEY NOT NULL,           -- UUIDv7
    kind          TEXT    NOT NULL CHECK (kind IN ('cron', 'once', 'background')),
    goal          TEXT    NOT NULL,                       -- natural-language task
    schedule      TEXT,                                   -- cron expr for kind='cron'
    status        TEXT    NOT NULL CHECK (status IN ('active', 'paused', 'done', 'failed')),
    next_run_at   INTEGER,                                -- NULL = not scheduled
    session_id    TEXT    REFERENCES core_session(id) ON DELETE SET NULL,
    config        TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(config)),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;
CREATE INDEX jobs_job_due ON jobs_job (status, next_run_at);

CREATE TABLE jobs_run (
    id            INTEGER PRIMARY KEY,
    job_id        TEXT    NOT NULL REFERENCES jobs_job(id) ON DELETE CASCADE,
    state         TEXT    NOT NULL CHECK (state IN ('claimed', 'running', 'succeeded', 'failed', 'cancelled')),
    claimed_by    TEXT    NOT NULL,                       -- "<hostname>:<pid>" lease owner
    lease_until   INTEGER NOT NULL,                       -- reclaimable after this
    started_at    INTEGER NOT NULL,
    finished_at   INTEGER,
    error         TEXT,
    result        TEXT    CHECK (result IS NULL OR json_valid(result))
) STRICT;
CREATE INDEX jobs_run_by_job ON jobs_run (job_id, started_at DESC);

CREATE TABLE jobs_log (
    id         INTEGER PRIMARY KEY,
    run_id     INTEGER NOT NULL REFERENCES jobs_run(id) ON DELETE CASCADE,
    ts         INTEGER NOT NULL,
    level      TEXT    NOT NULL,
    message    TEXT    NOT NULL
) STRICT;

-- artha-tools (memory): migrations/memory/0001_memory.sql
CREATE TABLE memory_entry (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL CHECK (kind IN ('fact', 'preference', 'instruction')),
    content      TEXT    NOT NULL,
    tags         TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(tags)),
    source_session TEXT  REFERENCES core_session(id) ON DELETE SET NULL,
    importance   INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    last_used_at INTEGER
) STRICT;
CREATE VIRTUAL TABLE memory_entry_fts USING fts5(content, tags, content='memory_entry', content_rowid='id');
-- + memory_entry_ai / _ad / _au triggers keeping the FTS index in sync

-- artha-tools (portfolio): migrations/portfolio/0001_portfolio.sql
-- Decimal quantities/prices are canonical decimal TEXT, never REAL.
CREATE TABLE portfolio_account (
    id          TEXT    PRIMARY KEY NOT NULL,
    name        TEXT    NOT NULL UNIQUE,
    kind        TEXT    NOT NULL CHECK (kind IN ('paper', 'tracked')),  -- paper ledger vs. mirror of a real account
    base_ccy    TEXT    NOT NULL DEFAULT 'USD',
    created_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE portfolio_ledger (
    id          INTEGER PRIMARY KEY,
    account_id  TEXT    NOT NULL REFERENCES portfolio_account(id) ON DELETE CASCADE,
    ts          INTEGER NOT NULL,
    entry_type  TEXT    NOT NULL CHECK (entry_type IN ('buy', 'sell', 'deposit', 'withdraw', 'dividend', 'fee', 'split')),
    symbol      TEXT,                                   -- NULL for pure cash entries
    quantity    TEXT,                                   -- decimal
    price       TEXT,                                   -- decimal, in ccy
    amount      TEXT    NOT NULL,                       -- signed cash delta, decimal
    ccy         TEXT    NOT NULL,
    note        TEXT,
    session_id  TEXT    REFERENCES core_session(id) ON DELETE SET NULL
) STRICT;
CREATE INDEX portfolio_ledger_by_account ON portfolio_ledger (account_id, ts);
-- Positions are derived (SUM over ledger) in Rust, not stored, so the ledger stays append-only truth.

CREATE TABLE portfolio_watchlist (
    id          TEXT    PRIMARY KEY NOT NULL,
    name        TEXT    NOT NULL UNIQUE,
    created_at  INTEGER NOT NULL
) STRICT;
CREATE TABLE portfolio_watchlist_item (
    watchlist_id TEXT   NOT NULL REFERENCES portfolio_watchlist(id) ON DELETE CASCADE,
    symbol       TEXT   NOT NULL,
    note         TEXT,
    added_at     INTEGER NOT NULL,
    PRIMARY KEY (watchlist_id, symbol)
) STRICT, WITHOUT ROWID;

-- artha-tools (automation): migrations/monitor/0001_monitors.sql
CREATE TABLE monitor_definition (
    id           TEXT    PRIMARY KEY NOT NULL,
    name         TEXT    NOT NULL,
    condition    TEXT    NOT NULL CHECK (json_valid(condition)),  -- e.g. {"symbol":"AAPL","op":"price_below","value":"180"}
    action       TEXT    NOT NULL CHECK (json_valid(action)),     -- notify / run agent goal
    enabled      INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    cooldown_ms  INTEGER NOT NULL DEFAULT 0,
    job_id       TEXT,                                            -- jobs_job id that evaluates it; no FK (cross-sibling)
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;
CREATE TABLE monitor_event (
    id           INTEGER PRIMARY KEY,
    monitor_id   TEXT    NOT NULL REFERENCES monitor_definition(id) ON DELETE CASCADE,
    fired_at     INTEGER NOT NULL,
    observed     TEXT    NOT NULL CHECK (json_valid(observed)),
    acknowledged INTEGER NOT NULL DEFAULT 0 CHECK (acknowledged IN (0, 1))
) STRICT;
```

`monitor_definition.job_id` has no foreign key on purpose. The jobs table
belongs to a sibling namespace, so we can't rely on it existing or on
migration order (rule 4 in `migrate/mod.rs`). The automation tool
enforces the link in code.

### 5.7 Repositories (`store/`)

```rust
pub struct SessionId(pub uuid::Uuid);     // Display/FromStr/Serialize; ::new() = v7
pub type MessageSeq = u32;
pub enum Role { System, User, Assistant, Tool }   // fn as_str()

pub struct Session { id, title: Option<String>, workdir: PathBuf, mode: Mode, provider: Option<String>,
                     model: Option<String>, parent_id: Option<SessionId>, archived: bool,
                     metadata: serde_json::Value, created_at: Timestamp, updated_at: Timestamp }
pub struct NewSession { title, workdir, mode, provider, model, parent_id, metadata }
pub struct SessionSummary { session: Session, message_count: u32, preview: Option<String> }
pub struct SessionFilter { workdir: Option<PathBuf>, include_archived: bool, limit: u32 /* 50 */ }
pub struct StoredMessage { id: i64, session_id, seq: MessageSeq, role, content: serde_json::Value,
                           input_tokens: Option<u32>, output_tokens: Option<u32>, created_at }
pub struct NewMessage { role, content: serde_json::Value, input_tokens, output_tokens }

impl SessionStore {
    pub fn new(db: Db) -> Self;
    pub fn create(&self, new: NewSession) -> Result<Session>;
    pub fn get(&self, id: SessionId) -> Result<Option<Session>>;
    pub fn most_recent(&self, workdir: &Path) -> Result<Option<Session>>;   // --continue
    pub fn list(&self, f: &SessionFilter) -> Result<Vec<SessionSummary>>;   // --resume
    pub fn rename(&self, id: SessionId, title: Option<&str>) -> Result<()>;
    pub fn set_archived(&self, id: SessionId, archived: bool) -> Result<()>;
    pub fn delete(&self, id: SessionId) -> Result<()>;
    pub fn append(&self, id: SessionId, msgs: &[NewMessage]) -> Result<Vec<StoredMessage>>; // one IMMEDIATE tx
    pub fn messages(&self, id: SessionId) -> Result<Vec<StoredMessage>>;
    pub fn messages_after(&self, id: SessionId, after: MessageSeq) -> Result<Vec<StoredMessage>>;
    pub fn truncate_after(&self, id: SessionId, keep_through: MessageSeq) -> Result<u64>;
    pub fn fork(&self, id: SessionId, through: MessageSeq) -> Result<Session>;
}

impl KvStore {
    pub fn new(db: Db, namespace: impl Into<String>) -> Self;
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>>;
    pub fn set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<()>;
    pub fn delete(&self, key: &str) -> Result<bool>;
    pub fn keys(&self) -> Result<Vec<String>>;
}

pub struct CheckpointLink { checkpoint: CheckpointId, workdir_hash: WorkdirHash, session_id: Option<SessionId>,
                            message_seq: Option<MessageSeq>, tool_call_id: Option<String>,
                            kind: CheckpointKind, created_at: Timestamp }
impl CheckpointLinkStore {
    pub fn new(db: Db) -> Self;
    pub fn insert(&self, link: &CheckpointLink) -> Result<()>;   // INSERT OR IGNORE
    pub fn first_pre_edit_from(&self, s: SessionId, seq: MessageSeq) -> Result<Option<CheckpointLink>>;
    pub fn for_session(&self, s: SessionId) -> Result<Vec<CheckpointLink>>;
}
```

`append` assigns sequence numbers inside its transaction with
`SELECT COALESCE(MAX(seq), -1) + 1`. It is safe because `write` is
IMMEDIATE, and the `UNIQUE(session_id, seq)` constraint backstops it.

## 6. Checkpoint repository (`checkpoint/`)

### 6.1 API

```rust
pub struct WorkdirHash(String);   // hex(sha256(canonical path))[..16]
impl WorkdirHash { pub const LEN: usize = 16; pub fn of(p: &Path) -> Self; pub fn parse(s: &str) -> Result<Self>; pub fn as_str(&self) -> &str; }

pub struct CheckpointId(pub(crate) git2::Oid);   // Display = 40 hex; FromStr; fn short()
pub enum CheckpointKind { PreEdit, PostEdit, PreRestore }   // as_str(): pre_edit|post_edit|pre_restore
pub struct EditContext { pub session_id: Option<SessionId>, pub tool_call_id: Option<String>, pub tool_name: String, pub label: String }
pub struct Checkpoint { pub id, pub parent: Option<CheckpointId>, pub kind, pub label: String,
                        pub session_id: Option<SessionId>, pub tool_call_id: Option<String>, pub tool_name: Option<String>,
                        pub files: Vec<PathBuf>, pub skipped: Vec<PathBuf>, pub created_at: Timestamp }
pub struct CheckpointFilter { pub session_id: Option<SessionId>, pub path: Option<PathBuf>, pub limit: usize }
pub enum RestoreScope { All, Files(Vec<PathBuf>) }
pub struct RestoreReport { pub restored: Vec<PathBuf>, pub deleted: Vec<PathBuf>, pub safety_checkpoint: Option<CheckpointId> }
pub enum ChangeStatus { Added, Modified, Deleted }
pub struct FileChange { pub path: PathBuf, pub status: ChangeStatus, pub patch: Option<String> }
pub struct CheckpointDiff { pub files: Vec<FileChange> }

pub struct CheckpointStore { workdir: PathBuf, hash: WorkdirHash, git_dir: PathBuf, repo: Mutex<Option<git2::Repository>> }
impl CheckpointStore {
    pub const MAX_FILE_BYTES: u64 = 16 MiB;
    pub const REF_NAME: &str = "refs/heads/checkpoints";
    pub fn new(paths: &ArthaPaths, workdir: &Path) -> Result<Self>;   // canonicalize only; NO writes
    pub fn workdir(&self) -> &Path;  pub fn git_dir(&self) -> &Path;  pub fn workdir_hash(&self) -> &WorkdirHash;
    pub fn is_initialized(&self) -> bool;
    pub fn begin_edit(&self, ctx: EditContext, files: &[PathBuf]) -> Result<EditGuard<'_>>;  // "init" happens here, lazily
    pub fn record(&self, ctx: &EditContext, kind: CheckpointKind, files: &[PathBuf]) -> Result<Option<Checkpoint>>;
    pub fn list(&self, f: &CheckpointFilter) -> Result<Vec<Checkpoint>>;   // [] if uninitialized
    pub fn get(&self, id: &CheckpointId) -> Result<Option<Checkpoint>>;
    pub fn diff(&self, id: &CheckpointId) -> Result<CheckpointDiff>;
    pub fn restore(&self, id: &CheckpointId, scope: &RestoreScope) -> Result<RestoreReport>;
    pub fn destroy(self) -> Result<()>;
}

#[must_use] pub struct EditGuard<'a> { store, ctx, files, pre: Option<Checkpoint>, finished: bool }
impl EditGuard<'_> {
    pub fn pre_edit(&self) -> Option<&Checkpoint>;
    pub fn finish(self) -> Result<(Option<Checkpoint>, Option<Checkpoint>)>;   // (pre, post)
}
```

### 6.2 Mechanics

- **Init** (`repo::open_or_init`, which runs on the first `begin_edit`):
  - Call `Repository::init_opts` with `bare(true)` on `snapshot/<hash>`.
  - Set the config: `core.worktree=<workdir>`, `artha.workdir=<workdir>`,
    `core.autocrlf=false`, `gc.auto=0`.
  - On later opens, `set_workdir(workdir, false)`, then verify that
    `artha.workdir` equals the workdir. A mismatch is
    `DataError::Checkpoint`.
- **Path validation** (`repo::relativize`):
  - Canonicalize the parent directory and require it to be under the
    workdir, so symlinks can't escape.
  - Store paths with `/` separators.
  - Paths outside the workdir are rejected with an error, not skipped
    silently.
- **Recording** (`record`):
  1. Take the cross-process `FileLock(snapshot/<hash>)`.
  2. Set `base` to the tip tree.
  3. Build the new tree with `TreeUpdateBuilder`. For each requested
     path, store its current content as a blob (`upsert`), or `remove` it
     if the file no longer exists. Skip files larger than
     `MAX_FILE_BYTES`.
  4. If the new tree equals `base`, return `Ok(None)`.
  5. Otherwise, commit on `REF_NAME` with
     `Signature::now("Artha", "artha@localhost")` and the encoded
     trailers.
- **`begin_edit`** calls `record(PreEdit)` for the declared files. On a
  file's first touch it always records a commit, because the path isn't
  in the tree yet, and that captures the original content. A file that
  doesn't exist yet is recorded as absent, and restoring it later deletes
  it. **`finish`** calls `record(PostEdit)`.
- **Restore:**
  1. `record(PreRestore)` over the tracked paths in scope.
  2. For each path, write the blob from the target tree back to disk
     atomically (`fsio::atomic_write`), or delete the file if the target
     tree doesn't have it.
  3. Return the report.
- **Diff:** `diff_tree_to_workdir` with pathspecs set to the tracked
  paths, and a patch rendered per file. Binary files get
  `patch: None`.
- **Trailers** (`trailers.rs`):
  - The subject line is the label, flattened to one line and capped at
    72 characters.
  - The trailers are `Artha-Kind`, `Artha-Tool`, `Artha-Tool-Call`, and
    `Artha-Session`.
  - Decoding ignores unknown trailers.
- **Concurrency:** see HLD §5.5. A `git2::Repository` is `Send` but not
  `Sync`, which is why it sits behind a `Mutex` and makes
  `CheckpointStore` `Sync`.
- **Settings gate:** `artha-core` checks
  `preferences.checkpoints_enabled` before calling `begin_edit`. The store
  itself has no policy.

## 7. Testing strategy

**Pure unit tests** need no filesystem, so they are fast and run in
parallel:
- `trailers::encode` and `decode` round-trip: multiline labels, missing
  trailers, unknown trailers.
- `WorkdirHash::of` is stable: a golden value for a fixed path, the
  length and charset, and `parse` rejecting bad input.
- `Settings` serde:
  - golden JSON round-trip, including the exact connector strings
    `"anthropic-messages"` and `"openai-chat"`
  - unknown keys preserved in `extra`
  - defaults when keys are missing
  - `validate` rejecting an empty model or an `ftp://` URL
- `Secret` `Debug` redaction.
- `MigrationSet::validate`: gaps, duplicates, bad namespaces.
- `checksum` stability.
- `Role` / `CheckpointKind` string forms match the SQL `CHECK` lists.
  The test parses them out of `CORE_MIGRATIONS` SQL.

**In-memory SQLite** (`Db::open_in_memory`) needs no temp directory:
- Core migrations apply, and a second run is a no-op.
- Ledger contents are correct.
- `ChecksumMismatch`: open, then reopen with an altered static set.
- `UnknownApplied`: insert a fake ledger row.
- The `SessionStore` CRUD surface:
  - `append` assigns dense `seq`
  - `truncate_after`
  - `fork`
  - `most_recent` ordering
  - cascade delete
- `KvStore` round-trip.
- `CheckpointLinkStore::first_pre_edit_from`.
- `Db::write` rolls back when the closure returns `Err` or panics, and the
  connection stays usable afterwards (mutex poisoning is recovered).

**Temp-directory tests** use `testing::TestEnv` on `tempfile::TempDir`.
They never touch `ARTHA_HOME` or the real home directory:
- `atomic_write`: the target is never partially written, and the mode is
  0600 on Unix.
- `JsonStore`:
  - a missing file gives defaults
  - a future `version` gives `UnsupportedDocumentVersion`
  - `update` under contention: two threads with separate `JsonStore`s
    each increment a counter N times, and the final count is 2N
- `FileLock::try_acquire` returns `None` while the lock is held.
- File-backed `Db`:
  - WAL is enabled
  - two `Db` handles on the same file, which approximates two processes,
    with concurrent `write` calls: one waits, both succeed
  - with a short `busy_timeout` and a held IMMEDIATE transaction, the
    other handle's `is_busy()` returns true
  - a backup file is written before a pending migration
  - `read_only` with pending migrations returns an error
- `CheckpointStore`:
  - `new` creates nothing on disk
  - `list` on a fresh store is empty
  - `begin_edit` then `finish` produces two commits
  - a no-change `record` returns `None`
  - restore brings back the original content and deletes a file created
    later
  - restore records a `PreRestore` safety checkpoint
  - a path outside the workdir is rejected, including via a symlink
  - oversized files go into `skipped`
  - the user's own `.git` in the workdir is untouched (compare its
    `HEAD`/index mtime before and after)
  - `artha.workdir` mismatch detection
- `ArthaPaths::purge` refuses `/` and a directory without Artha markers.

**Sibling contract tests** use the `test-util` feature. Each owner crate
adds a dev-dependency on `artha-data` with `features = ["test-util"]` and
calls:
- `assert_objects_prefixed(&X_MIGRATIONS)`
- `assert_migrations_idempotent(&X_MIGRATIONS)`

`artha-cli` adds one test that opens a `Db` with *all* sets together, to
catch namespace collisions.

**Not tested here:** real multi-process cron-plus-TUI contention. That is
covered by an `artha-jobs` integration test that spawns the binary twice.
