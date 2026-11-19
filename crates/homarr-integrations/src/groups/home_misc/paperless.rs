//! Paperless-ngx: statistics (`paperless-ngx-integration.ts`).

use homarr_common::HomarrResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::util::{get_json, to_json, unknown_operation, url};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Deserialize)]
struct Statistics {
    documents_total: f64,
    #[serde(default)]
    documents_inbox: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct PaginatedCount {
    count: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperlessNgxStats {
    pub documents_total: f64,
    pub documents_inbox: f64,
    pub correspondents_count: f64,
    pub tags_count: f64,
    pub document_types_count: f64,
}

fn headers(ctx: &IntegrationContext) -> HomarrResult<Vec<(&'static str, String)>> {
    Ok(vec![(
        "Authorization",
        format!("Token {}", ctx.require_secret("apiKey")?),
    )])
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let mut request = rt.http.standard().get(url(ctx, "/api/statistics/", &[])?);
    for (name, value) in headers(ctx)? {
        request = request.header(name, value);
    }
    super::util::send(ctx, request).await?;
    Ok(())
}

async fn fetch<T: serde::de::DeserializeOwned>(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<T> {
    get_json(ctx, &rt.http, path, url(ctx, path, &[])?, &headers(ctx)?).await
}

pub async fn stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<PaperlessNgxStats> {
    let (statistics, tags, correspondents, document_types) = tokio::try_join!(
        fetch::<Statistics>(rt, ctx, "/api/statistics/"),
        fetch::<PaginatedCount>(rt, ctx, "/api/tags/"),
        fetch::<PaginatedCount>(rt, ctx, "/api/correspondents/"),
        fetch::<PaginatedCount>(rt, ctx, "/api/document_types/"),
    )?;
    Ok(PaperlessNgxStats {
        documents_total: statistics.documents_total,
        documents_inbox: statistics.documents_inbox.unwrap_or(0.0),
        correspondents_count: correspondents.count,
        tags_count: tags.count,
        document_types_count: document_types.count,
    })
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    _input: &Value,
) -> HomarrResult<Value> {
    match operation {
        "getStats" => to_json(&stats(rt, ctx).await?),
        other => Err(unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_inbox_counts_as_zero() {
        let parsed: Statistics =
            serde_json::from_str(r#"{"documents_total": 12, "documents_inbox": null}"#).unwrap();
        assert_eq!(parsed.documents_inbox.unwrap_or(0.0), 0.0);
        assert!(serde_json::from_str::<Statistics>(r#"{"documents_inbox": 1}"#).is_err());
    }
}
