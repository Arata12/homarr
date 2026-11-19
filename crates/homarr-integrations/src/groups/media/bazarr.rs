//! Bazarr. Port of `bazarr/bazarr-integration.ts` (test connection; badges belong to the stats group).

use homarr_common::HomarrResult;
use serde::{Deserialize, Serialize};

use super::util::{send_json, url};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BazarrBadges {
    pub episodes: u64,
    pub movies: u64,
    pub providers: u64,
}

#[derive(Deserialize)]
struct SystemStatus {
    data: SystemStatusData,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SystemStatusData {
    bazarr_version: String,
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let key = ctx.require_secret("apiKey")?;
    let request = rt
        .http
        .standard()
        .get(url(ctx, "/api/system/status", &[])?)
        .header("X-API-KEY", key)
        .header("Accept", "application/json");
    let _: SystemStatus = send_json(ctx, "the system status", request).await?;
    Ok(())
}

pub async fn badges(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<BazarrBadges> {
    let key = ctx.require_secret("apiKey")?;
    let request = rt
        .http
        .standard()
        .get(url(ctx, "/api/badges", &[])?)
        .header("X-API-KEY", key)
        .header("Accept", "application/json");
    send_json(ctx, "the Bazarr badges", request).await
}
