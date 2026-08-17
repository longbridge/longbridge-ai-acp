//! Dumps one SVG preview per chart type for manual inspection.
use longbridge_ai_acp::RichContent;
use serde_json::json;

fn main() {
    let out = std::env::args().nth(1).expect("output dir");
    let samples = [
        json!({ "type": "funnel", "title": "Sales funnel", "data": [
            { "category": "Visit", "value": 1000 }, { "category": "Inquiry", "value": 600 },
            { "category": "Order", "value": 300 }, { "category": "Deal", "value": 150 } ] }),
        json!({ "type": "radar", "title": "Vendor score", "data": [
            { "name": "Speed", "value": 8 }, { "name": "Quality", "value": 9 },
            { "name": "Cost", "value": 6 }, { "name": "Service", "value": 7 }, { "name": "Innovation", "value": 8 } ] }),
        json!({ "type": "boxplot", "title": "Score distribution", "data": [
            { "category": "Class A", "value": 65 }, { "category": "Class A", "value": 72 },
            { "category": "Class A", "value": 78 }, { "category": "Class A", "value": 85 }, { "category": "Class A", "value": 95 },
            { "category": "Class B", "value": 55 }, { "category": "Class B", "value": 66 },
            { "category": "Class B", "value": 70 }, { "category": "Class B", "value": 81 }, { "category": "Class B", "value": 90 } ] }),
        json!({ "type": "treemap", "title": "Category sales", "data": [
            { "name": "Electronics", "value": 500 }, { "name": "Appliances", "value": 300 }, { "name": "Apparel", "value": 200 } ] }),
        json!({ "type": "word-cloud", "title": "Tech buzzwords", "data": [
            { "text": "AI", "value": 50 }, { "text": "Cloud", "value": 38 }, { "text": "Blockchain", "value": 22 },
            { "text": "5G", "value": 30 }, { "text": "Quantum", "value": 15 }, { "text": "Edge", "value": 18 },
            { "text": "AR", "value": 12 }, { "text": "Robotics", "value": 25 } ] }),
        json!({ "type": "sankey", "title": "Revenue flow", "data": [
            { "source": "Revenue", "target": "Cost", "value": 600 },
            { "source": "Revenue", "target": "Gross profit", "value": 400 },
            { "source": "Gross profit", "target": "Opex", "value": 250 },
            { "source": "Gross profit", "target": "Net profit", "value": 150 } ] }),
        json!({ "type": "heat-map", "title": "Traffic by hour", "data": [
            { "x": "Mon", "y": "AM", "value": 3 }, { "x": "Mon", "y": "PM", "value": 8 },
            { "x": "Tue", "y": "AM", "value": 5 }, { "x": "Tue", "y": "PM", "value": 2 },
            { "x": "Wed", "y": "AM", "value": 9 }, { "x": "Wed", "y": "PM", "value": 6 } ] }),
        json!({ "type": "scatter", "title": "Ad spend vs sales", "data": [
            { "x": 10, "y": 30 }, { "x": 20, "y": 55 }, { "x": 30, "y": 62 },
            { "x": 40, "y": 90 }, { "x": 55, "y": 110 }, { "x": 70, "y": 130 } ] }),
        json!({ "type": "histogram", "title": "Grade distribution", "data": [55, 58, 61, 62, 65, 68, 71, 72, 75, 75, 76, 79, 82, 85, 88, 91, 95] }),
        json!({ "type": "dual-axes", "title": "Sales and margin", "categories": ["2021", "2022", "2023", "2024"], "series": [
            { "type": "column", "data": [91, 99, 116, 135], "axisYTitle": "Sales" },
            { "type": "line", "data": [0.15, 0.17, 0.19, 0.22], "axisYTitle": "Margin" } ] }),
        json!({ "type": "mind-map", "title": "Project flow", "data": {
            "name": "Project", "children": [
                { "name": "Plan", "children": [{ "name": "Scope" }, { "name": "Schedule" }] },
                { "name": "Build", "children": [{ "name": "Develop" }, { "name": "Test" }] },
                { "name": "Launch" } ] } }),
        json!({ "type": "organization-chart", "title": "Org chart", "data": {
            "name": "CEO", "children": [
                { "name": "CTO", "children": [{ "name": "Platform" }, { "name": "Data" }] },
                { "name": "CFO" }, { "name": "COO", "children": [{ "name": "Ops" }] } ] } }),
        json!({ "type": "fishbone-diagram", "title": "Churn causes", "data": {
            "name": "Churn", "children": [
                { "name": "Price", "children": [{ "name": "Fees" }] },
                { "name": "Support", "children": [{ "name": "Latency" }] } ] } }),
        json!({ "type": "network-graph", "title": "Relations", "data": {
            "nodes": [{ "name": "Alice" }, { "name": "Bob" }, { "name": "Carol" }, { "name": "Dave" }, { "name": "Eve" }],
            "edges": [ { "source": "Alice", "target": "Bob" }, { "source": "Alice", "target": "Carol" },
                       { "source": "Bob", "target": "Dave" }, { "source": "Carol", "target": "Eve" },
                       { "source": "Dave", "target": "Eve" } ] } }),
        json!({ "type": "flow-diagram", "title": "Order flow", "data": {
            "nodes": [{ "name": "Order" }, { "name": "Pay" }, { "name": "Review" }, { "name": "Ship" }, { "name": "Done" }],
            "edges": [ { "source": "Order", "target": "Pay", "name": "checkout" },
                       { "source": "Pay", "target": "Review" }, { "source": "Review", "target": "Ship" },
                       { "source": "Ship", "target": "Done" } ] } }),
    ];
    for sample in samples {
        let name = sample["type"].as_str().unwrap().to_owned();
        let chart = RichContent::chart("preview", sample).expect("chart accepted");
        let svg = chart.svg.expect("svg rendered");
        std::fs::write(format!("{out}/{name}.svg"), svg).unwrap();
        println!("{name} ok");
    }
}
