use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{as_count, count, num, opt};

pub struct Tubearchivist;

const METRICS: &[MetricDef] = &[
    m("pendingDownloads", "Pending downloads", Count),
    m("videos", "Videos", Count),
    m("channels", "Channels", Count),
    m("playlists", "Playlists", Count),
];

#[async_trait]
impl StatsProvider for Tubearchivist {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let key = client.require_secret("apiKey")?;
        let headers = vec![("Authorization", format!("Token {key}"))];
        let (downloads, videos, channels, playlists) = tokio::try_join!(
            client.get("/api/stats/download/", &headers),
            client.get("/api/stats/video/", &headers),
            client.get("/api/stats/channel/", &headers),
            client.get("/api/stats/playlist/", &headers),
        )?;
        parsed(
            client.ctx,
            "Tube Archivist statistics",
            parse(&downloads, &videos, &channels, &playlists),
        )
    }
}

fn parse(
    downloads: &Value,
    videos: &Value,
    channels: &Value,
    playlists: &Value,
) -> Result<StatsValues, String> {
    // `pending` is null while nothing is queued.
    let pending = match opt(downloads, &["pending"]) {
        Some(value) => as_count(value, "pending")?,
        None if downloads.get("pending").is_some() => 0.0,
        None => return Err("missing field 'pending'".to_string()),
    };
    Ok(values([
        ("pendingDownloads", num(pending)),
        ("videos", num(count(videos, &["doc_count"])?)),
        ("channels", num(count(channels, &["doc_count"])?)),
        ("playlists", num(count(playlists, &["doc_count"])?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn null_pending_is_zero() {
        let doc = |n: u64| json!({ "doc_count": n });
        let out = parse(&json!({ "pending": null }), &doc(1), &doc(2), &doc(3)).unwrap();
        assert_eq!(out["pendingDownloads"], StatsValue::Number(0.0));
        assert_eq!(out["playlists"], StatsValue::Number(3.0));
        let out = parse(&json!({ "pending": 9 }), &doc(1), &doc(2), &doc(3)).unwrap();
        assert_eq!(out["pendingDownloads"], StatsValue::Number(9.0));
        assert!(parse(&json!({}), &doc(1), &doc(2), &doc(3)).is_err());
    }
}
