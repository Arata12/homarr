use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Count, Seconds},
    StatsValues,
};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{array, len, nonneg, num};

pub struct YourSpotify;

const METRICS: &[MetricDef] = &[
    m("songs", "Songs listened", Count),
    m("time", "Listening time", Seconds),
    m("artists", "Artists listened", Count),
];

const ALL_TIME_START: &str = "2006-04-23T00:00:00.000Z";

fn endpoint_path(endpoint: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("start", ALL_TIME_START)
        .append_pair("timeSplit", "all")
        .finish();
    format!("/spotify/{endpoint}?{query}")
}

#[async_trait]
impl StatsProvider for YourSpotify {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let none = Vec::new();
        let (songs_path, time_path, artists_path) = (
            endpoint_path("songs_per"),
            endpoint_path("time_per"),
            endpoint_path("different_artists_per"),
        );
        let (songs, time, artists) = tokio::try_join!(
            client.get(&songs_path, &none),
            client.get(&time_path, &none),
            client.get(&artists_path, &none),
        )?;
        parsed(
            client.ctx,
            "Your Spotify statistics",
            parse(&songs, &time, &artists),
        )
    }
}

/// First `count` of a list of `{ count }` rows, 0 when the list is empty.
fn first_count(response: &Value) -> Result<f64, String> {
    let rows = response.as_array().ok_or("expected a list")?;
    rows.iter()
        .try_for_each(|row| nonneg(row, &["count"]).map(drop))?;
    match rows.first() {
        Some(row) => nonneg(row, &["count"]),
        None => Ok(0.0),
    }
}

fn parse(songs: &Value, time: &Value, artists: &Value) -> Result<StatsValues, String> {
    let rows = artists.as_array().ok_or("expected a list")?;
    for row in rows {
        array(row, &["artists"])?;
    }
    let artist_count = match rows.first() {
        Some(row) => array(row, &["artists"])?.len(),
        None => 0,
    };
    Ok(values([
        ("songs", num(first_count(songs)?)),
        ("time", num(first_count(time)? / 1000.0)),
        ("artists", len(artist_count)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    #[test]
    fn converts_milliseconds_and_defaults_to_zero() {
        let out = parse(
            &json!([{ "count": 321 }]),
            &json!([{ "count": 90_000 }]),
            &json!([{ "artists": [{}, {}] }]),
        )
        .unwrap();
        assert_eq!(out["songs"], StatsValue::Number(321.0));
        assert_eq!(out["time"], StatsValue::Number(90.0));
        assert_eq!(out["artists"], StatsValue::Number(2.0));
        let empty = parse(&json!([]), &json!([]), &json!([])).unwrap();
        assert_eq!(empty["artists"], StatsValue::Number(0.0));
    }

    #[test]
    fn endpoint_path_encodes_query() {
        assert_eq!(
            endpoint_path("songs_per"),
            "/spotify/songs_per?start=2006-04-23T00%3A00%3A00.000Z&timeSplit=all"
        );
    }
}
