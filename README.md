# longbridge-ai-acp

Provider-neutral Agent Client Protocol runtime for Longbridge products. It
supports both sides needed by Longbridge products:

- expose a Longbridge AI agent to Zed and other ACP clients;
- connect a native Rust desktop UI to external ACP agents such as Codex and
  Claude through the official ACP SDK.

The crate has no dependency on the Longbridge OpenAPI SDK or any private desktop
API. A host implements `AgentBackend` with its own API client, endpoint,
authorization, and opaque session state. The CLI keeps its OpenAPI adapter in
`longbridge-terminal`; Longbridge Pro and Longbridge AI Desktop supply their own
private API adapters.

## Response compatibility

The server preserves provider-rich responses in two layers:

- one portable standard ACP representation for generic clients, using Markdown
  tables and links alongside plans, tool progress, and session titles;
- the exact provider event under `_meta["longbridge.ai/event"]` for Longbridge
  clients that reuse the existing Chat UI.

Standard fallbacks paired with a native event are tagged with
`longbridge.ai/standard-fallback`. Longbridge clients ignore those tagged
blocks, while generic ACP clients ignore the namespaced native event. This
prevents duplicate charts without reducing generic-client compatibility. ACP
v1 does not define output MIME negotiation, so the server does not send several
alternative representations and does not rely on a proprietary capability
handshake. Longbridge clients reconstruct richer UI from the native event;
other clients consistently receive the portable fallback.

Streaming normalization currently covers `vis-chart`, `html-live`,
`svg-inline`, `x-widget`, `[stock ...]`, and citation forms `[citation N]`,
`[^N]`, and `[N]`. Active HTML is never executed by this crate; unsafe SVG is
omitted instead of echoed as source markup.

Every non-geographic `vis-chart` type ships a static SVG preview for generic
ACP clients: column, bar, line, area, pie, scatter, histogram, funnel, radar,
boxplot, treemap, word-cloud, sankey, heat-map, dual-axes, mind-map,
organization-chart, indented-tree, fishbone-diagram, network-graph, and
flow-diagram. Map charts (`pin-map`, `path-map`) and `vis-text` keep the
Markdown fallback only. Tree and node/edge payloads whose `data` is an object
also degrade to a readable Markdown outline or edge table instead of
"Chart data is unavailable."

For an external subprocess agent, construct an `ExternalAgentConfig` and
connect `ExternalAgent` to an `agent_client_protocol::Client`.

Desktop applications normally use `with_session` for an embedded Longbridge
agent and `with_external_session` for a subprocess agent. Both yield one
persistent `ActiveSession`, so the UI can send multiple prompts without
recreating the conversation. Implement `ClientDelegate` to route permission
requests into the product's own confirmation UI; `DenyPermissions` is provided
for read-only/chat-only hosts.

Use `with_initialized_session` when the UI also needs the negotiated protocol
version, capabilities, and implementation metadata. `ExternalAgentKind::Codex`
and `ExternalAgentKind::Claude` produce launch configurations for the separately
installed `codex-acp` and `claude-agent-acp` adapters. The native provider CLIs
do not expose ACP themselves.

For a UI model that must outlive one callback, use `DesktopSession`. It owns the
ACP connection and, for external agents, the subprocess. The UI sends commands
and receives protocol events without retaining SDK lifetimes:

```rust,no_run
use longbridge_ai_acp::{
    acp_agent, AgentBackend, DenyPermissions, DesktopSession, DesktopSessionEvent,
};
use std::sync::Arc;

# async fn example(agent: impl AgentBackend) -> Result<(), Box<dyn std::error::Error>> {
let mut session = DesktopSession::connect(
    acp_agent(agent),
    std::env::current_dir()?,
    Arc::new(DenyPermissions),
).await?;

session.prompt("Summarize my portfolio risk").await?;
while let Some(event) = session.next_event().await {
    match event {
        DesktopSessionEvent::Update(update) => println!("{update:?}"),
        DesktopSessionEvent::TurnFinished(_) => break,
        DesktopSessionEvent::Failed(error) => return Err(error.into()),
    }
}
session.shutdown().await;
# Ok(())
# }
```

`DesktopSession::cancel` and `shutdown` are safe UI lifecycle operations. A
second prompt before `TurnFinished` returns `SessionControlError::Busy`.
