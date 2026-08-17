use crate::{with_initialized_session, AgentHandshake, ClientDelegate};
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, PromptRequest, PromptResponse, SessionNotification,
    SessionUpdate, StopReason,
};
use agent_client_protocol::{
    util::MatchDispatch, ActiveSession, Agent, Client, ConnectTo, SessionMessage,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{mpsc, oneshot};

enum SessionCommand {
    Prompt {
        blocks: Vec<ContentBlock>,
        accepted: oneshot::Sender<Result<(), SessionControlError>>,
    },
    Cancel,
    Shutdown,
}

/// A text block followed by the attachment blocks, in prompt order.
fn prompt_blocks(text: impl Into<String>, attachments: Vec<ContentBlock>) -> Vec<ContentBlock> {
    let mut blocks = Vec::with_capacity(attachments.len() + 1);
    blocks.push(ContentBlock::from(text.into()));
    blocks.extend(attachments);
    blocks
}

async fn send_prompt_command(
    commands: &mpsc::UnboundedSender<SessionCommand>,
    blocks: Vec<ContentBlock>,
) -> Result<(), SessionControlError> {
    let (accepted_tx, accepted_rx) = oneshot::channel();
    commands
        .send(SessionCommand::Prompt {
            blocks,
            accepted: accepted_tx,
        })
        .map_err(|_| SessionControlError::Closed)?;
    accepted_rx
        .await
        .unwrap_or(Err(SessionControlError::Closed))
}

/// Events emitted by a long-lived desktop ACP session.
#[derive(Debug)]
#[non_exhaustive]
pub enum DesktopSessionEvent {
    Update(Box<SessionUpdate>),
    TurnFinished(StopReason),
    Failed(String),
}

/// Errors returned when the UI cannot deliver a command to the session actor.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum SessionControlError {
    #[error("the ACP session is closed")]
    Closed,
    #[error("the ACP session is already processing a prompt")]
    Busy,
}

/// A persistent ACP session suitable for ownership by a GPUI or Tauri model.
///
/// Dropping the handle aborts the connection task. For an external ACP agent,
/// this also causes the official SDK to terminate the child process group.
pub struct DesktopSession {
    handshake: AgentHandshake,
    commands: mpsc::UnboundedSender<SessionCommand>,
    events: mpsc::UnboundedReceiver<DesktopSessionEvent>,
    task: Option<tokio::task::JoinHandle<()>>,
}

/// Cloneable command side of a desktop ACP session.
#[derive(Clone)]
pub struct DesktopSessionHandle {
    handshake: AgentHandshake,
    commands: mpsc::UnboundedSender<SessionCommand>,
}

/// Exclusive event side of a desktop ACP session.
pub struct DesktopSessionEvents {
    commands: mpsc::UnboundedSender<SessionCommand>,
    events: mpsc::UnboundedReceiver<DesktopSessionEvent>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl DesktopSession {
    /// Initialize an ACP agent and create one persistent chat session.
    pub async fn connect(
        agent: impl ConnectTo<Client> + 'static,
        cwd: impl Into<PathBuf>,
        delegate: Arc<dyn ClientDelegate>,
    ) -> Result<Self, agent_client_protocol::Error> {
        let cwd = cwd.into();
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let ready = Arc::new(std::sync::Mutex::new(Some(ready_tx)));
        let ready_for_session = Arc::clone(&ready);
        let events_for_task = events_tx.clone();

        let task = tokio::spawn(async move {
            let result =
                with_initialized_session(agent, cwd, delegate, async move |handshake, session| {
                    if let Some(sender) = ready_for_session.lock().expect("ready mutex").take() {
                        let _ = sender.send(Ok(handshake));
                    }
                    drive_session(session, commands_rx, events_tx).await
                })
                .await;

            if let Err(error) = result {
                if let Some(sender) = ready.lock().expect("ready mutex").take() {
                    let _ = sender.send(Err(error));
                } else {
                    let _ = events_for_task.send(DesktopSessionEvent::Failed(error.to_string()));
                }
            }
        });

        match ready_rx.await {
            Ok(Ok(handshake)) => Ok(Self {
                handshake,
                commands: commands_tx,
                events: events_rx,
                task: Some(task),
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(agent_client_protocol::Error::internal_error()),
        }
    }

    #[must_use]
    pub fn handshake(&self) -> &AgentHandshake {
        &self.handshake
    }

    /// Start a turn. A second prompt is rejected until `TurnFinished` arrives.
    pub async fn prompt(&self, text: impl Into<String>) -> Result<(), SessionControlError> {
        send_prompt_command(&self.commands, prompt_blocks(text, Vec::new())).await
    }

    /// Start a turn whose prompt carries attachment blocks after the text.
    ///
    /// Attachments should be marked with [`crate::ATTACHMENT_META_KEY`] so the
    /// server routes them to [`crate::Prompt::attachments`] instead of the
    /// flattened text.
    pub async fn prompt_with_attachments(
        &self,
        text: impl Into<String>,
        attachments: Vec<ContentBlock>,
    ) -> Result<(), SessionControlError> {
        send_prompt_command(&self.commands, prompt_blocks(text, attachments)).await
    }

    /// Request cancellation of the active turn. It is a no-op while idle.
    pub fn cancel(&self) -> Result<(), SessionControlError> {
        self.commands
            .send(SessionCommand::Cancel)
            .map_err(|_| SessionControlError::Closed)
    }

    /// Receive the next protocol update or turn completion.
    pub async fn next_event(&mut self) -> Option<DesktopSessionEvent> {
        self.events.recv().await
    }

    /// Gracefully stop the session actor and wait for transport cleanup.
    pub async fn shutdown(mut self) {
        let _ = self.commands.send(SessionCommand::Shutdown);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    /// Split the session so UI state can issue commands while a background task
    /// exclusively waits for streaming events.
    #[must_use]
    pub fn split(mut self) -> (DesktopSessionHandle, DesktopSessionEvents) {
        let handle = DesktopSessionHandle {
            handshake: self.handshake.clone(),
            commands: self.commands.clone(),
        };
        let events = DesktopSessionEvents {
            commands: self.commands.clone(),
            events: std::mem::replace(&mut self.events, mpsc::unbounded_channel().1),
            task: self.task.take(),
        };
        (handle, events)
    }
}

impl Drop for DesktopSession {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl DesktopSessionHandle {
    #[must_use]
    pub fn handshake(&self) -> &AgentHandshake {
        &self.handshake
    }

    pub async fn prompt(&self, text: impl Into<String>) -> Result<(), SessionControlError> {
        send_prompt_command(&self.commands, prompt_blocks(text, Vec::new())).await
    }

    /// Start a turn whose prompt carries attachment blocks after the text.
    ///
    /// See [`DesktopSession::prompt_with_attachments`].
    pub async fn prompt_with_attachments(
        &self,
        text: impl Into<String>,
        attachments: Vec<ContentBlock>,
    ) -> Result<(), SessionControlError> {
        send_prompt_command(&self.commands, prompt_blocks(text, attachments)).await
    }

    pub fn cancel(&self) -> Result<(), SessionControlError> {
        self.commands
            .send(SessionCommand::Cancel)
            .map_err(|_| SessionControlError::Closed)
    }
}

impl DesktopSessionEvents {
    pub async fn next_event(&mut self) -> Option<DesktopSessionEvent> {
        self.events.recv().await
    }

    pub async fn shutdown(mut self) {
        let _ = self.commands.send(SessionCommand::Shutdown);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for DesktopSessionEvents {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn drive_session(
    mut session: ActiveSession<'_, Agent>,
    mut commands: mpsc::UnboundedReceiver<SessionCommand>,
    events: mpsc::UnboundedSender<DesktopSessionEvent>,
) -> Result<(), agent_client_protocol::Error> {
    while let Some(command) = commands.recv().await {
        match command {
            SessionCommand::Prompt { blocks, accepted } => {
                // The SDK's `send_prompt` only supports a single text block, so
                // build the request here and route the terminal `PromptResponse`
                // through an actor-owned channel instead of the SDK's private one.
                let (stop_tx, stop_rx) = oneshot::channel();
                session
                    .connection()
                    .send_request_to(
                        Agent,
                        PromptRequest::new(session.session_id().clone(), blocks),
                    )
                    .on_receiving_result(
                        async move |result: agent_client_protocol::Result<PromptResponse>| {
                            let _ = stop_tx.send(result.map(|response| response.stop_reason));
                            Ok(())
                        },
                    )?;
                let _ = accepted.send(Ok(()));
                if !drive_turn(&mut session, &mut commands, &events, stop_rx).await? {
                    break;
                }
            }
            SessionCommand::Cancel => {}
            SessionCommand::Shutdown => break,
        }
    }
    Ok(())
}

async fn drive_turn(
    session: &mut ActiveSession<'_, Agent>,
    commands: &mut mpsc::UnboundedReceiver<SessionCommand>,
    events: &mpsc::UnboundedSender<DesktopSessionEvent>,
    mut stop_rx: oneshot::Receiver<agent_client_protocol::Result<StopReason>>,
) -> Result<bool, agent_client_protocol::Error> {
    loop {
        // Biased order matters: session updates and the prompt response arrive
        // on separate channels, so pending updates must drain before the turn
        // is declared finished — otherwise a queued chunk leaks into the next
        // turn.
        tokio::select! {
            biased;
            message = session.read_update() => {
                // The terminal stop reason arrives via `stop_rx`; the SDK's own
                // `StopReason` message is only produced by `send_prompt`.
                if let SessionMessage::SessionMessage(dispatch) = message? {
                    let events = events.clone();
                    MatchDispatch::new(dispatch)
                        .if_notification(async move |notification: SessionNotification| {
                            let _ = events.send(DesktopSessionEvent::Update(Box::new(notification.update)));
                            Ok(())
                        })
                        .await
                        .otherwise_ignore()?;
                }
            },
            outcome = &mut stop_rx => {
                let reason = outcome
                    .map_err(|_| agent_client_protocol::Error::internal_error())??;
                let _ = events.send(DesktopSessionEvent::TurnFinished(reason));
                return Ok(true);
            }
            command = commands.recv() => match command {
                Some(SessionCommand::Cancel) => {
                    session.connection().send_notification(
                        CancelNotification::new(session.session_id().clone())
                    )?;
                }
                Some(SessionCommand::Shutdown) | None => {
                    session.connection().send_notification(
                        CancelNotification::new(session.session_id().clone())
                    )?;
                    return Ok(false);
                }
                Some(SessionCommand::Prompt { accepted, .. }) => {
                    let _ = accepted.send(Err(SessionControlError::Busy));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{acp_agent, AgentBackend, AgentEvent, BackendError, DenyPermissions, Prompt};
    use agent_client_protocol::schema::v1::{ContentBlock, ContentChunk, ResourceLink};
    use async_trait::async_trait;
    use futures::{stream, stream::BoxStream};
    use serde_json::Map;
    use std::path::Path;

    struct Echo;
    struct Slow;

    #[async_trait]
    impl AgentBackend for Echo {
        type Session = ();

        async fn prompt(
            &self,
            session: (),
            prompt: Prompt,
            _cwd: &Path,
        ) -> Result<BoxStream<'static, Result<AgentEvent<()>, BackendError>>, BackendError>
        {
            Ok(Box::pin(stream::iter([
                Ok(AgentEvent::Text(format!(
                    "echo: {} attachments={}",
                    prompt.text,
                    prompt.attachments.len()
                ))),
                Ok(AgentEvent::Finished(session)),
            ])))
        }
    }

    #[async_trait]
    impl AgentBackend for Slow {
        type Session = ();

        async fn prompt(
            &self,
            _session: (),
            _prompt: Prompt,
            _cwd: &Path,
        ) -> Result<BoxStream<'static, Result<AgentEvent<()>, BackendError>>, BackendError>
        {
            Ok(Box::pin(stream::pending()))
        }
    }

    #[tokio::test]
    async fn persistent_session_streams_multiple_turns() {
        let mut session =
            DesktopSession::connect(acp_agent(Echo), "/tmp", Arc::new(DenyPermissions))
                .await
                .expect("desktop session");

        for prompt in ["one", "two"] {
            session.prompt(prompt).await.expect("accepted prompt");
            let update = session.next_event().await.expect("message update");
            let DesktopSessionEvent::Update(update) = update else {
                panic!("expected update");
            };
            let SessionUpdate::AgentMessageChunk(ContentChunk {
                content: ContentBlock::Text(text),
                ..
            }) = *update
            else {
                panic!("expected text update");
            };
            assert_eq!(text.text, format!("echo: {prompt} attachments=0"));
            assert!(matches!(
                session.next_event().await,
                Some(DesktopSessionEvent::TurnFinished(StopReason::EndTurn))
            ));
        }

        session.shutdown().await;
    }

    #[tokio::test]
    async fn split_session_accepts_commands_while_events_are_owned_elsewhere() {
        let session = DesktopSession::connect(acp_agent(Echo), "/tmp", Arc::new(DenyPermissions))
            .await
            .expect("desktop session");
        let (handle, mut events) = session.split();

        handle.prompt("split").await.expect("accepted prompt");
        assert!(matches!(
            events.next_event().await,
            Some(DesktopSessionEvent::Update(_))
        ));
        assert!(matches!(
            events.next_event().await,
            Some(DesktopSessionEvent::TurnFinished(_))
        ));
        events.shutdown().await;
    }

    #[tokio::test]
    async fn prompt_with_attachments_reaches_backend() {
        let mut session =
            DesktopSession::connect(acp_agent(Echo), "/tmp", Arc::new(DenyPermissions))
                .await
                .expect("desktop session");

        let mut meta = Map::new();
        meta.insert(
            crate::ATTACHMENT_META_KEY.into(),
            serde_json::json!({"oss_key": "k1"}),
        );
        let attachment =
            ContentBlock::ResourceLink(ResourceLink::new("chart.png", "oss://k1").meta(meta));
        session
            .prompt_with_attachments("with files", vec![attachment])
            .await
            .expect("accepted prompt");

        let update = session.next_event().await.expect("message update");
        let DesktopSessionEvent::Update(update) = update else {
            panic!("expected update");
        };
        let SessionUpdate::AgentMessageChunk(ContentChunk {
            content: ContentBlock::Text(text),
            ..
        }) = *update
        else {
            panic!("expected text update");
        };
        assert_eq!(text.text, "echo: with files attachments=1");
        assert!(matches!(
            session.next_event().await,
            Some(DesktopSessionEvent::TurnFinished(StopReason::EndTurn))
        ));
        session.shutdown().await;
    }

    #[tokio::test]
    async fn session_rejects_overlap_and_cancels_active_turn() {
        let mut session =
            DesktopSession::connect(acp_agent(Slow), "/tmp", Arc::new(DenyPermissions))
                .await
                .expect("desktop session");

        session.prompt("wait").await.expect("accepted prompt");
        assert_eq!(
            session.prompt("overlap").await,
            Err(SessionControlError::Busy)
        );
        session.cancel().expect("cancel command");
        assert!(matches!(
            session.next_event().await,
            Some(DesktopSessionEvent::TurnFinished(StopReason::Cancelled))
        ));
        session.shutdown().await;
    }
}
