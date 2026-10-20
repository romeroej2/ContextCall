//! Anthropic Messages API transport.
//!
//! `AnthropicSession` implements [`ConversationSession`] over
//! `POST {base}/v1/messages` with streaming SSE, keeping the conversation
//! history in memory so every turn builds on the previous ones. Prompt caching
//! breakpoints sit on the system prompt, the first user turn (resume / job
//! description) and the latest user turn, so each turn reuses the cached prefix.
//!
//! Auth is an Anthropic Console API key (`x-api-key`) or, for compatible
//! gateways, `Authorization: Bearer <token>`. Subscription logins are not
//! supported by design.

use crate::intelligence::conversation::{ContentBlock, ConversationSession, TurnEvent, TurnUsage};
use crate::intelligence::llm_client::Message;
use async_trait::async_trait;
use base64::Engine as _;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Notify};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

const INTERRUPTED: &str = "interrupted";

#[derive(Clone, PartialEq, Eq)]
pub enum AnthropicAuth {
    /// Sent as `x-api-key: <key>` (Anthropic Console API key).
    ApiKey(String),
    /// Sent as `Authorization: Bearer <token>` (compatible gateways).
    Bearer(String),
}

impl AnthropicAuth {
    /// Build from the settings auth mode (`"x-api-key"` or `"bearer"`).
    pub fn from_mode(mode: &str, secret: impl Into<String>) -> Self {
        if mode.trim().eq_ignore_ascii_case("bearer") {
            Self::Bearer(secret.into())
        } else {
            Self::ApiKey(secret.into())
        }
    }

    /// Header name and value for this auth mode.
    pub fn header(&self) -> (&'static str, String) {
        match self {
            Self::ApiKey(key) => ("x-api-key", key.trim().to_string()),
            Self::Bearer(token) => ("authorization", format!("Bearer {}", token.trim())),
        }
    }

    fn secret(&self) -> &str {
        match self {
            Self::ApiKey(value) | Self::Bearer(value) => value,
        }
    }
}

// Never print the secret.
impl fmt::Debug for AnthropicAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(_) => f.write_str("ApiKey(<redacted>)"),
            Self::Bearer(_) => f.write_str("Bearer(<redacted>)"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub base_url: String,
    pub auth: AnthropicAuth,
    pub model: String,
    /// Empty means no system prompt.
    pub system_prompt: String,
    pub max_tokens: u32,
    pub anthropic_version: String,
}

impl AnthropicConfig {
    pub fn new(auth: AnthropicAuth, model: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.into(),
            auth,
            model: model.into(),
            system_prompt: String::new(),
            max_tokens: DEFAULT_MAX_TOKENS,
            anthropic_version: DEFAULT_ANTHROPIC_VERSION.into(),
        }
    }

    /// `{base}/v1/messages`; a base that already ends in `/v1` is accepted too.
    pub fn messages_url(&self) -> String {
        let base = self.base_url.trim();
        let base = if base.is_empty() {
            DEFAULT_BASE_URL
        } else {
            base
        };
        let base = base.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    }

    fn version(&self) -> &str {
        let version = self.anthropic_version.trim();
        if version.is_empty() {
            DEFAULT_ANTHROPIC_VERSION
        } else {
            version
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoryMessage {
    role: Role,
    content: Vec<ContentBlock>,
}

fn shared_client() -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .pool_idle_timeout(Duration::from_secs(90))
                .build()
                .unwrap_or_else(|_| Client::new())
        })
        .clone()
}

// ---------------------------------------------------------------------------
// Request building
// ---------------------------------------------------------------------------

fn ephemeral() -> Value {
    json!({ "type": "ephemeral" })
}

fn block_to_json(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::Document { media_type, data } => {
            // Plain-text documents use the API's `text` source; everything else
            // (PDF) is passed through as base64.
            if media_type.eq_ignore_ascii_case("text/plain") {
                if let Some(text) = base64::engine::general_purpose::STANDARD
                    .decode(data.trim())
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                {
                    return json!({
                        "type": "document",
                        "source": { "type": "text", "media_type": "text/plain", "data": text },
                    });
                }
            }
            json!({
                "type": "document",
                "source": { "type": "base64", "media_type": media_type, "data": data },
            })
        }
    }
}

fn system_json(system_prompt: &str) -> Option<Value> {
    if system_prompt.trim().is_empty() {
        return None;
    }
    Some(json!([{
        "type": "text",
        "text": system_prompt,
        "cache_control": ephemeral(),
    }]))
}

/// Build the `/v1/messages` body for a conversation. Cache breakpoints go on
/// the system prompt, the first user message and the latest user message.
fn build_request_body(config: &AnthropicConfig, history: &[HistoryMessage], stream: bool) -> Value {
    let first_user = history.iter().position(|m| m.role == Role::User);
    let last_user = history.iter().rposition(|m| m.role == Role::User);

    let messages: Vec<Value> = history
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let mut content: Vec<Value> = message.content.iter().map(block_to_json).collect();
            let cache_here = Some(index) == first_user || Some(index) == last_user;
            if cache_here {
                if let Some(Value::Object(last)) = content.last_mut() {
                    last.insert("cache_control".into(), ephemeral());
                }
            }
            json!({ "role": message.role.as_str(), "content": content })
        })
        .collect();

    let mut body = json!({
        "model": config.model,
        "max_tokens": config.max_tokens.max(1),
        "messages": messages,
        "stream": stream,
    });
    if let Some(system) = system_json(&config.system_prompt) {
        body["system"] = system;
    }
    body
}

fn build_request(
    client: &Client,
    config: &AnthropicConfig,
    body: &Value,
) -> reqwest::RequestBuilder {
    let (name, value) = config.auth.header();
    client
        .post(config.messages_url())
        .header(name, value)
        .header("anthropic-version", config.version())
        .header("content-type", "application/json")
        .json(body)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

fn api_error_detail(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.trim().chars().take(300).collect())
}

/// Human-readable message for a non-2xx Messages API response.
pub fn http_error_message(status: u16, body: &str) -> String {
    let detail = api_error_detail(body);
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!(" Details: {detail}")
    };
    match status {
        401 => format!(
            "Anthropic API rejected the credentials (401). Check the API key and auth mode in Settings → AI Providers.{detail}"
        ),
        403 => format!(
            "Anthropic API denied access (403). The key may lack permission for this model or workspace.{detail}"
        ),
        404 => format!(
            "Anthropic API endpoint or model not found (404). Check the base URL and model id.{detail}"
        ),
        413 => format!("Anthropic API request is too large (413).{detail}"),
        429 => format!(
            "Anthropic API rate limit or credit limit reached (429). Wait a moment, or check your Console usage.{detail}"
        ),
        529 => format!("Anthropic API is overloaded (529). Try again shortly.{detail}"),
        400 => format!("Anthropic API rejected the request (400).{detail}"),
        500..=599 => format!("Anthropic API server error ({status}).{detail}"),
        _ => format!("Anthropic API returned HTTP {status}.{detail}"),
    }
}

fn stream_error_message(data: &Value) -> String {
    let kind = data
        .pointer("/error/type")
        .and_then(Value::as_str)
        .unwrap_or("error");
    let message = data
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    match kind {
        "overloaded_error" => {
            format!("Anthropic API is overloaded. Try again shortly. ({message})")
        }
        "rate_limit_error" => format!("Anthropic API rate limit reached. ({message})"),
        _ => format!("Anthropic API stream error ({kind}): {message}"),
    }
}

fn request_error_message(err: &reqwest::Error) -> String {
    if err.is_connect() {
        format!(
            "Could not connect to the Anthropic API: {err}. Check the base URL and your network."
        )
    } else if err.is_timeout() {
        format!("Anthropic API request timed out: {err}")
    } else {
        format!("Anthropic API request failed: {err}")
    }
}

// ---------------------------------------------------------------------------
// SSE parsing
// ---------------------------------------------------------------------------

/// Incremental Server-Sent Events parser: feed raw bytes, get `data:` payloads.
#[derive(Debug, Default)]
struct SseParser {
    buffer: String,
    pending: Vec<u8>,
}

impl SseParser {
    /// Returns the `data` payload of each complete event in `chunk`.
    fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        // Keep incomplete UTF-8 sequences until the next chunk.
        self.pending.extend_from_slice(chunk);
        let valid_up_to = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(err) => err.valid_up_to(),
        };
        let text = String::from_utf8_lossy(&self.pending[..valid_up_to]).into_owned();
        self.pending.drain(..valid_up_to);
        self.buffer.push_str(&text);
        // Normalize the whole (small) buffer so a CRLF split across chunks is caught.
        if self.buffer.contains('\r') {
            self.buffer = self.buffer.replace("\r\n", "\n");
        }

        let mut events = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let raw: String = self.buffer.drain(..end + 2).collect();
            let data: Vec<&str> = raw
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|value| value.strip_prefix(' ').unwrap_or(value))
                .collect();
            if !data.is_empty() {
                events.push(data.join("\n"));
            }
        }
        events
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamEvent {
    TextDelta(String),
    Usage(TurnUsage),
    Stop,
    Error(String),
    Ignored,
}

fn usage_from_json(value: &Value) -> TurnUsage {
    let field = |name: &str| value.get(name).and_then(Value::as_u64);
    TurnUsage {
        input_tokens: field("input_tokens"),
        cache_read_input_tokens: field("cache_read_input_tokens"),
        cache_creation_input_tokens: field("cache_creation_input_tokens"),
        output_tokens: field("output_tokens"),
    }
}

/// Later values win; fields absent from `update` are kept.
fn merge_usage(base: &mut TurnUsage, update: TurnUsage) {
    base.input_tokens = update.input_tokens.or(base.input_tokens);
    base.cache_read_input_tokens = update
        .cache_read_input_tokens
        .or(base.cache_read_input_tokens);
    base.cache_creation_input_tokens = update
        .cache_creation_input_tokens
        .or(base.cache_creation_input_tokens);
    base.output_tokens = update.output_tokens.or(base.output_tokens);
}

fn parse_stream_event(data: &str) -> StreamEvent {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return StreamEvent::Ignored;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("message_start") => value
            .pointer("/message/usage")
            .map(|usage| StreamEvent::Usage(usage_from_json(usage)))
            .unwrap_or(StreamEvent::Ignored),
        Some("content_block_delta") => {
            let delta = value.get("delta");
            match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                Some("text_delta") => delta
                    .and_then(|d| d.get("text"))
                    .and_then(Value::as_str)
                    .map(|text| StreamEvent::TextDelta(text.to_string()))
                    .unwrap_or(StreamEvent::Ignored),
                _ => StreamEvent::Ignored,
            }
        }
        Some("message_delta") => value
            .get("usage")
            .map(|usage| StreamEvent::Usage(usage_from_json(usage)))
            .unwrap_or(StreamEvent::Ignored),
        Some("message_stop") => StreamEvent::Stop,
        Some("error") => StreamEvent::Error(stream_error_message(&value)),
        _ => StreamEvent::Ignored,
    }
}

// ---------------------------------------------------------------------------
// Streaming request
// ---------------------------------------------------------------------------

struct StreamOutcome {
    text: String,
    usage: TurnUsage,
    ttft_ms: Option<u128>,
}

/// POST a streaming request and forward text deltas. Cancellation is done by
/// dropping this future.
async fn stream_message(
    client: &Client,
    config: &AnthropicConfig,
    body: &Value,
    events: &mpsc::UnboundedSender<TurnEvent>,
    started: Instant,
) -> Result<StreamOutcome, String> {
    let response = build_request(client, config, body)
        .send()
        .await
        .map_err(|e| request_error_message(&e))?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(http_error_message(status.as_u16(), &text));
    }

    let mut parser = SseParser::default();
    let mut outcome = StreamOutcome {
        text: String::new(),
        usage: TurnUsage::default(),
        ttft_ms: None,
    };
    let mut stopped = false;
    let mut bytes = response.bytes_stream();
    'read: while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|e| format!("Anthropic API stream failed: {e}"))?;
        for data in parser.feed(&chunk) {
            match parse_stream_event(&data) {
                StreamEvent::TextDelta(delta) => {
                    if outcome.ttft_ms.is_none() {
                        outcome.ttft_ms = Some(started.elapsed().as_millis());
                    }
                    outcome.text.push_str(&delta);
                    let _ = events.send(TurnEvent::TextDelta(delta));
                }
                StreamEvent::Usage(usage) => merge_usage(&mut outcome.usage, usage),
                StreamEvent::Stop => {
                    stopped = true;
                    break 'read;
                }
                StreamEvent::Error(message) => return Err(message),
                StreamEvent::Ignored => {}
            }
        }
    }

    if !stopped {
        if outcome.text.is_empty() {
            return Err("Anthropic API stream ended before any answer was received".into());
        }
        log::warn!("Anthropic API stream ended without message_stop; using the partial answer");
    }
    if outcome.text.trim().is_empty() {
        return Err("Anthropic API returned an empty answer".into());
    }
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

/// A multi-turn conversation over the Anthropic Messages API.
pub struct AnthropicSession {
    config: AnthropicConfig,
    client: Client,
    /// Completed turns; the lock is held for the whole turn, which serializes turns.
    history: Mutex<Vec<HistoryMessage>>,
    /// Cancel signal for the turn in flight.
    in_flight: std::sync::Mutex<Option<Arc<Notify>>>,
}

impl AnthropicSession {
    pub fn new(config: AnthropicConfig) -> Self {
        Self {
            config,
            client: shared_client(),
            history: Mutex::new(Vec::new()),
            in_flight: std::sync::Mutex::new(None),
        }
    }

    pub fn config(&self) -> &AnthropicConfig {
        &self.config
    }

    /// Number of messages (user + assistant) in the kept history.
    pub async fn history_len(&self) -> usize {
        self.history.lock().await.len()
    }

    /// Forget all turns (waits for an in-flight turn to finish).
    pub async fn reset(&self) {
        self.history.lock().await.clear();
    }

    fn set_in_flight(&self, cancel: Option<Arc<Notify>>) {
        *self.in_flight.lock().unwrap_or_else(|e| e.into_inner()) = cancel;
    }
}

#[async_trait]
impl ConversationSession for AnthropicSession {
    async fn send_turn(
        &self,
        content: Vec<ContentBlock>,
        events: mpsc::UnboundedSender<TurnEvent>,
    ) -> Result<(), String> {
        let mut history = self.history.lock().await;
        if content.is_empty() {
            let message = "Cannot send an empty turn".to_string();
            let _ = events.send(TurnEvent::Error(message.clone()));
            return Err(message);
        }
        if self.config.auth.secret().trim().is_empty() {
            let message =
                "Anthropic API key is not set. Add it in Settings → AI Providers.".to_string();
            let _ = events.send(TurnEvent::Error(message.clone()));
            return Err(message);
        }

        let cancel = Arc::new(Notify::new());
        self.set_in_flight(Some(cancel.clone()));

        history.push(HistoryMessage {
            role: Role::User,
            content,
        });
        let body = build_request_body(&self.config, &history, true);
        let started = Instant::now();

        let result = tokio::select! {
            outcome = stream_message(&self.client, &self.config, &body, &events, started) => outcome,
            _ = cancel.notified() => Err(INTERRUPTED.to_string()),
        };
        self.set_in_flight(None);
        let total_ms = started.elapsed().as_millis();

        match result {
            Ok(outcome) => {
                log::info!(
                    "Anthropic API turn completed: model={} ttft={}ms total={}ms turns={} usage={:?}",
                    self.config.model,
                    outcome.ttft_ms.map(|ms| ms.to_string()).unwrap_or_else(|| "-".into()),
                    total_ms,
                    history.len() / 2 + 1,
                    outcome.usage
                );
                history.push(HistoryMessage {
                    role: Role::Assistant,
                    content: vec![ContentBlock::text(outcome.text.clone())],
                });
                let _ = events.send(TurnEvent::Done {
                    text: outcome.text,
                    usage: Some(outcome.usage),
                });
                Ok(())
            }
            Err(message) => {
                // Drop this turn's user message so roles keep alternating.
                history.pop();
                if message == INTERRUPTED {
                    log::info!(
                        "Anthropic API turn interrupted after {total_ms}ms: model={}",
                        self.config.model
                    );
                } else {
                    log::warn!(
                        "Anthropic API turn failed after {total_ms}ms: model={} error={message}",
                        self.config.model
                    );
                }
                let _ = events.send(TurnEvent::Error(message.clone()));
                Err(message)
            }
        }
    }

    async fn interrupt(&self) {
        let cancel = self
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(cancel) = cancel {
            // notify_one stores a permit, so this works even before the turn
            // reaches its select.
            cancel.notify_one();
        }
    }

    fn model(&self) -> &str {
        &self.config.model
    }
}

// ---------------------------------------------------------------------------
// One-shot
// ---------------------------------------------------------------------------

/// Convert OpenAI-style messages: `system` messages join the top-level system
/// prompt (after `config.system_prompt`), consecutive same-role messages merge.
fn one_shot_parts(config: &AnthropicConfig, messages: &[Message]) -> (String, Vec<HistoryMessage>) {
    let mut system_parts: Vec<&str> = Vec::new();
    if !config.system_prompt.trim().is_empty() {
        system_parts.push(&config.system_prompt);
    }
    let mut history: Vec<HistoryMessage> = Vec::new();
    for message in messages {
        let role = match message.role.as_str() {
            "system" => {
                if !message.content.trim().is_empty() {
                    system_parts.push(&message.content);
                }
                continue;
            }
            "assistant" => Role::Assistant,
            _ => Role::User,
        };
        if message.content.trim().is_empty() {
            continue;
        }
        match history.last_mut() {
            Some(last) if last.role == role => last
                .content
                .push(ContentBlock::text(message.content.clone())),
            _ => history.push(HistoryMessage {
                role,
                content: vec![ContentBlock::text(message.content.clone())],
            }),
        }
    }
    (system_parts.join("\n\n"), history)
}

/// Single non-conversational completion (for the existing call sites).
pub async fn one_shot(config: &AnthropicConfig, messages: &[Message]) -> Result<String, String> {
    if config.auth.secret().trim().is_empty() {
        return Err("Anthropic API key is not set. Add it in Settings → AI Providers.".into());
    }
    let (system_prompt, history) = one_shot_parts(config, messages);
    if !history.iter().any(|m| m.role == Role::User) {
        return Err("Anthropic API request needs at least one user message".into());
    }
    let request_config = AnthropicConfig {
        system_prompt,
        ..config.clone()
    };
    let body = build_request_body(&request_config, &history, false);
    let started = Instant::now();
    let response = build_request(&shared_client(), &request_config, &body)
        .send()
        .await
        .map_err(|e| request_error_message(&e))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("Anthropic API response could not be read: {e}"))?;
    if !status.is_success() {
        log::warn!(
            "Anthropic API one-shot failed after {}ms: status={status}",
            started.elapsed().as_millis()
        );
        return Err(http_error_message(status.as_u16(), &text));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("Anthropic API returned invalid JSON: {e}"))?;
    let answer = response_text(&value);
    log::info!(
        "Anthropic API one-shot completed in {}ms: model={} usage={:?}",
        started.elapsed().as_millis(),
        request_config.model,
        value.get("usage").map(usage_from_json)
    );
    if answer.trim().is_empty() {
        Err("Anthropic API returned an empty answer".into())
    } else {
        Ok(answer)
    }
}

fn response_text(value: &Value) -> String {
    value
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const CANNED_SSE: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"usage\":{\"input_tokens\":12,\"cache_creation_input_tokens\":100,\"cache_read_input_tokens\":0,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: ping\n",
        "data: {\"type\":\"ping\"}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\", world\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    fn config(base_url: &str) -> AnthropicConfig {
        AnthropicConfig {
            base_url: base_url.into(),
            system_prompt: "You are helpful.".into(),
            ..AnthropicConfig::new(AnthropicAuth::ApiKey("sk-test".into()), "claude-test")
        }
    }

    fn user(text: &str) -> HistoryMessage {
        HistoryMessage {
            role: Role::User,
            content: vec![ContentBlock::text(text)],
        }
    }

    fn assistant(text: &str) -> HistoryMessage {
        HistoryMessage {
            role: Role::Assistant,
            content: vec![ContentBlock::text(text)],
        }
    }

    #[test]
    fn sse_parser_handles_split_chunks_and_crlf() {
        let bytes = CANNED_SSE.replace('\n', "\r\n").into_bytes();
        let mut parser = SseParser::default();
        let mut events = Vec::new();
        for chunk in bytes.chunks(7) {
            events.extend(parser.feed(chunk));
        }
        let parsed: Vec<StreamEvent> = events
            .iter()
            .map(|data| parse_stream_event(data))
            .filter(|event| *event != StreamEvent::Ignored)
            .collect();
        assert_eq!(parsed.len(), 5);
        assert_eq!(parsed[1], StreamEvent::TextDelta("Hello".into()));
        assert_eq!(parsed[2], StreamEvent::TextDelta(", world".into()));
        assert_eq!(parsed[4], StreamEvent::Stop);

        let mut usage = TurnUsage::default();
        for event in &parsed {
            if let StreamEvent::Usage(update) = event {
                merge_usage(&mut usage, update.clone());
            }
        }
        assert_eq!(usage.input_tokens, Some(12));
        assert_eq!(usage.cache_creation_input_tokens, Some(100));
        assert_eq!(usage.cache_read_input_tokens, Some(0));
        assert_eq!(usage.output_tokens, Some(7));
    }

    #[test]
    fn sse_parser_keeps_split_utf8() {
        let event = "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"héllo ✓\"}}\n\n";
        let bytes = event.as_bytes();
        let split = event.find('é').unwrap() + 1; // middle of the 2-byte sequence
        let mut parser = SseParser::default();
        assert!(parser.feed(&bytes[..split]).is_empty());
        let events = parser.feed(&bytes[split..]);
        assert_eq!(
            parse_stream_event(&events[0]),
            StreamEvent::TextDelta("héllo ✓".into())
        );
    }

    #[test]
    fn stream_error_event_is_mapped() {
        let event = parse_stream_event(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        );
        match event {
            StreamEvent::Error(message) => assert!(message.contains("overloaded")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn request_body_places_cache_breakpoints() {
        let history = vec![
            HistoryMessage {
                role: Role::User,
                content: vec![
                    ContentBlock::Document {
                        media_type: "application/pdf".into(),
                        data: "JVBERi0=".into(),
                    },
                    ContentBlock::text("Here is my resume."),
                ],
            },
            assistant("Got it."),
            user("Question two"),
            assistant("Answer two"),
            user("Question three"),
        ];
        let body = build_request_body(&config("http://x"), &history, true);
        assert_eq!(body["model"], "claude-test");
        assert_eq!(body["stream"], true);
        assert_eq!(body["system"][0]["text"], "You are helpful.");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 5);
        let doc = &messages[0]["content"][0];
        assert_eq!(doc["type"], "document");
        assert_eq!(doc["source"]["type"], "base64");
        assert_eq!(doc["source"]["media_type"], "application/pdf");
        assert_eq!(doc["source"]["data"], "JVBERi0=");
        assert!(doc.get("cache_control").is_none());
        // First user turn: breakpoint on its last block.
        assert_eq!(
            messages[0]["content"][1]["cache_control"]["type"],
            "ephemeral"
        );
        // Middle turns: none.
        for index in 1..4 {
            assert!(messages[index]["content"][0].get("cache_control").is_none());
        }
        // Latest user turn.
        assert_eq!(messages[4]["role"], "user");
        assert_eq!(
            messages[4]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );

        let breakpoints = body.to_string().matches("cache_control").count();
        assert_eq!(breakpoints, 3);
    }

    #[test]
    fn request_body_without_system_prompt_omits_it() {
        let mut cfg = config("http://x");
        cfg.system_prompt = "  ".into();
        let body = build_request_body(&cfg, &[user("hi")], false);
        assert!(body.get("system").is_none());
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn text_document_uses_text_source() {
        let data = base64::engine::general_purpose::STANDARD.encode("Plain resume");
        let block = block_to_json(&ContentBlock::Document {
            media_type: "text/plain".into(),
            data,
        });
        assert_eq!(block["source"]["type"], "text");
        assert_eq!(block["source"]["data"], "Plain resume");
    }

    #[test]
    fn auth_header_selection() {
        assert_eq!(
            AnthropicAuth::from_mode("x-api-key", "k1").header(),
            ("x-api-key", "k1".to_string())
        );
        assert_eq!(
            AnthropicAuth::from_mode("bearer", " t1 ").header(),
            ("authorization", "Bearer t1".to_string())
        );
        assert_eq!(
            AnthropicAuth::from_mode("", "k2"),
            AnthropicAuth::ApiKey("k2".into())
        );
        assert!(!format!("{:?}", AnthropicAuth::ApiKey("secret".into())).contains("secret"));
    }

    #[test]
    fn messages_url_accepts_v1_suffix() {
        assert_eq!(
            config("https://api.anthropic.com/").messages_url(),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            config("https://gw.example/anthropic/v1").messages_url(),
            "https://gw.example/anthropic/v1/messages"
        );
        assert_eq!(
            config("").messages_url(),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn http_errors_are_mapped() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let message = http_error_message(401, body);
        assert!(message.contains("401") && message.contains("invalid x-api-key"));
        assert!(http_error_message(429, "").contains("rate limit"));
        assert!(http_error_message(529, "").contains("overloaded"));
        assert!(http_error_message(403, "").contains("403"));
    }

    #[test]
    fn one_shot_moves_system_messages_and_merges_roles() {
        let messages = vec![
            Message::system("Be brief."),
            Message::user("one"),
            Message::user("two"),
        ];
        let (system, history) = one_shot_parts(&config("http://x"), &messages);
        assert_eq!(system, "You are helpful.\n\nBe brief.");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content.len(), 2);
    }

    // --- Local HTTP server -------------------------------------------------

    /// Read one HTTP request; returns (head, body).
    async fn read_request(socket: &mut TcpStream) -> (String, String) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = socket.read(&mut tmp).await.unwrap();
            assert!(n > 0, "client closed early");
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                let mut body = buf[pos + 4..].to_vec();
                while body.len() < length {
                    let n = socket.read(&mut tmp).await.unwrap();
                    body.extend_from_slice(&tmp[..n]);
                }
                return (head, String::from_utf8(body).unwrap());
            }
        }
    }

    enum Reply {
        Sse(&'static str),
        /// Send the first event, then stall until the client goes away.
        Stall,
        Status(u16, &'static str),
    }

    /// Serves `replies` in order (one connection each); request (head, body)
    /// pairs are sent back on the returned channel.
    async fn spawn_server(
        replies: Vec<Reply>,
    ) -> (String, mpsc::UnboundedReceiver<(String, Value)>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (head, body) = read_request(&mut socket).await;
                tx.send((head, serde_json::from_str(&body).unwrap())).ok();
                match reply {
                    Reply::Sse(sse) => {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{sse}"
                        );
                        socket.write_all(response.as_bytes()).await.unwrap();
                    }
                    Reply::Stall => {
                        let first = CANNED_SSE
                            .split("\n\n")
                            .take(4)
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{first}\n\n"
                        );
                        socket.write_all(response.as_bytes()).await.unwrap();
                        let mut sink = [0u8; 64];
                        let _ =
                            tokio::time::timeout(Duration::from_secs(10), socket.read(&mut sink))
                                .await;
                    }
                    Reply::Status(code, body) => {
                        let response = format!(
                            "HTTP/1.1 {code} Error\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        socket.write_all(response.as_bytes()).await.unwrap();
                    }
                }
                socket.shutdown().await.ok();
            }
        });
        (format!("http://{addr}"), rx)
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<TurnEvent>) -> Vec<TurnEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn session_streams_and_keeps_history() {
        let (base, mut requests) =
            spawn_server(vec![Reply::Sse(CANNED_SSE), Reply::Sse(CANNED_SSE)]).await;
        let session = AnthropicSession::new(config(&base));

        let (tx, mut rx) = mpsc::unbounded_channel();
        session
            .send_turn(vec![ContentBlock::text("first")], tx.clone())
            .await
            .unwrap();
        let events = drain(&mut rx);
        assert_eq!(events[0], TurnEvent::TextDelta("Hello".into()));
        assert_eq!(events[1], TurnEvent::TextDelta(", world".into()));
        match &events[2] {
            TurnEvent::Done { text, usage } => {
                assert_eq!(text, "Hello, world");
                assert_eq!(usage.as_ref().unwrap().output_tokens, Some(7));
            }
            other => panic!("unexpected {other:?}"),
        }
        let (head, _) = requests.recv().await.unwrap();
        let head = head.to_ascii_lowercase();
        assert!(head.starts_with("post /v1/messages"));
        assert!(head.contains("x-api-key: sk-test"));
        assert!(head.contains("anthropic-version: 2023-06-01"));

        session
            .send_turn(vec![ContentBlock::text("second")], tx)
            .await
            .unwrap();
        let (_, body) = requests.recv().await.unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["text"], "Hello, world");
        assert_eq!(messages[2]["content"][0]["text"], "second");
        assert_eq!(session.history_len().await, 4);
    }

    #[tokio::test]
    async fn interrupt_drops_the_turn_and_keeps_history_alternating() {
        let (base, mut requests) = spawn_server(vec![
            Reply::Sse(CANNED_SSE),
            Reply::Stall,
            Reply::Sse(CANNED_SSE),
        ])
        .await;
        let session = Arc::new(AnthropicSession::new(config(&base)));

        let (tx, mut rx) = mpsc::unbounded_channel();
        session
            .send_turn(vec![ContentBlock::text("first")], tx)
            .await
            .unwrap();
        drain(&mut rx);
        requests.recv().await.unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let turn = {
            let session = session.clone();
            tokio::spawn(async move {
                session
                    .send_turn(vec![ContentBlock::text("stalls")], tx)
                    .await
            })
        };
        // Wait for the first streamed delta, then interrupt.
        assert_eq!(
            rx.recv().await.unwrap(),
            TurnEvent::TextDelta("Hello".into())
        );
        session.interrupt().await;
        assert_eq!(
            rx.recv().await.unwrap(),
            TurnEvent::Error("interrupted".into())
        );
        assert_eq!(turn.await.unwrap(), Err("interrupted".into()));
        assert_eq!(session.history_len().await, 2);
        requests.recv().await.unwrap();

        let (tx, _rx) = mpsc::unbounded_channel();
        session
            .send_turn(vec![ContentBlock::text("third")], tx)
            .await
            .unwrap();
        let (_, body) = requests.recv().await.unwrap();
        let roles: Vec<&str> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, vec!["user", "assistant", "user"]);
        assert_eq!(body["messages"][2]["content"][0]["text"], "third");
    }

    #[tokio::test]
    async fn http_error_keeps_session_usable() {
        let (base, _requests) = spawn_server(vec![Reply::Status(
            529,
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        )])
        .await;
        let session = AnthropicSession::new(config(&base));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let result = session.send_turn(vec![ContentBlock::text("hi")], tx).await;
        assert!(result.unwrap_err().contains("overloaded"));
        assert!(matches!(drain(&mut rx).as_slice(), [TurnEvent::Error(_)]));
        assert_eq!(session.history_len().await, 0);
    }

    #[tokio::test]
    async fn one_shot_uses_bearer_and_system_field() {
        let reply = r#"{"id":"m","type":"message","role":"assistant","content":[{"type":"text","text":"OK"}],"usage":{"input_tokens":3,"output_tokens":1}}"#;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            request
        });
        let mut cfg = config(&format!("http://{addr}/v1"));
        cfg.auth = AnthropicAuth::Bearer("tok".into());
        cfg.system_prompt.clear();
        let answer = one_shot(
            &cfg,
            &[
                Message::system("Sys"),
                Message::user("Reply with exactly: OK"),
            ],
        )
        .await
        .unwrap();
        assert_eq!(answer, "OK");
        let (head, body) = server.await.unwrap();
        let head = head.to_ascii_lowercase();
        assert!(head.contains("authorization: bearer tok"));
        assert!(!head.contains("x-api-key"));
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["stream"], false);
        assert_eq!(body["system"][0]["text"], "Sys");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }
}
