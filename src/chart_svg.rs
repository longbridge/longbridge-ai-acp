//! Static SVG previews for `vis-chart` payloads.
//!
//! Every renderer draws into the same 800x450 shell so generic ACP clients
//! receive a consistent inline preview. Layouts are intentionally simple:
//! the preview approximates the chart, while Longbridge clients rebuild the
//! interactive version from the preserved JSON payload.

use serde_json::{Map, Value};
use std::fmt::Write as _;

const SERIES_COLORS: &[&str] = &[
    "#00b7b7", "#7fe5bc", "#ffc700", "#ff716e", "#fb6ebc", "#de3250", "#805cff",
];
const HEAT_SHADES: &[&str] = &[
    "#e5f8f8", "#bfefef", "#8ce3e3", "#55d3d3", "#22c2c2", "#00b7b7",
];
const MAX_TREE_DEPTH: usize = 6;
const MAX_TREE_LEAVES: usize = 24;

pub(crate) fn render_chart_svg(data: &Value) -> Option<String> {
    let chart_type = data.get("type")?.as_str()?;
    match chart_type {
        "dual-axes" => return render_dual_axes(data),
        "mind-map" | "organization-chart" | "indented-tree" | "fishbone-diagram" => {
            return render_tree(data);
        }
        "network-graph" => return render_network(data),
        "flow-diagram" => return render_flow(data),
        _ => {}
    }
    let items = data.get("data")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    match chart_type {
        "histogram" => render_histogram(data, items),
        "scatter" => render_scatter(data, items),
        "heat-map" => render_heat_map(data, items),
        "boxplot" => render_boxplot(data, items),
        "treemap" => render_treemap(data, items),
        "word-cloud" => render_word_cloud(data, items),
        "sankey" => render_sankey(data, items),
        "radar" => render_radar(data, items),
        _ => {
            if chart_type == "column" && has_multiple_groups(items) {
                if data.get("stack").and_then(Value::as_bool) == Some(true) {
                    return Some(render_stacked_columns(data, items));
                }
                return Some(render_grouped_columns(data, items));
            }
            if matches!(chart_type, "line" | "area") && has_multiple_groups(items) {
                return render_multi_plot(data, items, chart_type);
            }
            let points = chart_points(items);
            if points.is_empty() {
                return None;
            }
            match chart_type {
                "column" => Some(render_columns(data, &points)),
                "bar" => Some(render_bars(data, &points)),
                "line" | "area" => Some(render_plot(data, &points, chart_type)),
                "pie" => Some(render_pie(data, &points)),
                "funnel" => Some(render_funnel(data, &points)),
                _ => None,
            }
        }
    }
}

/// Markdown fallback for chart payloads whose `data` is not a flat array of
/// records: trees, node/edge graphs, and dual-axes series.
pub(crate) fn chart_structured_fallback(data: &Value) -> Option<String> {
    let title = data.get("title").and_then(Value::as_str);
    let body = tree_fallback(data)
        .or_else(|| graph_fallback(data))
        .or_else(|| dual_axes_fallback(data))?;
    Some(title.map_or_else(|| body.clone(), |title| format!("### {title}\n\n{body}")))
}

fn tree_fallback(data: &Value) -> Option<String> {
    let tree = parse_tree(data.get("data")?)?;
    let mut output = String::new();
    write_tree_list(&tree, 0, &mut output);
    Some(output.trim_end().to_owned())
}

fn write_tree_list(node: &TreeNode, depth: usize, output: &mut String) {
    if depth > MAX_TREE_DEPTH {
        return;
    }
    for _ in 0..depth {
        output.push_str("  ");
    }
    output.push_str("- ");
    output.push_str(&node.name);
    output.push('\n');
    for child in &node.children {
        write_tree_list(child, depth + 1, output);
    }
}

fn graph_fallback(data: &Value) -> Option<String> {
    let (_, edges) = parse_graph(data)?;
    if edges.is_empty() {
        return None;
    }
    let mut output = String::from("| From | To |\n| --- | --- |");
    for (source, target, _) in &edges {
        write!(output, "\n| {source} | {target} |").expect("writing to a String cannot fail");
    }
    Some(output)
}

fn dual_axes_fallback(data: &Value) -> Option<String> {
    let (categories, series) = parse_dual_axes(data)?;
    let mut output = String::from("| Category |");
    for (index, series) in series.iter().enumerate() {
        let name = series
            .title
            .clone()
            .unwrap_or_else(|| format!("Series {}", index + 1));
        write!(output, " {name} |").expect("writing to a String cannot fail");
    }
    output.push_str("\n| --- |");
    for _ in &series {
        output.push_str(" --- |");
    }
    for (index, category) in categories.iter().enumerate() {
        write!(output, "\n| {category} |").expect("writing to a String cannot fail");
        for series in &series {
            let cell = series
                .values
                .get(index)
                .map(std::string::ToString::to_string)
                .unwrap_or_default();
            write!(output, " {cell} |").expect("writing to a String cannot fail");
        }
    }
    Some(output)
}

fn has_multiple_groups(items: &[Value]) -> bool {
    let mut groups = Vec::new();
    for group in items
        .iter()
        .filter_map(|item| item.get("group").and_then(value_label))
    {
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    groups.len() > 1
}

fn render_grouped_columns(data: &Value, items: &[Value]) -> String {
    let mut categories = Vec::new();
    let mut groups = Vec::new();
    let mut values = Map::new();
    let mut max = 1.0_f64;
    for item in items {
        let Some(category) = item.get("category").and_then(value_label) else {
            continue;
        };
        let Some(group) = item.get("group").and_then(value_label) else {
            continue;
        };
        let Some(value) = item.get("value").and_then(value_number) else {
            continue;
        };
        if !categories.contains(&category) {
            categories.push(category.clone());
        }
        if !groups.contains(&group) {
            groups.push(group.clone());
        }
        max = max.max(value.abs());
        values.insert(format!("{category}\0{group}"), Value::from(value));
    }
    let category_width = 600.0 / usize_as_f64(categories.len().max(1));
    let bar_width = (category_width - 16.0) / usize_as_f64(groups.len().max(1));
    let mut body = String::from(r#"<line class="grid" x1="60" y1="390" x2="680" y2="390"/>"#);
    for (category_index, category) in categories.iter().enumerate() {
        for (group_index, group) in groups.iter().enumerate() {
            let value = values
                .get(&format!("{category}\0{group}"))
                .and_then(Value::as_f64)
                .unwrap_or_default();
            let height = value.abs() / max * 290.0;
            let x = 70.0
                + usize_as_f64(category_index) * category_width
                + usize_as_f64(group_index) * bar_width;
            let y = 390.0 - height;
            write!(
                body,
                r#"<rect x="{x:.1}" y="{y:.1}" width="{:.1}" height="{height:.1}" fill="{}"/>"#,
                (bar_width - 3.0).max(2.0),
                SERIES_COLORS[group_index % SERIES_COLORS.len()]
            )
            .expect("writing to a String cannot fail");
        }
        let label_x = 70.0 + usize_as_f64(category_index) * category_width + category_width / 2.0;
        write!(
            body,
            r#"<text x="{label_x:.1}" y="415" text-anchor="middle">{}</text>"#,
            xml_escape(category)
        )
        .expect("writing to a String cannot fail");
    }
    for (index, group) in groups.iter().enumerate() {
        write!(body, r#"<rect x="700" y="{}" width="12" height="12" fill="{}"/><text x="718" y="{}">{}</text>"#, 70 + index * 25, SERIES_COLORS[index % SERIES_COLORS.len()], 81 + index * 25, xml_escape(group)).expect("writing to a String cannot fail");
    }
    svg_shell(data, &body)
}

/// Stacked columns (the payload sets `stack: true`): the groups of each
/// category are piled bottom-up and scaled by the tallest stack, matching the
/// web renderer instead of drawing them side by side.
fn render_stacked_columns(data: &Value, items: &[Value]) -> String {
    let mut categories = Vec::new();
    let mut groups = Vec::new();
    let mut values = Map::new();
    for item in items {
        let Some(category) = item.get("category").and_then(value_label) else {
            continue;
        };
        let Some(group) = item.get("group").and_then(value_label) else {
            continue;
        };
        let Some(value) = item.get("value").and_then(value_number) else {
            continue;
        };
        if !categories.contains(&category) {
            categories.push(category.clone());
        }
        if !groups.contains(&group) {
            groups.push(group.clone());
        }
        values.insert(format!("{category}\0{group}"), Value::from(value.max(0.0)));
    }
    let mut max_total = 1.0_f64;
    for category in &categories {
        let total: f64 = groups
            .iter()
            .filter_map(|group| {
                values
                    .get(&format!("{category}\0{group}"))
                    .and_then(Value::as_f64)
            })
            .sum();
        max_total = max_total.max(total);
    }
    let category_width = 600.0 / usize_as_f64(categories.len().max(1));
    let bar_width = (category_width - 24.0).max(6.0);
    let mut body = String::from(r#"<line class="grid" x1="60" y1="390" x2="680" y2="390"/>"#);
    for (category_index, category) in categories.iter().enumerate() {
        let x = 70.0 + usize_as_f64(category_index) * category_width;
        let mut y = 390.0_f64;
        for (group_index, group) in groups.iter().enumerate() {
            let value = values
                .get(&format!("{category}\0{group}"))
                .and_then(Value::as_f64)
                .unwrap_or_default();
            let height = value / max_total * 290.0;
            if height <= 0.0 {
                continue;
            }
            y -= height;
            write!(
                body,
                r#"<rect x="{x:.1}" y="{y:.1}" width="{bar_width:.1}" height="{height:.1}" fill="{}"/>"#,
                SERIES_COLORS[group_index % SERIES_COLORS.len()]
            )
            .expect("writing to a String cannot fail");
        }
        write!(
            body,
            r#"<text x="{:.1}" y="415" text-anchor="middle">{}</text>"#,
            x + bar_width / 2.0,
            xml_escape(category)
        )
        .expect("writing to a String cannot fail");
    }
    for (index, group) in groups.iter().enumerate() {
        write!(body, r#"<rect x="700" y="{}" width="12" height="12" fill="{}"/><text x="718" y="{}">{}</text>"#, 70 + index * 25, SERIES_COLORS[index % SERIES_COLORS.len()], 81 + index * 25, xml_escape(group)).expect("writing to a String cannot fail");
    }
    svg_shell(data, &body)
}

fn chart_points(items: &[Value]) -> Vec<(String, f64)> {
    items
        .iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            let label = ["category", "time", "x", "name", "label", "date", "text"]
                .iter()
                .find_map(|key| object.get(*key).and_then(value_label))?;
            let value = ["value", "y", "count"]
                .iter()
                .find_map(|key| object.get(*key).and_then(value_number))?;
            value.is_finite().then_some((label, value))
        })
        .collect()
}

fn value_label(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.split_whitespace().collect::<Vec<_>>().join(" ")),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn value_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
}

fn svg_shell(data: &Value, body: &str) -> String {
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .map(xml_escape)
        .unwrap_or_default();
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" role="img" aria-label="{title}" viewBox="0 0 800 450"><style>text{{font:14px system-ui,sans-serif;fill:#334155}}.title{{font-size:20px;font-weight:600;fill:#0f172a}}.grid{{stroke:#e2e8f0}}.mark{{fill:#00b7b7}}.line{{fill:none;stroke:#00b7b7;stroke-width:3}}.edge{{stroke:#94a3b8;stroke-width:1.5;fill:none}}.node{{fill:#e0f2f2;stroke:#00b7b7;stroke-width:1.5}}</style><text class="title" x="50" y="35">{title}</text>{body}</svg>"#
    )
}

fn render_columns(data: &Value, points: &[(String, f64)]) -> String {
    let max = points
        .iter()
        .map(|(_, value)| value.abs())
        .fold(0.0, f64::max)
        .max(1.0);
    let width = 680.0 / usize_as_f64(points.len());
    let mut body = String::from(r#"<line class="grid" x1="60" y1="390" x2="760" y2="390"/>"#);
    for (index, (label, value)) in points.iter().enumerate() {
        let height = value.abs() / max * 300.0;
        let x = 70.0 + usize_as_f64(index) * width;
        let y = 390.0 - height;
        let fill = SERIES_COLORS[index % SERIES_COLORS.len()];
        write!(body, r#"<rect fill="{fill}" x="{x:.1}" y="{y:.1}" width="{:.1}" height="{height:.1}"/><text x="{:.1}" y="415" text-anchor="middle">{}</text>"#, (width - 12.0).max(2.0), x + (width - 12.0).max(2.0) / 2.0, xml_escape(label)).expect("writing to a String cannot fail");
    }
    svg_shell(data, &body)
}

fn render_bars(data: &Value, points: &[(String, f64)]) -> String {
    let max = points
        .iter()
        .map(|(_, value)| value.abs())
        .fold(0.0, f64::max)
        .max(1.0);
    let height = 330.0 / usize_as_f64(points.len());
    let mut body = String::new();
    for (index, (label, value)) in points.iter().enumerate() {
        let width = value.abs() / max * 560.0;
        let y = 60.0 + usize_as_f64(index) * height;
        let fill = SERIES_COLORS[index % SERIES_COLORS.len()];
        write!(body, r#"<text x="150" y="{:.1}" text-anchor="end">{}</text><rect fill="{fill}" x="165" y="{y:.1}" width="{width:.1}" height="{:.1}"/>"#, y + height * 0.55, xml_escape(label), (height - 8.0).max(2.0)).expect("writing to a String cannot fail");
    }
    svg_shell(data, &body)
}

fn render_plot(data: &Value, points: &[(String, f64)], chart_type: &str) -> String {
    let min = points
        .iter()
        .map(|(_, value)| *value)
        .fold(f64::INFINITY, f64::min);
    let max = points
        .iter()
        .map(|(_, value)| *value)
        .fold(f64::NEG_INFINITY, f64::max);
    let range = (max - min).max(1.0);
    let step = 680.0 / usize_as_f64(points.len().saturating_sub(1).max(1));
    let coordinates = points
        .iter()
        .enumerate()
        .map(|(index, (_, value))| {
            (
                60.0 + usize_as_f64(index) * step,
                390.0 - (*value - min) / range * 300.0,
            )
        })
        .collect::<Vec<_>>();
    let path = coordinates
        .iter()
        .enumerate()
        .map(|(index, (x, y))| format!("{} {x:.1} {y:.1}", if index == 0 { "M" } else { "L" }))
        .collect::<Vec<_>>()
        .join(" ");
    let mut body = if chart_type == "area" {
        format!(
            r##"<path d="{path} L 740 390 L 60 390 Z" fill="#00b7b7" fill-opacity=".18"/><path class="line" d="{path}"/>"##
        )
    } else if chart_type == "line" {
        format!(r#"<path class="line" d="{path}"/>"#)
    } else {
        String::new()
    };
    for (index, ((label, _), (x, y))) in points.iter().zip(&coordinates).enumerate() {
        write!(body, r#"<circle class="mark" cx="{x:.1}" cy="{y:.1}" r="4"/><text x="{x:.1}" y="415" text-anchor="middle">{}</text>"#, if points.len() <= 10 || index % 2 == 0 { xml_escape(label) } else { String::new() }).expect("writing to a String cannot fail");
    }
    svg_shell(data, &body)
}

/// Multi-series line/area: one polyline (or translucent area) per group,
/// coloured from the shared palette, with a legend. Without this, grouped
/// series were concatenated into a single zig-zag line.
fn render_multi_plot(data: &Value, items: &[Value], chart_type: &str) -> Option<String> {
    let mut categories: Vec<String> = Vec::new();
    let mut groups: Vec<(String, Vec<(usize, f64)>)> = Vec::new();
    for item in items {
        let Some(group) = item.get("group").and_then(value_label) else {
            continue;
        };
        let Some(label) = ["category", "x", "name", "label", "time"]
            .iter()
            .find_map(|key| item.get(*key).and_then(value_label))
        else {
            continue;
        };
        let Some(value) = ["value", "y"]
            .iter()
            .find_map(|key| item.get(*key).and_then(value_number))
        else {
            continue;
        };
        let column = categories
            .iter()
            .position(|existing| *existing == label)
            .unwrap_or_else(|| {
                categories.push(label);
                categories.len() - 1
            });
        if let Some((_, series)) = groups.iter_mut().find(|(name, _)| *name == group) {
            series.push((column, value));
        } else {
            groups.push((group, vec![(column, value)]));
        }
    }
    if categories.is_empty() || groups.is_empty() {
        return None;
    }
    let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
    for (_, series) in &groups {
        for (_, value) in series {
            min = min.min(*value);
            max = max.max(*value);
        }
    }
    let range = (max - min).max(1.0);
    let step = 640.0 / usize_as_f64(categories.len().saturating_sub(1).max(1));
    let scale = |column: usize, value: f64| {
        (
            60.0 + usize_as_f64(column) * step,
            390.0 - (value - min) / range * 300.0,
        )
    };
    let mut body = String::new();
    for (group_index, (group, series)) in groups.iter().enumerate() {
        let color = SERIES_COLORS[group_index % SERIES_COLORS.len()];
        let mut ordered = series.clone();
        ordered.sort_by_key(|(column, _)| *column);
        let path = ordered
            .iter()
            .enumerate()
            .map(|(index, (column, value))| {
                let (x, y) = scale(*column, *value);
                format!("{} {x:.1} {y:.1}", if index == 0 { "M" } else { "L" })
            })
            .collect::<Vec<_>>()
            .join(" ");
        if chart_type == "area" {
            let (first_x, _) = scale(ordered.first()?.0, 0.0);
            let (last_x, _) = scale(ordered.last()?.0, 0.0);
            write!(
                body,
                r#"<path d="{path} L {last_x:.1} 390 L {first_x:.1} 390 Z" fill="{color}" fill-opacity=".18"/>"#
            )
            .expect("writing to a String cannot fail");
        }
        write!(
            body,
            r#"<path d="{path}" fill="none" stroke="{color}" stroke-width="3"/>"#
        )
        .expect("writing to a String cannot fail");
        for (column, value) in &ordered {
            let (x, y) = scale(*column, *value);
            write!(
                body,
                r#"<circle cx="{x:.1}" cy="{y:.1}" r="4" fill="{color}"/>"#
            )
            .expect("writing to a String cannot fail");
        }
        write!(body, r#"<rect x="700" y="{}" width="12" height="12" fill="{color}"/><text x="718" y="{}">{}</text>"#, 70 + group_index * 25, 81 + group_index * 25, xml_escape(group)).expect("writing to a String cannot fail");
    }
    for (column, label) in categories.iter().enumerate() {
        if categories.len() <= 10 || column % 2 == 0 {
            let (x, _) = scale(column, min);
            write!(
                body,
                r#"<text x="{x:.1}" y="415" text-anchor="middle">{}</text>"#,
                xml_escape(label)
            )
            .expect("writing to a String cannot fail");
        }
    }
    Some(svg_shell(data, &body))
}

fn render_pie(data: &Value, points: &[(String, f64)]) -> String {
    let total = points.iter().map(|(_, value)| value.max(0.0)).sum::<f64>();
    if total <= 0.0 {
        return svg_shell(data, "");
    }
    let mut angle = -std::f64::consts::FRAC_PI_2;
    let mut body = String::new();
    for (index, (label, value)) in points.iter().enumerate() {
        let next = angle + value.max(0.0) / total * std::f64::consts::TAU;
        let (x1, y1) = (300.0 + 145.0 * angle.cos(), 235.0 + 145.0 * angle.sin());
        let (x2, y2) = (300.0 + 145.0 * next.cos(), 235.0 + 145.0 * next.sin());
        let large = i32::from(next - angle > std::f64::consts::PI);
        write!(body, r#"<path d="M 300 235 L {x1:.1} {y1:.1} A 145 145 0 {large} 1 {x2:.1} {y2:.1} Z" fill="{}"/><rect x="510" y="{}" width="14" height="14" fill="{}"/><text x="532" y="{}">{}</text>"#, SERIES_COLORS[index % SERIES_COLORS.len()], 90 + index * 28, SERIES_COLORS[index % SERIES_COLORS.len()], 102 + index * 28, xml_escape(label)).expect("writing to a String cannot fail");
        angle = next;
    }
    svg_shell(data, &body)
}

fn render_funnel(data: &Value, points: &[(String, f64)]) -> String {
    let max = points
        .iter()
        .map(|(_, value)| value.abs())
        .fold(0.0, f64::max)
        .max(1.0);
    let stage_height = 320.0 / usize_as_f64(points.len());
    let mut body = String::new();
    for (index, (label, value)) in points.iter().enumerate() {
        let top_width = value.abs() / max * 420.0;
        let bottom_width = points
            .get(index + 1)
            .map_or(top_width * 0.6, |(_, next)| next.abs() / max * 420.0);
        let y = 70.0 + usize_as_f64(index) * stage_height;
        let y2 = y + stage_height - 6.0;
        write!(
            body,
            r#"<polygon points="{:.1},{y:.1} {:.1},{y:.1} {:.1},{y2:.1} {:.1},{y2:.1}" fill="{}"/><text x="380" y="{:.1}" text-anchor="middle" style="fill:#ffffff">{} {}</text>"#,
            380.0 - top_width / 2.0,
            380.0 + top_width / 2.0,
            380.0 + bottom_width / 2.0,
            380.0 - bottom_width / 2.0,
            SERIES_COLORS[index % SERIES_COLORS.len()],
            y + stage_height / 2.0,
            xml_escape(label),
            value,
        )
        .expect("writing to a String cannot fail");
    }
    svg_shell(data, &body)
}

fn render_radar(data: &Value, items: &[Value]) -> Option<String> {
    let mut axes = Vec::new();
    let mut groups = Vec::new();
    let mut values = Map::new();
    let mut max = 1.0_f64;
    for item in items {
        let object = item.as_object()?;
        let label = ["name", "category", "label", "item"]
            .iter()
            .find_map(|key| object.get(*key).and_then(value_label))?;
        let value = ["value", "score", "y"]
            .iter()
            .find_map(|key| object.get(*key).and_then(value_number))?;
        let group = object
            .get("group")
            .and_then(value_label)
            .unwrap_or_default();
        if !axes.contains(&label) {
            axes.push(label.clone());
        }
        if !groups.contains(&group) {
            groups.push(group.clone());
        }
        max = max.max(value.abs());
        values.insert(format!("{group}\0{label}"), Value::from(value));
    }
    if axes.len() < 3 {
        return None;
    }
    let (cx, cy, radius) = (400.0, 240.0, 145.0);
    let axis_angle = |index: usize| {
        std::f64::consts::TAU * usize_as_f64(index) / usize_as_f64(axes.len())
            - std::f64::consts::FRAC_PI_2
    };
    let mut body = String::new();
    for ring in 1..=4_u32 {
        let ring_radius = radius * f64::from(ring) / 4.0;
        let ring_points = (0..axes.len())
            .map(|index| {
                let angle = axis_angle(index);
                format!(
                    "{:.1},{:.1}",
                    cx + ring_radius * angle.cos(),
                    cy + ring_radius * angle.sin()
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        write!(
            body,
            r#"<polygon points="{ring_points}" fill="none" class="grid"/>"#
        )
        .expect("writing to a String cannot fail");
    }
    for (index, axis) in axes.iter().enumerate() {
        let angle = axis_angle(index);
        let (x, y) = (cx + radius * angle.cos(), cy + radius * angle.sin());
        let (label_x, label_y) = (
            cx + (radius + 22.0) * angle.cos(),
            cy + (radius + 22.0) * angle.sin(),
        );
        write!(
            body,
            r#"<line class="grid" x1="{cx}" y1="{cy}" x2="{x:.1}" y2="{y:.1}"/><text x="{label_x:.1}" y="{label_y:.1}" text-anchor="middle">{}</text>"#,
            xml_escape(axis)
        )
        .expect("writing to a String cannot fail");
    }
    for (group_index, group) in groups.iter().enumerate() {
        let vertices = axes
            .iter()
            .enumerate()
            .map(|(index, axis)| {
                let value = values
                    .get(&format!("{group}\0{axis}"))
                    .and_then(Value::as_f64)
                    .unwrap_or_default();
                let scaled = value.abs() / max * radius;
                let angle = axis_angle(index);
                (cx + scaled * angle.cos(), cy + scaled * angle.sin())
            })
            .collect::<Vec<_>>();
        let polygon = vertices
            .iter()
            .map(|(x, y)| format!("{x:.1},{y:.1}"))
            .collect::<Vec<_>>()
            .join(" ");
        let color = SERIES_COLORS[group_index % SERIES_COLORS.len()];
        write!(
            body,
            r#"<polygon points="{polygon}" fill="{color}" fill-opacity=".22" stroke="{color}" stroke-width="2"/>"#
        )
        .expect("writing to a String cannot fail");
        // Colour each vertex by its axis, so a single-series radar is not one flat
        // teal shape — every spoke's value reads in its own colour.
        for (index, (x, y)) in vertices.iter().enumerate() {
            let dot = SERIES_COLORS[index % SERIES_COLORS.len()];
            write!(
                body,
                r#"<circle cx="{x:.1}" cy="{y:.1}" r="4.5" fill="{dot}"/>"#
            )
            .expect("writing to a String cannot fail");
        }
        if groups.len() > 1 && !group.is_empty() {
            write!(body, r#"<rect x="700" y="{}" width="12" height="12" fill="{color}"/><text x="718" y="{}">{}</text>"#, 70 + group_index * 25, 81 + group_index * 25, xml_escape(group)).expect("writing to a String cannot fail");
        }
    }
    Some(svg_shell(data, &body))
}

fn render_histogram(data: &Value, items: &[Value]) -> Option<String> {
    let labeled = chart_points(items);
    if !labeled.is_empty() {
        return Some(render_columns(data, &labeled));
    }
    let values = items
        .iter()
        .filter_map(|item| value_number(item).or_else(|| item.get("value").and_then(value_number)))
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let bin_count = values.len().clamp(2, 8);
    let width = ((max - min) / usize_as_f64(bin_count)).max(f64::EPSILON);
    let points = (0..bin_count)
        .map(|index| {
            let low = min + usize_as_f64(index) * width;
            let high = low + width;
            let count = values
                .iter()
                .filter(|value| **value >= low && (**value < high || index == bin_count - 1))
                .count();
            (format!("{low:.0}–{high:.0}"), usize_as_f64(count))
        })
        .collect::<Vec<_>>();
    Some(render_columns(data, &points))
}

fn render_scatter(data: &Value, items: &[Value]) -> Option<String> {
    let pairs = items
        .iter()
        .filter_map(|item| {
            let x = item.get("x").and_then(value_number)?;
            let y = item.get("y").and_then(value_number)?;
            (x.is_finite() && y.is_finite()).then_some((x, y))
        })
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        let points = chart_points(items);
        if points.is_empty() {
            return None;
        }
        return Some(render_plot(data, &points, "scatter"));
    }
    let x_min = pairs.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    let x_max = pairs
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    let y_min = pairs.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let y_max = pairs
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    let x_range = (x_max - x_min).max(1.0);
    let y_range = (y_max - y_min).max(1.0);
    let mut body = format!(
        r#"<line class="grid" x1="80" y1="390" x2="760" y2="390"/><line class="grid" x1="80" y1="70" x2="80" y2="390"/><text x="80" y="412" text-anchor="middle">{x_min}</text><text x="760" y="412" text-anchor="middle">{x_max}</text><text x="72" y="394" text-anchor="end">{y_min}</text><text x="72" y="78" text-anchor="end">{y_max}</text>"#
    );
    for (x, y) in &pairs {
        write!(
            body,
            r#"<circle class="mark" cx="{:.1}" cy="{:.1}" r="5" fill-opacity=".75"/>"#,
            80.0 + (x - x_min) / x_range * 660.0,
            390.0 - (y - y_min) / y_range * 310.0,
        )
        .expect("writing to a String cannot fail");
    }
    Some(svg_shell(data, &body))
}

fn render_heat_map(data: &Value, items: &[Value]) -> Option<String> {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut cells = Vec::new();
    let mut max = f64::EPSILON;
    for item in items {
        let x = item.get("x").and_then(value_label)?;
        let y = item.get("y").and_then(value_label)?;
        let value = item.get("value").and_then(value_number)?;
        if !xs.contains(&x) {
            xs.push(x.clone());
        }
        if !ys.contains(&y) {
            ys.push(y.clone());
        }
        max = max.max(value.abs());
        cells.push((x, y, value));
    }
    let cell_width = 620.0 / usize_as_f64(xs.len().max(1));
    let cell_height = 300.0 / usize_as_f64(ys.len().max(1));
    let mut body = String::new();
    for (x, y, value) in &cells {
        let column = xs.iter().position(|label| label == x)?;
        let row = ys.iter().position(|label| label == y)?;
        let intensity = value.abs() / max;
        let mut shade = 0;
        let mut threshold = 0.0;
        for index in 0..HEAT_SHADES.len() {
            if intensity >= threshold {
                shade = index;
            }
            threshold += 1.0 / usize_as_f64(HEAT_SHADES.len());
        }
        write!(
            body,
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{}"/>"#,
            130.0 + usize_as_f64(column) * cell_width,
            70.0 + usize_as_f64(row) * cell_height,
            (cell_width - 2.0).max(1.0),
            (cell_height - 2.0).max(1.0),
            HEAT_SHADES[shade]
        )
        .expect("writing to a String cannot fail");
    }
    for (column, x) in xs.iter().enumerate() {
        write!(
            body,
            r#"<text x="{:.1}" y="392" text-anchor="middle">{}</text>"#,
            130.0 + usize_as_f64(column) * cell_width + cell_width / 2.0,
            xml_escape(x)
        )
        .expect("writing to a String cannot fail");
    }
    for (row, y) in ys.iter().enumerate() {
        write!(
            body,
            r#"<text x="122" y="{:.1}" text-anchor="end">{}</text>"#,
            70.0 + usize_as_f64(row) * cell_height + cell_height / 2.0 + 5.0,
            xml_escape(y)
        )
        .expect("writing to a String cannot fail");
    }
    Some(svg_shell(data, &body))
}

struct BoxStats {
    label: String,
    min: f64,
    q1: f64,
    median: f64,
    q3: f64,
    max: f64,
}

fn render_boxplot(data: &Value, items: &[Value]) -> Option<String> {
    let mut labels = Vec::<String>::new();
    let mut samples = Vec::<Vec<f64>>::new();
    let mut boxes = Vec::<BoxStats>::new();
    for item in items {
        let object = item.as_object()?;
        let label = ["category", "name", "label", "x", "group"]
            .iter()
            .find_map(|key| object.get(*key).and_then(value_label))
            .unwrap_or_else(|| String::from("All"));
        let precomputed = ["min", "q1", "median", "q3", "max"]
            .iter()
            .map(|key| object.get(*key).and_then(value_number))
            .collect::<Option<Vec<_>>>();
        if let Some(stats) = precomputed {
            boxes.push(BoxStats {
                label,
                min: stats[0],
                q1: stats[1],
                median: stats[2],
                q3: stats[3],
                max: stats[4],
            });
            continue;
        }
        let Some(value) = ["value", "y"]
            .iter()
            .find_map(|key| object.get(*key).and_then(value_number))
        else {
            continue;
        };
        if let Some(index) = labels.iter().position(|existing| *existing == label) {
            samples[index].push(value);
        } else {
            labels.push(label);
            samples.push(vec![value]);
        }
    }
    for (label, mut values) in labels.into_iter().zip(samples) {
        values.sort_by(f64::total_cmp);
        boxes.push(BoxStats {
            label,
            min: values[0],
            q1: quantile(&values, 1, 4),
            median: quantile(&values, 1, 2),
            q3: quantile(&values, 3, 4),
            max: values[values.len() - 1],
        });
    }
    if boxes.is_empty() {
        return None;
    }
    let low = boxes
        .iter()
        .map(|item| item.min)
        .fold(f64::INFINITY, f64::min);
    let high = boxes
        .iter()
        .map(|item| item.max)
        .fold(f64::NEG_INFINITY, f64::max);
    let range = (high - low).max(1.0);
    let scale_y = |value: f64| 380.0 - (value - low) / range * 290.0;
    let slot = 700.0 / usize_as_f64(boxes.len());
    let mut body = String::from(r#"<line class="grid" x1="60" y1="390" x2="760" y2="390"/>"#);
    for (index, item) in boxes.iter().enumerate() {
        let center = 60.0 + usize_as_f64(index) * slot + slot / 2.0;
        let half = (slot * 0.18).clamp(12.0, 45.0);
        write!(
            body,
            r##"<line class="edge" x1="{center:.1}" y1="{:.1}" x2="{center:.1}" y2="{:.1}"/><line class="edge" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/><line class="edge" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/><rect class="node" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}"/><line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#00b7b7" stroke-width="2.5"/><text x="{center:.1}" y="415" text-anchor="middle">{}</text>"##,
            scale_y(item.min),
            scale_y(item.max),
            center - half,
            scale_y(item.min),
            center + half,
            scale_y(item.min),
            center - half,
            scale_y(item.max),
            center + half,
            scale_y(item.max),
            center - half,
            scale_y(item.q3),
            half * 2.0,
            (scale_y(item.q1) - scale_y(item.q3)).max(1.0),
            center - half,
            scale_y(item.median),
            center + half,
            scale_y(item.median),
            xml_escape(&item.label)
        )
        .expect("writing to a String cannot fail");
    }
    Some(svg_shell(data, &body))
}

fn quantile(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    if sorted.len() < 2 {
        return sorted.first().copied().unwrap_or_default();
    }
    let position = (sorted.len() - 1) * numerator;
    let index = position / denominator;
    let fraction = usize_as_f64(position % denominator) / usize_as_f64(denominator);
    let next = sorted.get(index + 1).copied().unwrap_or(sorted[index]);
    sorted[index] + (next - sorted[index]) * fraction
}

fn render_treemap(data: &Value, items: &[Value]) -> Option<String> {
    let mut leaves = items
        .iter()
        .filter_map(|item| {
            let label = ["name", "category", "label"]
                .iter()
                .find_map(|key| item.get(*key).and_then(value_label))?;
            let value = item.get("value").and_then(value_number)?;
            (value > 0.0).then_some((label, value))
        })
        .collect::<Vec<_>>();
    if leaves.is_empty() {
        return None;
    }
    leaves.sort_by(|left, right| right.1.total_cmp(&left.1));
    let mut body = String::new();
    layout_treemap(&leaves, 50.0, 60.0, 700.0, 330.0, &mut 0, &mut body);
    Some(svg_shell(data, &body))
}

fn layout_treemap(
    leaves: &[(String, f64)],
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    color_index: &mut usize,
    body: &mut String,
) {
    if leaves.is_empty() || width < 1.0 || height < 1.0 {
        return;
    }
    if leaves.len() == 1 {
        let (label, value) = &leaves[0];
        let color = SERIES_COLORS[*color_index % SERIES_COLORS.len()];
        *color_index += 1;
        write!(
            body,
            r##"<rect x="{x:.1}" y="{y:.1}" width="{:.1}" height="{:.1}" fill="{color}" fill-opacity=".85" stroke="#ffffff" stroke-width="2"/>"##,
            width.max(1.0),
            height.max(1.0)
        )
        .expect("writing to a String cannot fail");
        if width > 60.0 && height > 24.0 {
            write!(
                body,
                r#"<text x="{:.1}" y="{:.1}" style="fill:#ffffff">{} {}</text>"#,
                x + 8.0,
                y + 20.0,
                xml_escape(label),
                value
            )
            .expect("writing to a String cannot fail");
        }
        return;
    }
    let total = leaves.iter().map(|(_, value)| value).sum::<f64>();
    let mut split = 1;
    let mut accumulated = leaves[0].1;
    while split < leaves.len() - 1 && accumulated < total / 2.0 {
        accumulated += leaves[split].1;
        split += 1;
    }
    let ratio = if total > 0.0 {
        accumulated / total
    } else {
        0.5
    };
    let (first, second) = leaves.split_at(split);
    if width >= height {
        let first_width = width * ratio;
        layout_treemap(first, x, y, first_width, height, color_index, body);
        layout_treemap(
            second,
            x + first_width,
            y,
            width - first_width,
            height,
            color_index,
            body,
        );
    } else {
        let first_height = height * ratio;
        layout_treemap(first, x, y, width, first_height, color_index, body);
        layout_treemap(
            second,
            x,
            y + first_height,
            width,
            height - first_height,
            color_index,
            body,
        );
    }
}

fn render_word_cloud(data: &Value, items: &[Value]) -> Option<String> {
    let mut words = items
        .iter()
        .filter_map(|item| {
            let label = ["text", "name", "word", "label"]
                .iter()
                .find_map(|key| item.get(*key).and_then(value_label))?;
            let value = item
                .get("value")
                .and_then(value_number)
                .unwrap_or(1.0)
                .max(0.0);
            Some((label, value))
        })
        .collect::<Vec<_>>();
    if words.is_empty() {
        return None;
    }
    words.sort_by(|left, right| right.1.total_cmp(&left.1));
    words.truncate(30);
    let max = words
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0, f64::max)
        .max(1.0);
    let mut body = String::new();
    let mut x = 60.0;
    let mut y = 100.0;
    let mut row_height = 0.0_f64;
    for (index, (label, value)) in words.iter().enumerate() {
        let size = 12.0 + value / max * 28.0;
        let estimated_width = usize_as_f64(label.chars().count()).max(1.0) * size * 0.68 + 18.0;
        if x + estimated_width > 760.0 {
            x = 60.0;
            y += row_height + 12.0;
            row_height = 0.0;
        }
        if y > 420.0 {
            break;
        }
        write!(
            body,
            r#"<text x="{x:.1}" y="{y:.1}" style="font-size:{size:.0}px;fill:{}">{}</text>"#,
            SERIES_COLORS[index % SERIES_COLORS.len()],
            xml_escape(label)
        )
        .expect("writing to a String cannot fail");
        x += estimated_width;
        row_height = row_height.max(size);
    }
    Some(svg_shell(data, &body))
}

fn render_sankey(data: &Value, items: &[Value]) -> Option<String> {
    let edges = items
        .iter()
        .filter_map(|item| {
            let source = item.get("source").and_then(value_label)?;
            let target = item.get("target").and_then(value_label)?;
            let value = item.get("value").and_then(value_number).unwrap_or(1.0);
            (value > 0.0 && source != target).then_some((source, target, value))
        })
        .collect::<Vec<_>>();
    if edges.is_empty() {
        return None;
    }
    let mut nodes = Vec::<String>::new();
    for (source, target, _) in &edges {
        if !nodes.contains(source) {
            nodes.push(source.clone());
        }
        if !nodes.contains(target) {
            nodes.push(target.clone());
        }
    }
    let depths = node_depths(&nodes, &edges);
    let max_depth = depths.iter().copied().max().unwrap_or(0);
    let mut flow = vec![0.0_f64; nodes.len()];
    for (source, target, value) in &edges {
        let source_index = nodes.iter().position(|node| node == source)?;
        let target_index = nodes.iter().position(|node| node == target)?;
        flow[source_index] += value;
        flow[target_index] = flow[target_index].max(
            edges
                .iter()
                .filter(|(_, edge_target, _)| edge_target == target)
                .map(|(_, _, edge_value)| edge_value)
                .sum(),
        );
    }
    let mut column_sums = vec![0.0_f64; max_depth + 1];
    for (index, depth) in depths.iter().enumerate() {
        column_sums[*depth] += flow[index].max(1.0);
    }
    let max_column = column_sums.iter().copied().fold(1.0, f64::max);
    let scale = 280.0 / max_column;
    let column_width = 620.0 / usize_as_f64(max_depth.max(1));
    let mut node_x = vec![0.0_f64; nodes.len()];
    let mut node_y = vec![0.0_f64; nodes.len()];
    let mut node_height = vec![0.0_f64; nodes.len()];
    let mut column_cursor = vec![70.0_f64; max_depth + 1];
    let mut body = String::new();
    for (index, node) in nodes.iter().enumerate() {
        let depth = depths[index];
        let height = (flow[index].max(1.0) * scale).max(6.0);
        let x = 80.0 + usize_as_f64(depth) * column_width;
        let y = column_cursor[depth];
        column_cursor[depth] = y + height + 14.0;
        node_x[index] = x;
        node_y[index] = y;
        node_height[index] = height;
        write!(
            body,
            r#"<rect x="{x:.1}" y="{y:.1}" width="14" height="{height:.1}" fill="{}"/><text x="{:.1}" y="{:.1}">{}</text>"#,
            SERIES_COLORS[index % SERIES_COLORS.len()],
            x + 20.0,
            y + height / 2.0 + 5.0,
            xml_escape(node)
        )
        .expect("writing to a String cannot fail");
    }
    let mut out_offsets = vec![0.0_f64; nodes.len()];
    let mut in_offsets = vec![0.0_f64; nodes.len()];
    let mut links = String::new();
    for (source, target, value) in &edges {
        let source_index = nodes.iter().position(|node| node == source)?;
        let target_index = nodes.iter().position(|node| node == target)?;
        let stroke = (value * scale).max(1.5);
        let start_x = node_x[source_index] + 14.0;
        let start_y = node_y[source_index] + out_offsets[source_index] + stroke / 2.0;
        let end_x = node_x[target_index];
        let end_y = node_y[target_index] + in_offsets[target_index] + stroke / 2.0;
        out_offsets[source_index] += stroke;
        in_offsets[target_index] += stroke;
        let mid_x = f64::midpoint(start_x, end_x);
        write!(
            links,
            r##"<path d="M {start_x:.1} {start_y:.1} C {mid_x:.1} {start_y:.1}, {mid_x:.1} {end_y:.1}, {end_x:.1} {end_y:.1}" fill="none" stroke="#94a3b8" stroke-opacity=".35" stroke-width="{stroke:.1}"/>"##
        )
        .expect("writing to a String cannot fail");
    }
    links.push_str(&body);
    Some(svg_shell(data, &links))
}

fn node_depths(nodes: &[String], edges: &[(String, String, f64)]) -> Vec<usize> {
    let mut depths = vec![0_usize; nodes.len()];
    for _ in 0..nodes.len() {
        let mut changed = false;
        for (source, target, _) in edges {
            let Some(source_index) = nodes.iter().position(|node| node == source) else {
                continue;
            };
            let Some(target_index) = nodes.iter().position(|node| node == target) else {
                continue;
            };
            let candidate = depths[source_index] + 1;
            if candidate > depths[target_index] && candidate <= nodes.len() {
                depths[target_index] = candidate;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    depths
}

fn render_dual_axes(data: &Value) -> Option<String> {
    let (categories, series) = parse_dual_axes(data)?;
    let column = series.iter().find(|series| series.kind == "column");
    let line = series.iter().find(|series| series.kind == "line");
    let slot = 640.0 / usize_as_f64(categories.len().max(1));
    let mut body = String::from(r#"<line class="grid" x1="60" y1="390" x2="720" y2="390"/>"#);
    if let Some(column) = column {
        let max = column
            .values
            .iter()
            .map(|value| value.abs())
            .fold(0.0, f64::max)
            .max(1.0);
        for (index, value) in column.values.iter().enumerate() {
            let height = value.abs() / max * 290.0;
            write!(
                body,
                r#"<rect class="mark" x="{:.1}" y="{:.1}" width="{:.1}" height="{height:.1}" fill-opacity=".8"/>"#,
                70.0 + usize_as_f64(index) * slot,
                390.0 - height,
                (slot - 18.0).max(3.0)
            )
            .expect("writing to a String cannot fail");
        }
    }
    if let Some(line) = line {
        let min = line.values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = line
            .values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let range = (max - min).max(f64::EPSILON);
        let path = line
            .values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                format!(
                    "{} {:.1} {:.1}",
                    if index == 0 { "M" } else { "L" },
                    70.0 + usize_as_f64(index) * slot + (slot - 18.0).max(3.0) / 2.0,
                    380.0 - (value - min) / range * 270.0
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        write!(
            body,
            r##"<path d="{path}" fill="none" stroke="#ffc700" stroke-width="3"/>"##
        )
        .expect("writing to a String cannot fail");
    }
    for (index, category) in categories.iter().enumerate() {
        write!(
            body,
            r#"<text x="{:.1}" y="415" text-anchor="middle">{}</text>"#,
            70.0 + usize_as_f64(index) * slot + (slot - 18.0).max(3.0) / 2.0,
            xml_escape(category)
        )
        .expect("writing to a String cannot fail");
    }
    for (index, series) in series.iter().enumerate() {
        let Some(name) = &series.title else { continue };
        let color = if series.kind == "line" {
            "#f59e0b"
        } else {
            "#00b7b7"
        };
        write!(body, r#"<rect x="620" y="{}" width="12" height="12" fill="{color}"/><text x="638" y="{}">{}</text>"#, 60 + index * 25, 71 + index * 25, xml_escape(name)).expect("writing to a String cannot fail");
    }
    Some(svg_shell(data, &body))
}

struct DualAxesSeries {
    kind: String,
    title: Option<String>,
    values: Vec<f64>,
}

fn parse_dual_axes(data: &Value) -> Option<(Vec<String>, Vec<DualAxesSeries>)> {
    let root = data
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(data);
    let categories = root
        .get("categories")?
        .as_array()?
        .iter()
        .filter_map(value_label)
        .collect::<Vec<_>>();
    let series = root
        .get("series")?
        .as_array()?
        .iter()
        .filter_map(|entry| {
            let values = entry
                .get("data")?
                .as_array()?
                .iter()
                .filter_map(value_number)
                .collect::<Vec<_>>();
            if values.is_empty() {
                return None;
            }
            Some(DualAxesSeries {
                kind: entry
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("column")
                    .to_owned(),
                title: ["axisYTitle", "title", "name"]
                    .iter()
                    .find_map(|key| entry.get(*key).and_then(value_label)),
                values,
            })
        })
        .collect::<Vec<_>>();
    if categories.is_empty() || series.is_empty() {
        return None;
    }
    Some((categories, series))
}

struct TreeNode {
    name: String,
    children: Vec<TreeNode>,
}

fn parse_tree(value: &Value) -> Option<TreeNode> {
    parse_tree_depth(value, 0)
}

fn parse_tree_depth(value: &Value, depth: usize) -> Option<TreeNode> {
    if depth > MAX_TREE_DEPTH {
        return None;
    }
    let object = value.as_object()?;
    let name = ["name", "label", "title", "value"]
        .iter()
        .find_map(|key| object.get(*key).and_then(value_label))?;
    let children = object
        .get("children")
        .and_then(Value::as_array)
        .map(|children| {
            children
                .iter()
                .filter_map(|child| parse_tree_depth(child, depth + 1))
                .collect()
        })
        .unwrap_or_default();
    Some(TreeNode { name, children })
}

fn render_tree(data: &Value) -> Option<String> {
    let tree = parse_tree(data.get("data")?)?;
    let depth = tree_depth(&tree);
    let leaves = tree_leaves(&tree).clamp(1, MAX_TREE_LEAVES);
    let x_step = 640.0 / usize_as_f64(depth.max(1));
    let row_height = 330.0 / usize_as_f64(leaves);
    let mut next_leaf = 0_usize;
    let mut body = String::new();
    layout_tree(
        &tree,
        0,
        x_step,
        row_height,
        &mut next_leaf,
        &mut body,
        None,
    );
    Some(svg_shell(data, &body))
}

fn tree_depth(node: &TreeNode) -> usize {
    node.children
        .iter()
        .map(tree_depth)
        .max()
        .map_or(0, |max| max + 1)
}

fn tree_leaves(node: &TreeNode) -> usize {
    if node.children.is_empty() {
        1
    } else {
        node.children.iter().map(tree_leaves).sum()
    }
}

fn layout_tree(
    node: &TreeNode,
    depth: usize,
    x_step: f64,
    row_height: f64,
    next_leaf: &mut usize,
    body: &mut String,
    color: Option<&str>,
) -> f64 {
    let x = 70.0 + usize_as_f64(depth) * x_step;
    let mut child_positions = Vec::new();
    for (child_index, child) in node.children.iter().enumerate() {
        if *next_leaf >= MAX_TREE_LEAVES {
            break;
        }
        // Each first-level branch gets its own palette colour (the web
        // fishbone/mind-map look); deeper nodes inherit their branch colour.
        let child_color = if depth == 0 {
            Some(SERIES_COLORS[child_index % SERIES_COLORS.len()])
        } else {
            color
        };
        child_positions.push((
            layout_tree(
                child,
                depth + 1,
                x_step,
                row_height,
                next_leaf,
                body,
                child_color,
            ),
            child_color,
        ));
    }
    let y = if child_positions.is_empty() {
        let y = 80.0 + usize_as_f64(*next_leaf) * row_height;
        *next_leaf += 1;
        y
    } else {
        let first = child_positions
            .iter()
            .map(|(y, _)| *y)
            .fold(f64::INFINITY, f64::min);
        let last = child_positions
            .iter()
            .map(|(y, _)| *y)
            .fold(f64::NEG_INFINITY, f64::max);
        let y = f64::midpoint(first, last);
        let child_x = 70.0 + usize_as_f64(depth + 1) * x_step;
        let mid_x = f64::midpoint(x, child_x);
        for (child_y, child_color) in &child_positions {
            write!(
                body,
                r#"<path d="M {:.1} {y:.1} C {mid_x:.1} {y:.1}, {mid_x:.1} {child_y:.1}, {:.1} {child_y:.1}" fill="none" stroke="{}" stroke-width="1.5"/>"#,
                x + 6.0,
                child_x - 6.0,
                child_color.unwrap_or("#94a3b8")
            )
            .expect("writing to a String cannot fail");
        }
        y
    };
    write!(
        body,
        r#"<circle cx="{x:.1}" cy="{y:.1}" r="4" fill="{}"/><text x="{:.1}" y="{:.1}">{}</text>"#,
        color.unwrap_or("#00b7b7"),
        x + 10.0,
        y + 5.0,
        xml_escape(&node.name)
    )
    .expect("writing to a String cannot fail");
    y
}

type GraphEdge = (String, String, Option<String>);

fn parse_graph(data: &Value) -> Option<(Vec<String>, Vec<GraphEdge>)> {
    let root = data
        .get("data")
        .filter(|value| value.is_object() || value.is_array())
        .unwrap_or(data);
    let edge_list = match root {
        Value::Array(items) => Some(items),
        Value::Object(object) => object
            .get("edges")
            .or_else(|| object.get("links"))
            .and_then(Value::as_array),
        _ => None,
    };
    let edges = edge_list
        .map(|edges| {
            edges
                .iter()
                .filter_map(|edge| {
                    let source = edge.get("source").or_else(|| edge.get("from"))?;
                    let target = edge.get("target").or_else(|| edge.get("to"))?;
                    Some((
                        value_label(source)?,
                        value_label(target)?,
                        edge.get("name").and_then(value_label),
                    ))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut nodes = root
        .get("nodes")
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| {
                    ["name", "id", "label"]
                        .iter()
                        .find_map(|key| node.get(*key).and_then(value_label))
                        .or_else(|| value_label(node))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for (source, target, _) in &edges {
        if !nodes.contains(source) {
            nodes.push(source.clone());
        }
        if !nodes.contains(target) {
            nodes.push(target.clone());
        }
    }
    if nodes.is_empty() {
        return None;
    }
    Some((nodes, edges))
}

fn render_network(data: &Value) -> Option<String> {
    let (nodes, edges) = parse_graph(data)?;
    let (cx, cy, radius) = (400.0, 240.0, 155.0);
    let positions = nodes
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let angle = std::f64::consts::TAU * usize_as_f64(index) / usize_as_f64(nodes.len())
                - std::f64::consts::FRAC_PI_2;
            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect::<Vec<_>>();
    let mut body = String::new();
    for (source, target, _) in &edges {
        let source_index = nodes.iter().position(|node| node == source)?;
        let target_index = nodes.iter().position(|node| node == target)?;
        write!(
            body,
            r#"<line class="edge" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/>"#,
            positions[source_index].0,
            positions[source_index].1,
            positions[target_index].0,
            positions[target_index].1
        )
        .expect("writing to a String cannot fail");
    }
    for (index, node) in nodes.iter().enumerate() {
        let (x, y) = positions[index];
        let anchor = if x < cx - 1.0 { "end" } else { "start" };
        let label_x = if x < cx - 1.0 { x - 12.0 } else { x + 12.0 };
        write!(
            body,
            r##"<circle cx="{x:.1}" cy="{y:.1}" r="7" fill="#00b7b7"/><text x="{label_x:.1}" y="{:.1}" text-anchor="{anchor}">{}</text>"##,
            y + 5.0,
            xml_escape(node)
        )
        .expect("writing to a String cannot fail");
    }
    Some(svg_shell(data, &body))
}

fn render_flow(data: &Value) -> Option<String> {
    let (nodes, edges) = parse_graph(data)?;
    let weighted = edges
        .iter()
        .map(|(source, target, _)| (source.clone(), target.clone(), 1.0))
        .collect::<Vec<_>>();
    let depths = node_depths(&nodes, &weighted);
    let max_depth = depths.iter().copied().max().unwrap_or(0);
    let column_width = 660.0 / usize_as_f64(max_depth.max(1) + 1);
    let box_width = (column_width - 30.0).clamp(70.0, 150.0);
    let mut column_counts = vec![0_usize; max_depth + 1];
    let mut positions = vec![(0.0_f64, 0.0_f64); nodes.len()];
    let mut body = String::new();
    for (index, node) in nodes.iter().enumerate() {
        let depth = depths[index];
        let x = 70.0 + usize_as_f64(depth) * column_width;
        let y = 80.0 + usize_as_f64(column_counts[depth]) * 62.0;
        column_counts[depth] += 1;
        positions[index] = (x, y);
        write!(
            body,
            r#"<rect class="node" x="{x:.1}" y="{y:.1}" width="{box_width:.1}" height="36" rx="7"/><text x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"#,
            x + box_width / 2.0,
            y + 23.0,
            xml_escape(node)
        )
        .expect("writing to a String cannot fail");
    }
    for (source, target, label) in &edges {
        let source_index = nodes.iter().position(|node| node == source)?;
        let target_index = nodes.iter().position(|node| node == target)?;
        let (x1, y1) = positions[source_index];
        let (x2, y2) = positions[target_index];
        let (start_x, start_y) = (x1 + box_width, y1 + 18.0);
        let (end_x, end_y) = (x2, y2 + 18.0);
        write!(
            body,
            r##"<line class="edge" x1="{start_x:.1}" y1="{start_y:.1}" x2="{end_x:.1}" y2="{end_y:.1}"/><polygon points="{end_x:.1},{end_y:.1} {:.1},{:.1} {:.1},{:.1}" fill="#94a3b8"/>"##,
            end_x - 8.0,
            end_y - 4.0,
            end_x - 8.0,
            end_y + 4.0
        )
        .expect("writing to a String cannot fail");
        if let Some(label) = label {
            write!(
                body,
                r#"<text x="{:.1}" y="{:.1}" text-anchor="middle" style="font-size:12px">{}</text>"#,
                f64::midpoint(start_x, end_x),
                f64::midpoint(start_y, end_y) - 6.0,
                xml_escape(label)
            )
            .expect("writing to a String cannot fail");
        }
    }
    Some(svg_shell(data, &body))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn usize_as_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}
