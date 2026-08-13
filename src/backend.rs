use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub type BackendError = Box<dyn std::error::Error + Send + Sync + 'static>;

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
        prompt: String,
        cwd: &Path,
    ) -> Result<BoxStream<'static, Result<AgentEvent<Self::Session>, BackendError>>, BackendError>;
}
