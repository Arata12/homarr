use async_trait::async_trait;
use homarr_common::HomarrResult;
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{
    StatsUnit::{Bytes, Count},
    StatsValues,
};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{as_nonneg, count, num, opt};

pub struct Romm;

const METRICS: &[MetricDef] = &[
    m("platforms", "Platforms", Count),
    m("roms", "ROMs", Count),
    m("saves", "Saves", Count),
    m("states", "States", Count),
    m("screenshots", "Screenshots", Count),
    m("totalFileSize", "Total file size", Bytes),
];

#[async_trait]
impl StatsProvider for Romm {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let response = client.get("/api/stats", &Vec::new()).await?;
        parsed(client.ctx, "RomM statistics", parse(&response))
    }
}

fn parse(response: &Value) -> Result<StatsValues, String> {
    let size = opt(response, &["FILESIZE"])
        .or_else(|| opt(response, &["TOTAL_FILESIZE_BYTES"]))
        .ok_or("Expected FILESIZE or TOTAL_FILESIZE_BYTES")?;
    Ok(values([
        ("platforms", num(count(response, &["PLATFORMS"])?)),
        ("roms", num(count(response, &["ROMS"])?)),
        ("saves", num(count(response, &["SAVES"])?)),
        ("states", num(count(response, &["STATES"])?)),
        ("screenshots", num(count(response, &["SCREENSHOTS"])?)),
        ("totalFileSize", num(as_nonneg(size, "file size")?)),
    ]))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::capabilities::stats::StatsValue;

    fn base() -> Value {
        json!({ "PLATFORMS": 1, "ROMS": 2, "SAVES": 3, "STATES": 4, "SCREENSHOTS": 5 })
    }

    #[test]
    fn accepts_either_file_size_field() {
        let mut a = base();
        a["FILESIZE"] = json!(100);
        assert_eq!(
            parse(&a).unwrap()["totalFileSize"],
            StatsValue::Number(100.0)
        );
        let mut b = base();
        b["TOTAL_FILESIZE_BYTES"] = json!(200);
        assert_eq!(
            parse(&b).unwrap()["totalFileSize"],
            StatsValue::Number(200.0)
        );
        assert!(parse(&base()).is_err());
    }
}
