use agent_client_protocol::schema::v1::ContentBlock;
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub type BackendError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// `_meta` key marking a prompt content block as a structured attachment.
///
/// Blocks carrying this key are routed to [`Prompt::attachments`] verbatim
/// instead of being flattened into [`Prompt::text`], so backends can recover
/// provider-native file payloads losslessly.
pub const ATTACHMENT_META_KEY: &str = "longbridge.ai/attachment";

/// A user prompt split into flattened text and structured attachments.
#[derive(Clone, Debug, Default)]
pub struct Prompt {
    /// Text flattened from the prompt's text-bearing content blocks.
    pub text: String,
    /// Content blocks marked with [`ATTACHMENT_META_KEY`], in prompt order.
    pub attachments: Vec<ContentBlock>,
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self {
            text,
            attachments: Vec::new(),
        }
    }
}

impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

/// The `_meta` map of a content block, whichever variant it is.
pub(crate) fn block_meta(
    block: &ContentBlock,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    match block {
        ContentBlock::Text(content) => content.meta.as_ref(),
        ContentBlock::Image(content) => content.meta.as_ref(),
        ContentBlock::Audio(content) => content.meta.as_ref(),
        ContentBlock::ResourceLink(content) => content.meta.as_ref(),
        ContentBlock::Resource(content) => content.meta.as_ref(),
        _ => None,
    }
}

/// The payload a sender stored under [`ATTACHMENT_META_KEY`], or `None` if the
/// block is not an attachment.
///
/// [`Prompt::attachments`] hands back the blocks verbatim, so the payload lives
/// in `_meta` under a key whose position differs per `ContentBlock` variant.
/// This is the supported way to read it: a backend that matches the variants
/// itself will silently miss any variant added later, and the two sides would
/// drift on what the envelope looks like.
///
/// ```
/// use agent_client_protocol::schema::v1::{ContentBlock, ResourceLink};
/// use longbridge_ai_acp::{attachment_payload, ATTACHMENT_META_KEY};
///
/// let mut meta = serde_json::Map::new();
/// meta.insert(ATTACHMENT_META_KEY.into(), serde_json::json!({"oss_key": "k1"}));
/// let block = ContentBlock::ResourceLink(ResourceLink::new("chart.png", "oss://k1").meta(meta));
///
/// assert_eq!(attachment_payload(&block).unwrap()["oss_key"], "k1");
/// ```
#[must_use]
pub fn attachment_payload(block: &ContentBlock) -> Option<&serde_json::Value> {
    block_meta(block)?.get(ATTACHMENT_META_KEY)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentPlanEntry {
    pub content: String,
    pub priority: String,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSessionInfo {
    pub session_id: String,
    pub cwd: PathBuf,
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSessionPage {
    pub sessions: Vec<AgentSessionInfo>,
    pub next_cursor: Option<String>,
}

pub struct LoadedAgentSession<Session> {
    pub state: Session,
    pub history: BoxStream<'static, Result<AgentEvent<Session>, BackendError>>,
}

/// Events understood by the protocol adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AgentEvent<Session> {
    /// Historical user content emitted while loading a session.
    UserText(String),
    Text(String),
    Thought(String),
    /// A text or reasoning delta with provider metadata attached losslessly.
    Content {
        text: String,
        thought: bool,
        metadata: serde_json::Value,
    },
    ToolStarted {
        id: String,
        title: String,
        raw_input: Option<serde_json::Value>,
    },
    ToolFinished {
        id: String,
        title: String,
        success: bool,
        raw_output: Option<serde_json::Value>,
    },
    /// A tool start carrying the provider's complete event payload.
    ToolStartedRich {
        id: String,
        title: String,
        raw_input: Option<serde_json::Value>,
        metadata: serde_json::Value,
    },
    /// A tool completion carrying the provider's complete event payload.
    ToolFinishedRich {
        id: String,
        title: String,
        success: bool,
        raw_output: Option<serde_json::Value>,
        metadata: serde_json::Value,
    },
    /// Progress for an existing tool call with the complete provider payload.
    ToolProgressRich {
        id: String,
        title: String,
        raw_output: Option<serde_json::Value>,
        metadata: serde_json::Value,
    },
    /// Provider plan mapped to ACP's standard plan update.
    Plan {
        entries: Vec<AgentPlanEntry>,
        metadata: serde_json::Value,
    },
    /// Provider session title mapped to ACP session metadata.
    SessionTitle {
        title: String,
        metadata: serde_json::Value,
    },
    NeedsInput {
        session: Session,
        questions: Vec<String>,
        /// Provider-native interaction payload for rich host UIs.
        metadata: Option<serde_json::Value>,
    },
    /// A yes/no tool authorization that maps to ACP's standard permission UI.
    PermissionRequired {
        session: Session,
        tool_call_id: String,
        title: String,
        metadata: Option<serde_json::Value>,
    },
    /// A provider pause that ACP cannot safely satisfy (for example a trade
    /// password challenge). It is displayed as text and ends the turn.
    Notice {
        session: Session,
        text: String,
        metadata: Option<serde_json::Value>,
    },
    /// Versioned rich content with a standard ACP fallback and optional preview.
    RichContent(crate::RichContent),
    /// A provider event that has no lossless representation in core ACP v1.
    ///
    /// The server transports this through ACP `_meta` under the supplied
    /// namespace. Generic clients safely ignore it, while Longbridge clients
    /// can reconstruct their native chat event and reuse the existing UI.
    Extension {
        namespace: String,
        event: String,
        data: serde_json::Value,
    },
    Finished(Session),
    /// A completed turn with the provider's terminal payload.
    Completed {
        session: Session,
        metadata: serde_json::Value,
    },
}

/// Provider-neutral seam used by both the CLI and an embedded desktop client.
#[async_trait]
pub trait AgentBackend: Send + Sync + 'static {
    type Session: Clone + Default + Send + Sync + 'static;

    /// Whether this backend implements ACP `session/list` and `session/load`.
    const SESSION_HISTORY: bool = false;

    /// Creates backend state for a newly assigned ACP session identifier.
    ///
    /// Backends that persist sessions can retain this identifier so a later
    /// `session/load` request can resolve the same state after a restart.
    fn new_session(&self, _session_id: &str, _cwd: &Path) -> Self::Session {
        Self::Session::default()
    }

    async fn list_sessions(
        &self,
        _cwd: Option<&Path>,
        _cursor: Option<&str>,
    ) -> Result<AgentSessionPage, BackendError> {
        Err("session history is not supported".into())
    }

    async fn load_session(
        &self,
        _session_id: &str,
        _cwd: &Path,
    ) -> Result<LoadedAgentSession<Self::Session>, BackendError> {
        Err("session history is not supported".into())
    }

    async fn prompt(
        &self,
        session: Self::Session,
        prompt: Prompt,
        cwd: &Path,
    ) -> Result<BoxStream<'static, Result<AgentEvent<Self::Session>, BackendError>>, BackendError>;
}

#[cfg(test)]
mod tests {
    use super::{attachment_payload, ATTACHMENT_META_KEY};
    use agent_client_protocol::schema::v1::{ContentBlock, ResourceLink, TextContent};

    fn meta(key: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut meta = serde_json::Map::new();
        meta.insert(key.to_owned(), serde_json::json!({ "oss_key": "k1" }));
        meta
    }

    #[test]
    fn attachment_payload_reads_the_marked_meta_of_every_carrying_variant() {
        let link = ContentBlock::ResourceLink(
            ResourceLink::new("chart.png", "oss://k1").meta(meta(ATTACHMENT_META_KEY)),
        );
        assert_eq!(
            attachment_payload(&link),
            Some(&serde_json::json!({ "oss_key": "k1" }))
        );

        let text = ContentBlock::Text(TextContent::new("hi").meta(meta(ATTACHMENT_META_KEY)));
        assert_eq!(
            attachment_payload(&text),
            Some(&serde_json::json!({ "oss_key": "k1" }))
        );
    }

    #[test]
    fn attachment_payload_ignores_blocks_that_are_not_attachments() {
        // No `_meta` at all.
        assert_eq!(
            attachment_payload(&ContentBlock::Text(TextContent::new("hi"))),
            None
        );
        // `_meta` present, but under a different key: this is the case a caller
        // matching variants by hand tends to get wrong.
        let other = ContentBlock::Text(TextContent::new("hi").meta(meta("longbridge.ai/event")));
        assert_eq!(attachment_payload(&other), None);
    }
}
