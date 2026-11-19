//! Prowlarr (indexer manager). Port of `prowlarr/prowlarr-integration.ts`.

use std::collections::HashSet;

use homarr_common::{HomarrError, HomarrResult};
use reqwest::RequestBuilder;
use serde::Deserialize;

use super::servarr::post_empty;
use super::util::{send_text, url};
use crate::capabilities::indexer_manager::{Indexer, IndexerId};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexerResponse {
    id: i64,
    indexer_urls: Vec<String>,
    name: String,
    enable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexerStatus {
    indexer_id: i64,
}

fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<RequestBuilder> {
    let key = ctx.require_secret("apiKey")?;
    Ok(rt
        .http
        .standard()
        .get(url(ctx, path, &[])?)
        .header("X-Api-Key", key))
}

pub async fn indexers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<Indexer>> {
    let indexer_text = send_text(ctx, get(rt, ctx, "/api/v1/indexer")?).await?;
    let status_text = send_text(ctx, get(rt, ctx, "/api/v1/indexerstatus")?).await?;
    map_indexers(ctx, &indexer_text, &status_text)
}

fn map_indexers(
    ctx: &IntegrationContext,
    indexer_json: &str,
    status_json: &str,
) -> HomarrResult<Vec<Indexer>> {
    let indexers = serde_json::from_str::<Vec<IndexerResponse>>(indexer_json);
    let statuses = serde_json::from_str::<Vec<IndexerStatus>>(status_json);
    let mut errors = Vec::new();
    if let Err(err) = &indexers {
        errors.push(format!("Indexers parsing error: {err}"));
    }
    if let Err(err) = &statuses {
        errors.push(format!("Status parsing error: {err}"));
    }
    let (Ok(indexers), Ok(statuses)) = (indexers, statuses) else {
        return Err(HomarrError::Upstream(format!(
            "Failed to parse indexers for {}, most likely your api key is wrong:\n{}",
            ctx.name,
            errors.join("\n")
        )));
    };
    let inactive: HashSet<i64> = statuses.iter().map(|status| status.indexer_id).collect();
    Ok(indexers
        .into_iter()
        .map(|indexer| Indexer {
            id: IndexerId::Number(indexer.id),
            name: indexer.name,
            url: indexer.indexer_urls.into_iter().next().unwrap_or_default(),
            enabled: indexer.enable,
            status: Some(!inactive.contains(&indexer.id)),
        })
        .collect())
}

pub async fn test_all(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<()> {
    post_empty(rt, ctx, "/api/v1/indexer/testall").await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "i".into(),
            name: "Prowlarr".into(),
            url: "http://p".into(),
            external_url: None,
            kind: IntegrationKind::Prowlarr,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn indexers_with_a_status_entry_are_unhealthy() {
        let indexers = r#"[
            {"id":1,"name":"A","enable":true,"indexerUrls":["https://a"]},
            {"id":2,"name":"B","enable":false,"indexerUrls":[]}
        ]"#;
        let mapped = map_indexers(&ctx(), indexers, r#"[{"indexerId":2}]"#).unwrap();
        assert_eq!(mapped[0].status, Some(true));
        assert_eq!(mapped[0].url, "https://a");
        assert_eq!(mapped[1].status, Some(false));
        assert_eq!(mapped[1].url, "");
        assert!(!mapped[1].enabled);
    }

    #[test]
    fn invalid_payload_reports_wrong_api_key() {
        let err = map_indexers(&ctx(), r#"{"error":"Unauthorized"}"#, "[]")
            .unwrap_err()
            .to_string();
        assert!(err.contains("most likely your api key is wrong"));
    }
}
