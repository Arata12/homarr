//! Jellyfin (media server + media releases). Port of `jellyfin/jellyfin-integration.ts`.

use std::collections::{HashMap, HashSet};

use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::json;

use super::emby_jellyfin::{
    map_release_type, map_sessions, parse_item_date, to_fixed_1, SessionInfo,
};
use super::util::{external_url, q, send, send_json, url};
use crate::capabilities::media_releases::{MediaRelease, MediaReleaseImageUrls};
use crate::capabilities::media_server::{CurrentSessionsInput, StreamSession};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const AUTHORIZATION_PREFIX: &str =
    r#"MediaBrowser Client="Homarr", Device="Homarr", DeviceId="homarr", Version="0.0.1""#;
const OWN_DEVICE_ID: &str = "homarr";
const RELEASES_LIMIT: u32 = 100;

fn authentication_error() -> HomarrError {
    HomarrError::Upstream("Jellyfin authentication failed".to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AuthenticationResult {
    #[serde(default)]
    access_token: Option<String>,
}

async fn access_token(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<String> {
    if let Some(key) = ctx.secret("apiKey") {
        return Ok(key.to_string());
    }
    let username = ctx.require_secret("username")?;
    let password = ctx.require_secret("password")?;
    let request = rt
        .http
        .standard()
        .post(url(ctx, "/Users/AuthenticateByName", &[])?)
        .header("Authorization", AUTHORIZATION_PREFIX)
        .json(&json!({ "Username": username, "Pw": password }));
    let response = request.send().await.map_err(|_| authentication_error())?;
    if !response.status().is_success() {
        return Err(authentication_error());
    }
    let result: AuthenticationResult = response.json().await.map_err(|_| authentication_error())?;
    result
        .access_token
        .filter(|token| !token.is_empty())
        .ok_or_else(authentication_error)
}

async fn authorization(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<String> {
    let token = access_token(rt, ctx).await?;
    Ok(format!("{AUTHORIZATION_PREFIX}, Token=\"{token}\""))
}

async fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    authorization: &str,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<reqwest::RequestBuilder> {
    Ok(rt
        .http
        .standard()
        .get(url(ctx, path, query)?)
        .header("Authorization", authorization))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let authorization = authorization(rt, ctx).await?;
    send(
        ctx,
        get(rt, ctx, &authorization, "/System/Ping", &[]).await?,
    )
    .await?;
    Ok(())
}

pub async fn sessions(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    options: CurrentSessionsInput,
) -> HomarrResult<Vec<StreamSession>> {
    let authorization = authorization(rt, ctx).await?;
    let sessions: Vec<SessionInfo> = send_json(
        ctx,
        "the sessions",
        get(rt, ctx, &authorization, "/Sessions", &[]).await?,
    )
    .await?;
    Ok(map_sessions(
        ctx,
        sessions,
        OWN_DEVICE_ID,
        options.show_only_playing,
    ))
}

// ---- media releases --------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Studio {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Item {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, rename = "Type")]
    item_type: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    series_name: Option<String>,
    #[serde(default)]
    series_id: Option<String>,
    #[serde(default)]
    date_created: Option<String>,
    #[serde(default)]
    premiere_date: Option<String>,
    #[serde(default)]
    taglines: Option<Vec<String>>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    studios: Option<Vec<Studio>>,
    #[serde(default)]
    community_rating: Option<f64>,
    #[serde(default)]
    genres: Option<Vec<String>>,
    #[serde(default)]
    backdrop_image_tags: Option<Vec<String>>,
    #[serde(default)]
    parent_backdrop_image_tags: Option<Vec<String>>,
    #[serde(default)]
    parent_backdrop_item_id: Option<String>,
    #[serde(default)]
    server_id: Option<String>,
    #[serde(default)]
    parent_index_number: Option<i64>,
    #[serde(default)]
    index_number: Option<i64>,
    #[serde(default)]
    episode_title: Option<String>,
}

impl Item {
    fn first_tagline(&self) -> Option<String> {
        self.taglines.as_ref().and_then(|t| t.first()).cloned()
    }
}

/// Episodes only carry their own name, so the show name is used as the title and the episode as the subtitle.
pub fn release_title(item: &Item) -> String {
    if item.item_type.as_deref() == Some("Episode") {
        if let Some(series) = &item.series_name {
            return series.clone();
        }
    }
    item.name.clone().unwrap_or_default()
}

pub fn episode_subtitle(item: &Item) -> Option<String> {
    // Without a series name the title stays the item name, so the previous tagline is kept as the subtitle.
    if item.series_name.is_none() {
        return item.first_tagline();
    }
    let name = item
        .name
        .clone()
        .or_else(|| item.episode_title.clone())
        .unwrap_or_default();
    match (item.parent_index_number, item.index_number) {
        (Some(season), Some(episode)) => {
            let code = format!("S{season:02}E{episode:02}");
            Some(if name.is_empty() {
                code
            } else {
                format!("{code} \u{2013} {name}")
            })
        }
        _ => (!name.is_empty()).then_some(name),
    }
}

fn millis(item: &Item) -> Option<i64> {
    item.date_created
        .as_deref()
        .and_then(parse_item_date)
        .map(|date| date.timestamp_millis())
}

/// Episodes of the latest additions grouped by series. Additions within 24 hours of the newest
/// episode of a series are kept (like Jellyfin groups them).
fn group_recent_episodes(items: &[Item]) -> HashMap<String, Vec<Item>> {
    let mut by_series: HashMap<String, Vec<Item>> = HashMap::new();
    for item in items {
        let Some(series_id) = &item.series_id else {
            continue;
        };
        if item.item_type.as_deref() != Some("Episode") {
            continue;
        }
        by_series
            .entry(series_id.clone())
            .or_default()
            .push(item.clone());
    }
    for episodes in by_series.values_mut() {
        let dated: Vec<(Item, i64)> = episodes
            .iter()
            .filter_map(|episode| millis(episode).map(|at| (episode.clone(), at)))
            .collect();
        let Some(newest) = dated.iter().map(|(_, at)| *at).max() else {
            continue;
        };
        let cutoff = newest - 24 * 60 * 60 * 1000;
        *episodes = dated
            .into_iter()
            .filter(|(_, at)| *at >= cutoff)
            .map(|(episode, _)| episode)
            .collect();
    }
    by_series
}

/// Replaces recent episodes of a series with the series itself (when it could be loaded).
fn collapse_episodes(
    items: Vec<Item>,
    recent: &HashMap<String, Vec<Item>>,
    series: &HashMap<String, Item>,
) -> Vec<Item> {
    let mut displayed: HashSet<String> = HashSet::new();
    let mut result = Vec::new();
    for item in items {
        let series_id = match (&item.item_type, &item.series_id) {
            (Some(kind), Some(series_id)) if kind == "Episode" => series_id.clone(),
            _ => {
                result.push(item);
                continue;
            }
        };
        match series.get(&series_id) {
            None => {
                // Single additions use the newest episode; failed batch lookups keep the original episodes.
                if let Some(recent_episodes) = recent.get(&series_id) {
                    if recent_episodes.len() == 1 && recent_episodes[0].id != item.id {
                        continue;
                    }
                }
                result.push(item);
            }
            Some(parent) => {
                if displayed.insert(series_id) {
                    result.push(parent.clone());
                }
            }
        }
    }
    result
}

fn backdrop_url(ctx: &IntegrationContext, item: &Item) -> Option<String> {
    let item_id = if item
        .backdrop_image_tags
        .as_ref()
        .is_some_and(|t| !t.is_empty())
    {
        item.id.clone()
    } else if item
        .parent_backdrop_image_tags
        .as_ref()
        .is_some_and(|t| !t.is_empty())
    {
        item.parent_backdrop_item_id.clone()
    } else {
        None
    }?;
    Some(external_url(
        ctx,
        &format!("/Items/{item_id}/Images/Backdrop/0?maxWidth=960&quality=70"),
    ))
}

fn map_releases(ctx: &IntegrationContext, items: Vec<Item>) -> Vec<MediaRelease> {
    items
        .into_iter()
        .filter_map(|item| {
            let id = item.id.clone()?;
            let release_date = item
                .premiere_date
                .as_deref()
                .or(item.date_created.as_deref())
                .and_then(parse_item_date)?;
            let is_episode = item.item_type.as_deref() == Some("Episode");
            Some(MediaRelease {
                id: id.clone(),
                media_type: map_release_type(item.item_type.as_deref()),
                title: release_title(&item),
                subtitle: if is_episode {
                    episode_subtitle(&item)
                } else {
                    item.first_tagline()
                },
                description: item.overview.clone(),
                release_date,
                release_date_is_date_only: None,
                image_urls: MediaReleaseImageUrls {
                    poster: Some(external_url(
                        ctx,
                        &format!(
                            "/Items/{id}/Images/Primary?maxHeight=492&maxWidth=328&quality=90"
                        ),
                    )),
                    backdrop: backdrop_url(ctx, &item),
                },
                producer: item
                    .studios
                    .as_ref()
                    .and_then(|studios| studios.first())
                    .and_then(|studio| studio.name.clone()),
                price: None,
                rating: item.community_rating.map(to_fixed_1),
                tags: item.genres.clone().unwrap_or_default(),
                href: external_url(
                    ctx,
                    &format!(
                        "/web/index.html#!/details?id={id}&serverId={}",
                        item.server_id.as_deref().unwrap_or("undefined")
                    ),
                ),
                length: None,
            })
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct User {
    #[serde(default)]
    id: Option<String>,
}

pub async fn media_releases(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<MediaRelease>> {
    let authorization = authorization(rt, ctx).await?;
    let users: Vec<User> = send_json(
        ctx,
        "the users",
        get(rt, ctx, &authorization, "/Users", &[]).await?,
    )
    .await?;
    let user_id = users
        .into_iter()
        .next()
        .and_then(|user| user.id)
        .ok_or_else(|| HomarrError::Upstream("No users found".to_string()))?;

    let query = [
        ("userId", Some(user_id.clone())),
        (
            "fields",
            q("CustomRating,Studios,Genres,ChildCount,DateCreated,Overview,Taglines"),
        ),
        ("limit", q(RELEASES_LIMIT)),
        // Root-level grouping uses the first library's type and can omit items from other libraries.
        ("groupItems", q(false)),
    ];
    let latest: Vec<Item> = send_json(
        ctx,
        "the latest media",
        get(rt, ctx, &authorization, "/Items/Latest", &query).await?,
    )
    .await?;

    let recent = group_recent_episodes(&latest);
    let series_ids: Vec<&String> = recent
        .iter()
        .filter(|(_, episodes)| episodes.len() > 1)
        .map(|(id, _)| id)
        .collect();
    let lookups = series_ids.iter().map(|series_id| {
        let authorization = authorization.clone();
        let user_id = user_id.clone();
        async move {
            let path = format!("/Items/{series_id}");
            let request = get(rt, ctx, &authorization, &path, &[("userId", Some(user_id))]).await?;
            send_json::<Item>(ctx, "a series", request).await
        }
    });
    let mut series = HashMap::new();
    // A failed lookup keeps the original episodes.
    for item in futures_util::future::join_all(lookups)
        .await
        .into_iter()
        .flatten()
    {
        if let (Some("Series"), Some(id)) = (item.item_type.as_deref(), item.id.clone()) {
            series.insert(id, item);
        }
    }
    Ok(map_releases(
        ctx,
        collapse_episodes(latest, &recent, &series),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn item(value: serde_json::Value) -> Item {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn titles_use_series_name_for_episodes() {
        assert_eq!(
            release_title(&item(
                json!({"Type":"Episode","Name":"Ozymandias","SeriesName":"Breaking Bad"})
            )),
            "Breaking Bad"
        );
        assert_eq!(
            release_title(&item(json!({"Type":"Episode","Name":"Ozymandias"}))),
            "Ozymandias"
        );
        assert_eq!(
            release_title(&item(json!({"Type":"Movie","Name":"Inception"}))),
            "Inception"
        );
    }

    #[test]
    fn episode_subtitles() {
        assert_eq!(
            episode_subtitle(&item(
                json!({"Type":"Episode","Name":"Ozymandias","SeriesName":"BB","ParentIndexNumber":5,"IndexNumber":14})
            )),
            Some("S05E14 \u{2013} Ozymandias".to_string())
        );
        assert_eq!(
            episode_subtitle(&item(
                json!({"Type":"Episode","Name":"Pilot","SeriesName":"BB"})
            )),
            Some("Pilot".to_string())
        );
        assert_eq!(
            episode_subtitle(&item(
                json!({"Type":"Episode","Name":"Pilot","Taglines":["A tagline"]})
            )),
            Some("A tagline".to_string())
        );
    }

    #[test]
    fn recent_episodes_collapse_into_their_series() {
        let ctx = IntegrationContext {
            id: "i".into(),
            name: "Jellyfin".into(),
            url: "http://jf".into(),
            external_url: None,
            kind: homarr_definitions::IntegrationKind::Jellyfin,
            secrets: HashMap::new(),
        };
        let items = vec![
            item(
                json!({"Id":"e1","Type":"Episode","SeriesId":"s1","DateCreated":"2024-05-02T10:00:00.0000000Z","Name":"A"}),
            ),
            item(
                json!({"Id":"e2","Type":"Episode","SeriesId":"s1","DateCreated":"2024-05-02T09:00:00.0000000Z","Name":"B"}),
            ),
            item(
                json!({"Id":"e3","Type":"Episode","SeriesId":"s2","DateCreated":"2024-05-02T10:00:00.0000000Z","Name":"C"}),
            ),
            item(
                json!({"Id":"e4","Type":"Episode","SeriesId":"s2","DateCreated":"2024-04-01T10:00:00.0000000Z","Name":"D"}),
            ),
            item(
                json!({"Id":"m1","Type":"Movie","Name":"Film","DateCreated":"2024-05-01T00:00:00Z"}),
            ),
        ];
        let recent = group_recent_episodes(&items);
        assert_eq!(recent["s1"].len(), 2);
        // e4 was added more than 24h before the newest episode of s2.
        assert_eq!(recent["s2"].len(), 1);
        let series = HashMap::from([(
            "s1".to_string(),
            item(
                json!({"Id":"s1","Type":"Series","Name":"Show","DateCreated":"2024-01-01T00:00:00Z","ServerId":"srv"}),
            ),
        )]);
        let collapsed = collapse_episodes(items, &recent, &series);
        let ids: Vec<_> = collapsed.iter().map(|i| i.id.clone().unwrap()).collect();
        assert_eq!(ids, ["s1", "e3", "m1"]);
        let releases = map_releases(&ctx, collapsed);
        assert_eq!(releases[0].title, "Show");
        assert_eq!(
            releases[0].href,
            "http://jf/web/index.html#!/details?id=s1&serverId=srv"
        );
    }
}
