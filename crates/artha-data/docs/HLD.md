# artha-data — High-Level Design

## 1. Purpose and scope

`artha-data` owns **where and how** Artha's local state is stored. It does
not own what most of that state means.

In scope:

- The `~/.artha/` layout, with an `$ARTHA_HOME` override (`ArthaPaths`).
- Crash-safe, lock-protected JSON documents (`JsonStore<T>`), used for
  `settings.json` and `plugins/state.json`.
- The `settings.json` model: mode, the local-LLM connector, the cloud
  session, and preferences.
- The shared SQLite database `data/artha.db`. This covers the connection
  handle, pragmas, and a **namespaced migration runner** that lets sibling
  crates ship their own tables.
- Repositories for the few tables this crate owns (`core_*`): chat
  sessions, messages, a generic key-value table, and checkpoint links.
- The per-workdir checkpoint repo `data/snapshot/<hash>/`: a private bare
  git repository used to undo agent file edits.
- `artha purge` support (`ArthaPaths::purge`) and health checks for
  `artha doctor`.

Out of scope:

- Job, memory, portfolio, ledger, and monitor *logic*, and the tables
  behind them. Those crates own their own tables and migrations.
- The plugin registry schema. `artha-plugins` owns it; see §5.3.
- Turning settings into live LLM providers (`artha-llm`) and applying
  CLI/env overrides (`artha-cli`). This crate never reads environment
  variables for settings values. `ARTHA_HOME` is the only one it reads.
- Python sidecar management (`artha-finance`). This crate only exposes
  `sidecars_dir()`.

## 2. On-disk layout

```
~/.artha/                         (0700)
├── settings.json                 (0600; holds api_key / session token)
├── settings.json.lock            advisory lock for read-modify-write
├── data/
│   ├── artha.db (+ -wal, -shm)   SQLite, WAL mode
│   ├── artha.db.bak-<ms>         written before applying pending migrations
│   ├── sidecars/                 owned by artha-finance
│   ├── snapshot/<hash>/          bare checkpoint repo, one per launch dir; created lazily
│   └── snapshot/<hash>.lock      cross-process commit lock
└── plugins/
    ├── state.json                schema owned by artha-plugins
    └── state.json.lock
```

## 3. Public API surface

| Item | Kind | Used by |
|---|---|---|
| `ArthaPaths` | struct | everyone; built once by `artha-cli` and passed down |
| `JsonStore<T>`, `VersionedDocument` | generic struct + trait | `artha-plugins`, plus `settings` internally |
| `atomic_write`, `FileLock`, `FileMode` | fns/struct | `artha-plugins`, `artha-finance` (sidecar files) |
| `Settings`, `LlmConnector`, `ConnectorKind`, `CloudSession`, `Preferences`, `Mode`, `PermissionMode`, `Secret` | serde model | `artha-cli`, `artha-llm` (by value), `artha-tui` |
| `SettingsStore` | struct | `artha-cli` |
| `Db`, `DbOptions` | struct | everyone who persists data |
| `Migration`, `MigrationSet`, `CORE_MIGRATIONS`, `AppliedMigration`, `MigrationReport` | types | sibling crates declare `static` sets |
| `SessionStore` (+ `Session`, `StoredMessage`, `NewSession`, `NewMessage`, `Role`, `SessionId`, …) | repository | `artha-core`, `artha-cli` (`-c`/`-r`) |
| `KvStore` | repository | anyone with small ad-hoc state |
| `CheckpointStore`, `EditGuard`, `EditContext`, `Checkpoint`, `CheckpointId`, `RestoreScope`, … | checkpoint API | `artha-core`, `artha-tools` (file-editing tools) |
| `CheckpointLinkStore` | repository | `artha-core` (rewind) |
| `Timestamp` | newtype (epoch ms) | everyone |
| `DataError`, `Result` | error | everyone |
| `rusqlite` (re-export) | crate | siblings that write SQL |
| `testing::{TestEnv, assert_objects_prefixed, assert_migrations_idempotent}` | feature `test-util` | sibling test suites |

## 4. How each sibling crate uses it

**`artha-cli`** is the composition root.
1. `ArthaPaths::discover()`, then `ensure_dirs()`.
2. `SettingsStore::new(&paths).load()`, after which CLI flags override
   values for this run.
3. `Db::open(&paths.database_file(), DbOptions::default(), &[&JOBS_MIGRATIONS,
   &MEMORY_MIGRATIONS, &PORTFOLIO_MIGRATIONS, &MONITOR_MIGRATIONS])`. It
   collects every crate's `MigrationSet` and opens the database **once per
   process**.
4. Hands clones of `Db` and `ArthaPaths` to the subsystems.
5. `artha purge` calls `ArthaPaths::purge()`. `artha doctor` calls
   `Db::health_check()` and `Db::applied_migrations()`, and opens with
   `read_only` unless `--fix` is given.

**`artha-core`**
- `SessionStore` creates sessions, appends a whole turn atomically,
  reloads history, and handles `most_recent(workdir)` for `--continue`.
- Message `content` is **opaque JSON**. Core serializes its own message
  type into it, so `artha-data` never depends on `artha-llm` or
  `artha-core` types.
- Builds one `CheckpointStore::new(&paths, &cwd)` per process. This does
  no I/O. Core shares it (via `Arc`) with file-editing tools, and records
  each resulting checkpoint in `CheckpointLinkStore` alongside the current
  `message_seq`.
- Rewind to message N works in four steps: look up
  `CheckpointLinkStore::first_pre_edit_from(session, N)`, call
  `CheckpointStore::restore`, call `SessionStore::truncate_after(N-1)`,
  and run everything through `spawn_blocking`.

**`artha-tools`**
- **memory** owns namespace `memory` (`memory_entry` + FTS5 index).
- **portfolio** owns namespace `portfolio` (accounts, watchlists, paper
  ledger).
- **automation** owns namespace `monitor`.
- Each exports a `MigrationSet` and a repository struct over `Db`, and
  calls `db.call(...)` from its async `Tool::execute`.
- **File-editing tools never call `checkpoints.begin_edit` directly** —
  that's `artha-core`'s policy (see its checkpointing section), driven by
  a tool declaring `ToolPlan.writes` up front and/or calling
  `ctx.edits().before_write(&paths)` for paths discovered during
  execution. Per `artha-tools`' own design, `report_render` is currently
  the only tool that writes a file at all.

**`artha-jobs`**
- Owns namespace `jobs`.
- `artha jobs tick` (which may be run from system cron while a TUI is
  open) claims due jobs inside `Db::write`. That uses `BEGIN IMMEDIATE`,
  so two processes cannot claim the same job.
- `DataError::is_busy()` lets a tick skip a cycle instead of failing.

**`artha-plugins`**
- Defines `PluginState: VersionedDocument` and uses
  `JsonStore::<PluginState>::new(paths.plugin_state_file())`, calling
  `update(|s| …)` for install, enable, and disable.

## 5. Key design decisions

### 5.1 rusqlite over sqlx

- **Embedded, single-file, low volume.** sqlx's strengths are async
  network drivers, connection pools, and compile-time query checking
  against a live database. None of those help with an embedded SQLite
  file holding a few thousand rows. sqlx's SQLite driver also runs
  blocking calls on a worker thread internally, so its async gains us
  nothing.
- **`bundled` feature.** SQLite compiles into the binary (the static
  binary goal), and we get a known version with FTS5 and JSON support
  regardless of the system library.
- **Direct access to SQLite features:** `BEGIN IMMEDIATE`, the backup
  API, `STRICT` tables, and pragmas.
- **Smaller dependency tree and faster builds.**

### 5.2 git2 for checkpoints

- It works in-process, with no dependency on a `git` binary (Windows,
  minimal containers).
- It gives us object-database-level control. We build trees from an
  explicit path list using `TreeUpdateBuilder`, never porcelain checkout,
  so we cannot touch files the agent didn't edit.
- It uses `default-features = false` (no network transports, no
  OpenSSL/libssh2) plus `vendored-libgit2` for a static build.
- We rejected `gix` for now. Its write paths (tree editing and commit
  creation) are less mature and its API surface churns. We can revisit
  this in the de-sidecar phase.

The checkpoint design rules:
- **Lazy creation.** Startup and idle never create the repo or commit
  anything. The repo is created on the first `begin_edit`.
- **Explicit tracking only.** We track only paths a tool has declared it
  will edit, so launching in `$HOME` doesn't snapshot gigabytes.
- **Separate from the user's repo.** The repo is a separate git dir and
  never touches the user's `.git`.
- **Self-describing.** It has a fixed author identity and stores its
  metadata in commit trailers.
- **Undoable restores.** A restore first takes a `PreRestore` safety
  snapshot, so the restore itself can be undone.

### 5.3 Plugin state: this crate owns file I/O, `artha-plugins` owns the schema

The hard, cross-cutting parts of `state.json` are:
- atomic replace,
- an advisory lock so `artha plugin install` and a running TUI don't lose
  each other's writes,
- 0600/0644 permissions,
- a version gate that refuses files written by a newer Artha,
- missing-file-means-default.

These are identical to the needs of `settings.json`, so they live once, in
`JsonStore<T>`. The *shape* of a plugin entry (manifest source, enabled
flag, marketplace list) changes with plugin features and belongs with the
code that interprets it.

Putting the schema here would force `artha-data` to change for every
plugin feature. Putting the I/O in `artha-plugins` would mean duplicating
crash-safety code. The dependency direction stays clean too:
`artha-plugins` depends on `artha-data`, never the reverse.
`ArthaPaths::plugin_state_file()` keeps the *location* centralized.

### 5.4 Migration strategy: namespaced, forward-only, checksummed

We hand-roll the runner (about 150 lines) instead of using
`rusqlite_migration` or `refinery`. Those tools assume one linear
migration list per database. We need one list **per owning crate** in a
shared file.

- Each crate exports `static X_MIGRATIONS: MigrationSet { namespace, migrations }`.
  The SQL is pulled in with `include_str!` from
  `crates/<crate>/migrations/<ns>/NNNN_name.sql`.
- A ledger table, `_artha_migrations(namespace, version, name, checksum,
  applied_at)`, records what has run.
- `core` always runs first. Sibling sets then run in the order the CLI
  passes them.
- Foreign keys may point only within the same namespace or into `core`.
- Each migration runs in its own `BEGIN IMMEDIATE` transaction together
  with its ledger row, and the ledger is re-checked inside that
  transaction. This makes concurrent first-starts safe.
- Startup refuses to proceed in two cases:
  - An applied migration's SHA-256 no longer matches the SQL compiled
    into the binary (someone edited a released migration).
  - The ledger contains migrations this binary doesn't know (the database
    was written by a newer Artha).
- There are no down migrations. Before any pending migration runs, we
  write an automatic backup (`artha.db.bak-<ms>`) using the SQLite backup
  API.
- Tables use the naming rule `<namespace>_<table>`. The helper
  `testing::assert_objects_prefixed` enforces this in each owner's tests.

### 5.5 Concurrency model: sync core, opt-in async adapter

- **The crate's API is synchronous.** Both rusqlite and libgit2 are
  blocking, and pretending otherwise just hides where the blocking
  happens.
- **Inside a process** there is one `rusqlite::Connection` behind a
  `std::sync::Mutex`, inside an `Arc` (`Db: Clone + Send + Sync`). Writes
  are tiny and rare, so a pool would add contention bugs and no
  throughput. A single connection also makes `BEGIN IMMEDIATE` semantics
  easy to reason about.
- **Across processes**, WAL mode and `busy_timeout = 5s` let a TUI,
  `artha jobs tick`, and `artha ask` share the file. Readers never block
  writers.
- **Under tokio:**
  - The `async` feature (default on) adds `Db::call(f)`, which runs the
    closure through `Db::write` on `tokio::task::spawn_blocking`.
  - Git operations are called the same way, through `spawn_blocking`.
  - No `artha-data` call ever runs on a reactor thread for long.
  - `--no-default-features` builds a tokio-free crate, for example for
    small tools or tests.
- **Poisoned mutexes are recovered.** A panic inside a closure drops the
  `Transaction`, which rolls back.
- **JSON documents.** `load` takes no lock because writes are atomic
  renames. `update` holds `FileLock` (`std::fs::File::lock`, Rust 1.89 or
  later).
- **Checkpoint repo.** An in-process `Mutex<Option<Repository>>` plus a
  cross-process `snapshot/<hash>.lock`.

### 5.6 Timestamps and identifiers

- **Timestamps** are `INTEGER` epoch milliseconds (UTC), wrapped in a
  `Timestamp` newtype. This keeps a datetime crate out of the data layer.
- **Session ids** are UUIDv7: time-sortable and unique without
  coordination across processes.
- **Monetary and quantity values** in sibling tables (portfolio/ledger)
  are stored as canonical decimal `TEXT`, never `REAL`, to avoid
  floating-point drift. The owning crate parses them with its decimal
  type.

### 5.7 Secrets

`Secret` is a newtype around a string:
- It is serialized verbatim, because the file is 0600 and must round-trip.
- Its `Debug` output is redacted, so secrets never end up in `tracing`
  logs.
- `expose()` is the only accessor.

`artha-llm` converts it to its own secret type at the boundary. Keychain
integration is a possible later step. `Settings.extra` and the
`#[serde(flatten)]` unknown-key capture keep the file forward-compatible.

## 6. Error-handling philosophy

- There is one `thiserror` enum, `DataError`. It is `#[non_exhaustive]`,
  so new variants are not breaking changes.
- Every I/O or JSON error carries the **path** it happened on, and
  migration errors carry `namespace#version`. This crate's errors surface
  directly in `artha doctor` output and in startup failures, and "permission
  denied" without a filename is useless there.
- No `anyhow` in this crate. Siblings wrap it with `#[from] DataError`.
  `Db::read`/`write`/`call` take a generic `E: From<DataError>`, so a
  sibling's closure can return its own error type.
- We fail loudly on things that indicate corruption or a version mismatch:
  checksum mismatch, a database from a newer build, a document version
  newer than supported, or a checkpoint repo whose `artha.workdir` doesn't
  match. We never silently "repair" these.
- Expected conditions are values, not errors:
  - A missing settings file means defaults.
  - An uninitialized checkpoint repo gives an empty list.
  - A no-op checkpoint returns `Ok(None)`.
  - `DataError::is_busy()` lets callers tell lock contention apart from
    real failures.

## 7. Dependencies

All versions below were checked against crates.io on 2026-11-04, except
`dirs`, which is flagged as unverified.

| Crate | Version | Features | Why |
|---|---|---|---|
| `rusqlite` | 0.41 | `bundled`, `backup`, `serde_json` | Embedded SQLite (see §5.1). `backup` for pre-migration snapshots. `serde_json` for JSON columns. |
| `git2` | 0.21 | default off, `vendored-libgit2` | Checkpoint repo (see §5.2). No network, static libgit2. Feature names should be re-checked against the 0.21 changelog when implementing. |
| `serde` / `serde_json` | 1 (workspace) | `derive` | Settings and document model. |
| `thiserror` | workspace (`1`) | — | `DataError`. 2.x is current; we follow the workspace pin, and the derive syntax is the same in both. |
| `tracing` | 0.1 (workspace) | — | Diagnostics (migration runs, dropped edit guards). |
| `tempfile` | 3.27 | — | `NamedTempFile::persist` for atomic writes. Also `TestEnv`. |
| `dirs` | 6 | — | Cross-platform home directory. **Unverified as latest.** |
| `sha2` | 0.11 | — | Workdir hash and migration checksums. |
| `uuid` | 1.29 | `v7`, `serde` | `SessionId`. |
| `url` | 2.5 | `serde` | Validated `base_url` / `api_base`. |
| `tokio` | 1 (workspace), optional | via the `async` feature | Only `spawn_blocking` in `Db::call`. |

Deliberately not used:
- **`sqlx`, `r2d2`/`deadpool`:** see §5.1 and §5.5.
- **`rusqlite_migration`, `refinery`:** see §5.4.
- **`chrono`/`time`:** see §5.6.
- **`fs2`/`fs4`:** `std` file locks are enough as of Rust 1.89.
- **`secrecy`:** its serde support doesn't cover `str`, and a 20-line
  newtype does the job.

The MSRV is **1.89**, because of `File::lock`.
