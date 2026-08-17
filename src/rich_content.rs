use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, ImageContent, ResourceLink, TextContent,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

pub const RICH_CONTENT_NAMESPACE: &str = "longbridge.ai/rich-content";
pub const CHART_MIME_TYPE: &str = "application/vnd.longbridge.chart+json";
pub const TABLE_MIME_TYPE: &str = "application/vnd.longbridge.table+json";
pub const RICH_CONTENT_VERSION: u8 = 1;

const CHART_TYPES: &[&str] = &[
    "line",
    "area",
    "bar",
    "column",
    "pie",
    "scatter",
    "histogram",
    "treemap",
    "word-cloud",
    "dual-axes",
    "radar",
    "pin-map",
    "path-map",
    "heat-map",
    "mind-map",
    "fishbone-diagram",
    "flow-diagram",
    "indented-tree",
    "network-graph",
    "organization-chart",
    "vis-text",
    "funnel",
    "boxplot",
    "sankey",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RichContentKind {
    Chart,
    Table,
    Svg,
    Html,
    Widget,
    Artifact,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RichContent {
    pub version: u8,
    pub content_id: String,
    pub kind: RichContentKind,
    pub mime_type: String,
    pub data: Value,
    pub fallback: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RichContentError {
    #[error("rich content id cannot be empty")]
    EmptyContentId,
    #[error("chart configuration must be a JSON object")]
    InvalidChart,
    #[error("chart type is missing")]
    MissingChartType,
    #[error("unsupported chart type: {0}")]
    UnsupportedChartType(String),
    #[error("table must contain at least one column")]
    EmptyTable,
    #[error("table row {row} has {actual} cells, expected {expected}")]
    InvalidTableRow {
        row: usize,
        expected: usize,
        actual: usize,
    },
    #[error("SVG contains active or external content")]
    UnsafeSvg,
    #[error("widget URI must use the widget scheme and contain no control characters")]
    InvalidWidgetUri,
}

impl RichContent {
    pub fn chart(content_id: impl Into<String>, mut data: Value) -> Result<Self, RichContentError> {
        let content_id = checked_id(content_id.into())?;
        let object = data.as_object_mut().ok_or(RichContentError::InvalidChart)?;
        let raw_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RichContentError::MissingChartType)?;
        let chart_type = normalize_chart_type(raw_type);
        if !CHART_TYPES.contains(&chart_type.as_str()) {
            return Err(RichContentError::UnsupportedChartType(raw_type.to_owned()));
        }
        object.insert("type".to_owned(), Value::String(chart_type));
        let fallback = chart_markdown_fallback(&data);
        let svg = crate::chart_svg::render_chart_svg(&data);
        Ok(Self {
            version: RICH_CONTENT_VERSION,
            content_id,
            kind: RichContentKind::Chart,
            mime_type: CHART_MIME_TYPE.to_owned(),
            data,
            fallback,
            svg,
        })
    }

    pub fn table(content_id: impl Into<String>, table: &Table) -> Result<Self, RichContentError> {
        let content_id = checked_id(content_id.into())?;
        validate_table(table)?;
        let fallback = table.to_markdown();
        Ok(Self {
            version: RICH_CONTENT_VERSION,
            content_id,
            kind: RichContentKind::Table,
            mime_type: TABLE_MIME_TYPE.to_owned(),
            data: serde_json::to_value(table).expect("table always serializes"),
            fallback,
            svg: None,
        })
    }

    pub fn opaque(
        content_id: impl Into<String>,
        kind: RichContentKind,
        mime_type: impl Into<String>,
        data: Value,
        fallback: impl Into<String>,
    ) -> Result<Self, RichContentError> {
        Ok(Self {
            version: RICH_CONTENT_VERSION,
            content_id: checked_id(content_id.into())?,
            kind,
            mime_type: mime_type.into(),
            data,
            fallback: fallback.into(),
            svg: None,
        })
    }

    pub fn svg(
        content_id: impl Into<String>,
        svg: impl Into<String>,
        fallback_label: impl AsRef<str>,
    ) -> Result<Self, RichContentError> {
        let svg = svg.into();
        if !is_safe_svg(&svg) {
            return Err(RichContentError::UnsafeSvg);
        }
        let label = fallback_label.as_ref();
        Ok(Self {
            version: RICH_CONTENT_VERSION,
            content_id: checked_id(content_id.into())?,
            kind: RichContentKind::Svg,
            mime_type: "image/svg+xml".to_owned(),
            data: Value::String(svg.clone()),
            fallback: label.to_owned(),
            svg: Some(svg),
        })
    }

    pub fn widget(
        content_id: impl Into<String>,
        uri: impl Into<String>,
        fallback: impl Into<String>,
    ) -> Result<Self, RichContentError> {
        let uri = uri.into();
        if !uri.starts_with("widget://")
            || uri.len() > 2_048
            || uri.chars().any(char::is_control)
            || uri.contains(['<', '>', '"', '\'', '`'])
        {
            return Err(RichContentError::InvalidWidgetUri);
        }
        Ok(Self {
            version: RICH_CONTENT_VERSION,
            content_id: checked_id(content_id.into())?,
            kind: RichContentKind::Widget,
            mime_type: "application/vnd.longbridge.widget+uri".to_owned(),
            data: serde_json::json!({ "uri": uri }),
            fallback: fallback.into(),
            svg: None,
        })
    }

    #[must_use]
    pub fn to_acp_chunks(&self) -> Vec<ContentChunk> {
        self.to_acp_chunks_with_meta(None)
    }

    #[must_use]
    pub fn to_acp_chunks_with_meta(&self, extra: Option<&Map<String, Value>>) -> Vec<ContentChunk> {
        let metadata = self.metadata();
        let metadata = merge_metadata(metadata, extra);
        let text = ContentBlock::Text(TextContent::new(&self.fallback).meta(metadata.clone()));
        let mut chunks = vec![ContentChunk::new(text)];
        if let Some(svg) = &self.svg {
            let image = ImageContent::new(STANDARD.encode(svg), "image/svg+xml")
                .uri(format!("longbridge-rich://{}/preview.svg", self.content_id))
                .meta(metadata.clone());
            chunks.push(ContentChunk::new(ContentBlock::Image(image)));
        }
        if self.kind == RichContentKind::Widget {
            if let Some(uri) = self.data.get("uri").and_then(Value::as_str) {
                let resource = ResourceLink::new(widget_title(uri), uri)
                    .mime_type(&self.mime_type)
                    .description(&self.fallback)
                    .meta(metadata);
                chunks.push(ContentChunk::new(ContentBlock::ResourceLink(resource)));
            }
        }
        chunks
    }

    #[must_use]
    pub fn svg_preview_chunk(&self) -> Option<ContentChunk> {
        let svg = self.svg.as_ref()?;
        let image = ImageContent::new(STANDARD.encode(svg), "image/svg+xml")
            .uri(format!("longbridge-rich://{}/preview.svg", self.content_id))
            .meta(self.metadata());
        Some(ContentChunk::new(ContentBlock::Image(image)))
    }

    #[must_use]
    pub fn metadata(&self) -> Map<String, Value> {
        let mut metadata = Map::new();
        metadata.insert(
            RICH_CONTENT_NAMESPACE.to_owned(),
            serde_json::to_value(self).expect("rich content always serializes"),
        );
        metadata
    }
}

fn merge_metadata(
    mut metadata: Map<String, Value>,
    extra: Option<&Map<String, Value>>,
) -> Map<String, Value> {
    if let Some(extra) = extra {
        metadata.extend(extra.clone());
    }
    metadata
}

fn widget_title(uri: &str) -> &'static str {
    if uri.starts_with("widget://quote/security/comparison") {
        "Security comparison"
    } else if uri.starts_with("widget://quote/security") {
        "Security quote"
    } else if uri.starts_with("widget://stock/list") {
        "Security list"
    } else {
        "Longbridge interactive content"
    }
}

impl Table {
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut output = String::new();
        if let Some(title) = self.title.as_deref().filter(|title| !title.is_empty()) {
            output.push_str("### ");
            output.push_str(title);
            output.push_str("\n\n");
        }
        output.push('|');
        for column in &self.columns {
            output.push(' ');
            output.push_str(&escape_markdown_cell(column));
            output.push_str(" |");
        }
        output.push_str("\n|");
        for _ in &self.columns {
            output.push_str(" --- |");
        }
        for row in &self.rows {
            output.push_str("\n|");
            for cell in row {
                output.push(' ');
                output.push_str(&escape_markdown_cell(cell));
                output.push_str(" |");
            }
        }
        output
    }
}

#[must_use]
pub fn supported_chart_types() -> &'static [&'static str] {
    CHART_TYPES
}

#[must_use]
pub fn normalize_chart_type(value: &str) -> String {
    match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "dualaxes" => "dual-axes".to_owned(),
        "wordcloud" => "word-cloud".to_owned(),
        "pinmap" => "pin-map".to_owned(),
        "pathmap" => "path-map".to_owned(),
        "heatmap" => "heat-map".to_owned(),
        "mindmap" => "mind-map".to_owned(),
        "fishbone" => "fishbone-diagram".to_owned(),
        "flow" => "flow-diagram".to_owned(),
        "indentedtree" => "indented-tree".to_owned(),
        "network" => "network-graph".to_owned(),
        "organization" => "organization-chart".to_owned(),
        "text" => "vis-text".to_owned(),
        value => value.to_owned(),
    }
}

#[must_use]
pub fn charts_from_markdown(markdown: &str, content_id_prefix: &str) -> Vec<RichContent> {
    let mut charts = Vec::new();
    let mut rest = markdown;
    while let Some(open) = rest.find("```vis-chart") {
        let after_open = &rest[open + "```vis-chart".len()..];
        let Some(body) = after_open
            .strip_prefix("\r\n")
            .or_else(|| after_open.strip_prefix('\n'))
        else {
            rest = after_open;
            continue;
        };
        let Some(close) = body.find("```") else {
            break;
        };
        if let Ok(data) = serde_json::from_str::<Value>(body[..close].trim()) {
            let id = format!("{content_id_prefix}:chart-{}", charts.len() + 1);
            if let Ok(chart) = RichContent::chart(id, data) {
                charts.push(chart);
            }
        }
        rest = &body[close + 3..];
    }
    charts
}

fn checked_id(content_id: String) -> Result<String, RichContentError> {
    if content_id.trim().is_empty() {
        Err(RichContentError::EmptyContentId)
    } else {
        Ok(content_id)
    }
}

fn validate_table(table: &Table) -> Result<(), RichContentError> {
    if table.columns.is_empty() {
        return Err(RichContentError::EmptyTable);
    }
    for (index, row) in table.rows.iter().enumerate() {
        if row.len() != table.columns.len() {
            return Err(RichContentError::InvalidTableRow {
                row: index,
                expected: table.columns.len(),
                actual: row.len(),
            });
        }
    }
    Ok(())
}

pub(crate) fn chart_markdown_fallback(data: &Value) -> String {
    let title = data.get("title").and_then(Value::as_str);
    let rows = data.get("data").and_then(Value::as_array);
    let Some(rows) = rows.filter(|rows| !rows.is_empty()) else {
        if let Some(fallback) = crate::chart_svg::chart_structured_fallback(data) {
            return fallback;
        }
        return title.map_or_else(
            || "Chart data is unavailable.".to_owned(),
            |title| format!("### {title}\n\nChart data is unavailable."),
        );
    };

    let mut columns = Vec::<String>::new();
    for row in rows {
        if let Some(object) = row.as_object() {
            for key in object.keys() {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }
    if columns.is_empty() {
        if let Some(fallback) = crate::chart_svg::chart_structured_fallback(data) {
            return fallback;
        }
        return title.map_or_else(
            || "Chart contains non-tabular data.".to_owned(),
            |title| format!("### {title}\n\nChart contains non-tabular data."),
        );
    }

    let table = Table {
        columns: columns.clone(),
        rows: rows
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|column| row.get(column).map(value_to_cell).unwrap_or_default())
                    .collect()
            })
            .collect(),
        title: title.map(ToOwned::to_owned),
    };
    table.to_markdown()
}

fn value_to_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        value => serde_json::to_string(value).expect("JSON values always serialize"),
    }
}

fn escape_markdown_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], "<br>")
}

fn is_safe_svg(svg: &str) -> bool {
    let normalized = svg.to_ascii_lowercase();
    let active_fragments = [
        "<script",
        "<foreignobject",
        "javascript:",
        "data:text/html",
        "<!entity",
        "<!doctype",
    ];
    svg.trim_start().starts_with("<svg")
        && !active_fragments
            .iter()
            .any(|fragment| normalized.contains(fragment))
        && !has_svg_event_handler(&normalized)
        && !normalized.contains("href=\"http")
        && !normalized.contains("href='http")
}

fn has_svg_event_handler(svg: &str) -> bool {
    let bytes = svg.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index].is_ascii_whitespace()
            && bytes[index + 1] == b'o'
            && bytes[index + 2] == b'n'
        {
            let mut cursor = index + 3;
            while cursor < bytes.len()
                && (bytes[cursor].is_ascii_alphanumeric()
                    || matches!(bytes[cursor], b'-' | b'_' | b':'))
            {
                cursor += 1;
            }
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'=' {
                return true;
            }
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::ContentBlock;
    use serde_json::json;

    #[test]
    fn normalizes_every_supported_alias() {
        assert_eq!(normalize_chart_type("dualAxes"), "dual-axes");
        assert_eq!(normalize_chart_type("heatmap"), "heat-map");
        assert_eq!(normalize_chart_type("organization"), "organization-chart");
        assert_eq!(supported_chart_types().len(), 24);
    }

    #[test]
    fn rejects_unknown_chart_types_without_losing_the_name() {
        assert_eq!(
            RichContent::chart("chart-1", json!({ "type": "magic", "data": [] })),
            Err(RichContentError::UnsupportedChartType("magic".into()))
        );
    }

    #[test]
    fn chart_preserves_source_data_and_builds_fallback_and_svg() {
        let chart = RichContent::chart(
            "message-1:chart-1",
            json!({
                "type": "column",
                "title": "Profit < R&D",
                "data": [
                    { "category": "FY2024", "value": 7.09 },
                    { "category": "FY2025", "value": 3.79 }
                ]
            }),
        )
        .unwrap();
        assert!(chart.fallback.starts_with("### Profit < R&D\n\n|"));
        assert!(chart.fallback.contains("FY2024"));
        assert!(!chart.fallback.contains("```vis-chart"));
        let svg = chart.svg.as_deref().unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("Profit &lt; R&amp;D"));
        assert!(!svg.contains("<script"));
    }

    #[test]
    fn grouped_column_preview_preserves_categories_and_series_labels() {
        let chart = RichContent::chart(
            "chart-grouped",
            json!({
                "type": "column",
                "group": true,
                "data": [
                    { "category": "FY2024", "value": 7.09, "group": "Profit" },
                    { "category": "FY2025", "value": 3.79, "group": "Profit" },
                    { "category": "FY2024", "value": 4.54, "group": "R&D" },
                    { "category": "FY2025", "value": 6.41, "group": "R&D" }
                ]
            }),
        )
        .unwrap();
        assert!(chart.fallback.contains("FY2024"));
        assert!(chart.fallback.contains("Profit"));
        let svg = chart.svg.unwrap();
        assert_eq!(svg.matches("<rect x=").count(), 6);
        assert!(svg.contains("FY2024"));
        assert!(svg.contains("Profit"));
        assert!(svg.contains("R&amp;D"));
    }

    #[test]
    fn geo_chart_keeps_json_and_has_no_misleading_preview() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "pin-map", "data": [{ "longitude": 114.1, "latitude": 22.3 }] }),
        )
        .unwrap();
        assert_eq!(chart.data["type"], "pin-map");
        assert!(chart.svg.is_none());
        assert!(chart.fallback.contains("longitude"));
    }

    #[test]
    fn every_non_geo_chart_type_gets_an_svg_preview() {
        let samples = [
            json!({ "type": "column", "data": [{ "category": "Q1", "value": 1 }, { "category": "Q2", "value": 2 }] }),
            json!({ "type": "bar", "data": [{ "category": "East", "value": 5 }] }),
            json!({ "type": "line", "data": [{ "time": "Jan", "value": 3 }, { "time": "Feb", "value": 4 }] }),
            json!({ "type": "area", "data": [{ "time": "Jan", "value": 3 }, { "time": "Feb", "value": 4 }] }),
            json!({ "type": "pie", "data": [{ "category": "A", "value": 60 }, { "category": "B", "value": 40 }] }),
            json!({ "type": "scatter", "data": [{ "x": 1, "y": 2 }, { "x": 3, "y": 5 }] }),
            json!({ "type": "histogram", "data": [61, 62, 75, 75, 76, 88, 91, 95] }),
            json!({ "type": "funnel", "data": [
                { "category": "Visit", "value": 1000 },
                { "category": "Inquiry", "value": 600 },
                { "category": "Order", "value": 300 }
            ] }),
            json!({ "type": "radar", "data": [
                { "name": "Speed", "value": 8 },
                { "name": "Quality", "value": 9 },
                { "name": "Cost", "value": 6 }
            ] }),
            json!({ "type": "boxplot", "data": [
                { "category": "A", "value": 61 }, { "category": "A", "value": 70 },
                { "category": "A", "value": 82 }, { "category": "B", "value": 55 },
                { "category": "B", "value": 66 }, { "category": "B", "value": 74 }
            ] }),
            json!({ "type": "treemap", "data": [
                { "name": "Electronics", "value": 500 },
                { "name": "Appliances", "value": 300 },
                { "name": "Apparel", "value": 200 }
            ] }),
            json!({ "type": "word-cloud", "data": [
                { "text": "AI", "value": 50 }, { "text": "Cloud", "value": 30 }
            ] }),
            json!({ "type": "sankey", "data": [
                { "source": "Revenue", "target": "Cost", "value": 600 },
                { "source": "Revenue", "target": "Profit", "value": 400 }
            ] }),
            json!({ "type": "heat-map", "data": [
                { "x": "Mon", "y": "AM", "value": 3 }, { "x": "Mon", "y": "PM", "value": 8 },
                { "x": "Tue", "y": "AM", "value": 5 }, { "x": "Tue", "y": "PM", "value": 2 }
            ] }),
            json!({ "type": "dual-axes", "categories": ["2021", "2022"], "series": [
                { "type": "column", "data": [91, 99], "axisYTitle": "Sales" },
                { "type": "line", "data": [0.15, 0.22], "axisYTitle": "Margin" }
            ], "data": [] }),
            json!({ "type": "mind-map", "data": {
                "name": "Project", "children": [
                    { "name": "Plan", "children": [{ "name": "Scope" }] },
                    { "name": "Build" }
                ]
            } }),
            json!({ "type": "organization-chart", "data": {
                "name": "CEO", "children": [{ "name": "CTO" }, { "name": "CFO" }]
            } }),
            json!({ "type": "indented-tree", "data": {
                "name": "Docs", "children": [{ "name": "Guides" }]
            } }),
            json!({ "type": "fishbone-diagram", "data": {
                "name": "Churn", "children": [{ "name": "Price" }, { "name": "Support" }]
            } }),
            json!({ "type": "network-graph", "data": {
                "nodes": [{ "name": "A" }, { "name": "B" }, { "name": "C" }],
                "edges": [{ "source": "A", "target": "B" }, { "source": "B", "target": "C" }]
            } }),
            json!({ "type": "flow-diagram", "data": {
                "nodes": [{ "name": "Order" }, { "name": "Pay" }, { "name": "Ship" }],
                "edges": [
                    { "source": "Order", "target": "Pay", "name": "checkout" },
                    { "source": "Pay", "target": "Ship" }
                ]
            } }),
        ];
        for sample in samples {
            let chart_type = sample["type"].as_str().unwrap().to_owned();
            let chart = RichContent::chart("chart-1", sample).unwrap();
            let svg = chart
                .svg
                .as_deref()
                .unwrap_or_else(|| panic!("{chart_type} should render an SVG preview"));
            assert!(svg.starts_with("<svg"), "{chart_type} preview is SVG");
            assert!(!svg.contains("<script"), "{chart_type} preview is passive");
        }
    }

    #[test]
    fn network_chart_with_edge_array_data_renders_and_lists_edges() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "network", "data": [{ "source": "A", "target": "B" }] }),
        )
        .unwrap();
        assert_eq!(chart.data["type"], "network-graph");
        assert!(chart.svg.is_some());
        assert!(chart.fallback.contains("source"));
    }

    #[test]
    fn tree_chart_fallback_is_a_nested_list_not_unavailable() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "mind-map", "title": "Plan", "data": {
                "name": "Root", "children": [{ "name": "Leaf" }]
            } }),
        )
        .unwrap();
        assert!(chart.fallback.contains("### Plan"));
        assert!(chart.fallback.contains("- Root"));
        assert!(chart.fallback.contains("  - Leaf"));
        assert!(!chart.fallback.contains("unavailable"));
    }

    #[test]
    fn dual_axes_fallback_tabulates_categories_and_series() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "dual-axes", "categories": ["2021", "2022"], "series": [
                { "type": "column", "data": [91, 99], "axisYTitle": "Sales" },
                { "type": "line", "data": [0.15, 0.22], "axisYTitle": "Margin" }
            ] }),
        )
        .unwrap();
        assert!(chart.fallback.contains("| Category | Sales | Margin |"));
        assert!(chart.fallback.contains("| 2021 | 91 | 0.15 |"));
        let svg = chart.svg.unwrap();
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<path"));
    }

    #[test]
    fn radar_with_groups_draws_one_polygon_per_series() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "radar", "data": [
                { "name": "Speed", "value": 8, "group": "Vendor A" },
                { "name": "Quality", "value": 9, "group": "Vendor A" },
                { "name": "Cost", "value": 6, "group": "Vendor A" },
                { "name": "Speed", "value": 5, "group": "Vendor B" },
                { "name": "Quality", "value": 7, "group": "Vendor B" },
                { "name": "Cost", "value": 9, "group": "Vendor B" }
            ] }),
        )
        .unwrap();
        let svg = chart.svg.unwrap();
        assert!(svg.contains("Vendor A"));
        assert!(svg.contains("Vendor B"));
        assert_eq!(svg.matches("fill-opacity=\".22\"").count(), 2);
    }

    #[test]
    fn multi_series_line_draws_one_stroke_per_group() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "line", "data": [
                { "category": "2021", "value": 150, "group": "北京" },
                { "category": "2022", "value": 155, "group": "北京" },
                { "category": "2021", "value": 100, "group": "上海" },
                { "category": "2022", "value": 90, "group": "上海" }
            ] }),
        )
        .unwrap();
        let svg = chart.svg.unwrap();
        assert!(svg.contains("北京"));
        assert!(svg.contains("上海"));
        // one coloured polyline per series, not a single concatenated line
        assert_eq!(svg.matches("stroke-width=\"3\"").count(), 2);
        assert!(svg.contains("#00b7b7"));
        assert!(svg.contains("#7fe5bc"));
    }

    #[test]
    fn stacked_columns_pile_groups_instead_of_side_by_side() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "column", "stack": true, "data": [
                { "category": "2023", "value": 350, "group": "硬件" },
                { "category": "2023", "value": 220, "group": "软件" },
                { "category": "2024", "value": 380, "group": "硬件" },
                { "category": "2024", "value": 290, "group": "软件" }
            ] }),
        )
        .unwrap();
        let svg = chart.svg.unwrap();
        // 2 categories x 2 segments + 2 legend swatches = 6 rects
        assert_eq!(svg.matches("<rect").count(), 6);
        // stacked: both segments of a category share the same x
        let mut xs: Vec<&str> = svg
            .match_indices("<rect x=\"")
            .map(|(i, _)| &svg[i + 9..i + 14])
            .collect();
        xs.sort_unstable();
        xs.dedup();
        assert!(xs.len() <= 3, "expected shared x per category, got {xs:?}");
    }

    #[test]
    fn sankey_nodes_cycle_through_the_palette() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "sankey", "data": [
                { "source": "营收", "target": "硬件", "value": 60 },
                { "source": "营收", "target": "软件", "value": 40 },
                { "source": "硬件", "target": "毛利", "value": 30 }
            ] }),
        )
        .unwrap();
        let svg = chart.svg.unwrap();
        assert!(svg.contains("#00b7b7"));
        assert!(svg.contains("#7fe5bc"));
        assert!(svg.contains("#ffc700"));
        // links are neutral gray, matching the web renderer
        assert!(svg.contains("stroke=\"#94a3b8\""));
    }

    #[test]
    fn tree_branches_take_distinct_palette_colors() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "fishbone-diagram", "data": { "name": "股价下跌", "children": [
                { "name": "市场因素", "children": [{ "name": "流动性收紧" }] },
                { "name": "估值因素", "children": [{ "name": "利率上行" }] }
            ] } }),
        )
        .unwrap();
        let svg = chart.svg.unwrap();
        assert!(svg.contains("fill=\"#00b7b7\""));
        assert!(svg.contains("fill=\"#7fe5bc\""));
    }

    #[test]
    fn histogram_bins_raw_numeric_samples() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "histogram", "data": [60, 61, 62, 75, 76, 77, 90, 95] }),
        )
        .unwrap();
        let svg = chart.svg.unwrap();
        // Bars are now coloured per-index from the palette, not the flat teal mark.
        assert!(svg.contains("<rect fill=\"#00b7b7\""));
        assert!(svg.contains("\u{2013}"), "bin labels show ranges");
    }

    #[test]
    fn table_markdown_escapes_cells_and_keeps_rectangular_data() {
        let table = Table {
            columns: vec!["Year".into(), "Profit | USD".into()],
            rows: vec![vec!["FY2025".into(), "line 1\nline 2".into()]],
            title: Some("Results".into()),
        };
        let rich = RichContent::table("table-1", &table).unwrap();
        assert_eq!(rich.kind, RichContentKind::Table);
        assert!(rich.fallback.contains("Profit \\| USD"));
        assert!(rich.fallback.contains("line 1<br>line 2"));
    }

    #[test]
    fn malformed_table_is_rejected() {
        let error = RichContent::table(
            "table-1",
            &Table {
                columns: vec!["A".into(), "B".into()],
                rows: vec![vec!["only one".into()]],
                title: None,
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            RichContentError::InvalidTableRow {
                row: 0,
                expected: 2,
                actual: 1
            }
        );
    }

    #[test]
    fn acp_chunks_always_start_with_text_and_optionally_add_svg() {
        let chart = RichContent::chart(
            "chart-1",
            json!({ "type": "line", "data": [{ "time": "Q1", "value": 2 }] }),
        )
        .unwrap();
        let chunks = chart.to_acp_chunks();
        assert_eq!(chunks.len(), 2);
        let ContentBlock::Text(text) = &chunks[0].content else {
            panic!("expected text fallback");
        };
        assert!(text
            .meta
            .as_ref()
            .unwrap()
            .contains_key(RICH_CONTENT_NAMESPACE));
        let ContentBlock::Image(image) = &chunks[1].content else {
            panic!("expected SVG image");
        };
        assert_eq!(image.mime_type, "image/svg+xml");
        assert!(STANDARD.decode(&image.data).unwrap().starts_with(b"<svg"));
    }

    #[test]
    fn opaque_content_is_versioned_and_never_executes_itself() {
        let html = RichContent::opaque(
            "html-1",
            RichContentKind::Html,
            "text/html",
            json!({ "html": "<script>alert(1)</script>" }),
            "```html\n[Interactive content omitted]\n```",
        )
        .unwrap();
        assert_eq!(html.version, 1);
        assert!(html.svg.is_none());
        assert!(html
            .to_acp_chunks()
            .iter()
            .all(|chunk| matches!(chunk.content, ContentBlock::Text(_))));
    }

    #[test]
    fn widget_has_text_fallback_and_standard_resource_link() {
        let widget = RichContent::widget(
            "widget-1",
            "widget://quote/security/detail?symbol=TSLA.US&time_range=1",
            "[TSLA.US](https://longbridge.com/quote/tsla.us)",
        )
        .unwrap();
        let chunks = widget.to_acp_chunks();
        assert_eq!(chunks.len(), 2);
        assert!(matches!(chunks[0].content, ContentBlock::Text(_)));
        let ContentBlock::ResourceLink(resource) = &chunks[1].content else {
            panic!("expected widget resource link");
        };
        assert_eq!(
            resource.uri,
            "widget://quote/security/detail?symbol=TSLA.US&time_range=1"
        );
    }

    #[test]
    fn widget_rejects_non_widget_and_markup_injection_uris() {
        assert_eq!(
            RichContent::widget("widget-1", "https://example.com", "fallback"),
            Err(RichContentError::InvalidWidgetUri)
        );
        assert_eq!(
            RichContent::widget("widget-1", "widget://quote/<script>", "fallback"),
            Err(RichContentError::InvalidWidgetUri)
        );
    }

    #[test]
    fn extracts_multiple_complete_charts_and_ignores_partial_fences() {
        let markdown = concat!(
            "Before\n```vis-chart\n{\"type\":\"pie\",\"data\":[{\"category\":\"A\",\"value\":1}]}\n```\n",
            "Between\n```vis-chart\n{\"type\":\"line\",\"data\":[{\"time\":\"Q1\",\"value\":2}]}\n```\n",
            "```vis-chart\n{\"type\":\"column\""
        );
        let charts = charts_from_markdown(markdown, "message-1");
        assert_eq!(charts.len(), 2);
        assert_eq!(charts[0].content_id, "message-1:chart-1");
        assert_eq!(charts[1].data["type"], "line");
    }

    #[test]
    fn accepts_passive_svg_and_rejects_active_content() {
        let svg = RichContent::svg(
            "svg-1",
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="5" cy="5" r="4"/></svg>"#,
            "Circle",
        )
        .unwrap();
        assert_eq!(svg.kind, RichContentKind::Svg);
        assert!(svg.svg_preview_chunk().is_some());

        assert_eq!(
            RichContent::svg(
                "svg-2",
                r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
                "Unsafe",
            ),
            Err(RichContentError::UnsafeSvg)
        );
        assert_eq!(
            RichContent::svg(
                "svg-3",
                r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="https://example.com/a.png"/></svg>"#,
                "External",
            ),
            Err(RichContentError::UnsafeSvg)
        );
        assert_eq!(
            RichContent::svg(
                "svg-4",
                r#"<svg xmlns="http://www.w3.org/2000/svg" onload = "alert(1)"/>"#,
                "Handler",
            ),
            Err(RichContentError::UnsafeSvg)
        );
    }

    #[test]
    fn svg_acp_fallback_does_not_duplicate_source_markup() {
        let rich = RichContent::svg(
            "svg-1",
            r#"<svg xmlns="http://www.w3.org/2000/svg"><circle r="4"/></svg>"#,
            "Architecture diagram",
        )
        .unwrap();
        assert_eq!(rich.fallback, "Architecture diagram");
        let chunks = rich.to_acp_chunks();
        let ContentBlock::Text(text) = &chunks[0].content else {
            panic!("expected text fallback");
        };
        assert!(!text.text.contains("<svg"));
        assert!(matches!(chunks[1].content, ContentBlock::Image(_)));
    }
}
