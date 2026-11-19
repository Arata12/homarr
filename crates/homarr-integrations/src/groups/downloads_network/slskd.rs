//! slskd (port of `download-client/slskd/*`). slskd can only list downloads; the queue and item actions are
//! no-ops, as in the TS source.

use async_trait::async_trait;
use chrono::DateTime;
use homarr_common::HomarrResult;
use serde::Deserialize;
use serde_json::Value;

use super::http;
use super::DownloadClient;
use crate::capabilities::downloads::{
    DownloadClientItem, DownloadClientItemState, DownloadClientJobsAndStatus, DownloadClientRates,
    DownloadClientSelection, DownloadClientStatus, DownloadClientType,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub struct Slskd;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UserDownloads {
    directories: Vec<Directory>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Directory {
    files: Vec<TransferFile>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TransferFile {
    id: String,
    filename: String,
    size: f64,
    percent_complete: f64,
    average_speed: f64,
    /// `hh:mm:ss`, absent when aborted or errored.
    remaining_time: Option<String>,
    /// Comma separated flags, e.g. `Completed, Succeeded`.
    state: String,
    requested_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferState {
    Requested,
    Queued,
    Initializing,
    InProgress,
    Completed,
    Succeeded,
    Cancelled,
    TimedOut,
    Errored,
    Rejected,
    Aborted,
}

fn parse_transfer_states(csv: &str) -> Vec<TransferState> {
    csv.split(',')
        .map(str::trim)
        .filter_map(|part| match part {
            "Requested" => Some(TransferState::Requested),
            "Queued" => Some(TransferState::Queued),
            "Initializing" => Some(TransferState::Initializing),
            "InProgress" => Some(TransferState::InProgress),
            "Completed" => Some(TransferState::Completed),
            "Succeeded" => Some(TransferState::Succeeded),
            "Cancelled" => Some(TransferState::Cancelled),
            "TimedOut" => Some(TransferState::TimedOut),
            "Errored" => Some(TransferState::Errored),
            "Rejected" => Some(TransferState::Rejected),
            "Aborted" => Some(TransferState::Aborted),
            _ => None,
        })
        .collect()
}

fn download_state(state: &str) -> DownloadClientItemState {
    use TransferState as T;
    let states = parse_transfer_states(state);
    let has = |wanted: T| states.contains(&wanted);
    if has(T::InProgress) {
        DownloadClientItemState::Downloading
    } else if has(T::Errored)
        || has(T::Aborted)
        || has(T::TimedOut)
        || has(T::Rejected)
        || has(T::Cancelled)
    {
        DownloadClientItemState::Failed
    } else if has(T::Queued) || has(T::Initializing) {
        DownloadClientItemState::Processing
    // "completed" must come last, since the flags above are combined bit-wise even if the transfer failed.
    } else if has(T::Completed) || has(T::Succeeded) {
        DownloadClientItemState::Completed
    } else {
        tracing::warn!(
            "Unable to map the slskd transfer state '{state}', falling back to 'unknown'"
        );
        DownloadClientItemState::Unknown
    }
}

fn remaining_ms(remaining: Option<&str>) -> f64 {
    let Some(remaining) = remaining else {
        return 0.0;
    };
    let parts: Vec<f64> = remaining
        .split(':')
        .map(|part| part.trim().parse::<f64>().unwrap_or(0.0))
        .collect();
    let part = |index: usize| parts.get(index).copied().unwrap_or(0.0);
    (part(0) * 3600.0 + part(1) * 60.0 + part(2)) * 1000.0
}

fn map_response(downloads: Vec<UserDownloads>, limit: usize) -> DownloadClientJobsAndStatus {
    let files: Vec<TransferFile> = downloads
        .into_iter()
        .flat_map(|user| user.directories)
        .flat_map(|directory| directory.files)
        .collect();
    let down = files.iter().map(|file| file.average_speed).sum();
    let items = files
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(index, file)| {
            // The file name includes the full remote path, which is not relevant for Homarr.
            let name = file
                .filename
                .rsplit('\\')
                .next()
                .unwrap_or(&file.filename)
                .to_string();
            DownloadClientItem {
                id: file.id,
                index: index as f64,
                name,
                item_type: DownloadClientType::Miscellaneous,
                size: file.size,
                sent: None,
                received: None,
                down_speed: None,
                up_speed: None,
                time: remaining_ms(file.remaining_time.as_deref()),
                added: DateTime::parse_from_rfc3339(&file.requested_at)
                    .ok()
                    .map(|date| (date.timestamp() * 1000) as f64),
                state: download_state(&file.state),
                progress: file.percent_complete / 100.0,
                category: None,
            }
        })
        .collect();
    DownloadClientJobsAndStatus {
        status: DownloadClientStatus {
            paused: false, // slskd cannot be paused
            rates: DownloadClientRates {
                down,
                // slskd does not upload except sharing, which is not what the downloads widget shows.
                up: Some(0.0),
            },
            types: vec![DownloadClientType::Miscellaneous],
        },
        items,
        ratio_items: None,
    }
}

impl Slskd {
    async fn get(
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        path: &str,
    ) -> HomarrResult<Value> {
        let api_key = ctx.require_secret("apiKey")?;
        let url = http::url(ctx, path, &[])?;
        http::send_json(ctx, http::get(rt, url).header("X-API-Key", api_key)).await
    }
}

#[async_trait]
impl DownloadClient for Slskd {
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Self::get(rt, ctx, "/api/v0/server").await.map(drop)
    }

    async fn jobs_and_status(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        limit: usize,
        _selection: Option<&DownloadClientSelection>,
    ) -> HomarrResult<DownloadClientJobsAndStatus> {
        let value = Self::get(rt, ctx, "/api/v0/transfers/downloads").await?;
        let downloads: Vec<UserDownloads> = http::from_value(ctx, "the slskd downloads", value)?;
        Ok(map_response(downloads, limit))
    }

    async fn pause_queue(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Ok(())
    }

    async fn resume_queue(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
    ) -> HomarrResult<()> {
        Ok(())
    }

    async fn pause_item(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Ok(())
    }

    async fn resume_item(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _item: &DownloadClientItem,
    ) -> HomarrResult<()> {
        Ok(())
    }

    async fn delete_item(
        &self,
        _rt: &IntegrationRuntime,
        _ctx: &IntegrationContext,
        _item: &DownloadClientItem,
        _from_disk: bool,
    ) -> HomarrResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_transfer_states() {
        assert_eq!(
            download_state("InProgress"),
            DownloadClientItemState::Downloading
        );
        assert_eq!(
            download_state("Completed, Succeeded"),
            DownloadClientItemState::Completed
        );
        assert_eq!(
            download_state("Completed, Errored"),
            DownloadClientItemState::Failed
        );
        assert_eq!(
            download_state("Queued, Remotely"),
            DownloadClientItemState::Processing
        );
        assert_eq!(download_state("None"), DownloadClientItemState::Unknown);
    }

    #[test]
    fn maps_files() {
        let downloads: Vec<UserDownloads> = serde_json::from_value(json!([{
            "directories": [{"files": [
                {"id": "a", "filename": "C:\\music\\song.mp3", "size": 10, "percentComplete": 50,
                 "averageSpeed": 100, "remainingTime": "0:01:30", "state": "InProgress",
                 "requestedAt": "2025-01-01T00:00:00Z"},
                {"id": "b", "filename": "x", "size": 1, "percentComplete": 100, "averageSpeed": 20,
                 "state": "Completed, Succeeded", "requestedAt": "bad"}
            ]}]
        }]))
        .unwrap();
        let out = map_response(downloads, 10);
        assert_eq!(out.status.rates.down, 120.0);
        assert_eq!(out.items[0].name, "song.mp3");
        assert_eq!(out.items[0].time, 90_000.0);
        assert_eq!(out.items[0].progress, 0.5);
        assert_eq!(out.items[0].added, Some(1_735_689_600_000.0));
        assert_eq!(out.items[1].added, None);
        assert_eq!(out.items[1].time, 0.0);
    }
}
