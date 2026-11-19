//! Emby (media server + media releases). Port of `emby/emby-integration.ts`.

use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;

use super::emby_jellyfin::{
    map_release_type, map_sessions, parse_item_date, to_fixed_1, SessionInfo,
};
use super::util::{external_url, q, send, send_json, url};
use crate::capabilities::media_releases::{MediaRelease, MediaReleaseImageUrls};
use crate::capabilities::media_server::{CurrentSessionsInput, StreamSession};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const DEVICE_ID: &str = "homarr-emby-integration";
const AUTHORIZATION: &str = r#"Emby Client="Dashboard", Device="Homarr", DeviceId="homarr-emby-integration", Version="0.0.1""#;
const RELEASES_LIMIT: u32 = 100;

fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<reqwest::RequestBuilder> {
    let key = ctx.require_secret("apiKey")?;
    Ok(rt
        .http
        .standard()
        .get(url(ctx, path, query)?)
        .header("X-Emby-Token", key)
        .header("Authorization", AUTHORIZATION))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    send(ctx, get(rt, ctx, "/emby/System/Ping", &[])?).await?;
    Ok(())
}

pub async fn sessions(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    options: CurrentSessionsInput,
) -> HomarrResult<Vec<StreamSession>> {
    let sessions: Vec<SessionInfo> =
        send_json(ctx, "the sessions", get(rt, ctx, "/emby/Sessions", &[])?).await?;
    Ok(map_sessions(
        ctx,
        sessions,
        DEVICE_ID,
        options.show_only_playing,
    ))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Studio {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Item {
    id: String,
    server_id: String,
    name: String,
    taglines: Vec<String>,
    #[serde(default)]
    studios: Option<Vec<Studio>>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    premiere_date: Option<String>,
    date_created: String,
    genres: Vec<String>,
    #[serde(default)]
    community_rating: Option<f64>,
    #[serde(rename = "Type")]
    item_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct User {
    id: String,
}

fn map_releases(ctx: &IntegrationContext, items: Vec<Item>) -> Vec<MediaRelease> {
    items
        .into_iter()
        .filter_map(|item| {
            let release_date = item
                .premiere_date
                .as_deref()
                .and_then(parse_item_date)
                .or_else(|| parse_item_date(&item.date_created))?;
            Some(MediaRelease {
                media_type: map_release_type(Some(&item.item_type)),
                title: item.name,
                subtitle: item.taglines.first().cloned(),
                description: item.overview,
                release_date,
                release_date_is_date_only: None,
                image_urls: MediaReleaseImageUrls {
                    poster: Some(external_url(
                        ctx,
                        &format!(
                            "/Items/{}/Images/Primary?maxHeight=492&maxWidth=328&quality=90",
                            item.id
                        ),
                    )),
                    backdrop: Some(external_url(
                        ctx,
                        &format!(
                            "/Items/{}/Images/Backdrop/0?maxWidth=960&quality=70",
                            item.id
                        ),
                    )),
                },
                producer: item
                    .studios
                    .and_then(|studios| studios.into_iter().next())
                    .map(|studio| studio.name),
                price: None,
                rating: item.community_rating.map(to_fixed_1),
                tags: item.genres,
                href: external_url(
                    ctx,
                    &format!(
                        "/web/index.html#!/item?id={}&serverId={}",
                        item.id, item.server_id
                    ),
                ),
                length: None,
                id: item.id,
            })
        })
        .collect()
}

pub async fn media_releases(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<MediaRelease>> {
    // https://dev.emby.media/reference/RestAPI/UserService/getUsersPublic.html
    let users: Vec<User> = send_json(ctx, "the users", get(rt, ctx, "/Users/Public", &[])?).await?;
    let user_id = users
        .into_iter()
        .next()
        .map(|user| user.id)
        .ok_or_else(|| HomarrError::Upstream("No users found".to_string()))?;
    let query = [
        ("Limit", q(RELEASES_LIMIT)),
        (
            "Fields",
            q("CommunityRating,Studios,PremiereDate,Genres,ChildCount,ProductionYear,DateCreated,Overview,Taglines"),
        ),
    ];
    let items: Vec<Item> = send_json(
        ctx,
        "the latest media",
        get(rt, ctx, &format!("/Users/{user_id}/Items/Latest"), &query)?,
    )
    .await?;
    Ok(map_releases(ctx, items))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_latest_items() {
        let ctx = IntegrationContext {
            id: "i".into(),
            name: "Emby".into(),
            url: "http://emby:8096".into(),
            external_url: None,
            kind: IntegrationKind::Emby,
            secrets: HashMap::new(),
        };
        let items: Vec<Item> = serde_json::from_value(json!([{
            "Id": "42", "ServerId": "srv", "Name": "Movie", "Taglines": ["Tag"],
            "Studios": [{ "Name": "Studio" }], "Overview": "O",
            "PremiereDate": "2024-02-01T00:00:00.0000000Z", "DateCreated": "2024-03-01T00:00:00Z",
            "Genres": ["Drama"], "CommunityRating": 7.25, "Type": "Movie"
        }]))
        .unwrap();
        let releases = map_releases(&ctx, items);
        let release = &releases[0];
        assert_eq!(release.subtitle.as_deref(), Some("Tag"));
        assert_eq!(release.rating.as_deref(), Some("7.3"));
        assert_eq!(release.producer.as_deref(), Some("Studio"));
        assert_eq!(
            release.href,
            "http://emby:8096/web/index.html#!/item?id=42&serverId=srv"
        );
        assert_eq!(
            release.release_date.to_rfc3339(),
            "2024-02-01T00:00:00+00:00"
        );
    }
}
