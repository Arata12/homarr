//! Stats group: kinds whose only capability is the generic Statistics widget (TS `stats/providers/*`).
//!
//! `providers/` holds one file per TS provider (metric definitions, fetching, response mapping), `client.rs`
//! the shared request layer (`stats-integration.ts`), `json.rs` the validation helpers replacing zod and
//! `format.rs` the value formatting of the legacy `StatsResponse` adapter.

use async_trait::async_trait;
use homarr_common::HomarrResult;
use homarr_definitions::IntegrationKind;

use super::{Handled, IntegrationGroup};
use crate::capabilities::stats::{StatsMetric, StatsValues};
use crate::error::unsupported;
use crate::runtime::IntegrationRuntime;
use crate::types::{IntegrationContext, StatsResponse};

mod client;
pub(crate) mod format;
mod json;
mod providers;

use client::StatsClient;
use providers::provider;

pub struct StatsGroup;

const KINDS: &[IntegrationKind] = &[
    IntegrationKind::Autobrr,
    IntegrationKind::Jellystat,
    IntegrationKind::Scrutiny,
    IntegrationKind::Tubearchivist,
    IntegrationKind::Frigate,
    IntegrationKind::Komga,
    IntegrationKind::Netalertx,
    IntegrationKind::YourSpotify,
    IntegrationKind::Romm,
    IntegrationKind::Homebox,
    IntegrationKind::Mealie,
    IntegrationKind::Xteve,
    IntegrationKind::Unmanic,
    IntegrationKind::SyncthingRelay,
    IntegrationKind::Stash,
    IntegrationKind::Prometheus,
    IntegrationKind::Plantit,
    IntegrationKind::Yuvomi,
    IntegrationKind::Netdata,
    IntegrationKind::Fileflows,
    IntegrationKind::Trilium,
    IntegrationKind::Tandoor,
    IntegrationKind::Spoolman,
    IntegrationKind::Miniflux,
    IntegrationKind::Maintainerr,
    IntegrationKind::Linkwarden,
    IntegrationKind::Karakeep,
    IntegrationKind::Healthchecks,
    IntegrationKind::Gatus,
    IntegrationKind::Changedetection,
    IntegrationKind::Caddy,
];

async fn fetch_values(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> Option<HomarrResult<StatsValues>> {
    let provider = provider(ctx.kind)?;
    Some(async { provider.fetch(&StatsClient::new(rt, ctx)?).await }.await)
}

/// Stats for kinds the group does not own. The TS `existing.ts` adapters for those kinds are wired by their
/// own groups; until then the kind is reported as unsupported instead of returning made-up data.
pub(crate) async fn legacy_fetch_any(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<StatsResponse> {
    match legacy_stats_response(rt, ctx).await {
        Some(result) => result,
        None => Err(unsupported(ctx, "stats")),
    }
}

/// Adapter for the legacy `StatsResponse` consumers: metric definitions + values, formatted like the widget.
async fn legacy_stats_response(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> Option<HomarrResult<StatsResponse>> {
    let provider = provider(ctx.kind)?;
    let result = fetch_values(rt, ctx).await?;
    Some(result.map(|values| {
        let metrics: Vec<StatsMetric> = provider.metrics().iter().map(|m| m.to_metric()).collect();
        format::to_legacy_stats_response(ctx, &metrics, &values)
    }))
}

#[async_trait]
impl IntegrationGroup for StatsGroup {
    fn kinds(&self) -> &'static [IntegrationKind] {
        KINDS
    }

    /// TS `testingAsync`: a successful stats fetch is a successful connection.
    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        Some(fetch_values(rt, ctx).await?.map(|_| ()))
    }

    fn stats_metrics(&self, kind: IntegrationKind) -> Vec<StatsMetric> {
        provider(kind)
            .map(|provider| provider.metrics().iter().map(|m| m.to_metric()).collect())
            .unwrap_or_default()
    }

    async fn stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsValues> {
        fetch_values(rt, ctx).await
    }

    async fn legacy_stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsResponse> {
        legacy_stats_response(rt, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_provider_with_unique_metric_keys() {
        for kind in KINDS {
            let provider = provider(*kind).unwrap_or_else(|| panic!("no provider for {kind:?}"));
            let metrics = provider.metrics();
            assert!(!metrics.is_empty());
            let mut keys: Vec<_> = metrics.iter().map(|m| m.to_metric().key).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(
                keys.len(),
                metrics.len(),
                "duplicate metric keys for {kind:?}"
            );
        }
    }
}
