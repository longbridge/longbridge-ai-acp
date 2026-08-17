//! Dumps one SVG preview per chart type for manual inspection.
use longbridge_ai_acp::RichContent;
use serde_json::json;

fn main() {
    let out = std::env::args().nth(1).expect("output dir");
    let samples = [
        json!({ "type": "column", "stack": true, "title": "Stacked revenue", "data": [
            { "category": "2022", "value": 320, "group": "Hardware" }, { "category": "2022", "value": 180, "group": "Software" }, { "category": "2022", "value": 90, "group": "Services" },
            { "category": "2023", "value": 350, "group": "Hardware" }, { "category": "2023", "value": 220, "group": "Software" }, { "category": "2023", "value": 130, "group": "Services" } ] }),
        json!({ "type": "area", "title": "AQI by city", "data": [
            { "category": "2019", "value": 150, "group": "Beijing" }, { "category": "2020", "value": 160, "group": "Beijing" }, { "category": "2021", "value": 145, "group": "Beijing" },
            { "category": "2019", "value": 100, "group": "Shanghai" }, { "category": "2020", "value": 95, "group": "Shanghai" }, { "category": "2021", "value": 85, "group": "Shanghai" },
            { "category": "2019", "value": 90, "group": "Guangzhou" }, { "category": "2020", "value": 88, "group": "Guangzhou" }, { "category": "2021", "value": 80, "group": "Guangzhou" } ] }),
        json!({ "type": "fishbone-diagram", "title": "Price drop drivers", "data": { "name": "Price drop", "children": [
            { "name": "Market", "children": [{ "name": "Liquidity" }, { "name": "Position unwind" }] },
            { "name": "Valuation", "children": [{ "name": "Rates up" }] },
            { "name": "Policy", "children": [{ "name": "Antitrust" }] },
            { "name": "Earnings", "children": [{ "name": "Revenue miss" }] } ] } }),
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
        json!({ "type": "column", "title": "Quarterly revenue", "data": [
            { "category": "Q1", "value": 91 }, { "category": "Q2", "value": 99 },
            { "category": "Q3", "value": 116 }, { "category": "Q4", "value": 135 } ] }),
        json!({ "type": "bar", "title": "Sales by region", "data": [
            { "category": "East", "value": 850 }, { "category": "South", "value": 620 },
            { "category": "North", "value": 580 }, { "category": "West", "value": 420 } ] }),
        json!({ "type": "line", "title": "Monthly active users", "data": [
            { "category": "Jan", "value": 100 }, { "category": "Feb", "value": 118 },
            { "category": "Mar", "value": 132 }, { "category": "Apr", "value": 155 } ] }),
        json!({ "type": "area", "title": "Annual revenue", "data": [
            { "category": "2021", "value": 50 }, { "category": "2022", "value": 78 },
            { "category": "2023", "value": 96 }, { "category": "2024", "value": 120 } ] }),
        json!({ "type": "pie", "title": "Product sales share", "data": [
            { "category": "Phone", "value": 45 }, { "category": "Laptop", "value": 25 },
            { "category": "Tablet", "value": 15 }, { "category": "Other", "value": 15 } ] }),
    ];
    for sample in samples {
        let mut name = sample["type"].as_str().unwrap().to_owned();
        if let Some(title) = sample["title"].as_str() {
            let slug: String = title
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            name = format!("{name}-{slug}");
        }
        let chart = RichContent::chart("preview", sample).expect("chart accepted");
        let svg = chart.svg.expect("svg rendered");
        std::fs::write(format!("{out}/{name}.svg"), svg).unwrap();
        println!("{name} ok");
    }
}
