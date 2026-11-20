//! Thingport: library dashboard data and search (`thingport-integration.ts`).

use std::collections::BTreeMap;

use homarr_common::{HomarrError, HomarrResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::util::{external_url, get_json, input_str, q, to_json, unknown_operation, url};
use crate::capabilities::search::SearchResult;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

/// Thingport caps a search at 25; the search palette has room for fewer.
const SEARCH_MODEL_LIMIT: usize = 10;

const ACTIVITY_KINDS: [&str; 5] = ["import", "upload", "download", "slicer", "delete"];

#[derive(Debug, Deserialize)]
struct Summary {
    model_count: f64,
    collection_count: f64,
    category_count: f64,
    author_count: f64,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    id: String,
    name: String,
    thumb_url: Option<String>,
    view_count: f64,
    print_count: f64,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct Author {
    name: Option<String>,
    handle: Option<String>,
}

impl Author {
    fn display(&self) -> Option<String> {
        self.name.clone().or_else(|| self.handle.clone())
    }
}

#[derive(Debug, Deserialize)]
struct FeaturedPrint {
    id: String,
    name: String,
    view_count: f64,
    print_count: f64,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    thumb_url: Option<String>,
    plates: Vec<FeaturedPlate>,
}

#[derive(Debug, Deserialize)]
struct FeaturedPlate {
    #[serde(default)]
    thumb_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Featured {
    print: Option<FeaturedPrint>,
}

#[derive(Debug, Deserialize)]
struct SearchModel {
    id: String,
    name: String,
    #[serde(default)]
    thumb_url: Option<String>,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(default)]
    author: Option<Author>,
}

#[derive(Debug, Deserialize)]
struct SearchCollection {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct SearchTag {
    tag: String,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    models: Vec<SearchModel>,
    collections: Vec<SearchCollection>,
    tags: Vec<SearchTag>,
}

#[derive(Debug, Deserialize)]
struct ActivityDay {
    date: String,
    counts: BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize)]
struct RawActivity {
    days: Vec<ActivityDay>,
    total: f64,
    years: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryLinks {
    pub models: String,
    pub collections: String,
    pub categories: String,
    pub authors: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThingportLibraryStats {
    pub model_count: f64,
    pub collection_count: f64,
    pub category_count: f64,
    pub author_count: f64,
    pub links: LibraryLinks,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThingportModel {
    pub id: String,
    pub name: String,
    pub href: String,
    /// Thumbnails need the API token, which must never reach the browser, so they go through the image proxy.
    pub thumbnail_url: Option<String>,
    pub view_count: f64,
    pub print_count: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThingportFeaturedModel {
    pub id: String,
    pub name: String,
    pub href: String,
    pub thumbnail_url: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub view_count: f64,
    pub print_count: f64,
    /// Always `None`: the TS version serves GLB/STL meshes through the image proxy, which here only passes images.
    pub mesh: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThingportActivityDay {
    pub date: String,
    pub counts: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThingportActivity {
    pub days: Vec<ThingportActivityDay>,
    pub total: f64,
    pub years: Vec<f64>,
    pub profile_url: String,
}

fn headers(ctx: &IntegrationContext) -> HomarrResult<Vec<(&'static str, String)>> {
    Ok(vec![(
        "Authorization",
        format!("Bearer {}", ctx.require_secret("apiKey")?),
    )])
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

fn model_href(ctx: &IntegrationContext, id: &str) -> HomarrResult<String> {
    Ok(external_url(ctx, &format!("/models/{}", encode(id)))?.to_string())
}

/// `apiUrl`: media paths must stay within the integration's `/api` path.
fn api_url(ctx: &IntegrationContext, relative: &str) -> Option<url::Url> {
    if !relative.starts_with('/') || relative.starts_with("//") || relative.contains('\\') {
        return None;
    }
    let target = url(ctx, &format!("/api{relative}"), &[]).ok()?;
    let api_base = url(ctx, "/api", &[]).ok()?;
    let inside = target.fragment().is_none()
        && target.origin() == api_base.origin()
        && target.path().starts_with(&format!("{}/", api_base.path()));
    inside.then_some(target)
}

async fn proxy_thumbnail(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    relative: Option<&str>,
) -> Option<String> {
    let target = api_url(ctx, relative?)?;
    let headers = headers(ctx)
        .ok()?
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
    super::gotify::proxy_integration_image(rt, ctx, &target, headers).await
}

async fn fetch<T: serde::de::DeserializeOwned>(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    what: &str,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<T> {
    get_json(ctx, &rt.http, what, url(ctx, path, query)?, &headers(ctx)?).await
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    // The health route is public, so only an authenticated route proves the API token works.
    let _: Value = fetch(rt, ctx, "Thingport summary", "/api/dashboard/summary", &[]).await?;
    Ok(())
}

pub async fn library_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<ThingportLibraryStats> {
    let summary: Summary =
        fetch(rt, ctx, "Thingport summary", "/api/dashboard/summary", &[]).await?;
    // Thingport has no category or author index page, so those open the dashboard, as its own count cards do.
    let dashboard = external_url(ctx, "/")?.to_string();
    Ok(ThingportLibraryStats {
        model_count: summary.model_count,
        collection_count: summary.collection_count,
        category_count: summary.category_count,
        author_count: summary.author_count,
        links: LibraryLinks {
            models: external_url(ctx, "/models")?.to_string(),
            collections: external_url(ctx, "/models/collections")?.to_string(),
            categories: dashboard.clone(),
            authors: dashboard,
        },
    })
}

/// `list`: `recentlyAdded`, `topViewed` or `topPrinted`.
pub async fn models(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    list: &str,
    limit: usize,
) -> HomarrResult<Vec<ThingportModel>> {
    let (path, query): (&str, Vec<(&str, Option<String>)>) = match list {
        // Thingport has no "recently added" dashboard list beyond the three in its summary, so that one is the
        // library itself, newest first.
        "recentlyAdded" => (
            "/api/prints",
            vec![("orderBy", q("newest")), ("limit", q(limit))],
        ),
        "topViewed" => ("/api/dashboard/top-viewed", Vec::new()),
        "topPrinted" => ("/api/dashboard/top-printed", Vec::new()),
        other => {
            return Err(HomarrError::BadRequest(format!(
                "Unknown Thingport model list '{other}'"
            )))
        }
    };
    let mut models: Vec<RawModel> = fetch(rt, ctx, "Thingport models", path, &query).await?;
    models.truncate(limit);
    let thumbnails = futures_util::future::join_all(
        models
            .iter()
            .map(|model| proxy_thumbnail(rt, ctx, model.thumb_url.as_deref())),
    )
    .await;
    models
        .into_iter()
        .zip(thumbnails)
        .map(|(model, thumbnail_url)| {
            Ok(ThingportModel {
                href: model_href(ctx, &model.id)?,
                id: model.id,
                name: model.name,
                thumbnail_url,
                view_count: model.view_count,
                print_count: model.print_count,
                created_at: model.created_at,
            })
        })
        .collect()
}

/// Thingport picks one model a day; `date` is the viewer's local day.
pub async fn featured_model(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    date: &str,
) -> HomarrResult<Option<ThingportFeaturedModel>> {
    let featured: Featured = fetch(
        rt,
        ctx,
        "Thingport featured model",
        "/api/dashboard/featured",
        &[("date", q(date))],
    )
    .await?;
    let Some(print) = featured.print else {
        return Ok(None);
    };
    let Some(plate) = print.plates.first() else {
        return Ok(None);
    };
    let thumbnail = plate.thumb_url.as_deref().or(print.thumb_url.as_deref());
    let thumbnail_url = proxy_thumbnail(rt, ctx, thumbnail).await;
    Ok(Some(ThingportFeaturedModel {
        href: model_href(ctx, &print.id)?,
        id: print.id,
        name: print.name,
        thumbnail_url,
        author: print.author.as_ref().and_then(Author::display),
        category: print.category_name,
        view_count: print.view_count,
        print_count: print.print_count,
        mesh: None,
    }))
}

/// `from`/`to` are inclusive YYYY-MM-DD days, counted in the `time_zone` the board is viewed in.
pub async fn activity(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    from: &str,
    to: &str,
    time_zone: &str,
) -> HomarrResult<ThingportActivity> {
    let raw: RawActivity = fetch(
        rt,
        ctx,
        "Thingport activity",
        "/api/activity",
        &[("from", q(from)), ("to", q(to)), ("tz", q(time_zone))],
    )
    .await?;
    Ok(map_activity(
        raw,
        external_url(ctx, "/profile")?.to_string(),
    ))
}

fn map_activity(raw: RawActivity, profile_url: String) -> ThingportActivity {
    ThingportActivity {
        days: raw
            .days
            .into_iter()
            .map(|day| ThingportActivityDay {
                date: day.date,
                // Unknown kinds are dropped instead of failing the parse; zero counts are omitted.
                counts: day
                    .counts
                    .into_iter()
                    .filter(|(kind, count)| {
                        ACTIVITY_KINDS.contains(&kind.as_str()) && *count != 0.0
                    })
                    .collect(),
            })
            .collect(),
        total: raw.total,
        years: raw.years,
        profile_url,
    }
}

/// Models first, as Thingport ranks them, then matching collections and tags.
pub async fn search(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    query: &str,
) -> HomarrResult<Vec<SearchResult>> {
    // The search router hands over the query already passed through encodeURI; the URL builder encodes it again.
    let decoded = percent_encoding_decode(query);
    let query = decoded.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let result: SearchResponse = fetch(
        rt,
        ctx,
        "Thingport search",
        "/api/search",
        &[("q", q(query)), ("limit", q(SEARCH_MODEL_LIMIT))],
    )
    .await?;
    let thumbnails = futures_util::future::join_all(
        result
            .models
            .iter()
            .map(|model| proxy_thumbnail(rt, ctx, model.thumb_url.as_deref())),
    )
    .await;
    map_search(ctx, result, thumbnails)
}

/// `decodeURI`.
fn percent_encoding_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}

/// `thumbnails` holds the proxied thumbnail of each model, in order.
fn map_search(
    ctx: &IntegrationContext,
    result: SearchResponse,
    thumbnails: Vec<Option<String>>,
) -> HomarrResult<Vec<SearchResult>> {
    let mut out = Vec::new();
    for (model, image) in result.models.into_iter().zip(thumbnails) {
        let details = [
            model.author.as_ref().and_then(Author::display),
            model.category_name.clone(),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        out.push(SearchResult {
            id: None,
            link: model_href(ctx, &model.id)?,
            name: model.name,
            image,
            text: (!details.is_empty()).then_some(details),
            result_type: None,
            in_library: None,
            availability: None,
        });
    }
    for collection in result.collections {
        out.push(SearchResult {
            id: None,
            link: external_url(
                ctx,
                &format!("/models/collections/{}", encode(&collection.id)),
            )?
            .to_string(),
            name: collection.name,
            image: None,
            text: None,
            result_type: None,
            in_library: None,
            availability: None,
        });
    }
    for tag in result.tags {
        out.push(SearchResult {
            id: None,
            link: external_url(ctx, &format!("/models/tags/{}", encode(&tag.tag)))?.to_string(),
            name: format!("#{}", tag.tag),
            image: None,
            text: None,
            result_type: None,
            in_library: None,
            availability: None,
        });
    }
    Ok(out)
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    input: &Value,
) -> HomarrResult<Value> {
    let required = |key: &str| {
        input_str(input, key)
            .map(str::to_string)
            .ok_or_else(|| HomarrError::BadRequest(format!("{key} is required")))
    };
    match operation {
        "getLibraryStats" => to_json(&library_stats(rt, ctx).await?),
        "getModels" => {
            let limit = super::util::input_u64(input, "limit").unwrap_or(10) as usize;
            to_json(&models(rt, ctx, &required("list")?, limit).await?)
        }
        "getFeaturedModel" => to_json(&featured_model(rt, ctx, &required("date")?).await?),
        "getActivity" => to_json(
            &activity(
                rt,
                ctx,
                &required("from")?,
                &required("to")?,
                &required("timeZone")?,
            )
            .await?,
        ),
        other => Err(unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;
    use serde_json::json;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "t".into(),
            name: "Thingport".into(),
            url: "http://thingport.local".into(),
            external_url: Some("https://things.example.com".into()),
            kind: IntegrationKind::Thingport,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn search_orders_models_collections_tags() {
        let response: SearchResponse = serde_json::from_value(json!({
            "models": [{"id": "m 1", "name": "Benchy", "author": {"name": null, "handle": "bob"}, "category_name": "Boats"}],
            "collections": [{"id": "c1", "name": "Calibration"}],
            "tags": [{"tag": "test print", "count": 3}]
        }))
        .unwrap();
        let out = map_search(&ctx(), response, vec![None]).unwrap();
        assert_eq!(out[0].link, "https://things.example.com/models/m%201");
        assert_eq!(out[0].text.as_deref(), Some("bob · Boats"));
        assert_eq!(
            out[1].link,
            "https://things.example.com/models/collections/c1"
        );
        assert_eq!(out[2].name, "#test print");
        assert_eq!(
            out[2].link,
            "https://things.example.com/models/tags/test%20print"
        );
    }

    #[test]
    fn api_url_stays_inside_api_path() {
        let ctx = ctx();
        assert_eq!(
            api_url(&ctx, "/thumbs/a.png?v=1").unwrap().as_str(),
            "http://thingport.local/api/thumbs/a.png?v=1"
        );
        assert!(api_url(&ctx, "/../secret").is_none());
        assert!(api_url(&ctx, "//evil.example/x").is_none());
        assert!(api_url(&ctx, "thumbs/a.png").is_none());
    }

    #[test]
    fn activity_drops_unknown_kinds_and_zero_counts() {
        let raw: RawActivity = serde_json::from_value(json!({
            "days": [{"date": "2026-01-01", "counts": {"upload": 2, "download": 0, "future": 5}}],
            "total": 2, "years": [2026]
        }))
        .unwrap();
        let out = map_activity(raw, "p".into());
        assert_eq!(
            out.days[0].counts,
            BTreeMap::from([("upload".to_string(), 2.0)])
        );
    }
}
