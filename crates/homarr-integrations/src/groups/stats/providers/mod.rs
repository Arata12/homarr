//! Stats providers: one file per TS `stats/providers/*.ts`. A provider declares its metrics and fetches
//! them; the pure response mapping lives in a `parse` function next to it so it can be tested with fixtures.

use async_trait::async_trait;
use homarr_common::HomarrResult;
use homarr_definitions::IntegrationKind;

use super::client::StatsClient;
use crate::capabilities::stats::{StatsMetric, StatsUnit, StatsValue, StatsValues};

mod autobrr;
mod caddy;
mod changedetection;
mod fileflows;
mod frigate;
mod gatus;
mod healthchecks;
mod homebox;
mod jellystat;
mod karakeep;
mod komga;
mod linkwarden;
mod maintainerr;
mod mealie;
mod miniflux;
mod netalertx;
mod netdata;
mod plantit;
mod prometheus;
mod romm;
mod scrutiny;
mod spoolman;
mod stash;
mod syncthing_relay;
mod tandoor;
mod trilium;
mod tubearchivist;
mod unmanic;
mod xteve;
mod your_spotify;
mod yuvomi;

/// Compiled-in metric definition (TS `StatsMetric`).
pub struct MetricDef {
    key: &'static str,
    label: &'static str,
    unit: StatsUnit,
}

impl MetricDef {
    pub fn to_metric(&self) -> StatsMetric {
        StatsMetric {
            key: self.key.to_string(),
            label: self.label.to_string(),
            unit: self.unit,
        }
    }
}

pub const fn m(key: &'static str, label: &'static str, unit: StatsUnit) -> MetricDef {
    MetricDef { key, label, unit }
}

/// Collects `(key, value)` pairs into the provider result.
pub fn values<const N: usize>(entries: [(&str, StatsValue); N]) -> StatsValues {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[async_trait]
pub trait StatsProvider: Sync {
    fn metrics(&self) -> &'static [MetricDef];

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues>;
}

pub fn provider(kind: IntegrationKind) -> Option<&'static dyn StatsProvider> {
    use IntegrationKind as K;
    Some(match kind {
        K::Autobrr => &autobrr::Autobrr,
        K::Caddy => &caddy::Caddy,
        K::Changedetection => &changedetection::Changedetection,
        K::Fileflows => &fileflows::Fileflows,
        K::Frigate => &frigate::Frigate,
        K::Gatus => &gatus::Gatus,
        K::Healthchecks => &healthchecks::Healthchecks,
        K::Homebox => &homebox::Homebox,
        K::Jellystat => &jellystat::Jellystat,
        K::Karakeep => &karakeep::Karakeep,
        K::Komga => &komga::Komga,
        K::Linkwarden => &linkwarden::Linkwarden,
        K::Maintainerr => &maintainerr::Maintainerr,
        K::Mealie => &mealie::Mealie,
        K::Miniflux => &miniflux::Miniflux,
        K::Netalertx => &netalertx::Netalertx,
        K::Netdata => &netdata::Netdata,
        K::Plantit => &plantit::Plantit,
        K::Prometheus => &prometheus::Prometheus,
        K::Romm => &romm::Romm,
        K::Scrutiny => &scrutiny::Scrutiny,
        K::Spoolman => &spoolman::Spoolman,
        K::Stash => &stash::Stash,
        K::SyncthingRelay => &syncthing_relay::SyncthingRelay,
        K::Tandoor => &tandoor::Tandoor,
        K::Trilium => &trilium::Trilium,
        K::Tubearchivist => &tubearchivist::Tubearchivist,
        K::Unmanic => &unmanic::Unmanic,
        K::Xteve => &xteve::Xteve,
        K::YourSpotify => &your_spotify::YourSpotify,
        K::Yuvomi => &yuvomi::Yuvomi,
        _ => return None,
    })
}
