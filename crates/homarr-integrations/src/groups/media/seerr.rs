//! Overseerr, Jellyseerr and Seerr (media requests + search). Port of `overseerr/overseerr-integration.ts`,
//! `jellyseerr/*` and `seerr/*` (Seerr is API compatible with Overseerr).
//!
//! See <https://api-docs.overseerr.dev>.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use homarr_common::{HomarrError, HomarrResult};
use homarr_definitions::IntegrationKind;
use reqwest::{Method, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};

use super::util::{external_url, q, send, send_json, url};
use crate::capabilities::media_requests::{
    MediaAvailability, MediaInformation, MediaRequest, MediaRequestMediaType, MediaRequestStatus,
    MediaRequestUser, RequestStats, RequestUser, SerieSeason,
};
use crate::capabilities::search::SearchResult;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

fn is_jellyseerr(ctx: &IntegrationContext) -> bool {
    ctx.kind == IntegrationKind::Jellyseerr
}

fn api(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    method: Method,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<RequestBuilder> {
    let key = ctx.require_secret("apiKey")?;
    Ok(rt
        .http
        .standard()
        .request(method, url(ctx, path, query)?)
        .header("X-Api-Key", key))
}

pub fn media_type_str(media_type: MediaRequestMediaType) -> &'static str {
    match media_type {
        MediaRequestMediaType::Movie => "movie",
        MediaRequestMediaType::Tv => "tv",
    }
}

// ---- status mapping --------------------------------------------------------------------------

fn map_request_status(status: i64) -> MediaRequestStatus {
    match status {
        1 => MediaRequestStatus::Pending,
        2 => MediaRequestStatus::Approved,
        3 => MediaRequestStatus::Declined,
        5 => MediaRequestStatus::Completed,
        _ => MediaRequestStatus::Failed,
    }
}

/// See <https://github.com/seerr-team/seerr/blob/af083a3cd5c3e3d5d7917fdf4fdd67fe3f39c46b/src/components/StatusBadge/index.tsx#L153-L387>.
/// Jellyseerr additionally knows "blacklisted" (6) and "deleted" (7); Overseerr's 6 means "deleted".
fn map_availability(status: i64, in_progress: bool, jellyseerr: bool) -> MediaAvailability {
    if jellyseerr {
        match status {
            6 => return MediaAvailability::Blacklisted,
            7 => return MediaAvailability::Deleted,
            _ => {}
        }
    }
    match status {
        5 if in_progress => MediaAvailability::Processing,
        5 => MediaAvailability::Available,
        4 if in_progress => MediaAvailability::Processing,
        4 => MediaAvailability::PartiallyAvailable,
        3 if in_progress => MediaAvailability::Processing,
        3 => MediaAvailability::Requested,
        2 => MediaAvailability::Pending,
        6 => MediaAvailability::Deleted,
        _ if in_progress => MediaAvailability::Processing,
        _ => MediaAvailability::Unknown,
    }
}

fn availability_name(availability: MediaAvailability) -> String {
    serde_json::to_value(availability)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn construct_avatar_url(ctx: &IntegrationContext, avatar: &str) -> String {
    if avatar.starts_with("http://") || avatar.starts_with("https://") {
        return avatar.to_string();
    }
    external_url(ctx, &format!("/{avatar}"))
}

// ---- test connection -------------------------------------------------------------------------

#[derive(Deserialize)]
#[allow(dead_code)]
struct Me {
    id: i64,
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let _: Me = send_json(
        ctx,
        "the current user",
        api(rt, ctx, Method::GET, "/api/v1/auth/me", &[])?,
    )
    .await?;
    Ok(())
}

// ---- search ----------------------------------------------------------------------------------

fn tmdb_image(path: &str) -> String {
    format!("https://image.tmdb.org/t/p/w600_and_h900_bestv2{path}")
}

fn map_search(ctx: &IntegrationContext, response: &Value) -> Vec<SearchResult> {
    let jellyseerr = is_jellyseerr(ctx);
    let Some(results) = response.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|result| {
            let media_type = result.get("mediaType")?.as_str()?;
            let id = result.get("id")?.as_i64()?;
            let (name, image_path) = match media_type {
                "tv" => (result.get("name"), result.get("posterPath")),
                "movie" => (result.get("title"), result.get("posterPath")),
                "person" => (result.get("name"), result.get("profilePath")),
                // Collections and other result kinds have no supported link.
                _ => return None,
            };
            let media_info_status = result
                .get("mediaInfo")
                .filter(|info| !info.is_null())
                .map(|info| info.get("status").and_then(Value::as_i64).unwrap_or(1));
            Some(SearchResult {
                id: Some(id),
                name: name.and_then(Value::as_str).unwrap_or_default().to_string(),
                link: external_url(ctx, &format!("/{media_type}/{id}")),
                image: image_path.and_then(Value::as_str).map(tmdb_image),
                text: result
                    .get("overview")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                result_type: Some(media_type.to_string()),
                in_library: Some(media_info_status.is_some()),
                availability: media_info_status
                    .map(|status| availability_name(map_availability(status, false, jellyseerr))),
            })
        })
        .collect()
}

pub async fn search(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    query: &str,
) -> HomarrResult<Vec<SearchResult>> {
    let response: Value = send_json(
        ctx,
        "the search results",
        api(
            rt,
            ctx,
            Method::GET,
            "/api/v1/search",
            &[("query", q(query))],
        )?,
    )
    .await?;
    Ok(map_search(ctx, &response))
}

// ---- media information & request creation ----------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeasonResponse {
    id: i64,
    season_number: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    episode_count: i64,
}

#[derive(Debug, Deserialize)]
struct MediaInfoSeason {
    #[serde(rename = "seasonNumber")]
    season_number: i64,
}

#[derive(Debug, Deserialize)]
struct MediaInfoRequest {
    #[serde(default)]
    seasons: Vec<MediaInfoSeason>,
}

#[derive(Debug, Deserialize)]
struct MediaInfo {
    #[serde(default)]
    requests: Option<Vec<MediaInfoRequest>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InformationResponse {
    id: i64,
    overview: String,
    #[serde(default)]
    seasons: Option<Vec<SeasonResponse>>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    media_info: Option<MediaInfo>,
}

fn map_information(
    media_type: MediaRequestMediaType,
    data: InformationResponse,
) -> MediaInformation {
    let mut requested_seasons: Vec<i64> = Vec::new();
    for season in data
        .media_info
        .and_then(|info| info.requests)
        .unwrap_or_default()
        .iter()
        .flat_map(|request| request.seasons.iter())
    {
        if !requested_seasons.contains(&season.season_number) {
            requested_seasons.push(season.season_number);
        }
    }
    MediaInformation {
        id: data.id,
        overview: data.overview,
        seasons: (media_type == MediaRequestMediaType::Tv).then(|| {
            data.seasons
                .unwrap_or_default()
                .into_iter()
                .map(|season| SerieSeason {
                    id: season.id,
                    season_number: season.season_number,
                    name: season.name,
                    episode_count: season.episode_count,
                })
                .collect()
        }),
        poster_path: data.poster_path.unwrap_or_default(),
        requested_seasons: Some(requested_seasons),
    }
}

pub async fn series_information(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    media_type: MediaRequestMediaType,
    id: i64,
) -> HomarrResult<MediaInformation> {
    let path = format!("/api/v1/{}/{id}", media_type_str(media_type));
    let data: InformationResponse = send_json(
        ctx,
        "the media information",
        api(rt, ctx, Method::GET, &path, &[])?,
    )
    .await?;
    Ok(map_information(media_type, data))
}

/// Request a media. See <https://api-docs.overseerr.dev/#/request/post_request>.
pub async fn create_request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    media_type: MediaRequestMediaType,
    id: i64,
    seasons: Option<&[i64]>,
) -> HomarrResult<()> {
    let mut body = json!({ "mediaType": media_type_str(media_type), "mediaId": id });
    if let (Some(seasons), Some(object)) = (seasons, body.as_object_mut()) {
        object.insert("seasons".to_string(), json!(seasons));
    }
    let request = api(rt, ctx, Method::POST, "/api/v1/request", &[])?.json(&body);
    let response = request
        .send()
        .await
        .map_err(|err| crate::error::request_error(ctx, &err))?;
    if response.status().as_u16() != 201 {
        return Err(HomarrError::Upstream(format!(
            "{} returned status code {} instead of 201. The request was likely not created",
            ctx.name,
            response.status().as_u16()
        )));
    }
    Ok(())
}

// ---- requests --------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestMedia {
    status: i64,
    tmdb_id: i64,
    #[serde(default)]
    download_status: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestedBy {
    id: i64,
    display_name: String,
    avatar: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestEntry {
    id: i64,
    status: i64,
    created_at: String,
    media: RequestMedia,
    #[serde(rename = "type")]
    media_type: MediaRequestMediaType,
    #[serde(default)]
    requested_by: Option<RequestedBy>,
}

#[derive(Debug, Deserialize)]
struct RequestsResponse {
    #[serde(default)]
    results: Option<Vec<RequestEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemInformation {
    /// Series name.
    #[serde(default)]
    name: Option<String>,
    /// Movie title.
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
}

#[derive(Debug, Clone)]
struct Information {
    name: String,
    backdrop_path: String,
    poster_path: String,
    air_date: Option<DateTime<Utc>>,
}

fn into_information(media_type: MediaRequestMediaType, item: ItemInformation) -> Information {
    let (name, air_date) = match media_type {
        MediaRequestMediaType::Tv => (item.name, item.first_air_date),
        MediaRequestMediaType::Movie => (item.title, item.release_date),
    };
    let backdrop = item
        .backdrop_path
        .clone()
        .or_else(|| item.poster_path.clone());
    let poster = item.poster_path.or(item.backdrop_path);
    Information {
        name: name.unwrap_or_default(),
        backdrop_path: backdrop.unwrap_or_default(),
        poster_path: poster.unwrap_or_default(),
        air_date: air_date.as_deref().and_then(parse_air_date),
    }
}

fn parse_air_date(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some(date.with_timezone(&Utc));
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc())
}

/// Pending requests first, then the most recently modified ones without the pending duplicates.
fn merge_requests(pending: Vec<RequestEntry>, all: Vec<RequestEntry>) -> Vec<RequestEntry> {
    if pending.is_empty() {
        return all;
    }
    let mut merged = pending;
    merged.extend(all.into_iter().filter(|request| request.status != 1));
    merged
}

fn map_request(
    ctx: &IntegrationContext,
    request: &RequestEntry,
    information: &Information,
) -> Option<MediaRequest> {
    // See https://github.com/seerr-team/seerr/blob/af083a3cd5c3e3d5d7917fdf4fdd67fe3f39c46b/src/components/StatusBadge/index.tsx#L40
    let in_progress = request
        .media
        .download_status
        .as_ref()
        .is_some_and(|status| !status.is_empty());
    let media_type = media_type_str(request.media_type);
    Some(MediaRequest {
        id: request.id,
        name: information.name.clone(),
        status: map_request_status(request.status),
        availability: map_availability(request.media.status, in_progress, is_jellyseerr(ctx)),
        backdrop_image_url: format!(
            "https://image.tmdb.org/t/p/w780/{}",
            information.backdrop_path
        ),
        poster_image_path: format!(
            "https://image.tmdb.org/t/p/w154/{}",
            information.poster_path
        ),
        href: external_url(ctx, &format!("/{media_type}/{}", request.media.tmdb_id)),
        media_type: request.media_type,
        created_at: parse_air_date(&request.created_at)?,
        air_date: information.air_date,
        requested_by: request.requested_by.as_ref().map(|user| MediaRequestUser {
            id: user.id,
            display_name: user.display_name.clone(),
            link: external_url(ctx, &format!("/users/{}", user.id)),
            avatar: construct_avatar_url(ctx, &user.avatar),
        }),
    })
}

async fn fetch_requests(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    query: &[(&str, Option<String>)],
) -> HomarrResult<Vec<RequestEntry>> {
    let response: RequestsResponse = send_json(
        ctx,
        "the requests",
        api(rt, ctx, Method::GET, "/api/v1/request", query)?,
    )
    .await?;
    Ok(response.results.unwrap_or_default())
}

pub async fn requests(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<MediaRequest>> {
    let (pending, all) = futures_util::future::join(
        fetch_requests(rt, ctx, &[("take", q(20)), ("filter", q("pending"))]),
        fetch_requests(rt, ctx, &[("take", q(20)), ("sort", q("modified"))]),
    )
    .await;
    let requests = merge_requests(pending?, all?);
    if requests.is_empty() {
        return Ok(Vec::new());
    }

    // Requests for different users can reference the same movie or series: look each one up once.
    let mut keys: Vec<(MediaRequestMediaType, i64)> = Vec::new();
    for request in &requests {
        let key = (request.media_type, request.media.tmdb_id);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let lookups = keys.iter().map(|(media_type, tmdb_id)| async move {
        let path = format!("/api/v1/{}/{tmdb_id}", media_type_str(*media_type));
        let item: ItemInformation = send_json(
            ctx,
            "the media information",
            api(rt, ctx, Method::GET, &path, &[])?,
        )
        .await?;
        HomarrResult::Ok(into_information(*media_type, item))
    });
    let mut information: HashMap<(&'static str, i64), Information> = HashMap::new();
    for ((media_type, tmdb_id), result) in keys
        .iter()
        .zip(futures_util::future::join_all(lookups).await)
    {
        match result {
            Ok(info) => {
                information.insert((media_type_str(*media_type), *tmdb_id), info);
            }
            Err(err) => tracing::warn!(
                integration = %ctx.name,
                tmdb_id,
                error = %err,
                "Failed to fetch media information of a request"
            ),
        }
    }

    let mapped: Vec<MediaRequest> = requests
        .iter()
        .filter_map(|request| {
            let info =
                information.get(&(media_type_str(request.media_type), request.media.tmdb_id))?;
            map_request(ctx, request, info)
        })
        .collect();
    if mapped.is_empty() {
        return Err(HomarrError::Upstream(
            "Failed to resolve any media request information".to_string(),
        ));
    }
    Ok(mapped)
}

// ---- stats, users, actions -------------------------------------------------------------------

pub async fn stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<RequestStats> {
    send_json(
        ctx,
        "the request statistics",
        api(rt, ctx, Method::GET, "/api/v1/request/count", &[])?,
    )
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserEntry {
    id: i64,
    display_name: String,
    avatar: String,
    request_count: u64,
}

#[derive(Debug, Deserialize)]
struct UsersResponse {
    #[serde(default)]
    results: Option<Vec<UserEntry>>,
}

pub async fn users(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<RequestUser>> {
    let response: UsersResponse = send_json(
        ctx,
        "the users",
        api(
            rt,
            ctx,
            Method::GET,
            "/api/v1/user",
            &[("take", q(10)), ("sort", q("requests"))],
        )?,
    )
    .await?;
    Ok(response
        .results
        .unwrap_or_default()
        .into_iter()
        .map(|user| RequestUser {
            link: external_url(ctx, &format!("/users/{}", user.id)),
            avatar: construct_avatar_url(ctx, &user.avatar),
            id: user.id,
            display_name: user.display_name,
            request_count: user.request_count,
        })
        .collect())
}

pub async fn decide_request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    request_id: i64,
    approve: bool,
) -> HomarrResult<()> {
    let action = if approve { "approve" } else { "decline" };
    tracing::info!(request_id, integration = %ctx.name, "{action} media request");
    let path = format!("/api/v1/request/{request_id}/{action}");
    send(ctx, api(rt, ctx, Method::POST, &path, &[])?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ctx(kind: IntegrationKind) -> IntegrationContext {
        IntegrationContext {
            id: "i".into(),
            name: "Seerr".into(),
            url: "http://seerr:5055".into(),
            external_url: Some("https://requests.example.com".into()),
            kind,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn availability_follows_the_status_badge_rules() {
        use MediaAvailability::*;
        assert_eq!(map_availability(5, false, false), Available);
        assert_eq!(map_availability(5, true, false), Processing);
        assert_eq!(map_availability(4, false, false), PartiallyAvailable);
        assert_eq!(map_availability(3, false, false), Requested);
        assert_eq!(map_availability(2, true, false), Pending);
        assert_eq!(map_availability(1, false, false), Unknown);
        assert_eq!(map_availability(1, true, false), Processing);
        // 6 is "deleted" for Overseerr but "blacklisted" for Jellyseerr, which also knows 7 (deleted).
        assert_eq!(map_availability(6, false, false), Deleted);
        assert_eq!(map_availability(6, false, true), Blacklisted);
        assert_eq!(map_availability(7, false, true), Deleted);
    }

    #[test]
    fn request_status_codes() {
        assert_eq!(map_request_status(1), MediaRequestStatus::Pending);
        assert_eq!(map_request_status(2), MediaRequestStatus::Approved);
        assert_eq!(map_request_status(3), MediaRequestStatus::Declined);
        assert_eq!(map_request_status(4), MediaRequestStatus::Failed);
        assert_eq!(map_request_status(5), MediaRequestStatus::Completed);
        assert_eq!(map_request_status(99), MediaRequestStatus::Failed);
    }

    fn entry(id: i64, status: i64) -> RequestEntry {
        serde_json::from_value(json!({
            "id": id, "status": status, "createdAt": "2024-01-01T00:00:00.000Z",
            "media": { "status": 3, "tmdbId": 100 + id, "downloadStatus": [{}] },
            "type": "movie",
            "requestedBy": { "id": 7, "displayName": "Alice", "avatar": "avatar/a.png" }
        }))
        .unwrap()
    }

    #[test]
    fn pending_requests_are_not_duplicated_in_the_modified_list() {
        let merged = merge_requests(vec![entry(1, 1)], vec![entry(1, 1), entry(2, 2)]);
        let ids: Vec<i64> = merged.iter().map(|request| request.id).collect();
        assert_eq!(ids, [1, 2]);
        assert_eq!(merge_requests(vec![], vec![entry(3, 1)]).len(), 1);
    }

    #[test]
    fn maps_a_request_with_links_and_avatars() {
        let information = into_information(
            MediaRequestMediaType::Movie,
            serde_json::from_value(json!({
                "title": "Film", "posterPath": "/p.jpg", "releaseDate": "2024-05-01"
            }))
            .unwrap(),
        );
        let request =
            map_request(&ctx(IntegrationKind::Overseerr), &entry(1, 1), &information).unwrap();
        assert_eq!(request.name, "Film");
        // downloadStatus is not empty
        assert_eq!(request.availability, MediaAvailability::Processing);
        assert_eq!(request.href, "https://requests.example.com/movie/101");
        assert_eq!(
            request.poster_image_path,
            "https://image.tmdb.org/t/p/w154//p.jpg"
        );
        assert_eq!(
            request.backdrop_image_url,
            "https://image.tmdb.org/t/p/w780//p.jpg"
        );
        let user = request.requested_by.unwrap();
        assert_eq!(user.avatar, "https://requests.example.com/avatar/a.png");
        assert_eq!(user.link, "https://requests.example.com/users/7");
        assert_eq!(
            request.air_date.unwrap().to_rfc3339(),
            "2024-05-01T00:00:00+00:00"
        );
    }

    #[test]
    fn search_results_include_library_state_and_skip_unknown_types() {
        let response = json!({ "results": [
            { "id": 1, "mediaType": "movie", "title": "Film", "posterPath": "/p.jpg", "overview": "O",
              "mediaInfo": { "status": 5 } },
            { "id": 2, "mediaType": "person", "name": "Actor", "profilePath": null },
            { "id": 3, "mediaType": "collection", "name": "Saga" }
        ]});
        let results = map_search(&ctx(IntegrationKind::Overseerr), &response);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].in_library, Some(true));
        assert_eq!(results[0].availability.as_deref(), Some("available"));
        assert_eq!(results[0].link, "https://requests.example.com/movie/1");
        assert_eq!(
            results[0].image.as_deref(),
            Some("https://image.tmdb.org/t/p/w600_and_h900_bestv2/p.jpg")
        );
        assert_eq!(results[1].in_library, Some(false));
        assert_eq!(results[1].availability, None);
        assert_eq!(results[1].image, None);
    }

    #[test]
    fn series_information_lists_unique_requested_seasons() {
        let data: InformationResponse = serde_json::from_value(json!({
            "id": 9, "overview": "O", "posterPath": "/p.jpg",
            "seasons": [{ "id": 1, "seasonNumber": 1, "name": "S1", "episodeCount": 8 }],
            "mediaInfo": { "requests": [
                { "seasons": [{ "seasonNumber": 1 }, { "seasonNumber": 2 }] },
                { "seasons": [{ "seasonNumber": 2 }] }
            ]}
        }))
        .unwrap();
        let info = map_information(MediaRequestMediaType::Tv, data);
        assert_eq!(info.requested_seasons, Some(vec![1, 2]));
        assert_eq!(info.seasons.unwrap().len(), 1);
    }
}
