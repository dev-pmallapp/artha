# Artha (अर्थ) — Architecture Overview

Artha is a local-first, terminal-based AI finance assistant: a chat agent
that pulls live market data, runs the numbers, and watches markets for you,
built as a Rust multi-crate workspace. It can run entirely against a local
LLM endpoint (Anthropic- or OpenAI-compatible) or against a hosted backend —
local is a first-class mode, not an afterthought, which is the main reason
for a ground-up Rust implementation: a single static binary, no bundled
runtime, no companion native addon, no required subprocess sidecars.

## 1. Goals

- **Fully local-capable**: works end-to-end with a self-hosted/local LLM and
  local data sources, with no mandatory network dependency beyond the LLM
  call itself and whatever market-data source a given tool needs.
- **Single static binary** per platform (macOS arm64/x64, Linux x64, Windows
  x64) — no runtime, no native addon, minimal optional subprocesses.
- **Pluggable tools** over a clean `Tool` trait, loadable both as built-ins
  and via MCP servers, with a marketplace mechanism for community plugins.
- **Terminal-native UX**: a responsive TUI chat interface with syntax
  highlighting, streaming responses, and tool-call visibility.

## 2. Reference behavior (compatibility target)

The following is the known CLI/API/on-disk contract of comparable prior art
in this space, used here purely as a compatibility and scope reference —
i.e. "what a complete version 1 needs to cover," not a spec to copy
mechanically. Items marked **(unconfirmed)** are inferred from naming only
and need their own design investigation before being treated as settled.

**CLI surface:**

```
artha [message]                 start the TUI, optional initial prompt
artha plugin <list|install <spec>|uninstall <name>|enable|disable|marketplace <add|list|remove> [src]>
artha jobs <add <goal...>|list|run <id>|tick|logs <id>|pause <id>|resume <id>>
artha memory <tick|stats|model <name>|import|install>
artha ask <prompt>               headless single chat turn (same engine as the TUI)
artha verify [file]              static lookahead-bias check for a Python strategy file
artha doctor [--fix]             environment check: python, sidecars, helper binaries
artha purge                      delete all local Artha state

--log-level, --verbose, --quiet, -c/--continue, -r/--resume,
--skip-permissions/--yolo
```

**Backend surface (cloud mode):**

```
https://api.<project>.io
  /v1/auth/status
  /v1/sessions
  /v1/sessions/otp         email + one-time-code login
  /v1/sessions/mfa
  /v1/sessions/me
```

**Local-LLM connector config shape** (the contract any local-mode
implementation must satisfy):

```
{ id: "anthropic-messages" | "openai-chat", model: string, base_url: string, api_key: string }
```

→ exactly two wire formats to support for local mode; cloud mode is a third
implementation of the same contract, authenticated by session token instead
of a bearer key.

**On-disk state layout:**

```
~/.artha/settings.json          session + local-LLM config
~/.artha/data/artha.db          SQLite — sessions/jobs/memory/portfolio/etc.
~/.artha/data/sidecars/*.py     optional Python sidecars (see §5)
~/.artha/data/snapshot/<hash>/  bare git repo, checkpoint/undo over the cwd
                                 Artha was launched from
~/.artha/plugins/state.json     plugin registry
```

**Tool categories** (README-level scope, matching prior art): Markets, Macro
& economy, News & web, Research, Your data (portfolios/watchlists + a local
paper-trading ledger), Automation (monitors, cron schedules, background
research tasks), Memory (durable facts/preferences across sessions).

## 3. Target architecture

```
                         ┌───────────────────────────┐
                         │         artha-cli            │  (binary entry point)
                         │  clap-based arg parsing,     │
                         │  dispatches to subcommand    │
                         └──────────────┬───────────────┘
                                         │
        ┌────────────────┬──────────────┼───────────────┬────────────────┐
        ▼                ▼              ▼               ▼                ▼
   artha-tui        artha-core      artha-jobs      artha-plugins    (doctor/verify/
  (ratatui chat    (agent loop,     (cron +         (marketplace,     purge — thin,
   UI, drives the   session mgmt,   background      MCP plugin        non-interactive
   same core)       checkpoints)    task runner)     loading)          commands in artha-cli)
        │                │              │               │
        └────────┬───────┴──────┬───────┴───────┬───────┘
                 ▼               ▼               ▼
          artha-llm       artha-tools      artha-data
          (provider        (markets, macro,   (settings.json,
           trait: cloud     news, research,    SQLite via
           vs local          portfolio,        rusqlite, git2
           Anthropic/        automation,       checkpoint repo)
           OpenAI wire       memory — each
           formats)          a Tool impl)
                                 │
                        ┌────────┴─────────┐
                        ▼                  ▼
                artha-finance       artha-report
                (market data,       (typst — a native
                 fundamentals,       Rust crate, embed
                 technical           it directly instead
                 indicators)         of a subprocess)
```

### Crate layout (Cargo workspace)

| crate             | responsibility                                          | notes |
|-------------------|-----------------------------------------------------------|-------|
| `artha-cli`       | entry point, argument parsing, subcommand dispatch        | `clap` v4, derive API |
| `artha-tui`       | terminal chat UI, rendering, input handling                | `ratatui` + `crossterm` — no native rendering addon needed |
| `artha-core`      | agent/session orchestration, chat loop, tool dispatch       | owns streaming + tool-call sequencing |
| `artha-llm`       | LLM provider abstraction                                    | one trait, Anthropic + OpenAI wire-format impls, used identically for cloud and local modes |
| `artha-tools`     | built-in tool implementations                               | one module per tool category; each implements a `Tool` trait |
| `artha-finance`   | market/fundamentals/technical-indicator data access          | biggest open design question — see §5 |
| `artha-report`    | report rendering                                             | embed the `typst` crate directly — no bundled binary/packages dir |
| `artha-mcp`       | Model Context Protocol client (and possibly server)          | `rmcp` |
| `artha-plugins`   | plugin/marketplace management                                | git-based fetch of a marketplace index, manifest parsing, enable/disable state |
| `artha-jobs`      | scheduled + background agent tasks                           | `tokio-cron-scheduler` + a durable task queue in SQLite |
| `artha-data`      | settings file + SQLite access + checkpoint repo               | `serde`/`serde_json`, `rusqlite`, `git2` |
| `artha-highlight` | syntax highlighting                                          | native `tree-sitter` + per-language grammar crates, no WASM runtime |

### Key crate choices

| Concern                         | Crate                                              |
|----------------------------------|-----------------------------------------------------|
| CLI parsing                      | `clap` (derive)                                     |
| Config/schema validation         | `serde` + `validator` (or hand-rolled)              |
| Markdown rendering                | `pulldown-cmark` or `comrak`                        |
| TUI                               | `ratatui` + `crossterm`                             |
| Syntax highlighting               | `tree-sitter` + native grammar crates               |
| Plugin protocol                   | `rmcp` (MCP)                                        |
| Report rendering                  | `typst` (embedded, no subprocess)                   |
| SQLite                            | `rusqlite`                                          |
| Checkpoint/snapshot repo           | `git2` (libgit2 bindings)                           |
| HTTP client                       | `reqwest` (rustls)                                  |
| Async runtime                     | `tokio`                                             |
| Cron scheduling                   | `tokio-cron-scheduler`                              |

## 4. LLM provider abstraction

This is the part that actually delivers on "runs locally" — design and test
it first, via `artha ask`, before building the TUI on top of it:

```rust
#[async_trait]
trait LlmProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
    ) -> Result<impl Stream<Item = Result<ChatEvent>>>;
}

struct AnthropicMessages { base_url: Url, api_key: SecretString, model: String }
struct OpenAiChat        { base_url: Url, api_key: SecretString, model: String }
```

Cloud mode is a third implementation that proxies through the hosted backend
with a session token from the `/v1/sessions/*` flow instead of a bearer key —
same trait, same tool-calling contract, no special-casing needed elsewhere.

## 5. Open problem: the finance-data layer

There is no single mature Rust crate with broad market-data coverage
(quotes, fundamentals, macro series) comparable to Python's data-vendor
ecosystem. Three options, roughly in priority order:

1. **Optional Python sidecar** for the data-fetching half only, callable
   from `artha-finance` via a subprocess + JSON-over-stdio contract. Lowest
   risk, fastest parity, keeps a runtime dependency the "zero extra install"
   story doesn't love — but it's opt-in and `doctor`-reported, not required
   for the app to start or chat.
2. **Direct Rust HTTP clients** against the underlying data provider
   endpoints for the specific slices actually used (live quotes,
   fundamentals snapshots) via `reqwest` — more maintenance, no Python
   dependency.
3. **Swap to providers with real, stable APIs** (paid or freemium) for parts
   of the surface, keeping scraping-style access only where nothing else
   exists.

Recommendation: ship v1 with (1) for the data-fetching tools, track (2) as a
fast-follow per data source, and don't block the rest of the rewrite on it.

## 6. Checkpoint / snapshot feature

A pre-chat checkpoint mechanism: initialize a bare git repository under
`~/.artha/data/snapshot/<hash>/` with its worktree pointed at the directory
Artha was launched from, so agent-made file edits can be diffed/undone. In
Rust: `git2::Repository::init_bare`, then point the worktree at the launch
cwd, and wire actual commits into the file-editing tool path (nothing should
auto-commit on idle startup — only around actual edits).

## 7. Phased build plan

1. **Foundations** — `artha-cli` skeleton, `settings.json` read/write,
   `artha doctor`, `artha-llm` with both wire formats, `artha ask` as the
   first end-to-end path (no TUI, no tools — just prove the provider
   abstraction and streaming work).
2. **Core loop + TUI shell** — `artha-core` session/message state,
   `artha-tui` chat view wired to `ask`'s same code path, `artha-data`
   SQLite schema for sessions.
3. **Tools, batch 1** — markets/fundamentals (via the optional sidecar per
   §5), memory, portfolio/watchlist (local paper-trading ledger first — no
   cloud dependency).
4. **Automation** — `artha-jobs` (cron + background tasks), monitors.
5. **Plugins/MCP** — `artha-mcp` via `rmcp`, `artha-plugins`
   marketplace fetch/install/enable against this project's own marketplace
   index (hosted under this project's own GitHub account/org — not any
   third party's).
6. **Checkpointing** — `git2`-based snapshot repo, §6.
7. **De-sidecar pass** — `artha-report` via the embedded `typst` crate,
   native highlighting (no WASM grammars), revisit §5 option (2)/(3) for
   finance data, decide on computer-use and P2P-learnings scope (both
   **(unconfirmed)** — need their own design investigation before deciding
   a Rust approach for either).
8. **Cloud parity + cutover** — `/v1/sessions/*`-equivalent auth flow (if a
   hosted mode is wanted at all), shared test matrix across `doctor`/`ask`/
   `verify` once feature-complete.

## 8. Open questions

- **Finance data layer** — see §5; the single biggest open design question.
- **Computer-use capability** — **(unconfirmed)** whether/how this is in
  scope for v1; if it is, likely needs `enigo` (input injection) + a
  screenshot crate, but don't design this until requirements are nailed down.
- **P2P learnings sharing** — **(unconfirmed)** whether this is in scope;
  `libp2p` would be the natural crate if a peer-to-peer shared-learnings
  feature is wanted.
- **`artha verify`**'s "structurally verify a Python strategy file for
  lookahead bias" operates on *user* Python files, not app internals — needs
  either a Python AST parser (`rustpython-parser`) or a `python3 -c "import
  ast; ..."` subprocess call. Decide explicitly rather than defaulting
  silently.
- **Syntax-highlighting grammar coverage** — confirm a published Rust crate
  exists for every language actually needed (the reference tool covers bash,
  javascript, markdown, markdown_inline, powershell, python, typescript,
  zig) before assuming 1:1 native-crate coverage.
