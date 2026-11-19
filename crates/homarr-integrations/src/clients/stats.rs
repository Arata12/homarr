use homarr_common::HomarrResult;
use homarr_core::HttpClientFactory;
use homarr_definitions::IntegrationKind;

use crate::clients::{get_json, simple_get_test};
use crate::types::{IntegrationContext, StatMetric, StatsResponse};

struct StatsEndpointSpec {
    test_path: &'static str,
    label: &'static str,
}

fn spec_for(kind: IntegrationKind) -> StatsEndpointSpec {
    use IntegrationKind as K;
    match kind {
        K::Autobrr => StatsEndpointSpec {
            test_path: "/api/release/stats",
            label: "Releases",
        },
        K::Jellystat => StatsEndpointSpec {
            test_path: "/api/getLibraryOverview",
            label: "Library",
        },
        K::Scrutiny => StatsEndpointSpec {
            test_path: "/api/summary",
            label: "Disks",
        },
        K::Tubearchivist => StatsEndpointSpec {
            test_path: "/api/stats/video/",
            label: "Videos",
        },
        K::Frigate => StatsEndpointSpec {
            test_path: "/api/stats",
            label: "Detections",
        },
        K::Komga => StatsEndpointSpec {
            test_path: "/api/v1/series?size=1",
            label: "Series",
        },
        K::Netalertx => StatsEndpointSpec {
            test_path: "/api/app_state.json",
            label: "Devices",
        },
        K::YourSpotify => StatsEndpointSpec {
            test_path: "/api/spotify/stats",
            label: "Plays",
        },
        K::Romm => StatsEndpointSpec {
            test_path: "/api/stats",
            label: "ROMs",
        },
        K::Homebox => StatsEndpointSpec {
            test_path: "/api/v1/status",
            label: "Items",
        },
        K::Mealie => StatsEndpointSpec {
            test_path: "/api/groups/statistics",
            label: "Recipes",
        },
        K::Xteve => StatsEndpointSpec {
            test_path: "/api/status",
            label: "Channels",
        },
        K::Unmanic => StatsEndpointSpec {
            test_path: "/unmanic/api/v2/version/read",
            label: "Workers",
        },
        K::SyncthingRelay => StatsEndpointSpec {
            test_path: "/status",
            label: "Sessions",
        },
        K::Stash => StatsEndpointSpec {
            test_path: "/graphql?query=%7Bstats%7Bscene_count%7D%7D",
            label: "Scenes",
        },
        K::Prometheus => StatsEndpointSpec {
            test_path: "/api/v1/status/runtimeinfo",
            label: "Targets",
        },
        K::Plantit => StatsEndpointSpec {
            test_path: "/api/info/version",
            label: "Plants",
        },
        K::Yuvomi => StatsEndpointSpec {
            test_path: "/api/stats",
            label: "Stats",
        },
        K::Netdata => StatsEndpointSpec {
            test_path: "/api/v1/info",
            label: "Alarms",
        },
        K::Fileflows => StatsEndpointSpec {
            test_path: "/api/status",
            label: "Flows",
        },
        K::Trilium => StatsEndpointSpec {
            test_path: "/etapi/app-info",
            label: "Notes",
        },
        K::Tandoor => StatsEndpointSpec {
            test_path: "/api/recipe/?page_size=1",
            label: "Recipes",
        },
        K::Spoolman => StatsEndpointSpec {
            test_path: "/api/v1/info",
            label: "Spools",
        },
        K::Miniflux => StatsEndpointSpec {
            test_path: "/v1/me",
            label: "Feeds",
        },
        K::Maintainerr => StatsEndpointSpec {
            test_path: "/api/app/status",
            label: "Rules",
        },
        K::Linkwarden => StatsEndpointSpec {
            test_path: "/api/v1/dashboard",
            label: "Links",
        },
        K::Karakeep => StatsEndpointSpec {
            test_path: "/api/v1/bookmarks?limit=1",
            label: "Bookmarks",
        },
        K::Healthchecks => StatsEndpointSpec {
            test_path: "/api/v3/checks/",
            label: "Checks",
        },
        K::Gatus => StatsEndpointSpec {
            test_path: "/api/v1/endpoints/statuses",
            label: "Endpoints",
        },
        K::Changedetection => StatsEndpointSpec {
            test_path: "/api/v1/watch",
            label: "Watches",
        },
        K::Caddy => StatsEndpointSpec {
            test_path: "/config/",
            label: "Routes",
        },
        _ => StatsEndpointSpec {
            test_path: "/",
            label: "Status",
        },
    }
}

pub async fn test_connection(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let spec = spec_for(ctx.kind);
    simple_get_test(http, ctx, spec.test_path).await
}

pub async fn fetch_stats(
    http: &HttpClientFactory,
    ctx: &IntegrationContext,
) -> HomarrResult<StatsResponse> {
    let spec = spec_for(ctx.kind);
    let val = get_json(http, ctx, spec.test_path).await?;
    let mut metrics = Vec::new();
    if let Some(obj) = val.as_object() {
        for (k, v) in obj.iter().take(6) {
            let formatted = match v {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Array(a) => a.len().to_string(),
                _ => continue,
            };
            metrics.push(StatMetric {
                label: k.clone(),
                value: formatted,
                unit: None,
            });
        }
    } else if let Some(arr) = val.as_array() {
        metrics.push(StatMetric {
            label: spec.label.to_string(),
            value: arr.len().to_string(),
            unit: None,
        });
    }
    if metrics.is_empty() {
        metrics.push(StatMetric {
            label: "Status".to_string(),
            value: "Connected".to_string(),
            unit: None,
        });
    }
    Ok(StatsResponse {
        integration_name: ctx.name.clone(),
        kind: ctx.kind.as_str().to_string(),
        metrics,
    })
}
