# artha-llm — High-Level Design

## 1. Purpose and scope

`artha-llm` turns "a conversation plus a set of tools" into "a stream of
normalized model events", across every LLM backend Artha can talk to. It
is the part of Artha that makes local mode real, so it is designed and
tested first. The first end-to-end path is `artha ask`, which has no TUI
and no tools (ARCHITECTURE §4, §7 phase 1).

In scope:

- One object-safe provider trait, `LlmProvider`, and the provider-neutral
  conversation model it speaks: `Message`, `ContentBlock`, `ToolSpec`,
  `ChatRequest`, `ChatEvent`, `Usage`, `StopReason`.
- Two wire formats, each a pure **codec** (request encoder plus stream
  decoder) over one shared HTTP engine:
  - `anthropic-messages`: `POST {base}/v1/messages`, SSE with named
    events.
  - `openai-chat`: `POST {base}/chat/completions`, SSE with `data:`
    chunks.
- Three public providers built from those codecs:
  - `AnthropicMessages` and `OpenAiChat` authenticate with an API key
    (local mode, or any compatible endpoint).
  - `CloudProvider` authenticates with a session token supplied through a
    `TokenSource` seam (cloud mode).
- Tool-call assembly across streamed chunks. The final assistant
  `Message` is rebuilt inside this crate, so callers never reassemble
  JSON fragments.
- Retry with backoff, rate-limit handling, and timeouts, all *before* the
  first streamed byte. Classification of mid-stream errors.
- Usage (token) reporting on every completed turn.
- Secret hygiene for API keys and session tokens.
- Construction from `artha_data::LlmConnector` / `CloudSession`
  (feature `settings`, on by default).

Out of scope:

- **The agent loop.** Deciding to run a tool, executing it, permission
  prompts, and appending results all belong to `artha-core`. This crate
  never executes a tool.
- **Tool implementations and tool *naming policy*.** MCP name mangling,
  for example, belongs to `artha-tools` and `artha-mcp`. This crate only
  validates that names are legal on the wire.
- **The cloud login flow.** `/v1/sessions/*`, OTP, MFA, and token refresh
  belong to the auth owner (`artha-cli` today). This crate defines only
  the `TokenSource` trait it calls.
- **Persistence.** `Message` is `serde`-serializable with a stable shape
  so `artha-core` can store it as opaque JSON in `core_message.content`.
  This crate never touches SQLite.
- **Cost in currency.** Pricing is policy, and local models are free.
  This crate reports token counts. Converting them to money, if Artha
  ever does that, happens in `artha-core`.
- **Context-window management.** Truncation and summarization are
  `artha-core` concerns. This crate surfaces `ContextLengthExceeded` as a
  distinct error so core can react.
- **Tool calls that arrive as plain text**, as some local models produce.
  Not in v1. The codec seam leaves room for a text-protocol decoder later.

## 2. Public API surface

| Item | Kind | Used by |
|---|---|---|
| `LlmProvider` | trait (object-safe, `Send + Sync`) | `artha-core`, `artha-cli` (`ask`, `doctor`) |
| `EventStream` | `Pin<Box<dyn Stream<Item = Result<ChatEvent>> + Send>>` | `artha-core` |
| `ChatRequest`, `GenerationOptions`, `ToolChoice`, `ThinkingConfig`, `CacheHint` | request model | `artha-core` |
| `Message`, `Role`, `ContentBlock`, `ToolResultContent` | conversation model (serde) | `artha-core` (and persisted via `artha-data`) |
| `ToolSpec` | tool declaration (name, description, JSON schema) | `artha-tools` / `artha-mcp` produce it, core passes it through |
| `ChatEvent`, `BlockIndex`, `ToolCall`, `StopReason`, `Usage` | stream events | `artha-core`, and indirectly `artha-tui` |
| `AnthropicMessages`, `OpenAiChat`, `CloudProvider` | providers | `artha-cli` (composition root) |
| `ProviderConfig`, `WireFormat`, `HttpOptions`, `RetryPolicy`, `Timeouts`, `AnthropicCompat`, `OpenAiCompat` | configuration | `artha-cli` |
| `TokenSource`, `StaticToken`, `AuthFailure` | cloud-auth seam | auth owner (`artha-cli`) implements it |
| `build_provider`, `ProviderConfig::from_connector`, `CloudConfig::from_session` | factory (feature `settings`) | `artha-cli` |
| `ProviderInfo` | provider identity (format, model, base URL host) | `artha-core` (session `provider`/`model` columns), `doctor` |
| `LlmError`, `ErrorKind`, `Result` | errors | everyone above |
| `testing::{ScriptedProvider, Script}` | feature `test-util` | `artha-core` / `artha-tui` / `artha-jobs` tests |

## 3. How `artha-core` consumes it

The lifecycle of one user turn:

```
artha-core                                         artha-llm
──────────                                         ─────────
history: Vec<Message>  ← user text appended
loop {
  req = ChatRequest::new(&history).system(&sys).tools(&tools).options(opts)
  stream = provider.chat(&req).await?   ──────────▶ validate, encode body once,
                                                    POST with retries/backoff,
                                                    wait for 2xx headers
                                        ◀────────── Ok(EventStream)
  while let Some(ev) = stream.next().await {        each poll: read bytes → SSE
     TextDelta     → forward to TUI                  frames → codec decoder →
     ToolCallStart → TUI shows "calling get_quote…"  TurnAssembler → ChatEvent
     ToolCallDelta → (optional) show arg progress
     ToolCallEnd   → may start the tool early
     Usage         → live token counter
     MessageEnd { message, stop_reason, usage } → break
  }
  history.push(message)            // assembled assistant message, incl. tool_use blocks
  if stop_reason != ToolUse { break }
  results = run tools (artha-tools / artha-mcp, outside this crate)
  history.push(Message::tool_results(results))     // one user-role message
}
persist the turn via artha_data::SessionStore::append (content = serde_json of Message)
```

There is no "pause and resume" of a live stream. A tool call ends the
model's turn: Anthropic sends `stop_reason: "tool_use"` and OpenAI sends
`finish_reason: "tool_calls"`. The stream then finishes normally with
`MessageEnd`. "Resuming" means calling `chat` again with a history that
now holds the assistant's tool-use message and the user-role tool-result
message. Both wire protocols work this way. Each request is stateless and
carries the full history.

The stream is **pull-based**. Nothing is read from the socket until core
polls it, so a slow consumer applies natural TCP backpressure. Core may:

- **Execute a tool as soon as its `ToolCallEnd` arrives** while the
  stream continues, for example when the model emits two parallel calls.
  Its result is simply held until `MessageEnd`.
- **Drop the stream at any point**, for example on Ctrl-C or a TUI
  cancel. Dropping it closes the HTTP connection, which aborts generation
  server-side for both APIs and stops billing on hosted ones. There is no
  separate cancel API, and no background task is left running.
- **Move the stream into a task** (`EventStream: Send + 'static`) and
  forward events to the TUI over an `mpsc` channel. That is the
  recommended shape, because the TUI must keep rendering while the model
  streams.

On error, `chat` returns `Err` only after the retry policy is exhausted,
or immediately for an error that is not retryable. An `Err` item *inside*
the stream ends that stream. `LlmError::is_retryable()` tells core
whether re-issuing the same request is sensible. Core then discards the
partial output it showed and calls `chat` again. The crate never retries
mid-stream itself, because the user has already seen partial text and
only core knows how to reconcile the UI.

## 4. Key design decisions

### 4.1 The trait shape: `async fn chat(&self, &ChatRequest) -> Result<EventStream>`

```rust
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ChatEvent>> + Send + 'static>>;

#[async_trait]
pub trait LlmProvider: Send + Sync + fmt::Debug {
    fn info(&self) -> ProviderInfo;
    async fn chat(&self, request: &ChatRequest<'_>) -> Result<EventStream>;
}
```

`ChatRequest<'a>` borrows `&'a [Message]`, `&'a [ToolSpec]`, and
`Option<&'a str>` for the system prompt. Only the small
`GenerationOptions` is owned.

The sketch in ARCHITECTURE §4 returns `impl Stream` from an `async fn`.
We refined it in four ways.

1. **Object safety.** `artha-cli` picks the provider at runtime from
   `settings.json` (`mode`, `llm.id`). Everything above it holds an
   `Arc<dyn LlmProvider>`. A return-position `impl Trait` in a trait
   (including native `async fn`, as of Rust 1.89) makes the trait not
   dyn-compatible. So the stream is type-erased as
   `Pin<Box<dyn Stream + Send>>`, and the method's own future is boxed by
   `#[async_trait]`. Both are one allocation per turn, which is
   negligible next to an HTTP round trip. We considered three
   alternatives:
   - Generics everywhere (`Core<P: LlmProvider>`) would infect every
     crate's types with a provider parameter. It would also make the
     runtime choice an enum dispatch in `artha-cli` anyway.
   - An enum `AnyProvider { Anthropic, OpenAi, Cloud }` would close the
     set, so test doubles would need a variant of their own. `Arc<dyn>`
     keeps `ScriptedProvider` a peer.
   - A synchronous `fn chat(..) -> EventStream` with the connection made
     lazily on first poll. That loses the useful two-phase signal
     described in point 2.
2. **Two-phase result.** The outer `Result` means "the request was
   accepted": after retries, the server answered 2xx and streaming has
   begun. That separates errors before any output (retried internally,
   nothing shown yet) from errors mid-output (not retried internally,
   partial output already shown). The UI and the retry logic behave
   differently in those two cases, so the type makes the difference
   visible.
3. **`&ChatRequest<'_>` instead of `(&[Message], &[ToolSpec])`.** A turn
   needs more than messages and tools: the system prompt, `tool_choice`,
   `max_tokens`, sampling settings, a thinking budget, and cache hints.
   One struct keeps the trait stable as options grow, because adding a
   field is not a breaking change: the struct is `#[non_exhaustive]` and
   is built with a builder.
   The request is **borrowed**. The body is serialized to `Bytes` inside
   `chat` before it returns, so the returned stream borrows nothing
   (`'static`) and core never clones its history per turn. Retries
   reuse the same `Bytes`, which is a cheap clone.
4. **`Send + 'static` stream.** Core can `tokio::spawn` the consumer.

### 4.2 A provider-neutral conversation model, closest to content blocks

The internal `Message` is a list of typed `ContentBlock`s, with variants
`Text`, `ToolUse`, `ToolResult`, `Thinking`, and `RedactedThinking`, plus
a `Role` of `User | Assistant`. System prompts live on `ChatRequest`, not
in the history. We chose this shape because the content-block model is
the **superset** of the two formats:

- Anthropic's history *is* content blocks, so encoding is close to 1:1.
- OpenAI's history is a lossy projection of it:
  - assistant `content` plus `tool_calls[]`
  - one `role: "tool"` message per result
  - `system` as the first message
  - thinking dropped

Going the other way, from OpenAI's model to blocks, would lose ordering
and the thinking signatures that Anthropic requires to be echoed back
during tool use.

One history therefore works with either provider, and a user can switch
`llm.id` mid-session. The codecs handle what doesn't carry over:
- tool-call ids are sanitized deterministically for Anthropic's id
  charset
- thinking blocks are dropped for OpenAI
- missing ids from lax local servers are synthesized

`Message`'s serde shape is part of Artha's on-disk contract, because core
persists it. Changing it requires the same care as a migration: add
fields with defaults and never rename tags.

### 4.3 Codec plus shared engine, not two full clients

The two formats differ in body shape, endpoint, auth header, SSE
framing, and error-body shape. They do **not** differ in transport,
retry, timeouts, secrets, usage accounting, or turn assembly. So:

- `codec::anthropic` and `codec::openai` are **pure**, with no I/O:
  - `encode(model, &ChatRequest) -> Result<serde_json::Value>`, with
    the codec holding its own compat flags
  - a stateful `StreamDecoder` that turns `SseFrame`s into
    `Vec<WireEvent>`
  - `classify_error(status, headers, body) -> LlmError`
- `engine::HttpEngine` owns `reqwest::Client`, auth, retries, timeouts,
  the SSE byte decoder, and the shared `TurnAssembler`.
- `AnthropicMessages`, `OpenAiChat`, and `CloudProvider` are thin
  structs: an `HttpEngine`, a codec choice, and an auth source.

Cloud mode really is "same trait, same tool-calling contract, no
special-casing elsewhere". It reuses a codec (Anthropic Messages by
default, configurable) and swaps only how the `Authorization` header is
obtained and what a 401 means.

Pure codecs also make the hardest part, the streaming state machines,
testable without a network or a runtime. Fixture bytes can be fed in
split at every possible byte boundary.

### 4.4 The crate assembles tool calls; core sees whole calls

Tool arguments arrive as JSON *fragments*:
- Anthropic sends `input_json_delta.partial_json`.
- OpenAI sends `delta.tool_calls[i].function.arguments`.

Both need per-call buffers keyed by index, and both need a "this call is
complete" moment:
- Anthropic: `content_block_stop`.
- OpenAI: `finish_reason`, since it has no per-call stop.

The shared `TurnAssembler` does this once. It emits:
- `ToolCallDelta` events, for progress display,
- one `ToolCallEnd { call }` per call with fully parsed `input`,
- the full assistant `Message` in `MessageEnd`.

Malformed argument JSON is common with small local models. It is **not a
stream error**. The call is delivered with `ToolCall::input_error()` set
and the raw text preserved. Core can then answer with an `is_error` tool
result such as "your arguments were not valid JSON: …", and the model
gets a chance to correct itself instead of the whole turn failing.

### 4.5 Retries happen before the first byte, and are bounded and honest

- Retries happen only while no stream has been handed out. That covers
  connect failures, a timeout waiting for headers, and retryable statuses:
  408, 409, 425, 429, 500, 502, 503, 504, and 529 (overloaded).
- Exponential backoff with full jitter: base 500 ms, factor 2, cap 20 s,
  3 retries (4 attempts), and a total budget of 90 s.
- `Retry-After` (seconds or HTTP-date) and `retry-after-ms` are honored
  when present.
- If the server asks for longer than `max_retry_after` (60 s), we stop
  and return `RateLimited { retry_after }` immediately. The TUI can show
  "rate limited, retry in 3 min", and `artha-jobs` can reschedule.
  Silently sleeping for minutes is worse than either.
- An explicit `x-should-retry: true|false` response header overrides the
  status-based decision.
- Every retry is logged at `warn` with the attempt number, status, and
  request id, so `--verbose` shows why a turn is slow.

### 4.6 Timeouts are phase-specific, not one global deadline

A streaming response can legitimately last minutes, so `reqwest`'s
whole-request `timeout()` is wrong here. We use three independent
timeouts instead:

| Phase | Default | Why |
|---|---|---|
| connect | 10 s | TCP plus TLS. A dead host fails fast. |
| response headers (time to first byte of headers) | 180 s | Local servers may load a model or prefill a long prompt before replying. |
| stream idle (gap between successive bytes) | 120 s | Anthropic sends `ping` events. Local servers usually stream steadily once started. A gap this long means a stalled server. |

`total` is optional and off by default. `artha-jobs` sets it for
unattended runs. When `base_url` is a loopback address, the defaults for
headers and idle are doubled (`Timeouts::for_local`), because a local
GPU can be much slower than a hosted API without being broken.

### 4.7 Secrets

- API keys and session tokens are held as `secrecy::SecretString`, which
  zeroizes on drop and has a redacted `Debug`.
- They are converted from `artha_data::Secret` at the boundary
  (`ProviderConfig::from_connector`). This is the conversion
  artha-data's HLD §5.7 refers to.
- `ProviderConfig` and every provider implement `Debug` by hand and print
  `api_key: "***"`. A test asserts that the key's text never appears in
  `format!("{:?}")` of any public type.
- The `HeaderValue` built for `x-api-key` or `Authorization` is marked
  `set_sensitive(true)`, so `hyper` and `reqwest` debug output redact it.
- `tracing` fields never include headers. Request and response bodies,
  which hold the user's financial questions, are logged only at `trace`
  and only with `ARTHA_LOG_BODIES=1`.
- Error bodies returned by servers are truncated to 4 KiB before they go
  into `LlmError`, and they never contain our credentials.
- An empty `api_key` sends **no** auth header. Ollama, llama.cpp, and
  vLLM commonly need none, and some reject a dummy bearer token.

### 4.8 Usage is reported in-band, and cost is not computed here

Every turn ends with `MessageEnd { usage }`. Intermediate `Usage` events
are emitted when the wire provides them:
- Anthropic sends input tokens at `message_start` and cumulative output
  tokens in `message_delta`.
- OpenAI sends a final usage chunk only when
  `stream_options.include_usage` is set.

`Usage` fields are all `Option<u32>`: input, output, cache read, cache
write, and reasoning. Many local servers report nothing, and "unknown"
is not "zero". Core writes `input_tokens` and `output_tokens` into
`core_message`, which already has those columns. No callback or
"usage sink" trait is needed, because the event stream already reaches
the one party that cares.

### 4.9 Prompt caching is on by default for Anthropic

An agent loop resends the same system prompt and tool list every turn.
The Anthropic encoder places `cache_control: {"type":"ephemeral"}`
breakpoints on three places:
- the last tool,
- the system prompt,
- the last user message.

That is the standard three-breakpoint layout, which cuts input cost and
latency on every tool round trip. Some Anthropic-compatible local
servers reject unknown fields, so this can be turned off through
`AnthropicCompat::prompt_caching`. OpenAI caches automatically, so no
field is sent.

## 5. Dependencies

Versions were checked against crates.io on 2026-11-04. Feature names for
`reqwest` 0.13 must be re-checked against its changelog at implementation
time, because the TLS feature naming changed around 0.12 → 0.13.

| Crate | Version | Features | Why |
|---|---|---|---|
| `reqwest` | 0.13 | default off; rustls TLS, `stream`, `http2` | The HTTP client from ARCHITECTURE §3. rustls keeps the binary static (no OpenSSL). `bytes_stream()` gives a `Stream<Item = Bytes>`. Its connection pool is shared across turns. |
| `tokio` | 1 (workspace) | `time` | `sleep` for backoff, `timeout` for the header and idle phases. No `spawn`: the crate starts no tasks. |
| `futures-util` | 0.3 | `std` (no `async-await-macro` needed) | `Stream`, `StreamExt`, `stream::unfold`. |
| `bytes` | 1 | — | Zero-copy request body reused across retries, and the SSE input buffer. |
| `serde` / `serde_json` | 1 (workspace) | `derive` | Wire types and the persisted `Message` model. |
| `async-trait` | 0.1 (workspace) | — | Dyn-compatible `async fn chat` (see §4.1). |
| `thiserror` | workspace | — | `LlmError`. |
| `tracing` | 0.1 (workspace) | — | Per-request spans and retry warnings. |
| `secrecy` | 0.10 | — | `SecretString`. This crate never needs to *serialize* secrets, so artha-data's objection to `secrecy` (its serde coverage) doesn't apply here. |
| `url` | 2.5 | — | `base_url` handling and endpoint joining. |
| `httpdate` | 1 | — | `Retry-After: <HTTP-date>`. |
| `fastrand` | 2 | — | Backoff jitter. No need for `rand`'s weight or a cryptographic RNG. |
| `artha-data` | path, optional | via the `settings` feature (default on) | `LlmConnector`, `ConnectorKind`, `CloudSession`, `Secret` → `ProviderConfig`. |

Dev-only: `wiremock` 0.6 for the HTTP mock server, `tokio` with `macros`
and `rt-multi-thread`, and `insta` for snapshot tests of encoded request
bodies.

Deliberately not used:

- **`reqwest-eventsource` / `eventsource-client`.** These implement the
  browser `EventSource` model, which reconnects automatically and
  resends `Last-Event-ID`. That model is wrong for a non-idempotent `POST`
  that would regenerate, and re-bill, a completion.
- **`eventsource-stream`.** It's a reasonable fallback, but the SSE
  subset we need is about 120 lines. Owning it lets us handle quirks of
  local servers: missing `event:` lines, `:` keep-alive comments, CRLF,
  a final frame with no trailing blank line, and `data: [DONE]`. A pure
  incremental parser is also easy to fuzz and to test at every split
  point.
- **Provider SDK crates** (`async-openai`, Anthropic community crates).
  They model one vendor's full API surface, pin their own `reqwest` and
  `serde` shapes, and can't express "OpenAI-ish local server with quirks".
  We need about 5% of each API and full control over tolerance.
- **`backoff` / `tokio-retry`.** About 40 lines of policy code. The real
  logic is deciding *what* is retryable, which is ours either way.
- **`async-stream`.** `stream::unfold` over an explicit state struct keeps
  the state machine visible and avoids a proc-macro.

## 6. Error-handling philosophy

- **One `thiserror` enum, `LlmError`, `#[non_exhaustive]`.** Variants
  carry the HTTP status, the provider's error `type` string, the
  server's message (truncated), and the request id when available. That
  is what a user pasting `artha doctor` output needs.
- **Classification is part of the error, not left to the caller.**
  `LlmError::kind() -> ErrorKind` returns one of `Config`,
  `InvalidRequest`, `Auth`, `RateLimited`, `QuotaExceeded`, `Overloaded`,
  `ContextLength`, `Api`, `Network`, `Timeout`, `Protocol`, `Stream`.
  There is no `Cancelled`, because cancellation is a drop, not an error.
  Alongside `kind()`:
  - `is_retryable()`
  - `retry_after()`
  Core and jobs branch on these, never on message text.
- **Retryable versus not**:

  | Retryable | Not retryable |
  |---|---|
  | network and connect errors, header timeout, idle timeout | 400 bad request |
  | 408, 409, 425, 429 (except quota) | 401 and 403 auth; 429 `insufficient_quota` |
  | 500, 502, 503, 504, 529 | 404 (wrong `base_url` or model) |
  | Anthropic `overloaded_error` / `api_error` SSE events | 413 or context-length exceeded |
  | stream ended before its terminator (truncated) | 422 |
  | | local validation failures |
  | | malformed SSE or JSON in a 2xx stream |

  We treat malformed data in a 2xx stream as a server bug, so retrying
  won't help and would hide it.
- **Local validation happens before any network I/O.** These checks run
  first, so a bad request costs nothing:
  - tool names match `^[A-Za-z0-9_-]{1,64}$` and are unique
  - every `ToolUse` in history is answered by exactly one `ToolResult` in
    the next message
  - `max_tokens > 0`
  - the history is not empty and does not end with an assistant message
    that has unanswered tool calls
- **Context-length detection.** Both APIs report "prompt too long" as a
  400 with a recognizable error type or code:
  - Anthropic: `invalid_request_error` with a "prompt is too long"
    message.
  - OpenAI: `code: "context_length_exceeded"`.
  - Local servers: a variety of phrasings.
  The codecs map these to `ContextLengthExceeded` using a small
  provider-specific matcher, because core's compaction logic needs that
  signal.
- **Auth errors in cloud mode go back through the seam once.** On a 401,
  `CloudProvider` calls `TokenSource::on_unauthorized()`. If that returns
  `Refreshed`, the request is retried once with the new token, and this
  retry doesn't count against the backoff budget. Otherwise the error is
  `Auth { reauth_required: true }` and the CLI prompts for a login.
- **No `anyhow`, no panics on wire data.** Unknown SSE event types,
  unknown content-block types, and unknown `finish_reason`s are logged at
  `debug` and ignored (or mapped to `StopReason::Other(String)`). We do
  this for forward compatibility. Both APIs add event types over time,
  and an older Artha must keep working.

## 7. Concurrency model

- **Providers are immutable after construction and `Send + Sync`.** The
  CLI wraps one in `Arc<dyn LlmProvider>` and shares it with the TUI
  session, `artha ask`, and background jobs. Concurrent `chat` calls are
  independent. `reqwest::Client` is an `Arc` internally and pools
  connections, including HTTP/2 multiplexing to hosted APIs.
- **No spawned tasks.** All work happens inside the `chat` future, which
  covers connecting, retrying, and sleeping, or while the returned stream
  is polled. Dropping either one cancels everything, with no leaked
  tasks, no `CancellationToken` plumbing, and no shutdown ordering.
- **Optional per-provider concurrency cap.** `HttpOptions::max_concurrent`
  defaults to `None` for hosted APIs. Many local servers process one
  sequence at a time and queue or reject the rest, so when the config
  targets loopback, the factory sets it to `Some(1)`. An `OwnedSemaphorePermit`
  is acquired before the request and moved *into* the stream, so it is
  held until the stream ends or is dropped. A TUI turn and an
  `artha-jobs` monitor then queue politely instead of thrashing a local
  GPU.
- **Runtime requirement.** Callers must be inside a tokio runtime with
  the time driver enabled, for `reqwest`, `sleep`, and `timeout`. That is
  true of every Artha binary path.
- **Backpressure.** See §3. The stream is lazy and pull-driven, and only
  the SSE decoder's in-flight buffer is held in memory. That buffer is
  bounded: one frame over 8 MiB is a `Protocol` error, which protects
  against a misbehaving server.
