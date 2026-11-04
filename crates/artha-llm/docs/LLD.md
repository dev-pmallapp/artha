# artha-llm: Low-Level Design

This document is the companion to `HLD.md`. It describes the code to be
written. `src/` currently holds only a placeholder `lib.rs`, and none of
the files listed below exist yet.

## 1. Module layout

```
crates/artha-llm/
├── Cargo.toml
├── src/
│   ├── lib.rs               pub mods, flat re-exports, crate docs
│   ├── error.rs             LlmError, ErrorKind, TimeoutPhase, Result, body-truncation helper
│   ├── model/
│   │   ├── mod.rs           re-exports
│   │   ├── message.rs       Message, Role, ContentBlock, ToolResultContent, ToolResult (+ ctors)
│   │   ├── tool.rs          ToolSpec, ToolChoice, name validation
│   │   ├── request.rs       ChatRequest<'a> (+ builder), GenerationOptions, ThinkingConfig, CacheHint
│   │   ├── event.rs         ChatEvent, BlockIndex, ToolCall, MalformedInput, StopReason, Usage
│   │   └── validate.rs      validate_request(): tool names, history pairing, max_tokens
│   ├── provider/
│   │   ├── mod.rs           LlmProvider trait, EventStream, ProviderInfo, WireFormat, AuthMode
│   │   ├── config.rs        ProviderConfig, HttpOptions, Timeouts, RetryPolicy, AnthropicCompat,
│   │   │                    OpenAiCompat, AnthropicAuthHeader, MaxTokensField; manual Debug impls
│   │   ├── anthropic.rs     AnthropicMessages (struct + LlmProvider impl; thin)
│   │   ├── openai.rs        OpenAiChat (struct + LlmProvider impl; thin)
│   │   ├── cloud.rs         CloudProvider, CloudConfig
│   │   ├── auth.rs          TokenSource, StaticToken, RefreshOutcome, AuthFailure, Auth (crate-private)
│   │   └── factory.rs       cfg(feature="settings"): build_provider, BuildOptions, from_connector/from_session
│   ├── codec/
│   │   ├── mod.rs           Codec + StreamDecoder traits, WireEvent (all crate-private)
│   │   ├── endpoint.rs      endpoint joining rules for both formats
│   │   ├── anthropic/
│   │   │   ├── mod.rs       AnthropicCodec: endpoint, headers, classify_error
│   │   │   ├── wire.rs      serde wire types (request, SSE payloads, error body)
│   │   │   ├── encode.rs    ChatRequest → request JSON (history, tools, cache_control, id sanitizing)
│   │   │   └── decode.rs    AnthropicDecoder: SseFrame → WireEvent state machine
│   │   └── openai/
│   │       ├── mod.rs       OpenAiCodec: endpoint, headers, classify_error
│   │       ├── wire.rs      serde wire types (request, chunk, error body)
│   │       ├── encode.rs    ChatRequest → request JSON (message flattening, tool messages)
│   │       └── decode.rs    OpenAiDecoder: SseFrame → WireEvent state machine (tool_calls slots)
│   ├── stream/
│   │   ├── mod.rs           re-exports (crate-private)
│   │   ├── sse.rs           SseDecoder, SseFrame (pure incremental parser)
│   │   ├── assemble.rs      TurnAssembler: WireEvent → ChatEvent + final Message
│   │   └── pipeline.rs      Pipeline: bytes stream → SSE → decoder → assembler, idle/total timeouts
│   ├── engine/
│   │   ├── mod.rs           HttpEngine: build client, start(), headers, span
│   │   ├── retry.rs         RetryPolicy::delay_for, retry decision, Retry-After parsing
│   │   └── limit.rs         optional concurrency Semaphore wrapper
│   └── testing.rs           cfg(test | feature="test-util"): ScriptedProvider, Script, TurnScript
└── tests/
    ├── support/mod.rs       fixture loader, chunk-splitting helpers, StallingServer
    ├── fixtures/anthropic/*.sse  (+ *.expected.json)
    ├── fixtures/openai/*.sse     (+ *.expected.json)
    ├── decode_fixtures.rs   decoder + assembler tests over every fixture and split strategy
    ├── encode_snapshots.rs  insta snapshots of encoded bodies
    ├── http_anthropic.rs    wiremock: headers, endpoint, retries, errors
    ├── http_openai.rs       wiremock: same matrix
    ├── cloud_auth.rs        TokenSource refresh / reauth behavior
    ├── secrets.rs           Debug/log redaction assertions
    └── live.rs              #[ignore] tests against real endpoints (env-configured)
```

Cargo features:
- `default = ["settings"]`
- `settings`: enables `dep:artha-data` and `provider/factory.rs`.
- `test-util`: exposes `testing`. `ChatEvent` derives `Serialize` and
  `Deserialize` unconditionally, so fixtures and `artha-jobs` logs can
  use it.

Planned `Cargo.toml` dependencies (not written yet):

```toml
[dependencies]
artha-data   = { path = "../artha-data", optional = true, default-features = false }
async-trait  = { workspace = true }
bytes        = "1"
fastrand     = "2"
futures-util = { version = "0.3", default-features = false, features = ["std"] }
httpdate     = "1"
reqwest      = { version = "0.13", default-features = false, features = ["stream", "http2", "<rustls feature>"] }
secrecy      = "0.10"
serde        = { workspace = true }
serde_json   = { workspace = true }
thiserror    = { workspace = true }
tokio        = { workspace = true }          # uses only time + sync (Semaphore)
tracing      = { workspace = true }
url          = "2.5"

[dev-dependencies]
insta    = { version = "1", features = ["json"] }
tokio    = { workspace = true }
wiremock = "0.6"
```

The crate root denies `unsafe_code`.

## 2. Errors (`error.rs`)

```rust
pub type Result<T, E = LlmError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LlmError {
    /// Bad provider configuration (base_url scheme, empty model, unbuildable client).
    #[error("invalid LLM provider configuration: {0}")]
    Config(String),

    /// Rejected locally before any network I/O.
    #[error("invalid chat request: {0}")]
    InvalidRequest(String),

    /// 401/403, or a TokenSource failure.
    #[error("authentication failed{}: {message}", status_suffix(*status))]
    Auth { status: Option<u16>, message: String, reauth_required: bool, request_id: Option<String> },

    /// 429 (after retries, or with Retry-After above the cap).
    #[error("rate limited by provider: {message}")]
    RateLimited { retry_after: Option<Duration>, message: String, request_id: Option<String> },

    /// 429 with a quota/billing code: waiting will not help.
    #[error("provider quota exhausted: {message}")]
    QuotaExceeded { message: String, request_id: Option<String> },

    /// 503/529 or an `overloaded_error`, after retries.
    #[error("provider overloaded ({status}): {message}")]
    Overloaded { status: u16, message: String, request_id: Option<String> },

    /// Prompt plus max_tokens exceed the model's context window.
    #[error("context length exceeded: {message}")]
    ContextLengthExceeded { message: String, request_id: Option<String> },

    /// Any other non-2xx status.
    #[error("provider returned HTTP {status}{}: {message}", type_suffix(error_type))]
    Api { status: u16, error_type: Option<String>, message: String, request_id: Option<String> },

    /// DNS, connect, TLS, reset, or a body read error.
    #[error("network error talking to LLM provider: {message}")]
    Network { message: String, #[source] source: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("LLM request timed out ({phase:?} after {after:?})")]
    Timeout { phase: TimeoutPhase, after: Duration },

    /// A 2xx response whose body could not be understood (bad JSON, oversized frame, bad event order).
    #[error("malformed response from provider: {0}")]
    Protocol(String),

    /// An error event delivered inside a 2xx SSE stream.
    #[error("provider stream error ({error_type}): {message}")]
    Stream { error_type: String, message: String },

    /// The connection closed before the stream terminator (`message_stop` / `[DONE]` / `finish_reason`).
    #[error("provider stream ended unexpectedly")]
    Truncated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutPhase { Connect, ResponseHeaders, StreamIdle, Total }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    Config, InvalidRequest, Auth, RateLimited, QuotaExceeded, Overloaded,
    ContextLength, Api, Network, Timeout, Protocol, Stream,
}

impl LlmError {
    pub fn kind(&self) -> ErrorKind;
    /// Whether re-issuing the identical request may succeed (see the table below).
    pub fn is_retryable(&self) -> bool;
    pub fn retry_after(&self) -> Option<Duration>;
    pub fn request_id(&self) -> Option<&str>;
    pub fn status(&self) -> Option<u16>;
}

/// Truncates a server error body to 4 KiB on a char boundary, marking the cut with "…".
pub(crate) fn truncate_body(body: &[u8]) -> String;
```

`is_retryable()`:

| Variant | Retryable |
|---|---|
| `Network`, `Timeout { Connect \| ResponseHeaders \| StreamIdle }`, `Truncated` | yes |
| `Timeout { Total }` | no. The caller's budget is spent. |
| `RateLimited`, `Overloaded` | yes |
| `Api { status: 408 \| 409 \| 425 \| 500 \| 502 \| 504 }` | yes |
| `Stream { error_type: "overloaded_error" \| "api_error" \| "rate_limit_error" }` | yes |
| everything else | no |

The engine uses the same predicate internally, but applies it only to
errors that happen *before* a stream is returned (see §8.3).

## 3. Conversation model (`model/message.rs`, `model/tool.rs`)

### 3.1 Types

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role { User, Assistant }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContentBlock {
    Text { text: String },
    ToolUse {
        id: String,
        name: String,
        /// Always a JSON object. `{}` when the model's arguments were malformed.
        input: serde_json::Value,
        /// Set only when the model's argument text did not parse as a JSON object.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw_input: Option<String>,
    },
    ToolResult {
        tool_use_id: String,
        content: Vec<ToolResultContent>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
    /// Model reasoning. `signature` is opaque, Anthropic-only, and must be echoed back verbatim.
    Thinking {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    RedactedThinking { data: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolResultContent {
    Text { text: String },
    /// Structured result. Sent to the model as compact JSON text.
    Json { value: serde_json::Value },
}

/// Convenience input for building the tool-results message.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: Vec<ToolResultContent>,
    pub is_error: bool,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self;
    pub fn assistant_text(text: impl Into<String>) -> Self;
    /// One user-role message containing every result for the preceding assistant turn.
    pub fn tool_results(results: impl IntoIterator<Item = ToolResult>) -> Self;
    /// Concatenated `Text` blocks.
    pub fn text(&self) -> String;
    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &str, &serde_json::Value)>; // (id, name, input)
    pub fn has_tool_uses(&self) -> bool;
    pub fn is_tool_results(&self) -> bool;  // user message made only of ToolResult blocks
}

impl ToolResult {
    pub fn text(id: impl Into<String>, text: impl Into<String>) -> Self;
    pub fn json(id: impl Into<String>, value: serde_json::Value) -> Self;
    pub fn error(id: impl Into<String>, text: impl Into<String>) -> Self;
}
```

There is no `Role::System`. The system prompt is
`ChatRequest::system`, because both formats treat it specially:
- Anthropic: a top-level field.
- OpenAI: a leading message.

`artha-core` maps roles to `artha_data::Role` when persisting.
`is_tool_results()` messages are stored as `Role::Tool`, so `--resume`
listings read naturally. `content` is `serde_json::to_value(&message)`.

Images and documents are not in v1. `ContentBlock` is
`#[non_exhaustive]`, so they can be added later.

### 3.2 Persisted JSON shape (stable contract)

```json
{"role":"assistant","content":[
  {"type":"thinking","text":"Need the quote first.","signature":"EqQB…"},
  {"type":"text","text":"Let me check."},
  {"type":"tool_use","id":"toolu_01A","name":"get_quote","input":{"symbol":"AAPL"}}
]}
{"role":"user","content":[
  {"type":"tool_result","tool_use_id":"toolu_01A","content":[{"type":"json","value":{"price":"187.12"}}]}
]}
```

A golden-file test pins this shape. Changes may only add optional fields
or new variants.

### 3.3 Tools (`model/tool.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema. The root must be `{"type":"object", ...}`.
    pub input_schema: serde_json::Value,
}

impl ToolSpec {
    pub const MAX_NAME_LEN: usize = 64;
    pub fn new(name: impl Into<String>, description: impl Into<String>, input_schema: serde_json::Value) -> Result<Self>;
    /// Name matches `^[A-Za-z0-9_-]{1,64}$`; schema root is an object with `"type":"object"`.
    pub fn validate(&self) -> Result<()>;
    pub fn is_valid_name(name: &str) -> bool;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ToolChoice {
    #[default] Auto,
    /// Tools are declared, but the model must not call them this turn.
    None,
    /// The model must call some tool.
    Required,
    /// The model must call this tool.
    Tool(String),
}
```

Both providers' common name rule is the intersection, which is why the
validation is strict. `artha-mcp` is responsible for mapping names like
`server.tool` to a legal alias, and for mapping them back.

## 4. Request model (`model/request.rs`)

```rust
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ChatRequest<'a> {
    pub system: Option<&'a str>,
    pub messages: &'a [Message],
    pub tools: &'a [ToolSpec],
    pub tool_choice: &'a ToolChoice,
    pub options: &'a GenerationOptions,
}

impl<'a> ChatRequest<'a> {
    pub fn new(messages: &'a [Message]) -> Self;          // defaults: no system, no tools, Auto, &DEFAULT_OPTIONS
    pub fn system(self, s: &'a str) -> Self;
    pub fn tools(self, t: &'a [ToolSpec]) -> Self;
    pub fn tool_choice(self, c: &'a ToolChoice) -> Self;
    pub fn options(self, o: &'a GenerationOptions) -> Self;
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GenerationOptions {
    /// Required by Anthropic. Sent to OpenAI as max_tokens or max_completion_tokens.
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Vec<String>,
    pub thinking: Option<ThinkingConfig>,
    /// `Some(false)` asks for at most one tool call per turn.
    pub parallel_tool_calls: Option<bool>,
    pub cache: CacheHint,
    /// Overrides the provider's configured model for this one call (e.g. a cheaper model for job triage).
    pub model_override: Option<String>,
}
impl Default for GenerationOptions { /* max_tokens: 8192, cache: Auto, rest None/empty */ }
pub static DEFAULT_OPTIONS: LazyLock<GenerationOptions>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkingConfig { pub budget_tokens: u32 }   // must be < max_tokens; Anthropic requires >= 1024

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CacheHint {
    #[default] Auto,
    Off,
}
```

`ChatRequest` is `Copy`, made only of references, so one turn's request
costs nothing to build. `artha-core` keeps `history`, `tools`, and
`options` as owned fields of its session and borrows them for each call.

### 4.1 Validation (`model/validate.rs`)

`pub(crate) fn validate_request(req: &ChatRequest<'_>) -> Result<()>` runs
at the start of every `chat`, before any network I/O. It checks:

1. `messages` is not empty, and the first message is `User`.
2. `options.max_tokens > 0`. If `thinking` is set, `budget_tokens` must
   be less than `max_tokens`.
3. Every `ToolSpec::validate()` passes, and names are unique.
4. `ToolChoice::Tool(n)` names a declared tool. `Required` and `Tool`
   require tools to be non-empty.
5. **Pairing.** For every assistant message with `ToolUse` ids `{a, b}`,
   the next message must be a `User` message containing `ToolResult`s
   for exactly `{a, b}`. Extra text blocks are allowed. Orphaned
   `ToolResult`s are rejected.
6. The last message is not an assistant message with unanswered
   `ToolUse`. A trailing assistant text message is allowed. Anthropic
   treats it as a prefill and OpenAI as plain history.

A violation returns `LlmError::InvalidRequest` naming the message index,
for example `"messages[7]: tool_use toolu_01A has no tool_result in messages[8]"`.

## 5. Stream events (`model/event.rs`)

```rust
/// Dense index of a content block within the assistant message being assembled (0, 1, 2, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockIndex(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChatEvent {
    /// Always first. Fields are None when the server didn't say.
    MessageStart { id: Option<String>, model: Option<String> },
    TextDelta { index: BlockIndex, text: String },
    ThinkingDelta { index: BlockIndex, text: String },
    ToolCallStart { index: BlockIndex, id: String, name: String },
    /// Raw argument fragment, for progress display only. Do not parse it.
    ToolCallDelta { index: BlockIndex, partial_json: String },
    /// The call is complete and its arguments are parsed. Safe to execute.
    ToolCallEnd { index: BlockIndex, call: ToolCall },
    /// Running totals so far. Each field is replaced by the newer value when present.
    Usage(Usage),
    /// Always last on success. `message` is the complete assistant message to append to history.
    MessageEnd { stop_reason: StopReason, usage: Usage, message: Message },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Parsed JSON object. `{}` if malformed (see `malformed`).
    pub input: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub malformed: Option<MalformedInput>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MalformedInput { pub raw: String, pub error: String }

impl ToolCall {
    /// `Err` describes the problem in a form that can go straight into an `is_error` ToolResult.
    pub fn input_checked(&self) -> Result<&serde_json::Value, String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence(Option<String>),
    /// Anthropic `pause_turn`. Core should call `chat` again with the message appended.
    PauseTurn,
    Refusal,
    /// OpenAI `content_filter`.
    ContentFilter,
    Other(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
}
impl Usage {
    /// Field-wise: take `newer` where it is Some.
    pub fn merge(&mut self, newer: Usage);
    pub fn total(&self) -> Option<u32>;  // input + output when both known
}
```

**Event-order guarantees**, which tests assert for every fixture:
- `MessageStart` comes first, exactly once.
- For each tool block the order is `ToolCallStart`, then zero or more
  `ToolCallDelta`, then exactly one `ToolCallEnd`.
- `BlockIndex` values are assigned in increasing order of first
  appearance.
- The text or thinking deltas of different blocks may interleave only in
  the order the wire delivered them.
- `MessageEnd` comes last, exactly once on success.
- An `Err` item is always the final item. The stream is fused after it.
- `MessageEnd.message.content` equals the blocks implied by the deltas,
  in `BlockIndex` order, with empty text blocks removed.

## 6. Provider trait, configuration, auth seam

### 6.1 Trait (`provider/mod.rs`)

```rust
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ChatEvent>> + Send + 'static>>;

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync + fmt::Debug {
    fn info(&self) -> ProviderInfo;

    /// Validates, encodes, sends (with retries), and returns once a 2xx response's headers arrive.
    /// The returned stream owns everything it needs. Dropping it aborts the request.
    async fn chat(&self, request: &ChatRequest<'_>) -> Result<EventStream>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    pub format: WireFormat,
    pub model: String,
    /// Host only (no path, no credentials). Safe to display and log.
    pub host: String,
    pub auth: AuthMode,
    pub is_local: bool,       // base_url host is loopback
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireFormat { AnthropicMessages, OpenAiChat }
impl WireFormat { pub fn as_str(&self) -> &'static str; /* "anthropic-messages" | "openai-chat" */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode { None, ApiKey, Session }
```

`artha-core` writes `info().format.as_str()` and `info().model` into the
`core_session.provider` and `core_session.model` columns.

### 6.2 Configuration (`provider/config.rs`)

```rust
#[non_exhaustive]
pub struct ProviderConfig {
    pub format: WireFormat,
    pub model: String,
    pub base_url: Url,
    pub api_key: SecretString,   // empty ⇒ no auth header
    pub http: HttpOptions,
    pub anthropic: AnthropicCompat,
    pub openai: OpenAiCompat,
}
impl ProviderConfig {
    pub fn new(format: WireFormat, model: impl Into<String>, base_url: Url, api_key: SecretString) -> Self;
    /// Applies local-friendly defaults (Timeouts::for_local, max_concurrent = Some(1)) when base_url is loopback.
    pub fn with_defaults_for_host(self) -> Self;
}
impl fmt::Debug for ProviderConfig { /* api_key: "***" (or "<empty>"), base_url printed without userinfo */ }

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HttpOptions {
    pub timeouts: Timeouts,
    pub retry: RetryPolicy,
    pub max_concurrent: Option<usize>,
    pub user_agent: String,                       // "artha/<version>"
    /// Extra non-secret headers (e.g. a proxy route header). Rejected if the name is an auth header.
    pub extra_headers: Vec<(String, String)>,
    /// Max bytes of one SSE frame before a Protocol error.
    pub max_frame_bytes: usize,                   // 8 MiB
}

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub connect: Duration,            // 10s
    pub response_headers: Duration,   // 180s
    pub stream_idle: Duration,        // 120s
    pub total: Option<Duration>,      // None
}
impl Timeouts {
    pub fn for_local() -> Self;       // connect 5s, headers 360s, idle 240s
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,             // 3  (=> up to 4 attempts)
    pub base_delay: Duration,         // 500ms
    pub max_delay: Duration,          // 20s  (cap on computed backoff)
    pub max_retry_after: Duration,    // 60s  (larger server hints fail fast)
    pub budget: Duration,             // 90s  (wall clock across all attempts, incl. sleeps)
}
impl RetryPolicy {
    pub const NONE: RetryPolicy;      // max_retries 0
    /// Pure: delay before retry number `attempt` (1-based), given an optional server hint.
    pub fn delay_for(&self, attempt: u32, retry_after: Option<Duration>, jitter: f64 /* [0,1) */) -> Option<Duration>;
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AnthropicCompat {
    pub api_version: String,              // "2023-06-01" → `anthropic-version`
    pub beta: Vec<String>,                // joined into `anthropic-beta` if non-empty
    pub prompt_caching: bool,             // true
    pub auth_header: AnthropicAuthHeader, // XApiKey
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnthropicAuthHeader { XApiKey, Bearer, Both }

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OpenAiCompat {
    pub stream_usage: bool,               // true → stream_options.include_usage
    pub max_tokens_field: MaxTokensField, // MaxTokens (widest local-server support)
    pub send_parallel_tool_calls: bool,   // true; some local servers reject the field → false
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxTokensField { MaxTokens, MaxCompletionTokens }
```

`is_local` (and `with_defaults_for_host`) treats `localhost`,
`127.0.0.0/8`, `::1`, and `*.localhost` as loopback. Private-range IPs,
such as a GPU box on the LAN, are *not* treated as local for timeouts.
Users who want that can set the timeouts explicitly, so the defaults
stay predictable.

### 6.3 Providers

```rust
pub struct AnthropicMessages { engine: HttpEngine, codec: AnthropicCodec }
pub struct OpenAiChat        { engine: HttpEngine, codec: OpenAiCodec }

impl AnthropicMessages { pub fn new(cfg: ProviderConfig) -> Result<Self>; }  // Err(Config) if format != AnthropicMessages
impl OpenAiChat        { pub fn new(cfg: ProviderConfig) -> Result<Self>; }

#[async_trait]
impl LlmProvider for AnthropicMessages {
    fn info(&self) -> ProviderInfo { self.engine.info(WireFormat::AnthropicMessages) }
    async fn chat(&self, req: &ChatRequest<'_>) -> Result<EventStream> { self.engine.start(&self.codec, req).await }
}
// OpenAiChat: identical shape with OpenAiCodec.
```

Both have hand-written `Debug` impls that print format, model, and host,
and never the engine's auth.

### 6.4 Cloud mode seam (`provider/cloud.rs`, `provider/auth.rs`)

```rust
#[async_trait]
pub trait TokenSource: Send + Sync + fmt::Debug {
    /// Current bearer token for the hosted backend. Called once per attempt (cheap; implementations cache).
    async fn token(&self) -> Result<SecretString, AuthFailure>;
    /// The backend rejected `rejected` with 401. Refresh if possible.
    async fn on_unauthorized(&self, rejected: &SecretString) -> Result<RefreshOutcome, AuthFailure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome { Refreshed, ReauthRequired }

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct AuthFailure { pub message: String, pub reauth_required: bool }

/// A fixed token (from `settings.json` `session.token`). `on_unauthorized` → ReauthRequired.
pub struct StaticToken(SecretString);
impl StaticToken { pub fn new(t: SecretString) -> Self; }

#[non_exhaustive]
pub struct CloudConfig {
    /// The backend's wire-compatible LLM base. The codec's normal endpoint joining is applied to it.
    /// Placeholder default: `{api_base}/v1/llm/` until the backend contract is fixed.
    pub endpoint_base: Url,
    pub format: WireFormat,        // AnthropicMessages by default
    pub model: String,
    pub http: HttpOptions,
    pub anthropic: AnthropicCompat,
    pub openai: OpenAiCompat,
}

pub struct CloudProvider { engine: HttpEngine, codec: AnyCodec }
impl CloudProvider { pub fn new(cfg: CloudConfig, tokens: Arc<dyn TokenSource>) -> Result<Self>; }
// LlmProvider impl: engine.start(&self.codec, req); info().auth == AuthMode::Session

pub(crate) enum AnyCodec { Anthropic(AnthropicCodec), OpenAi(OpenAiCodec) }  // enum dispatch, no dyn

pub(crate) enum Auth {
    None,
    ApiKey { key: SecretString },                 // header placement decided by the codec
    Session { source: Arc<dyn TokenSource> },     // always `Authorization: Bearer <token>`
}
```

In session mode, a 401 is handled inside the engine's attempt loop:
1. Call `on_unauthorized(&token_used)`.
2. If that returns `Refreshed`, run one more attempt immediately. It
   doesn't count against `max_retries` and is allowed only once per
   `chat`.
3. Otherwise the result is `Auth { reauth_required: true }`.

`TokenSource` errors map to `Auth { status: None, .. }`. How the token is
obtained (OTP, MFA, refresh endpoint) is entirely the implementor's
business.

### 6.5 Factory (`provider/factory.rs`, feature `settings`)

```rust
impl ProviderConfig {
    /// `LlmConnector { id, model, base_url, api_key }` → config with host-appropriate defaults.
    pub fn from_connector(c: &artha_data::LlmConnector) -> Self;   // Secret → SecretString via expose()
}
impl From<artha_data::ConnectorKind> for WireFormat { /* AnthropicMessages | OpenAiChat */ }

#[derive(Default)]
#[non_exhaustive]
pub struct BuildOptions {
    pub model_override: Option<String>,                // CLI flag
    pub http: Option<HttpOptions>,                     // e.g. artha-jobs sets Timeouts.total
    pub token_source: Option<Arc<dyn TokenSource>>,    // cloud; defaults to StaticToken(session.token)
    pub cloud_format: Option<WireFormat>,
    pub cloud_model: Option<String>,
    pub default_api_base: Option<Url>,                 // used when session.api_base is None
}

/// Mode::Local → AnthropicMessages | OpenAiChat from `settings.llm`.
/// Mode::Cloud → CloudProvider from `settings.session`.
/// Missing connector or session → Err(Config("no LLM configured; …")).
pub fn build_provider(settings: &artha_data::Settings, opts: BuildOptions) -> Result<Arc<dyn LlmProvider>>;
```

## 7. Wire-format mapping

### 7.1 Endpoints (`codec/endpoint.rs`)

Let `p` be `base_url.path()` with trailing `/` trimmed. The query string
is preserved.

| Format | Rule | `base_url` → endpoint |
|---|---|---|
| anthropic | `p` ends with `/v1` → `p + "/messages"`. Otherwise `p + "/v1/messages"`. | `https://api.anthropic.com` → `/v1/messages`; `http://127.0.0.1:8080/v1` → `/v1/messages`; `https://proxy/x` → `/x/v1/messages` |
| openai | `p` ends with `/chat/completions` → use it as is. `p` is empty → `/v1/chat/completions`. Otherwise `p + "/chat/completions"`. | `http://localhost:11434/v1` → `/v1/chat/completions`; `http://localhost:8000` → `/v1/chat/completions`; `https://host/openai/v1` → `/openai/v1/chat/completions` |

A URL with userinfo (`https://user:pass@…`) is rejected with `Config`,
because credentials belong in `api_key`. So are schemes other than
`http` and `https`.

### 7.2 Headers

| | anthropic-messages | openai-chat | cloud |
|---|---|---|---|
| auth | `x-api-key: <key>` (or `Authorization: Bearer`, per `AnthropicAuthHeader`) | `Authorization: Bearer <key>` | `Authorization: Bearer <session token>` |
| empty key | no auth header | no auth header | n/a |
| version | `anthropic-version: 2023-06-01`, plus `anthropic-beta` if configured | — | as per `format` |
| common | `content-type: application/json`, `accept: text/event-stream`, `user-agent: artha/<ver>` | same | same |
| request id read from | `request-id` | `x-request-id` | both, first found |

Every auth `HeaderValue` is built with `HeaderValue::from_str(secret.expose_secret())?`
followed by `.set_sensitive(true)`.

### 7.3 anthropic-messages request (`codec/anthropic/encode.rs`)

```json
{
  "model": "<model or options.model_override>",
  "max_tokens": 8192,
  "stream": true,
  "system": [{"type":"text","text":"<system>","cache_control":{"type":"ephemeral"}}],
  "messages": [ … ],
  "tools": [{"name":"get_quote","description":"…","input_schema":{…},"cache_control":{"type":"ephemeral"}}],
  "tool_choice": {"type":"auto","disable_parallel_tool_use":false},
  "temperature": 0.2, "top_p": 0.9, "stop_sequences": ["…"],
  "thinking": {"type":"enabled","budget_tokens": 4096}
}
```

Encoding rules:
- **`system`** is omitted when `None`. It is sent as a single text
  block, so it can carry `cache_control`.
- **`tools` and `tool_choice`** are both omitted when `tools` is empty.
- **`ToolChoice` mapping:**

  | `ToolChoice` | `tool_choice` |
  |---|---|
  | `Auto` | `{"type":"auto"}` |
  | `None` | `{"type":"none"}` |
  | `Required` | `{"type":"any"}` |
  | `Tool(n)` | `{"type":"tool","name":n}` |

  `parallel_tool_calls: Some(false)` adds `"disable_parallel_tool_use": true`.
- **`thinking`.** When set, `temperature` and `top_p` are dropped, with
  a `debug` log, because the API rejects them together.
  `tool_choice` `Required` and `Tool` are downgraded to `Auto`, with a
  `warn` log, because forced tool use is incompatible with thinking.
- **`cache_control`** (when `prompt_caching && options.cache == Auto`) is
  placed on three things:
  - the last tool,
  - the system block,
  - the **last content block of the last user message**.

  Those are three of the four allowed breakpoints.
- **History**, one wire message per `Message` after these steps:
  1. **Merge** adjacent messages with the same role. This can't arise
     from valid history but is cheap insurance.
  2. **Order** a user message's blocks with `tool_result` blocks first,
     then the rest. The API requires that order.
  3. **Drop** empty `Text` blocks. **Drop** `Thinking` blocks with
     `signature: None`. Those originate from an OpenAI-compatible
     reasoning stream and can't be verified.
  4. **Sanitize ids** in `tool_use.id` and `tool_result.tool_use_id` with
     `sanitize_id`. Characters outside `[A-Za-z0-9_-]` become `_`, and
     an empty id becomes `"artha_empty"`. The function is deterministic,
     so pairs still match.

Block mapping:

| `ContentBlock` | Anthropic wire block |
|---|---|
| `Text{text}` | `{"type":"text","text"}` |
| `ToolUse{id,name,input,..}` | `{"type":"tool_use","id","name","input"}`. `input` is always an object; `raw_input` is not sent. |
| `ToolResult{tool_use_id,content,is_error}` | `{"type":"tool_result","tool_use_id","content":[{"type":"text","text"}…],"is_error"}`. `Json{value}` → a text block with compact JSON. `is_error` is omitted when false. |
| `Thinking{text,signature:Some(s)}` | `{"type":"thinking","thinking":text,"signature":s}` |
| `RedactedThinking{data}` | `{"type":"redacted_thinking","data"}` |

### 7.4 anthropic-messages stream (`codec/anthropic/decode.rs`)

Recognized SSE payloads. The decoder dispatches on the JSON `type` field,
not on the `event:` line, so proxies that drop `event:` lines still work.

| `type` | Payload used | → `WireEvent` |
|---|---|---|
| `message_start` | `message.id`, `message.model`, `message.usage{input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens}` | `Start{id,model}` + `Usage` |
| `content_block_start` | `index`, `content_block{type:"text",text}` | `BlockOpen{block, Text}`, plus `TextDelta` if `text` is non-empty |
| | `content_block{type:"tool_use",id,name,input}` | `ToolStart{block,id:Some,name,initial_input:Some(input)}` |
| | `content_block{type:"thinking",thinking}` | `BlockOpen{block, Thinking}`, plus `ThinkingDelta` if non-empty |
| | `content_block{type:"redacted_thinking",data}` | `RedactedThinking{block,data}` |
| | any other block type (server tools, etc.) | ignored. Its `index` is marked *skipped* and later deltas for it are dropped. |
| `content_block_delta` | `delta{type:"text_delta",text}` | `TextDelta` |
| | `delta{type:"input_json_delta",partial_json}` | `ToolArgs{block,fragment}` |
| | `delta{type:"thinking_delta",thinking}` | `ThinkingDelta` |
| | `delta{type:"signature_delta",signature}` | `Signature{block,signature}` |
| | other (`citations_delta`, …) | ignored |
| `content_block_stop` | `index` | `BlockStop{block}` |
| `message_delta` | `delta{stop_reason,stop_sequence}`, `usage{output_tokens,…}` (cumulative) | `Usage` + `StopReason(..)` (recorded, not final) |
| `message_stop` | — | `Done` |
| `ping` | — | nothing |
| `error` | `error{type,message}` | `Err(LlmError::Stream{error_type,message})` |
| unknown | — | `debug!` and ignored |

`stop_reason` mapping:

| wire value | `StopReason` |
|---|---|
| `end_turn` | `EndTurn` |
| `tool_use` | `ToolUse` |
| `max_tokens` | `MaxTokens` |
| `stop_sequence` | `StopSequence(stop_sequence)` |
| `pause_turn` | `PauseTurn` |
| `refusal` | `Refusal` |
| other | `Other(s)` |

**Decoder states:**

```
 AwaitStart ──message_start──▶ InMessage ──message_delta(stop_reason)──▶ Stopping ──message_stop──▶ Done
     │                          │  ▲  content_block_* / ping / message_delta(usage only)
     │                          └──┘
     └─ any other content event before message_start ⇒ Protocol("event before message_start")
 EOF in AwaitStart | InMessage | Stopping ⇒ Truncated
 EOF in Done ⇒ ok
 error event in any state ⇒ Err(Stream{..}), decoder → Failed
```

Some compatible servers send `message_stop` without a `message_delta`.
In that case `stop_reason` defaults to `EndTurn`, or to `ToolUse` if the
message contains a tool block; see §9.

### 7.5 openai-chat request (`codec/openai/encode.rs`)

```json
{
  "model": "<model>",
  "stream": true,
  "stream_options": {"include_usage": true},
  "max_tokens": 8192,
  "messages": [
    {"role":"system","content":"<system>"},
    {"role":"user","content":"What's AAPL at?"},
    {"role":"assistant","content":"Let me check.","tool_calls":[
      {"id":"call_1","type":"function","function":{"name":"get_quote","arguments":"{\"symbol\":\"AAPL\"}"}}]},
    {"role":"tool","tool_call_id":"call_1","content":"{\"price\":\"187.12\"}"},
    {"role":"user","content":"and MSFT?"}
  ],
  "tools": [{"type":"function","function":{"name":"get_quote","description":"…","parameters":{…}}}],
  "tool_choice": "auto",
  "parallel_tool_calls": true,
  "temperature": 0.2, "top_p": 0.9, "stop": ["…"]
}
```

Encoding rules:
- **`system`** becomes the first message `{"role":"system"}`.
- **`stream_options`** is sent only if `OpenAiCompat::stream_usage` is
  set.
- **`max_tokens`** is sent under the field name from `max_tokens_field`.
- **`tools`, `tool_choice`, and `parallel_tool_calls`** are omitted when
  there are no tools. `parallel_tool_calls` is also omitted when
  `send_parallel_tool_calls == false` or the option is `None`.
- **`ToolChoice` mapping:**

  | `ToolChoice` | `tool_choice` |
  |---|---|
  | `Auto` | `"auto"` |
  | `None` | `"none"` |
  | `Required` | `"required"` |
  | `Tool(n)` | `{"type":"function","function":{"name":n}}` |
- **`thinking`** is ignored in v1 (logged at `debug`). OpenAI-style
  reasoning controls vary by server and are not standardized enough to
  map.
- **Cache hints** are ignored. Prefix caching is automatic.

History flattening, applied to each `Message` in order:

| Internal | OpenAI wire |
|---|---|
| `User` with only `Text` blocks | one `{"role":"user","content": texts.join("\n\n")}` |
| `User` with `ToolResult` blocks (± text) | one `{"role":"tool","tool_call_id":id,"content":s}` **per result**, in block order, **then** a user message with the text blocks if any. `s` is the joined text parts, with `Json{value}` as compact JSON. `is_error` prefixes `"Error: "`. |
| `Assistant` | one `{"role":"assistant","content": text-or-null, "tool_calls": [...]}` with `tool_calls` omitted if there are none. `content` is `null` (not `""`) when there is no text and there are tool calls. `Thinking`/`RedactedThinking` are dropped. |
| `ToolUse{id,name,input,raw_input}` | `{"id","type":"function","function":{"name","arguments": raw_input.unwrap_or(to_string(input))}}`. A malformed call is echoed back exactly as the model produced it. |

Tool-call ids are passed through unchanged. OpenAI accepts any string.

### 7.6 openai-chat stream (`codec/openai/decode.rs`)

Each SSE `data:` payload is one of:
- `[DONE]`, which ends the stream.
- A chunk:
  `{"id","model","choices":[{"index":0,"delta":{"role"?,"content"?,"reasoning_content"?|"reasoning"?,"refusal"?,"tool_calls"?:[{"index"?,"id"?,"type"?,"function":{"name"?,"arguments"?}}]},"finish_reason"?}],"usage"?:{"prompt_tokens","completion_tokens","prompt_tokens_details":{"cached_tokens"},"completion_tokens_details":{"reasoning_tokens"}}}`
- An in-stream error: `{"error":{"message","type"?,"code"?}}`, which
  becomes `Err(Stream{..})`.

The decoder keeps this state:

```rust
pub(crate) struct OpenAiDecoder {
    state: OaState,                     // AwaitFirst | Streaming | Finished{reason} | Done | Failed
    next_block: u32,                    // dense block allocator
    text_block: Option<u32>,            // currently open text block
    reasoning_block: Option<u32>,
    last_opened: Option<u32>,           // to decide whether new text needs a new block
    slots: Vec<ToolSlot>,               // indexed by slot key
    key_by_wire_index: HashMap<u32, usize>,
}
struct ToolSlot {
    block: u32,
    id: Option<String>,
    name: String,
    started: bool,                      // ToolStart emitted (needs a non-empty name)
    pending_args: String,               // args seen before the name arrived
    closed: bool,
}
```

Per chunk:
1. **First chunk** (in `AwaitFirst`): emit `Start{id, model}`, then move
   to `Streaming`.
2. Use only `choices[0]`. Other choices are ignored, because `n` is
   never sent.
3. **`delta.content`** (non-empty):
   1. If `text_block` is `None`, or `last_opened != text_block`, which
      means a tool block was opened after the text, allocate a new block.
   2. Emit `TextDelta`.
4. **`delta.reasoning_content` or `delta.reasoning`**: the same as
   content, but for the reasoning block, producing `ThinkingDelta`.
5. **`delta.refusal`**: treated as text, with a recorded flag. If no
   other stop is known, the stop reason becomes `Refusal`.
6. **`delta.tool_calls[*]`**, for each item:
   1. **Resolve the slot key.**
      - If `index` is present, look it up in `key_by_wire_index`, or
        create a new slot.
      - If `index` is absent and `id` is present and differs from the
        last slot's id, create a new slot.
      - If `index` is absent otherwise, use the last slot. Some local
        servers omit `index`.
   2. On a new slot, allocate a block.
   3. **Record id and name.**
      - If `id` is present and the slot has none, record it. Later ids
        are ignored, since some servers repeat them.
      - If `function.name` is present and the slot has not started,
        append it. Once started, further `name` fields are ignored,
        since some servers resend the full name on every chunk.
   4. If `function.arguments` is present: if the slot has started, emit
      `ToolArgs`, otherwise append it to `pending_args`.
   5. If the slot has not started and `name` is non-empty:
      1. Emit `ToolStart{block, id, name, initial_input: None}`.
      2. If `pending_args` is non-empty, flush it as `ToolArgs`.
      3. Mark the slot started.
7. **`finish_reason`** (non-null): close every open block. That means
   `BlockStop` for text, reasoning, and each started slot. A slot that
   never got a name is a `Protocol("tool call without name")` error.
   Record the reason, then move to `Finished`. Usage may still follow.
8. **`usage`** (any chunk, typically the final one with `choices: []`):
   emit `Usage{input: prompt_tokens, output: completion_tokens,
   cache_read: cached_tokens, reasoning: reasoning_tokens}`.

The chunk's `type: "function"` field, when present, is checked and
otherwise ignored.

`finish_reason` mapping:

| wire value | `StopReason` |
|---|---|
| `stop` | `EndTurn` |
| `length` | `MaxTokens` |
| `tool_calls` / `function_call` | `ToolUse` |
| `content_filter` | `ContentFilter` |
| other | `Other(s)` |

**Termination:**

| Event | In state | Result |
|---|---|---|
| `[DONE]` | `Finished` | `Stop(reason)`, then `Done` |
| `[DONE]` | `Streaming` (no `finish_reason`, seen with lax servers) | close open blocks, `Stop(EndTurn)`, `Done`, with a `debug` log |
| EOF | `Finished` (server omitted `[DONE]`) | `Stop(reason)`, then `Done` |
| EOF | `AwaitFirst` or `Streaming` | `Err(Truncated)` |
| data after `Done` | — | ignored |

### 7.7 Error bodies (`classify_error`)

Each codec reads up to 64 KiB of a non-2xx body, bounded by
`response_headers` timeout, parses it leniently, and builds an
`LlmError`:

| Condition | anthropic-messages | openai-chat (and lax local servers) |
|---|---|---|
| body shape | `{"type":"error","error":{"type","message"}}` | `{"error":{"message","type","code","param"}}`, `{"error":"text"}`, `{"detail":"text"}`, or plain text |
| 401/403 | `Auth` | `Auth` |
| 429 | `RateLimited{retry_after}` | `QuotaExceeded` if `code`/`type` is `insufficient_quota`, else `RateLimited` |
| 503/529, `overloaded_error` | `Overloaded` | `Overloaded` (503) |
| context too long | 400/413 + `invalid_request_error` / `request_too_large` + message contains one of the substrings below | 400 + `code == "context_length_exceeded"`, or message contains one of the substrings below (local servers) |
| other | `Api{status, error_type, message}` | `Api{..}` |

The context-length matcher lowercases the message and checks it against
a fixed list of substrings. It uses no regex dependency:
- `"prompt is too long"`
- `"context length"`
- `"context_length"`
- `"context window"`
- `"maximum context"`
- `"too many tokens"`
- `"exceeds the context"`

## 8. Engine (`engine/`)

### 8.1 Construction

```rust
pub(crate) struct HttpEngine {
    client: reqwest::Client,
    endpoint: Url,               // computed once via codec::endpoint
    model: String,
    auth: Auth,
    opts: HttpOptions,
    limiter: Option<Arc<tokio::sync::Semaphore>>,
    info: ProviderInfo,
}

impl HttpEngine {
    pub(crate) fn new(format: WireFormat, base: &Url, model: String, auth: Auth, opts: HttpOptions) -> Result<Self>;
    pub(crate) fn info(&self) -> ProviderInfo;
    pub(crate) async fn start<C: Codec>(&self, codec: &C, req: &ChatRequest<'_>) -> Result<EventStream>;
}
```

The `reqwest::Client` is built with:
- `connect_timeout(timeouts.connect)`
- `pool_idle_timeout(90s)`
- `tcp_keepalive(30s)`
- `http2_adaptive_window(true)`
- the rustls TLS backend
- **no** whole-request `timeout()`
- **no** automatic redirects for POST (`redirect::Policy::none()`)

A 3xx is surfaced as `Api` with a hint to fix `base_url`. Following it
would silently drop the auth header.

### 8.2 Codec traits (`codec/mod.rs`)

```rust
pub(crate) trait Codec: Send + Sync {
    type Decoder: StreamDecoder;
    fn format(&self) -> WireFormat;
    fn endpoint(&self, base: &Url) -> Result<Url>;
    fn static_headers(&self, h: &mut HeaderMap);                       // version, beta, accept, content-type
    fn auth_headers(&self, key: &SecretString, h: &mut HeaderMap) -> Result<()>; // ApiKey mode only
    fn encode(&self, model: &str, req: &ChatRequest<'_>) -> Result<serde_json::Value>;
    fn new_decoder(&self) -> Self::Decoder;
    fn classify_error(&self, status: StatusCode, headers: &HeaderMap, body: &[u8]) -> LlmError;
}

pub(crate) trait StreamDecoder: Send + 'static {
    fn on_frame(&mut self, frame: &SseFrame, out: &mut Vec<WireEvent>) -> Result<()>;
    /// Called at EOF. Returns Err(Truncated) if the terminator was not seen, else pushes Stop/Done.
    fn on_eof(&mut self, out: &mut Vec<WireEvent>) -> Result<()>;
}

/// Normalized, provider-neutral stream vocabulary. `block` is the decoder's dense block id.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WireEvent {
    Start { id: Option<String>, model: Option<String> },
    BlockOpen { block: u32, kind: OpenKind },            // Text | Thinking
    TextDelta { block: u32, text: String },
    ThinkingDelta { block: u32, text: String },
    Signature { block: u32, signature: String },
    RedactedThinking { block: u32, data: String },
    ToolStart { block: u32, id: Option<String>, name: String, initial_input: Option<serde_json::Value> },
    ToolArgs { block: u32, fragment: String },
    BlockStop { block: u32 },
    Usage(Usage),
    Stop(StopReason),
    Done,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub(crate) enum OpenKind { Text, Thinking }

impl Codec for AnyCodec { /* match-dispatch; Decoder = AnyDecoder enum */ }
```

### 8.3 `start`: the request flow and retry loop

```text
start(codec, req):
  validate_request(req)?                                       // InvalidRequest, no I/O
  model  = req.options.model_override or self.model
  body   = Bytes::from(serde_json::to_vec(&codec.encode(model, req)?)?)
  permit = limiter.acquire_owned().await (if configured)       // bounded by `total` if set
  span   = info_span!("llm.chat", format, model, host, attempt = Empty, status = Empty, request_id = Empty)
  t0 = now; attempt = 0; refreshed = false
  loop {
    attempt += 1
    headers = static + common + auth (ApiKey: codec.auth_headers; Session: tokens.token().await → Bearer)
    outcome = timeout(min(response_headers, remaining_total), client.post(endpoint).headers(h).body(body.clone()).send())
    err = match outcome {
      Elapsed           → Timeout{ResponseHeaders}
      Err(e)            → Network{e} (reqwest connect timeout → Timeout{Connect})
      Ok(resp) if 2xx   → return Ok(Pipeline::new(resp.bytes_stream(), codec.new_decoder(), permit, …).into_event_stream())
      Ok(resp) 401 && Session && !refreshed →
            match tokens.on_unauthorized(&used).await {
              Ok(Refreshed) → { refreshed = true; continue }   // free extra attempt
              _             → Auth{reauth_required: true}
            }
      Ok(resp)          → codec.classify_error(status, headers, read_body_capped(resp))
    }
    decision = should_retry(&err, resp_headers)     // x-should-retry overrides is_retryable()
    if !decision || attempt > policy.max_retries { return Err(err) }
    delay = policy.delay_for(attempt, retry_after(&err, headers), fastrand::f64())
    match delay {
      None                                   → return Err(err)  // hint exceeded max_retry_after
      Some(d) if t0.elapsed() + d > budget   → return Err(err)
      Some(d)                                → warn!(attempt, status, ?d, "retrying LLM request"); sleep(d).await
    }
  }
```

Notes on the loop:
- **The body is encoded once.** `Bytes::clone` is a refcount bump.
- **`Timeout{Total}`** is checked before each attempt and before each
  sleep. If the total budget is too small for the next sleep, the last
  error is returned instead of sleeping past it.
- **`should_retry`.**
  - If the response carries `x-should-retry: true`, retry. If it carries
    `false`, don't.
  - Otherwise use `err.is_retryable()`.
  - `RateLimited` with no `Retry-After` is retryable with computed
    backoff.
- **No retry after a 2xx.** Once a stream exists, mid-stream failures
  are returned *as stream items*. See HLD §3 for how core handles them.
- **Logging.**
  - `request_id` is recorded on the span for every response.
  - `debug!` on success logs time-to-headers.
  - When the pipeline ends, `debug!` logs stop reason, usage,
    time-to-first-token, total time, and bytes.
  - Bodies are logged at `trace` only when `ARTHA_LOG_BODIES=1`, and
    headers are never logged.

### 8.4 Retry/backoff parameters (`engine/retry.rs`)

| Parameter | Default | Notes |
|---|---|---|
| `max_retries` | 3 | 4 attempts total. `artha-jobs` may raise it for unattended runs. |
| `base_delay` | 500 ms | |
| `max_delay` | 20 s | cap on computed backoff |
| backoff | `d = max(100ms, jitter * min(max_delay, base * 2^(attempt-1)))` | "full jitter". The ceilings for attempts 1, 2, and 3 are 0.5 s, 1 s, and 2 s. |
| `Retry-After` | `d = hint + jitter*250ms` | Parsed from `retry-after-ms` (ms), `retry-after` (integer seconds), or `retry-after` (HTTP-date, via `httpdate`, relative to now). The first one present wins. |
| `max_retry_after` | 60 s | A larger hint gives `None`, so we fail fast with `RateLimited{retry_after: Some(hint)}`. |
| `budget` | 90 s | wall clock from the first attempt, including sleeps |
| retryable statuses | 408, 409, 425, 429, 500, 502, 503, 504, 529 | 429 `insufficient_quota` excluded |
| session refresh | +1 attempt on 401 | not counted, once per `chat` |

```rust
impl RetryPolicy {
    pub fn delay_for(&self, attempt: u32, retry_after: Option<Duration>, jitter: f64) -> Option<Duration> {
        if let Some(hint) = retry_after {
            if hint > self.max_retry_after { return None; }
            return Some(hint + Duration::from_millis((jitter * 250.0) as u64));
        }
        let ceiling = self.base_delay.saturating_mul(1 << (attempt - 1).min(16)).min(self.max_delay);
        Some(ceiling.mul_f64(jitter).max(Duration::from_millis(100)))
    }
}
pub(crate) fn parse_retry_after(h: &HeaderMap, now: SystemTime) -> Option<Duration>;
pub(crate) fn should_retry(err: &LlmError, h: Option<&HeaderMap>) -> bool;
```

### 8.5 Concurrency limiter (`engine/limit.rs`)

`Semaphore::acquire_owned` runs before the first attempt. The permit is
held across retries and moved into the `Pipeline`, so it is released
when the stream finishes, errors, or is dropped. Waiting for a permit
counts against `Timeouts::total` when that is set. Otherwise the wait is
unbounded, which is fine because the holder's stream always terminates
through the idle timeout.

## 9. Streaming pipeline and turn assembly (`stream/`)

### 9.1 SSE decoder (`stream/sse.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseFrame { pub event: Option<String>, pub data: String }

pub(crate) struct SseDecoder {
    line: Vec<u8>,          // bytes of the current, incomplete line
    pending_cr: bool,       // last chunk ended in '\r'; swallow a following '\n'
    event: Option<String>,
    data: String,
    has_data: bool,
    frame_bytes: usize,
    max_frame_bytes: usize,
}
impl SseDecoder {
    pub(crate) fn new(max_frame_bytes: usize) -> Self;
    pub(crate) fn feed(&mut self, chunk: &[u8], out: &mut Vec<SseFrame>) -> Result<()>;
    /// EOF. Dispatches a final frame that lacked its terminating blank line.
    pub(crate) fn finish(&mut self, out: &mut Vec<SseFrame>) -> Result<()>;
}
```

The parser implements the event-stream parsing rules, restricted to what
we need:
- Lines end with `\n`, `\r\n`, or a lone `\r`. A `\r\n` split across two
  chunks is handled through `pending_cr`.
- **UTF-8.** Bytes are buffered per line and decoded with
  `String::from_utf8` only at line end, so a multi-byte character split
  across chunks is safe. Invalid UTF-8 is a `Protocol` error.
- **Fields.**
  - A line starting with `:` is a comment, such as OpenRouter-style
    keep-alives. It is ignored, but it still resets the idle timer
    because bytes arrived.
  - On `field:value`, one leading space is stripped from the value.
  - `event` sets the event name.
  - `data` appends the value, separated by `\n` from previous data lines.
  - `id` and `retry` are ignored, because we never reconnect.
  - A line with no colon is a field with an empty value.
- **Dispatch.** A blank line dispatches the frame if `has_data`, then
  resets the frame state. A frame with an event name but no data is
  dropped.
- **Size limit.** Exceeding `max_frame_bytes` is
  `Protocol("SSE frame exceeds limit")`.
- A leading UTF-8 BOM in the first chunk is stripped.

### 9.2 Turn assembler (`stream/assemble.rs`)

```rust
pub(crate) struct TurnAssembler {
    blocks: Vec<Block>,                       // BlockIndex(i) == blocks[i]
    by_wire: HashMap<u32, usize>,             // decoder block id → index
    usage: Usage,
    stop: Option<StopReason>,
    started: bool,
    done: bool,
    call_seq: u32,                            // for synthesized ids
    id_salt: u32,                             // fastrand per turn
}
enum Block {
    Text(String),
    Thinking { text: String, signature: Option<String> },
    Redacted(String),
    Tool { id: String, name: String, args: String, initial: Option<serde_json::Value>, closed: bool },
}
impl TurnAssembler {
    pub(crate) fn new() -> Self;
    pub(crate) fn push(&mut self, ev: WireEvent, out: &mut VecDeque<ChatEvent>) -> Result<()>;
    /// Only after `Done`: builds the final MessageEnd.
    pub(crate) fn finish(&mut self) -> Result<ChatEvent>;
}
```

Transitions. Any event before `Start` is a `Protocol` error. Any event
after `Done` is ignored.

| WireEvent | Effect | Emits |
|---|---|---|
| `Start` | `started = true` | `MessageStart` |
| `BlockOpen{b,kind}` | allocate an index for `b` if new | — |
| `TextDelta{b,t}` | allocate if new (Text), then append | `TextDelta{idx,t}` |
| `ThinkingDelta{b,t}` | allocate if new (Thinking), then append | `ThinkingDelta` |
| `Signature{b,s}` | set the signature on the Thinking block | — |
| `RedactedThinking{b,d}` | allocate a Redacted block | — |
| `ToolStart{b,id,name,initial}` | allocate a Tool block. `id = id.unwrap_or(format!("call_{salt:x}_{seq}"))`. | `ToolCallStart{idx,id,name}` |
| `ToolArgs{b,f}` | append to `args` (unknown `b` is a `Protocol` error) | `ToolCallDelta{idx,f}` |
| `BlockStop{b}` on a Tool | `closed = true`, then **parse** (below) | `ToolCallEnd{idx, call}` |
| `BlockStop{b}` on other blocks | — | — |
| `Usage(u)` | `usage.merge(u)` | `Usage(self.usage)` |
| `Stop(r)` | `stop = Some(r)` | — |
| `Done` | close any unclosed Tool blocks (parse and emit `ToolCallEnd` for each), then `done = true` | — |

**Argument parsing on close:**
1. `s = args.trim()`.
2. If `s` is empty, use `initial`. That is the Anthropic `content_block_start.input`,
   which is usually `{}`. If there is no `initial`, use `{}`.
3. Otherwise `serde_json::from_str::<Value>(s)`:
   - `Ok(Value::Object)` → `input`.
   - `Ok(other)` → malformed, with `"arguments must be a JSON object"`.
   - `Err(e)` → malformed, with `e.to_string()`.
4. A malformed call has `input = {}` and
   `malformed = Some(MalformedInput{raw: args, error})`. The final
   `ContentBlock::ToolUse` carries `raw_input = Some(args)`.

**`finish()`**:
1. Build the `Message{role: Assistant}` from `blocks` in index order:
   - drop empty `Text` blocks
   - `Thinking` → `ContentBlock::Thinking`
   - `Tool` → `ContentBlock::ToolUse`
2. Determine the stop reason as `stop.unwrap_or(EndTurn)`.
3. **Tool-use override.** If the message has at least one `ToolUse` and
   the reason is `EndTurn`, set it to `ToolUse` and log at `debug`.
   Several local servers report `stop` despite emitting tool calls.
   Core's loop keys on `ToolUse`, so normalizing here keeps that logic
   provider-agnostic. `MaxTokens` with tool calls is left alone: the
   calls are probably truncated, and their `malformed` flag will show it.
4. Emit `MessageEnd{stop_reason, usage, message}`.

### 9.3 Pipeline (`stream/pipeline.rs`)

```rust
pub(crate) struct Pipeline<D: StreamDecoder> {
    body: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    sse: SseDecoder,
    decoder: D,
    asm: TurnAssembler,
    frames: Vec<SseFrame>,             // scratch, reused
    wire: Vec<WireEvent>,              // scratch, reused
    pending: VecDeque<ChatEvent>,
    idle: Duration,
    deadline: Option<tokio::time::Instant>,
    state: PipeState,                  // Streaming | Ended | Failed
    _permit: Option<OwnedSemaphorePermit>,
    span: tracing::Span,
}
impl<D: StreamDecoder> Pipeline<D> {
    pub(crate) fn into_event_stream(self) -> EventStream;  // futures_util::stream::unfold(self, Self::next_item).boxed()
    async fn next_item(mut self) -> Option<(Result<ChatEvent>, Self)>;
}
```

`next_item` loop:

```text
loop {
  if let Some(ev) = pending.pop_front()        → return Some(Ok(ev))
  match state { Ended | Failed                  → return None }
  wait = min(idle, deadline - now)
  match timeout(wait, body.next()).await {
    Elapsed                                     → fail(Timeout{StreamIdle | Total})
    Some(Err(e))                                → fail(Network{e})
    Some(Ok(bytes))                             → sse.feed(bytes, &mut frames)?;
                                                  for f in frames.drain(..) { decoder.on_frame(&f, &mut wire)? }
                                                  for w in wire.drain(..)  { asm.push(w, &mut pending)? }
                                                  if asm.done { pending.push_back(asm.finish()?); state = Ended }
    None (EOF)                                  → sse.finish(..)?; decoder.on_frame for the last frames; decoder.on_eof(&mut wire)?;
                                                  drain wire into asm; pending.push_back(asm.finish()?); state = Ended
  }
}
fail(e): state = Failed; return Some(Err(e))   // anything still in `pending` was already yielded first
```

Ordering around `Done`:
- OpenAI's trailing usage chunk always arrives *before* `Done`, because
  the OpenAI decoder emits `Done` only on `[DONE]` or EOF. That holds even
  when the usage chunk and `[DONE]` share one TCP read, since frames are
  processed in order. Usage is never lost.
- The Anthropic decoder emits `Done` on `message_stop`, which is the last
  meaningful event.
- Once `Ended`, the pipeline yields its remaining `pending` events, then
  returns `None` and drops `body` without reading to EOF. That returns
  the connection to the pool if the body was complete, or closes it.

## 10. Test double (`testing.rs`, feature `test-util`)

```rust
#[derive(Debug, Clone)]
pub struct ScriptedProvider { inner: Arc<Mutex<ScriptState>>, info: ProviderInfo }
#[derive(Debug, Default)]
struct ScriptState { turns: VecDeque<TurnScript>, requests: Vec<RecordedRequest> }

#[derive(Debug, Clone)]
pub enum TurnScript {
    /// Events are yielded in order. MessageStart/MessageEnd are synthesized if absent,
    /// and MessageEnd's `message` is assembled from the deltas by the real TurnAssembler.
    Events(Vec<ChatEvent>),
    /// `chat` itself fails.
    FailStart(LlmError),
    /// Yield the events, then fail mid-stream.
    FailAfter(Vec<ChatEvent>, LlmError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedRequest {
    pub system: Option<String>, pub messages: Vec<Message>, pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice, pub options: GenerationOptions,
}

impl ScriptedProvider {
    pub fn new(turns: impl IntoIterator<Item = TurnScript>) -> Self;
    pub fn requests(&self) -> Vec<RecordedRequest>;
}
pub struct Script;
impl Script {
    pub fn text(s: &str) -> TurnScript;                                   // one text block, EndTurn
    pub fn tool_call(name: &str, input: serde_json::Value) -> TurnScript;  // one tool call, ToolUse
    pub fn tool_calls(calls: &[(&str, serde_json::Value)]) -> TurnScript;
}
```

`ScriptedProvider::chat` runs the real `validate_request`, so core's
tests also catch history-pairing bugs. Running out of turns is an `Err`
that names the request index.

## 11. Testing strategy

**1. Pure unit tests.** No I/O, no runtime.
- `SseDecoder`:
  - the event-stream parsing rule examples
  - CRLF, lone CR, and CR at a chunk end followed by LF
  - comments
  - multi-line `data`
  - an event name with no data
  - a missing final blank line at EOF
  - a BOM
  - a multi-byte UTF-8 character split across chunks
  - invalid UTF-8
  - the frame-size limit
- `endpoint` joining: every row of §7.1, plus userinfo and `ftp://`
  rejection.
- `RetryPolicy::delay_for`:
  - ceilings for `jitter = 0.999`
  - the 100 ms floor for `jitter = 0`
  - a hint above `max_retry_after` gives `None`
- `parse_retry_after`: ms, seconds, HTTP-date, and garbage.
- `should_retry`: the full status matrix, and `x-should-retry` overrides.
- `validate_request`: each rule in §4.1, with error messages naming the
  index.
- `ToolSpec::validate`: names at length 64 and 65, dots, unicode, and a
  non-object schema.
- `TurnAssembler`:
  - id synthesis
  - malformed and non-object arguments
  - `initial_input` fallback
  - the tool-use stop override
  - empty text dropped
  - `Usage` merging
- `classify_error`: one case per row of §7.7, including plain-text bodies
  and `{"detail": …}` bodies.
- Golden serde tests for `Message` (§3.2) and `ChatEvent`.

**2. Encoder snapshot tests** (`insta`, JSON). One canonical request
matrix is encoded by *both* codecs:
- text only
- with system
- a single tool round trip
- parallel tool calls with results
- an `is_error` result
- a `Json` result
- a malformed `raw_input` call echoed back
- thinking with a signature, and without one
- adjacent same-role messages
- ids needing sanitization
- each `ToolChoice`
- `parallel_tool_calls: Some(false)`
- thinking with temperature (dropped)
- cache hints on and off
- each `OpenAiCompat` flag

Snapshots are reviewed by hand against the public API documentation
once, then guard against regressions.

**3. Recorded SSE fixtures.** Each `tests/fixtures/<format>/<name>.sse`
has a sibling `<name>.expected.json` holding the expected `ChatEvent`
sequence (or terminal error). Every fixture is run through
`SseDecoder → Decoder → TurnAssembler` under four split strategies:
- the whole body in one chunk
- one byte per chunk
- each line separately
- 200 seeded random splits

All four must yield identical event sequences. The event-order
invariants of §5 are asserted on every run.

| anthropic-messages fixtures | openai-chat fixtures |
|---|---|
| `text_only` | `text_only` |
| `tool_use_single` | `tool_call_single` |
| `tool_use_parallel` | `tool_calls_parallel_interleaved` (args for index 0 and 1 alternate) |
| `text_then_tool` | `text_then_tool` |
| `thinking_signature_then_tool` | `reasoning_content_then_text` |
| `empty_tool_input` (no `input_json_delta`) | `whole_args_one_chunk` |
| `ping_and_unknown_events` (incl. unknown block type with deltas) | `missing_index` (index omitted, id only on first) |
| `no_event_lines` (data only) | `name_repeated_every_chunk` |
| `message_stop_without_delta` | `usage_final_chunk` (`choices: []`) |
| `error_overloaded_midstream` | `no_done_marker` |
| `truncated_no_message_stop` | `done_without_finish_reason` |
| `max_tokens_mid_tool_args` (→ malformed) | `stop_reason_with_tool_calls` (→ ToolUse) |
| `cache_usage_fields` | `inline_error_object` |
| | `keepalive_comments` |
| | `truncated_mid_args` |

Fixtures are of two kinds:
- **Hand-authored** from the public API documentation, for the canonical
  cases.
- **Captured** from real servers for the quirk cases: llama.cpp server,
  Ollama, vLLM, and LM Studio. These are captured with the live-test
  recorder (step 7), which writes only the response body. Headers and
  request bodies are never written, so no credentials can leak into the
  repo.

The fixture filename records which server and version produced it.

**4. HTTP-level tests** (`wiremock` 0.6, real `reqwest` on a loopback
port), one file per format, each running the same matrix:
- the correct endpoint path for several `base_url` shapes
- `x-api-key`/`anthropic-version`, or `Authorization: Bearer`, present
- no auth header when the key is empty
- `user-agent` set
- the body matches the snapshot
- retry on 529, 503, and 429 then success, with the request count
  asserted. The test policy uses `base_delay = 1ms` so tests stay fast.
  Real timing is covered by the pure `delay_for` tests.
- `Retry-After: 120` gives an immediate `RateLimited{retry_after: 120s}`
  after one request
- `x-should-retry: false` on a 500 gives one request
- 400, 401, 404, and 413 are never retried
- context-length bodies give `ContextLengthExceeded`
- a 3xx gives `Api`, and the redirect is not followed
- the response-headers timeout: a wiremock `set_delay` longer than the
  timeout gives `Timeout{ResponseHeaders}`, retried up to the policy
- the concurrency cap: with `max_concurrent = Some(1)`, a second `chat`
  doesn't hit the server until the first stream is dropped

**5. Streaming-transport tests** (`tests/support::StallingServer`, a
small `tokio::net::TcpListener` that writes scripted chunks with
delays). wiremock can't stall mid-body, so these need their own server:
- stream idle timeout mid-body gives `Err(Timeout{StreamIdle})` as the
  final item, after the deltas already sent
- a connection reset mid-stream gives `Network`
- dropping the `EventStream` mid-body closes the socket. The server
  observes EOF within 100 ms.
- keep-alive comments reset the idle timer

**6. Cloud and auth tests** (`cloud_auth.rs`), with a mock `TokenSource`
that counts calls:
- the bearer token comes from `token()`
- a 401 leads to `on_unauthorized`, then `Refreshed`, then a second
  request carrying the *new* token, which succeeds
- a second 401 after a refresh gives `Auth{reauth_required: true}`, with
  no loop
- `ReauthRequired` gives an immediate error
- a `token()` failure gives `Auth{status: None}`, with no HTTP request
  made

**7. Secret-hygiene tests** (`secrets.rs`). Use the key
`sk-test-SECRET-9f8e7d` and assert that string is absent from:
- `format!("{:?}")` of `ProviderConfig`, `AnthropicMessages`,
  `OpenAiChat`, `CloudProvider`, and `StaticToken`
- every `LlmError` produced in steps 4 and 6, both `Display` and `Debug`
- all `tracing` output captured with a test subscriber at `TRACE` level
  during the step 4 matrix (with `ARTHA_LOG_BODIES` unset)

The test also asserts that the built auth `HeaderValue::is_sensitive()`.

**8. Live tests** (`live.rs`, `#[ignore]`, run with
`cargo test -p artha-llm -- --ignored`). They are configured from
environment variables:

```
ARTHA_LLM_LIVE_<NAME>_FORMAT=anthropic-messages|openai-chat
ARTHA_LLM_LIVE_<NAME>_BASE_URL
ARTHA_LLM_LIVE_<NAME>_API_KEY
ARTHA_LLM_LIVE_<NAME>_MODEL
```

`<NAME>` is a free label, such as `LLAMACPP` or `HOSTED_ANTHROPIC`.
Each configured target runs three scenarios:
- **Text turn.**
- **Two-turn tool round trip.** An `add(a, b)` tool. Assert the model
  calls it, feed back the result, and assert the final text contains the
  sum.
- **Cancellation.** Drop the stream after the first delta.

`ARTHA_LLM_RECORD_DIR=<dir>` saves each raw response body as a candidate
fixture (step 3).

**9. Fuzzing (optional, CI nightly).** `cargo-fuzz` targets for
`SseDecoder::feed`, and for `AnthropicDecoder` and `OpenAiDecoder` over
arbitrary frames. The property is: no panic, and either a well-formed
event sequence or an `Err`.

**Downstream contract:** `artha-core`'s tests depend on
`artha-llm = { features = ["test-util"] }` and drive the whole agent loop
with `ScriptedProvider`. That covers tool-call sequencing, persistence of
`MessageEnd.message`, mid-stream error handling, and `PauseTurn`
continuation, all without a network.
