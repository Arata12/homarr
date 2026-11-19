//! SABnzbd (port of `download-client/sabnzbd/*`).

use async_trait::async_trait;
use chrono::Utc;
use homarr_common::HomarrResult;
use serde::Deserialize;
use serde_json::Value;

use super::http;
use super::DownloadClient;
use crate::capabilities::downloads::{
    DownloadClientCategory, DownloadClientItem, DownloadClientItemState,
    DownloadClientJobsAndStatus, DownloadClientRates, DownloadClientSelection,
    DownloadClientStatus, DownloadClientType,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct Sabnzbd;

#[derive(Debug, Deserialize)]
struct QueueResponse {
    queue: Queue,
}

#[derive(Debug, Deserialize)]
struct Queue {
    paused: bool,
    kbpersec: String,
    slots: Vec<QueueSlot>,
}

#[derive(Debug, Deserialize)]
struct QueueSlot {
    status: String,
    index: f64,
    mb: String,
    filename: String,
    cat: String,
    timeleft: String,
    percentage: String,
    nzo_id: String,
}

#[derive(Debug, Deserialize)]
struct HistoryResponse {
    history: History,
}

#[derive(Debug, Deserialize)]
struct History {
    slots: Vec<HistorySlot>,
}

#[derive(Debug, Deserialize)]
struct HistorySlot {
    category: String,
    download_time: f64,
    status: String,
    completed: f64,
    nzo_id: String,
    postproc_time: f64,
    name: String,
    bytes: f64,
}

fn queue_state(status: &str) -> DownloadClientItemState {
    match status {
        "Queued" => DownloadClientItemState::Queued,
        "Paused" => DownloadClientItemState::Paused,
        _ => DownloadClientItemState::Downloading,
    }
}

fn history_state(status: &str) -> DownloadClientItemState {
    match status {
        "Completed" => DownloadClientItemState::Completed,
        "Failed" => DownloadClientItemState::Failed,
        _ => DownloadClientItemState::Processing,
    }
}

/// `"H:MM:SS"` (also `MM:SS`, `D:H:MM:SS`) to milliseconds, like the dayjs duration in TS.
fn time_left_ms(timeleft: &str) -> f64 {
    let parts: Vec<f64> = timeleft
        .split(':')
        .rev()
        .map(|part| part.trim().parse::<f64>().unwrap_or(f64::NAN))
        .collect();
    let part = |index: usize| {
        let value = parts.get(index).copied().unwrap_or(0.0);
        if value.is_nan() {
            0.0
        } else {
            value
        }
    };
    (part(0) + part(1) * 60.0 + part(2) * 3600.0 + part(3) * 86_400.0) * 1000.0
}

fn map_response(
    queue: Queue,
    history: History,
    limit: usize,
    now_ms: f64,
) -> DownloadClientJobsAndStatus {
    let kind = DownloadClientType::Usenet;
    // SABnzbd's legacy kbpersec field is kibibytes per second; normalize it to bytes per second.
    let down = (queue.kbpersec.trim().parse::<f64>().unwrap_or(0.0) * 1024.0).floor();
    let status = DownloadClientStatus {
        paused: queue.paused,
        rates: DownloadClientRates { down, up: None },
        types: vec![kind],
    };
    let queue_items = queue.slots.into_iter().map(|slot| DownloadClientItem {
        item_type: kind,
        id: slot.nzo_id,
        index: slot.index,
        name: slot.filename,
        size: (slot.mb.trim().parse::<f64>().unwrap_or(0.0) * 1024.0 * 1024.0).ceil(),
        sent: None,
        received: None,
        down_speed: Some(if slot.index > 0.0 { 0.0 } else { down }),
        up_speed: None,
        time: time_left_ms(&slot.timeleft),
        added: None,
        state: queue_state(&slot.status),
        progress: slot.percentage.trim().parse::<f64>().unwrap_or(0.0) / 100.0,
        category: Some(DownloadClientCategory::One(slot.cat)),
    });
    let history_items =
        history
            .slots
            .into_iter()
            .enumerate()
            .map(|(index, slot)| DownloadClientItem {
                item_type: kind,
                id: slot.nzo_id,
                index: index as f64,
                name: slot.name,
                size: slot.bytes,
                sent: None,
                received: None,
                down_speed: None,
                up_speed: None,
                time: slot.completed * 1000.0 - now_ms,
                added: Some((slot.completed - slot.download_time - slot.postproc_time) * 1000.0),
                state: history_state(&slot.status),
                progress: 1.0,
                category: Some(DownloadClientCategory::One(slot.category)),
            });
    DownloadClientJobsAndStatus {
        status,
        items: queue_items.chain(history_items).take(limit).collect(),
        ratio_items: None,
    }
}

impl Sabnzbd {
    async fn call(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        mode: &str,
        params: &[(&str, &str)],
    ) -> HomarrResult<Value> {
        let api_key = ctx.require_secret("apiKey")?;
        let mut query: Vec<(&str, &str)> = params.to_vec();
        query.push(("output", "json"));
        query.push(("mode", mode));
        query.push(("apikey", api_key));
        let url = http::url(ctx, "/api", &query)?;
        http::send_json(ctx, http::get(rt, url)).await
    }
}

#[async_trait]
impl DownloadClient for Sabnzbd {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        // The call that uses the least data while requiring the api key.
        Self::call(rt, ctx, "translate", &[("value", "ping")]).await?;
        Ok(())
    }

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        _selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus> {
        let limit_text = limit.to_string();
        let params = [("limit", limit_text.as_str())];
        let (queue, history) = tokio::join!(
            Self::call(rt, ctx, "queue", &params),
            Self::call(rt, ctx, "history", &params),
        );
        let queue: QueueResponse = http::from_value(ctx, "the SABnzbd queue", queue?)?;
        let history: HistoryResponse = http::from_value(ctx, "the SABnzbd history", history?)?;
        Ok(map_response(
            queue.queue,
            history.history,
            limit,
            Utc::now().timestamp_millis() as f64,
        ))
    }

    async fn pause_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "pause", &[]).await.map(drop)
    }

    async fn resume_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "resume", &[]).await.map(drop)
    }

    async fn pause_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "queue", &[("name", "pause"), ("value", &item.id)])
            .await
            .map(drop)
    }

    async fn resume_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Self::call(rt, ctx, "queue", &[("name", "resume"), ("value", &item.id)])
            .await
            .map(drop)
    }

    // Delete files is prevented on completed files (sabnzbd/sabnzbd#2754); it works while downloading and
    // post-processing.
    async fn delete_item(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        item: &DownloadClientItem,
        from_disk: bool,
    ) -> HomarrResult<()> {
        let mode = if item.progress != 1.0 {
            "queue"
        } else {
            "history"
        };
        Self::call(
            rt,
            ctx,
            mode,
            &[
                ("name", "delete"),
                ("archive", if from_disk { "0" } else { "1" }),
                ("value", &item.id),
                ("del_files", if from_disk { "1" } else { "0" }),
            ],
        )
        .await
        .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_queue_and_history() {
        let queue: QueueResponse = serde_json::from_value(json!({"queue": {
        "paused": false, "kbpersec": "2.5",
        "slots": [
            {"status": "Downloading", "index": 0, "mb": "100.5", "filename": "a", "cat": "tv",
             "timeleft": "1:02:03", "percentage": "50", "nzo_id": "n1"},
            {"status": "Paused", "index": 1, "mb": "10", "filename": "b", "cat": "*",
             "timeleft": "0:00:10", "percentage": "0", "nzo_id": "n2"}
        ]}}))
        .unwrap();
        let history: HistoryResponse = serde_json::from_value(json!({"history": {"slots": [
            {"category": "movies", "download_time": 60, "status": "Failed", "completed": 1000,
             "nzo_id": "h1", "postproc_time": 30, "name": "c", "bytes": 5000}
        ]}}))
        .unwrap();
        let out = map_response(queue.queue, history.history, 10, 2_000_000.0);
        assert_eq!(out.status.rates.down, 2560.0);
        assert_eq!(out.items.len(), 3);
        assert_eq!(out.items[0].down_speed, Some(2560.0));
        assert_eq!(out.items[0].size, (100.5_f64 * 1048576.0).ceil());
        assert_eq!(out.items[0].time, (3600.0 + 120.0 + 3.0) * 1000.0);
        assert_eq!(out.items[0].progress, 0.5);
        assert_eq!(out.items[1].down_speed, Some(0.0));
        assert_eq!(out.items[1].state, DownloadClientItemState::Paused);
        assert_eq!(out.items[2].state, DownloadClientItemState::Failed);
        assert_eq!(out.items[2].time, 1_000_000.0 - 2_000_000.0);
        assert_eq!(out.items[2].added, Some((1000.0 - 90.0) * 1000.0));
    }

    #[test]
    fn limit_applies_to_combined_items() {
        let queue = Queue {
            paused: true,
            kbpersec: "0".into(),
            slots: vec![],
        };
        let history = History {
            slots: (0..3)
                .map(|i| HistorySlot {
                    category: String::new(),
                    download_time: 0.0,
                    status: "Completed".into(),
                    completed: 0.0,
                    nzo_id: i.to_string(),
                    postproc_time: 0.0,
                    name: String::new(),
                    bytes: 0.0,
                })
                .collect(),
        };
        let out = map_response(queue, history, 2, 0.0);
        assert!(out.status.paused);
        assert_eq!(out.items.len(), 2);
    }
}
