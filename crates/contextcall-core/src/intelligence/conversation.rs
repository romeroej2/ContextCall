//! Shared interface for persistent, multi-turn LLM conversations.
//!
//! Interview mode (and context-keeping live sessions) talk to a backend through
//! this trait so the transport — the local `claude` CLI or the Anthropic HTTP
//! API — is interchangeable. Each session keeps its own history: every
//! `send_turn` builds on the turns before it.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    /// Base64-encoded document such as a resume PDF (`media_type` e.g. "application/pdf").
    Document { media_type: String, data: String },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnUsage {
    pub input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// Incremental answer text, in order.
    TextDelta(String),
    /// The turn finished; `text` is the full answer.
    Done {
        text: String,
        usage: Option<TurnUsage>,
    },
    /// The turn failed or was interrupted; the session stays usable.
    Error(String),
}

#[async_trait]
pub trait ConversationSession: Send + Sync {
    /// Send one user turn and stream its events. Turns are serialized: a call
    /// waits for any in-flight turn to finish first. Returns once the turn ends.
    async fn send_turn(
        &self,
        content: Vec<ContentBlock>,
        events: mpsc::UnboundedSender<TurnEvent>,
    ) -> Result<(), String>;

    /// Cancel the in-flight turn, if any. History up to the last completed turn is kept.
    async fn interrupt(&self);

    /// Model id or alias, for logs and the UI.
    fn model(&self) -> &str;
}
