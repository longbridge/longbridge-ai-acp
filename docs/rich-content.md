# Longbridge rich content over ACP (`_meta` contract)

`longbridge agent` (and `longbridge acp`) speak standard [ACP](https://agentclientprotocol.com).
Every turn is delivered as ordinary ACP `session/update` notifications so **any**
ACP client works out of the box. Longbridge-specific richness (charts, tables,
widgets, interactive artifacts, and native provider events) rides **alongside**
that standard content in the content block's `_meta`, so a Longbridge-aware
client can render natively while generic clients keep working from the standard
fallback.

This document is the contract for **a Longbridge client** consuming that `_meta`.

## Where it lives

A turn streams as `session/update` notifications. The ones that carry content:

- `AgentMessageChunk` — assistant answer content
- `AgentThoughtChunk` — reasoning (has no rich `_meta`; plain text)
- `UserMessageChunk` — echoed user content while loading a session

Each carries a single ACP **content block** (`text`, `image`, `resource_link`, …).
The Longbridge metadata is on that content block's `_meta` map, under two keys:

| `_meta` key | Purpose |
| --- | --- |
| `longbridge.ai/rich-content` | A renderable artifact (chart / table / widget / …) with its native data, a Markdown fallback, and an optional SVG. |
| `longbridge.ai/event` | The raw Longbridge provider event, so a client can rebuild its native chat timeline. |

Both keys are optional and independent. A plain answer chunk may carry only
`longbridge.ai/event` (or neither). A chart chunk carries both.

## `longbridge.ai/rich-content`

For every renderable artifact the server emits **two** content blocks in order:

1. a `text` block whose text is the Markdown **fallback** (what generic clients show), and
2. when an SVG preview exists, an `image` block (`mimeType: "image/svg+xml"`,
   inline base64 `data`, plus a `longbridge-rich://<content_id>/preview.svg` `uri`).

**Both** blocks repeat the same object under `_meta["longbridge.ai/rich-content"]`:

```jsonc
{
  "version": 1,                       // RICH_CONTENT_VERSION; bump = breaking
  "content_id": "message-1:chart-0",  // stable per artifact within the turn
  "kind": "chart",                    // chart | table | svg | html | widget | artifact
  "mime_type": "application/vnd.longbridge.chart+json",
  "data": { /* native payload — see per-kind below */ },
  "fallback": "### Title\n\n| … |",   // Markdown; already shown as the text block
  "svg": "<svg …>…</svg>"             // optional; omitted when not renderable
}
```

### How a Longbridge client should render

Prefer the richest representation you support, in this order:

1. **`data`** → render natively (same payloads your web/TUI already use). Best;
   interactive and pixel-perfect.
2. **`svg`** → drop-in inline SVG if you don't (yet) have a native renderer for
   that `kind`.
3. **`text` block / `fallback`** → last resort (this is what generic clients use).

To avoid showing the fallback twice, when you render from `data`/`svg`, suppress
the paired `text`/`image` blocks that carry the same `content_id`.

### Per-`kind` payloads

| `kind` | `mime_type` | `data` shape | Native render |
| --- | --- | --- | --- |
| `chart` | `application/vnd.longbridge.chart+json` | a **vis-chart** spec: `{ "type": "line"\|"area"\|"column"\|"bar"\|"pie"\|"scatter"\|"histogram"\|"boxplot"\|"radar"\|"funnel"\|"treemap"\|"word-cloud"\|"dual-axes"\|"sankey"\|"network-graph"\|"flow-diagram"\|"mind-map"\|"organization-chart"\|"fishbone-diagram"\|"heat-map", … }` | your existing vis-chart renderer |
| `table` | `application/vnd.longbridge.table+json` | `{ "columns": [string], "rows": [[string]], "title"?: string }` | a native table |
| `widget` | (widget mime) | the widget descriptor; the paired block is also a `resource_link` whose `uri` is the `widget://…` URI | your in-app widget (quote / comparison / stock list / order ticket …) |
| `svg` | `image/svg+xml` | `{ }` (the SVG is in `svg`) | inline SVG |
| `html` | `text/html` | `{ "html": "…" }` | sandboxed HTML |
| `artifact` | varies | opaque; render `svg`/`fallback` | generic |

`content_id` is stable within a turn (`"<message-id>:chart-<n>"`), so repeated
chunks for the same artifact can be de-duplicated / updated in place.

## `longbridge.ai/event`

Carries the underlying Longbridge provider event verbatim so a Longbridge client
can reconstruct its native chat timeline (thinking, tool calls, references,
interactions) instead of inferring it from ACP updates:

```jsonc
{
  "event": "workflow_finished",   // e.g. message, node_tool_use_started/finished,
                                  //      human_interaction_required, workflow_finished
  "data": { /* the raw provider event body */ }
}
```

Generic clients ignore this key and render the standard ACP update. A Longbridge
client may prefer it as the source of truth for the timeline.

## Notes

- **Security:** whether a given chat/session can be prompted is enforced by the
  backend, not by the presence of these keys. SVG previews are sanitized before
  emission (no scripts / external references).
- **Forward-compat:** unknown `kind`s and unknown `_meta` keys must be ignored,
  not rejected. `version` gates breaking changes to the `longbridge.ai/rich-content`
  object.
- **Constants** (crate `longbridge_ai_acp`): `RICH_CONTENT_NAMESPACE`,
  `CHART_MIME_TYPE`, `TABLE_MIME_TYPE`, `RICH_CONTENT_VERSION`, and the
  `RichContent` / `RichContentKind` types are public — a Rust client can
  deserialize `_meta["longbridge.ai/rich-content"]` straight into `RichContent`.
