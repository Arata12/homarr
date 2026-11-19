//! Navidrome (Subsonic API; media server sessions). Port of `navidrome/navidrome-integration.ts`.

use homarr_common::{HomarrError, HomarrResult};
use serde::{Deserialize, Serialize};

use super::util::{send_json, url};
use crate::capabilities::media_server::{
    CurrentlyPlaying, Playback, PlayingType, SessionUser, StreamSession,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const SUBSONIC_VERSION: &str = "1.16.1";
const SUBSONIC_CLIENT: &str = "homarr";
const ALBUM_LIST_PAGE_SIZE: usize = 500;
const MAX_ALBUM_LIST_PAGES: usize = 20;
const EMPTY_LIBRARY_MESSAGE: &str = "Library not found or empty";
const AUTH_ERROR_CODES: [i64; 2] = [40, 41];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavidromeDashboardData {
    pub artist_count: u64,
    pub album_count: u64,
    pub song_count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    Many(Vec<T>),
    One(T),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::Many(items) => items,
            Self::One(item) => vec![item],
        }
    }
}

#[derive(Debug, Deserialize)]
struct SubsonicError {
    code: i64,
    message: String,
}

#[derive(Debug, Default, Deserialize)]
struct NowPlayingEntry {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default, rename = "playerName")]
    player_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NowPlaying {
    #[serde(default)]
    entry: Option<OneOrMany<NowPlayingEntry>>,
}

#[derive(Debug, Deserialize)]
struct SubsonicArtistEntry {
    #[allow(dead_code)]
    id: String,
}

#[derive(Debug, Deserialize)]
struct SubsonicArtistIndex {
    #[serde(default)]
    artist: Option<OneOrMany<SubsonicArtistEntry>>,
}

#[derive(Debug, Deserialize)]
struct SubsonicArtistsContainer {
    #[serde(default)]
    index: Option<OneOrMany<SubsonicArtistIndex>>,
}

#[derive(Debug, Deserialize)]
struct SubsonicAlbumEntry {
    #[serde(default, rename = "songCount")]
    song_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SubsonicAlbumListContainer {
    #[serde(default)]
    album: Option<OneOrMany<SubsonicAlbumEntry>>,
}

#[derive(Debug, Deserialize)]
struct SubsonicBody {
    status: String,
    #[serde(default)]
    error: Option<SubsonicError>,
    #[serde(default, rename = "nowPlaying")]
    now_playing: Option<NowPlaying>,
    #[serde(default)]
    artists: Option<SubsonicArtistsContainer>,
    #[serde(default, rename = "albumList2")]
    album_list_2: Option<SubsonicAlbumListContainer>,
}

#[derive(Debug, Deserialize)]
struct SubsonicEnvelope {
    #[serde(rename = "subsonic-response")]
    response: SubsonicBody,
}

async fn subsonic_request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    extra: &[(&str, String)],
) -> HomarrResult<SubsonicBody> {
    let username = ctx.require_secret("username")?;
    let password = ctx.require_secret("password")?;
    let mut query = vec![
        ("u", Some(username.to_string())),
        ("p", Some(password.to_string())),
        ("v", Some(SUBSONIC_VERSION.to_string())),
        ("c", Some(SUBSONIC_CLIENT.to_string())),
        ("f", Some("json".to_string())),
    ];
    for (k, v) in extra {
        query.push((*k, Some(v.clone())));
    }
    let request = rt.http.standard().get(url(ctx, path, &query)?);
    let envelope: SubsonicEnvelope = send_json(ctx, "the Subsonic response", request).await?;
    Ok(envelope.response)
}

async fn subsonic_request_ok(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    extra: &[(&str, String)],
    tolerate_empty_library: bool,
) -> HomarrResult<SubsonicBody> {
    let body = subsonic_request(rt, ctx, path, extra).await?;
    if body.status == "failed" {
        let message = body.error.as_ref().map_or_else(
            || "Subsonic request failed".to_string(),
            |e| e.message.clone(),
        );
        if tolerate_empty_library && message == EMPTY_LIBRARY_MESSAGE {
            return Ok(body);
        }
        return Err(HomarrError::Upstream(message));
    }
    Ok(body)
}

pub async fn dashboard_data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<NavidromeDashboardData> {
    let artists_fut = async {
        let body = subsonic_request_ok(rt, ctx, "/rest/getArtists.view", &[], true).await?;
        let count: u64 = body
            .artists
            .and_then(|a| a.index)
            .map(OneOrMany::into_vec)
            .unwrap_or_default()
            .into_iter()
            .map(|idx| {
                idx.artist
                    .map(OneOrMany::into_vec)
                    .unwrap_or_default()
                    .len() as u64
            })
            .sum();
        HomarrResult::Ok(count)
    };
    let albums_fut = async {
        let mut offset = 0usize;
        let mut album_count = 0u64;
        let mut song_count = 0u64;
        for _ in 0..MAX_ALBUM_LIST_PAGES {
            let body = subsonic_request_ok(
                rt,
                ctx,
                "/rest/getAlbumList2.view",
                &[
                    ("type", "alphabeticalByName".to_string()),
                    ("size", ALBUM_LIST_PAGE_SIZE.to_string()),
                    ("offset", offset.to_string()),
                ],
                true,
            )
            .await?;
            let albums = body
                .album_list_2
                .and_then(|c| c.album)
                .map(OneOrMany::into_vec)
                .unwrap_or_default();
            if albums.is_empty() {
                break;
            }
            let page_len = albums.len();
            album_count += page_len as u64;
            for album in albums {
                song_count += album.song_count.unwrap_or(0);
            }
            if page_len < ALBUM_LIST_PAGE_SIZE {
                break;
            }
            offset += ALBUM_LIST_PAGE_SIZE;
        }
        HomarrResult::Ok((album_count, song_count))
    };
    let (artist_count, (album_count, song_count)) = tokio::try_join!(artists_fut, albums_fut)?;
    Ok(NavidromeDashboardData {
        artist_count,
        album_count,
        song_count,
    })
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let response = subsonic_request(rt, ctx, "/rest/ping.view", &[]).await?;
    if response.status == "failed" {
        return Err(match response.error {
            Some(error) if AUTH_ERROR_CODES.contains(&error.code) => HomarrError::Upstream(format!(
                "{} rejected the credentials (HTTP 401). Check the configured username and password",
                ctx.name
            )),
            Some(error) => HomarrError::Upstream(error.message),
            None => HomarrError::Upstream("Subsonic request failed".to_string()),
        });
    }
    Ok(())
}

fn map_now_playing(response: SubsonicBody) -> HomarrResult<Vec<StreamSession>> {
    if response.status == "failed" {
        let message = response
            .error
            .map_or_else(|| "Subsonic request failed".to_string(), |e| e.message);
        // An empty library is not an error.
        if message == EMPTY_LIBRARY_MESSAGE {
            return Ok(Vec::new());
        }
        return Err(HomarrError::Upstream(message));
    }
    let entries = response
        .now_playing
        .and_then(|now_playing| now_playing.entry)
        .map(OneOrMany::into_vec)
        .unwrap_or_default();
    Ok(entries
        .into_iter()
        .map(|entry| {
            let username = entry.username.unwrap_or_default();
            let player_name = entry.player_name.unwrap_or_default();
            StreamSession {
                session_id: format!("{username}-{player_name}"),
                session_name: player_name,
                user: SessionUser {
                    user_id: username.clone(),
                    username,
                    profile_picture_url: None,
                },
                currently_playing: Some(CurrentlyPlaying {
                    playing_type: PlayingType::Audio,
                    name: entry.title.unwrap_or_default(),
                    season_name: entry.artist.filter(|artist| !artist.is_empty()),
                    season_number: None,
                    episode_name: None,
                    episode_number: None,
                    album_name: Some(entry.album.unwrap_or_default()),
                    episode_count: None,
                    playback: Playback {
                        state: None,
                        position_ms: None,
                        duration_ms: None,
                    },
                    location: None,
                    metadata: None,
                }),
            }
        })
        .collect())
}

pub async fn sessions(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<StreamSession>> {
    map_now_playing(subsonic_request(rt, ctx, "/rest/getNowPlaying.view", &[]).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: &str) -> SubsonicBody {
        serde_json::from_str::<SubsonicEnvelope>(json)
            .unwrap()
            .response
    }

    #[test]
    fn a_single_now_playing_entry_is_not_wrapped_in_an_array() {
        let sessions = map_now_playing(body(
            r#"{"subsonic-response":{"status":"ok","nowPlaying":{"entry":
                {"title":"Song","artist":"Artist","album":"Album","username":"bob","playerName":"Web"}}}}"#,
        ))
        .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "bob-Web");
        let playing = sessions[0].currently_playing.as_ref().unwrap();
        assert_eq!(playing.season_name.as_deref(), Some("Artist"));
        assert_eq!(playing.album_name.as_deref(), Some("Album"));
    }

    #[test]
    fn empty_library_is_tolerated_but_other_failures_are_not() {
        let empty = body(
            r#"{"subsonic-response":{"status":"failed","error":{"code":70,"message":"Library not found or empty"}}}"#,
        );
        assert!(map_now_playing(empty).unwrap().is_empty());
        let failed = body(
            r#"{"subsonic-response":{"status":"failed","error":{"code":0,"message":"Boom"}}}"#,
        );
        assert!(map_now_playing(failed).is_err());
        assert!(
            map_now_playing(body(r#"{"subsonic-response":{"status":"ok"}}"#))
                .unwrap()
                .is_empty()
        );
    }
}
