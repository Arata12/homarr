use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{array, as_count, count, len, num, opt, text};

pub struct Yuvomi;

const METRICS: &[MetricDef] = &[
    m("openTasks", "Open tasks", Count),
    m("highPriorityTasks", "High priority tasks", Count),
    m("shoppingItems", "Items to buy", Count),
];

#[async_trait]
impl StatsProvider for Yuvomi {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let none = Vec::new();
        // Top-level tasks that are not done and not archived, as the token's user sees them.
        let (tasks, shopping) = tokio::try_join!(
            client.get("/api/v1/tasks?status=open&status=in_progress", &none),
            client.get("/api/v1/shopping", &none),
        )?;
        parsed(client.ctx, "Yuvomi statistics", parse(&tasks, &shopping))
    }
}

fn parse(tasks: &Value, shopping: &Value) -> Result<StatsValues, String> {
    let tasks = array(tasks, &["data"])?;
    let mut high = 0;
    for task in tasks {
        if matches!(text(task, &["priority"])?, "high" | "urgent") {
            high += 1;
        }
    }
    let mut items = 0.0;
    for list in array(shopping, &["data"])? {
        let total = count(list, &["item_total"])?;
        let checked = match opt(list, &["item_checked"]) {
            Some(value) => as_count(value, "item_checked")?,
            None => 0.0,
        };
        items += (total - checked).max(0.0);
    }
    Ok(values([
        ("openTasks", len(tasks.len())),
        ("highPriorityTasks", len(high)),
        ("shoppingItems", num(items)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn counts_tasks_and_unchecked_items() {
        let out = parse(
            &json!({ "data": [{ "priority": "high" }, { "priority": "low" }, { "priority": "urgent" }] }),
            &json!({ "data": [
                { "item_total": 5, "item_checked": 2 },
                { "item_total": 1, "item_checked": null },
                { "item_total": 1, "item_checked": 3 }
            ] }),
        )
        .unwrap();
        assert_eq!(out["openTasks"], StatsValue::Number(3.0));
        assert_eq!(out["highPriorityTasks"], StatsValue::Number(2.0));
        assert_eq!(out["shoppingItems"], StatsValue::Number(4.0));
    }
}
