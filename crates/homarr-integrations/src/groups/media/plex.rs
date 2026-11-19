//! Plex (media server + media releases). Port of `plex/plex-integration.ts`.
//!
//! Sessions come from the XML API, everything else from the JSON API.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use homarr_common::HomarrResult;
use serde::Deserialize;

use super::util::{external_url, send, send_json, send_text, url};
use super::xml::{self, Node};
use crate::capabilities::media_releases::{MediaRelease, MediaReleaseImageUrls, MediaType};
use crate::capabilities::media_server::{
    AudioMetadata, CurrentSessionsInput, CurrentlyPlaying, Playback, PlaybackLocation,
    PlaybackState, PlayingType, Resolution, SessionMetadata, SessionUser, StreamSession,
    TranscodingMetadata, TranscodingTarget, VideoMetadata,
};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

// Same limit as the Jellyfin and Emby integrations
const MEDIA_RELEASES_LIMIT: usize = 100;
// Default page size of /library/recentlyAdded, enough to cover the newest episode of every recently updated season
const EPISODES_PAGE_SIZE: usize = 50;
// A stalled Plex request must not block the merge, the fallback or the widget
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<reqwest::RequestBuilder> {
    let token = ctx.require_secret("apiKey")?;
    Ok(rt
        .http
        .standard()
        .get(url(ctx, path, &[])?)
        .header("X-Plex-Token", token))
}

fn get_json(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<reqwest::RequestBuilder> {
    Ok(get(rt, ctx, path)?
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT))
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let request = get(rt, ctx, "/prefs")?.header("Accept", "application/json");
    send(ctx, request).await?;
    Ok(())
}

// ---- sessions --------------------------------------------------------------------------------

fn parse_optional_number(value: Option<&str>) -> Option<f64> {
    value
        .filter(|value| !value.is_empty())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| !value.is_nan())
}

fn parse_location(value: Option<&str>) -> Option<PlaybackLocation> {
    match value {
        Some("lan") => Some(PlaybackLocation::Lan),
        Some("wan") => Some(PlaybackLocation::Wan),
        _ => None,
    }
}

fn parse_playback_state(value: Option<&str>) -> Option<PlaybackState> {
    match value {
        Some("playing") => Some(PlaybackState::Playing),
        Some("paused") => Some(PlaybackState::Paused),
        Some("buffering") => Some(PlaybackState::Buffering),
        _ => None,
    }
}

fn parse_resolution(width: Option<&str>, height: Option<&str>) -> Option<Resolution> {
    Some(Resolution {
        width: parse_optional_number(width)? as u32,
        height: parse_optional_number(height)? as u32,
    })
}

fn playing_type(kind: Option<&str>) -> PlayingType {
    match kind {
        Some("movie") => PlayingType::Movie,
        Some("track") => PlayingType::Audio,
        _ => PlayingType::Video,
    }
}

/// Plex reports "transcode" when a stream is re-encoded. "copy" and an absent decision both count as direct.
fn is_stream_direct(decision: Option<&str>) -> bool {
    decision != Some("transcode")
}

fn map_session(element: &Node) -> Option<StreamSession> {
    let user = element.child("User");
    let player = element.child("Player")?;
    let session = element.child("Session");
    let transcode = element.child("TranscodeSession");
    let media = element.child("Media");

    let streams: Vec<&Node> = media
        .and_then(|media| media.child("Part"))
        .map(|part| part.children_named("Stream").collect())
        .unwrap_or_default();
    let video_stream = streams
        .iter()
        .find(|stream| stream.attr("streamType") == Some("1"));
    let audio_stream = streams
        .iter()
        .find(|stream| stream.attr("streamType") == Some("2"));

    let is_episode = element.attr("type") == Some("episode");
    let container_changed = match (
        transcode.and_then(|node| node.attr("container")),
        media.and_then(|node| node.attr("container")),
    ) {
        (Some(transcoded), Some(source)) => !transcoded.is_empty() && transcoded != source,
        _ => false,
    };

    Some(StreamSession {
        session_id: session
            .and_then(|node| node.attr("id"))
            .unwrap_or("unknown")
            .to_string(),
        session_name: format!(
            "{} ({})",
            player.attr("product").unwrap_or("undefined"),
            player.attr("title").unwrap_or("undefined")
        ),
        user: SessionUser {
            user_id: user
                .and_then(|node| node.attr("id"))
                .unwrap_or("Anonymous")
                .to_string(),
            username: user
                .and_then(|node| node.attr("title"))
                .unwrap_or("Anonymous")
                .to_string(),
            profile_picture_url: user.and_then(|node| node.attr("thumb")).map(str::to_string),
        },
        currently_playing: Some(CurrentlyPlaying {
            playing_type: if element.attr("live") == Some("1") {
                PlayingType::Tv
            } else {
                playing_type(element.attr("type"))
            },
            name: element
                .attr("grandparentTitle")
                .or_else(|| element.attr("title"))
                .unwrap_or("Unknown")
                .to_string(),
            season_name: element.attr("parentTitle").map(str::to_string),
            season_number: is_episode
                .then(|| parse_optional_number(element.attr("parentIndex")))
                .flatten()
                .map(|n| n as i64),
            episode_name: is_episode
                .then(|| element.attr("title").map(str::to_string))
                .flatten(),
            episode_number: is_episode
                .then(|| parse_optional_number(element.attr("index")))
                .flatten()
                .map(|n| n as i64),
            album_name: (element.attr("type") == Some("track"))
                .then(|| element.attr("parentTitle").map(str::to_string))
                .flatten(),
            episode_count: parse_optional_number(element.attr("index")).map(|n| n as i64),
            playback: Playback {
                state: parse_playback_state(player.attr("state")),
                position_ms: parse_optional_number(element.attr("viewOffset")),
                duration_ms: parse_optional_number(element.attr("duration")),
            },
            location: parse_location(session.and_then(|node| node.attr("location"))),
            metadata: Some(SessionMetadata {
                video: VideoMetadata {
                    resolution: parse_resolution(
                        video_stream.and_then(|s| s.attr("width")),
                        video_stream.and_then(|s| s.attr("height")),
                    ),
                    frame_rate: parse_optional_number(
                        video_stream.and_then(|s| s.attr("frameRate")),
                    ),
                },
                audio: AudioMetadata {
                    channel_count: parse_optional_number(
                        audio_stream.and_then(|s| s.attr("channels")),
                    )
                    .map(|n| n as u32),
                    codec: audio_stream
                        .and_then(|s| s.attr("codec"))
                        .or_else(|| media.and_then(|m| m.attr("audioCodec")))
                        .map(str::to_string),
                },
                transcoding: TranscodingMetadata {
                    container: transcode
                        .and_then(|node| node.attr("container"))
                        .map(str::to_string),
                    resolution: parse_resolution(
                        transcode.and_then(|n| n.attr("width")),
                        transcode.and_then(|n| n.attr("height")),
                    ),
                    target: TranscodingTarget {
                        audio_codec: transcode
                            .and_then(|n| n.attr("audioCodec"))
                            .map(str::to_string),
                        video_codec: transcode
                            .and_then(|n| n.attr("videoCodec"))
                            .map(str::to_string),
                    },
                    is_video_direct: is_stream_direct(
                        transcode.and_then(|n| n.attr("videoDecision")),
                    ),
                    is_audio_direct: is_stream_direct(
                        transcode.and_then(|n| n.attr("audioDecision")),
                    ),
                    container_changed,
                },
                bitrate_kbps: parse_optional_number(session.and_then(|n| n.attr("bandwidth")))
                    .or_else(|| parse_optional_number(media.and_then(|n| n.attr("bitrate")))),
            }),
        }),
    })
}

fn map_sessions(root: &Node) -> Vec<StreamSession> {
    // Videos first, then tracks.
    root.children_named("Video")
        .chain(root.children_named("Track"))
        .filter_map(map_session)
        .collect()
}

pub async fn sessions(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    _options: CurrentSessionsInput,
) -> HomarrResult<Vec<StreamSession>> {
    // There is no JSON API for the sessions, convert the XML response.
    let body = send_text(ctx, get(rt, ctx, "/status/sessions")?).await?;
    let root = xml::parse(&body).map_err(|err| parse_error(ctx, "the sessions", err))?;
    if root.name != "MediaContainer" {
        return Err(parse_error(
            ctx,
            "the sessions",
            format!("unexpected root element <{}>", root.name),
        ));
    }
    Ok(map_sessions(&root))
}

// ---- media releases --------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct MediaEntry {
    id: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ImageEntry {
    #[serde(rename = "type")]
    image_type: String,
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Genre {
    tag: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentlyAddedItem {
    key: String,
    #[serde(default)]
    parent_key: Option<String>,
    #[serde(default)]
    studio: Option<String>,
    #[serde(rename = "type")]
    item_type: String,
    title: String,
    #[serde(default)]
    parent_title: Option<String>,
    #[serde(default)]
    grandparent_title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    added_at: i64,
    #[serde(default)]
    rating: Option<f64>,
    #[serde(default)]
    tagline: Option<String>,
    #[serde(default)]
    originally_available_at: Option<String>,
    #[serde(default, rename = "Media")]
    media: Option<Vec<MediaEntry>>,
    /// Entries can be `null` in the Plex response.
    #[serde(default, rename = "Image")]
    image: Option<Vec<Option<ImageEntry>>>,
    #[serde(default, rename = "Genre")]
    genre: Option<Vec<Genre>>,
    /// The newest episode that put this item at its position (set by the merge).
    #[serde(skip)]
    latest_episode: Option<RecentlyAddedEpisode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentlyAddedEpisode {
    key: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    index: Option<i64>,
    #[serde(default)]
    parent_key: Option<String>,
    #[serde(default)]
    parent_index: Option<i64>,
    #[serde(default)]
    parent_title: Option<String>,
    #[serde(default)]
    parent_thumb: Option<String>,
    #[serde(default)]
    thumb: Option<String>,
    #[serde(default)]
    grandparent_key: Option<String>,
    #[serde(default)]
    grandparent_title: Option<String>,
    #[serde(default)]
    grandparent_thumb: Option<String>,
    #[serde(default)]
    grandparent_art: Option<String>,
    #[serde(default)]
    art: Option<String>,
    added_at: i64,
    #[serde(default)]
    originally_available_at: Option<String>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(rename = "MediaContainer")]
    media_container: T,
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct MetadataContainer<T> {
    #[serde(default, rename = "Metadata")]
    metadata: Option<Vec<T>>,
}

#[derive(Deserialize)]
struct Section {
    key: String,
    #[serde(rename = "type")]
    section_type: String,
}

#[derive(Deserialize)]
struct SectionsContainer {
    #[serde(default, rename = "Directory")]
    directory: Option<Vec<Section>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityContainer {
    machine_identifier: String,
}

fn season_key_of(item: &RecentlyAddedItem) -> Option<String> {
    // Seasons use "/library/metadata/{ratingKey}/children" as key, episodes reference their season through
    // parentKey "/library/metadata/{ratingKey}".
    match item.item_type.as_str() {
        "season" => Some(item.key.trim_end_matches("/children").to_string()),
        "episode" => item.parent_key.clone(),
        _ => None,
    }
}

fn format_episode_subtitle(episode: &RecentlyAddedEpisode) -> Option<String> {
    let (season, number) = (episode.parent_index?, episode.index?);
    let code = format!("S{season:02}E{number:02}");
    Some(
        match episode.title.as_deref().filter(|title| !title.is_empty()) {
            Some(title) => format!("{code} \u{2013} {title}"),
            None => code,
        },
    )
}

fn episode_images(episode: &RecentlyAddedEpisode) -> Option<Vec<Option<ImageEntry>>> {
    let mut images = Vec::new();
    let poster = episode
        .grandparent_thumb
        .as_ref()
        .or(episode.parent_thumb.as_ref())
        .or(episode.thumb.as_ref());
    let backdrop = episode.art.as_ref().or(episode.grandparent_art.as_ref());
    if let Some(url) = poster {
        images.push(Some(ImageEntry {
            image_type: "coverPoster".into(),
            url: url.clone(),
        }));
    }
    if let Some(url) = backdrop {
        images.push(Some(ImageEntry {
            image_type: "background".into(),
            url: url.clone(),
        }));
    }
    (!images.is_empty()).then_some(images)
}

/// `/library/recentlyAdded` lists seasons with the `addedAt` of the season itself, so episodes added to an
/// existing season never surface. Merges the most recently added episode of every season into the list.
fn merge_recently_added_episodes(
    items: Vec<RecentlyAddedItem>,
    episodes: &[RecentlyAddedEpisode],
) -> Vec<RecentlyAddedItem> {
    if episodes.is_empty() {
        return items;
    }
    // Episodes are sorted by addedAt descending, so the first one per season or show is the newest.
    let mut newest_by_season: Vec<(String, &RecentlyAddedEpisode)> = Vec::new();
    let mut newest_by_show: HashMap<String, &RecentlyAddedEpisode> = HashMap::new();
    for episode in episodes {
        if let Some(parent_key) = &episode.parent_key {
            if !newest_by_season.iter().any(|(key, _)| key == parent_key) {
                newest_by_season.push((parent_key.clone(), episode));
            }
        }
        if let Some(grandparent_key) = &episode.grandparent_key {
            newest_by_show
                .entry(grandparent_key.clone())
                .or_insert(episode);
        }
    }

    let listed_keys: HashSet<String> = items
        .iter()
        .filter_map(|item| {
            if item.item_type == "show" {
                Some(item.key.clone())
            } else {
                season_key_of(item)
            }
        })
        .collect();

    let mut merged: Vec<RecentlyAddedItem> = items
        .into_iter()
        .map(|item| {
            let season_key = season_key_of(&item);
            let episode = if item.item_type == "show" {
                newest_by_show.get(&item.key).copied()
            } else {
                season_key.as_ref().and_then(|key| {
                    newest_by_season
                        .iter()
                        .find(|(season, _)| season == key)
                        .map(|(_, episode)| *episode)
                })
            };
            let Some(episode) = episode else {
                return item;
            };
            if item.item_type != "show" && episode.added_at <= item.added_at {
                return item;
            }
            if item.item_type == "episode" {
                return RecentlyAddedItem {
                    key: episode.key.clone(),
                    media: None,
                    title: episode.title.clone().unwrap_or(item.title.clone()),
                    parent_title: episode.parent_title.clone().or(item.parent_title.clone()),
                    grandparent_title: episode
                        .grandparent_title
                        .clone()
                        .or(item.grandparent_title.clone()),
                    image: episode_images(episode).or(item.image.clone()),
                    summary: episode.summary.clone(),
                    duration: episode.duration,
                    added_at: episode.added_at,
                    originally_available_at: episode.originally_available_at.clone(),
                    latest_episode: Some(episode.clone()),
                    ..item
                };
            }
            RecentlyAddedItem {
                added_at: item.added_at.max(episode.added_at),
                originally_available_at: episode
                    .originally_available_at
                    .clone()
                    .or(item.originally_available_at.clone()),
                latest_episode: Some(episode.clone()),
                ..item
            }
        })
        .collect();

    let missing_seasons = newest_by_season
        .iter()
        .filter(|(season_key, episode)| {
            !listed_keys.contains(season_key)
                && !listed_keys.contains(episode.grandparent_key.as_deref().unwrap_or(""))
        })
        .map(|(season_key, episode)| RecentlyAddedItem {
            key: format!("{season_key}/children"),
            parent_key: None,
            studio: None,
            item_type: "season".into(),
            title: episode
                .parent_title
                .clone()
                .unwrap_or_else(|| match episode.parent_index {
                    Some(index) => format!("Season {index}"),
                    None => "Season".to_string(),
                }),
            parent_title: episode.grandparent_title.clone(),
            grandparent_title: None,
            summary: None,
            duration: None,
            added_at: episode.added_at,
            rating: None,
            tagline: None,
            originally_available_at: episode.originally_available_at.clone(),
            media: None,
            image: episode_images(episode),
            genre: None,
            latest_episode: Some((*episode).clone()),
        });
    merged.extend(missing_seasons);
    merged.sort_by_key(|item| std::cmp::Reverse(item.added_at));
    merged
}

fn map_type(kind: &str) -> MediaType {
    match kind {
        "movie" => MediaType::Movie,
        "show" | "season" | "episode" => MediaType::Tv,
        "album" | "artist" | "track" => MediaType::Music,
        _ => MediaType::Unknown,
    }
}

fn is_date_only(value: &str) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() && value.len() == 10
}

fn parse_release_date(item: &RecentlyAddedItem) -> Option<DateTime<Utc>> {
    let from_added_at = || DateTime::from_timestamp(item.added_at, 0);
    let Some(air_date) = item
        .originally_available_at
        .as_deref()
        .filter(|d| !d.is_empty())
    else {
        return from_added_at();
    };
    if let Ok(date) = DateTime::parse_from_rfc3339(air_date) {
        return Some(date.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(air_date, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc())
        .or_else(from_added_at)
}

/// JavaScript's `encodeURIComponent`.
fn encode_uri_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

struct RawReleaseImages {
    poster: Option<String>,
    backdrop: Option<String>,
}

fn find_image_url(item: &RecentlyAddedItem, kind: &str) -> Option<String> {
    item.image
        .as_ref()?
        .iter()
        .flatten()
        .find(|entry| entry.image_type == kind && !entry.url.trim().is_empty())
        .map(|entry| entry.url.clone())
}

fn map_releases(
    ctx: &IntegrationContext,
    machine_identifier: &str,
    items: Vec<RecentlyAddedItem>,
) -> Vec<MediaRelease> {
    map_releases_with_images(ctx, machine_identifier, items).0
}

fn map_releases_with_images(
    ctx: &IntegrationContext,
    machine_identifier: &str,
    items: Vec<RecentlyAddedItem>,
) -> (Vec<MediaRelease>, Vec<RawReleaseImages>) {
    let mut releases = Vec::new();
    let mut raw_images = Vec::new();
    for item in items
        .into_iter()
        .filter(|item| item.image.is_some())
        .take(MEDIA_RELEASES_LIMIT)
    {
        let Some(release_date) = parse_release_date(&item) else {
            continue;
        };
        let mut latest_subtitle = None;
        if let Some(latest) = &item.latest_episode {
            latest_subtitle = format_episode_subtitle(latest);
            if latest_subtitle.is_none() && item.item_type == "show" {
                latest_subtitle = latest.title.clone();
            }
        }
        let title = match item.item_type.as_str() {
            "episode" => item
                .grandparent_title
                .clone()
                .unwrap_or_else(|| item.title.clone()),
            "season" => item
                .parent_title
                .clone()
                .unwrap_or_else(|| item.title.clone()),
            _ => item.title.clone(),
        };
        let subtitle = latest_subtitle.or_else(|| {
            if title == item.title {
                item.tagline.clone()
            } else {
                Some(item.title.clone())
            }
        });
        let raw = RawReleaseImages {
            poster: find_image_url(&item, "coverPoster"),
            backdrop: find_image_url(&item, "background"),
        };
        releases.push(MediaRelease {
            id: item
                .media
                .as_ref()
                .and_then(|media| media.first())
                .map_or_else(|| item.key.clone(), |media| media.id.to_string()),
            media_type: map_type(&item.item_type),
            title,
            subtitle,
            description: item.summary.clone(),
            release_date,
            release_date_is_date_only: Some(
                item.originally_available_at
                    .as_deref()
                    .is_some_and(is_date_only),
            ),
            image_urls: MediaReleaseImageUrls {
                poster: None,
                backdrop: None,
            },
            producer: item.studio.clone(),
            price: None,
            rating: item.rating.map(super::emby_jellyfin::to_fixed_1),
            tags: item
                .genre
                .as_ref()
                .map(|genres| genres.iter().map(|genre| genre.tag.clone()).collect())
                .unwrap_or_default(),
            href: external_url(
                ctx,
                &format!(
                    "/web/index.html#!/server/{machine_identifier}/details?key={}",
                    encode_uri_component(&item.key.replace("/children", ""))
                ),
            ),
            length: item.duration.map(|duration| (duration / 1000.0).round()),
        });
        raw_images.push(raw);
    }
    (releases, raw_images)
}

async fn proxy_plex_artwork(
    proxy: &dyn crate::runtime::ImageProxyProvider,
    base: &url::Url,
    plex_origin: &str,
    token: Option<&str>,
    raw_url: &str,
) -> Option<String> {
    let resolved = base.join(raw_url.trim()).ok()?;
    if !matches!(resolved.scheme(), "http" | "https") {
        return None;
    }
    let mut headers = HashMap::new();
    let trusted_origin = if resolved.origin().ascii_serialization() == plex_origin {
        if let Some(token) = token {
            headers.insert("X-Plex-Token".to_string(), token.to_string());
        }
        Some(plex_origin.to_string())
    } else {
        None
    };
    proxy
        .proxy_image(resolved.as_str(), headers, trusted_origin)
        .await
        .ok()
}

async fn proxy_release_images(
    proxy: &dyn crate::runtime::ImageProxyProvider,
    ctx: &IntegrationContext,
    releases: &mut [MediaRelease],
    raw_images: &[RawReleaseImages],
) {
    let Ok(base) = url::Url::parse(ctx.url.trim()) else {
        return;
    };
    let plex_origin = base.origin().ascii_serialization();
    let token = ctx.secret("apiKey");
    let futures = raw_images.iter().map(|raw| async {
        let poster_fut = async {
            match raw.poster.as_deref() {
                Some(path) => proxy_plex_artwork(proxy, &base, &plex_origin, token, path).await,
                None => None,
            }
        };
        let backdrop_fut = async {
            match raw.backdrop.as_deref() {
                Some(path) => proxy_plex_artwork(proxy, &base, &plex_origin, token, path).await,
                None => None,
            }
        };
        futures_util::future::join(poster_fut, backdrop_fut).await
    });
    let resolved = futures_util::future::join_all(futures).await;
    for (release, (poster, backdrop)) in releases.iter_mut().zip(resolved) {
        release.image_urls.poster = poster;
        release.image_urls.backdrop = backdrop;
    }
}

async fn episodes_of_show_sections(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> Vec<RecentlyAddedEpisode> {
    let sections = async {
        let sections: Envelope<SectionsContainer> = send_json(
            ctx,
            "the libraries",
            get_json(rt, ctx, "/library/sections")?,
        )
        .await?;
        HomarrResult::Ok(
            sections
                .media_container
                .directory
                .unwrap_or_default()
                .into_iter()
                .filter(|section| section.section_type == "show")
                .collect::<Vec<_>>(),
        )
    }
    .await;
    let sections = match sections {
        Ok(sections) => sections,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to list Plex libraries, media releases fall back to seasons only");
            return Vec::new();
        }
    };

    let lookups = sections.iter().map(|section| async move {
        let path = format!(
            "/library/sections/{}/all?type=4&sort=addedAt:desc&X-Plex-Container-Start=0&X-Plex-Container-Size={EPISODES_PAGE_SIZE}",
            encode_uri_component(&section.key)
        );
        let data: Envelope<MetadataContainer<RecentlyAddedEpisode>> =
            send_json(ctx, "the recently added episodes", get_json(rt, ctx, &path)?).await?;
        HomarrResult::Ok(data.media_container.metadata.unwrap_or_default())
    });
    let mut episodes = Vec::new();
    for (section, result) in sections
        .iter()
        .zip(futures_util::future::join_all(lookups).await)
    {
        match result {
            Ok(mut list) => episodes.append(&mut list),
            // A library that fails or times out is skipped so the remaining libraries still contribute.
            Err(err) => tracing::warn!(
                section = %section.key,
                error = %err,
                "Failed to fetch recently added episodes of Plex library"
            ),
        }
    }
    episodes.sort_by_key(|episode| std::cmp::Reverse(episode.added_at));
    episodes
}

pub async fn media_releases(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<MediaRelease>> {
    // The episode requests are an addition to /library/recentlyAdded and run alongside it.
    let identity = async {
        let data: Envelope<IdentityContainer> =
            send_json(ctx, "the server identity", get_json(rt, ctx, "/identity")?).await?;
        HomarrResult::Ok(data.media_container.machine_identifier)
    };
    let recently_added = async {
        let data: Envelope<MetadataContainer<RecentlyAddedItem>> = send_json(
            ctx,
            "the recently added items",
            get_json(rt, ctx, "/library/recentlyAdded")?,
        )
        .await?;
        HomarrResult::Ok(data.media_container.metadata.unwrap_or_default())
    };
    let (machine_identifier, items, episodes) =
        futures_util::future::join3(identity, recently_added, episodes_of_show_sections(rt, ctx))
            .await;
    let (machine_identifier, items) = (machine_identifier?, items?);
    let merged = merge_recently_added_episodes(items, &episodes);
    let (mut releases, raw_images) = map_releases_with_images(ctx, &machine_identifier, merged);
    if let Some(proxy) = &rt.image_proxy {
        proxy_release_images(proxy.as_ref(), ctx, &mut releases, &raw_images).await;
    }
    Ok(releases)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;
    use serde_json::json;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "i".into(),
            name: "Plex".into(),
            url: "http://plex:32400".into(),
            external_url: Some("https://plex.example.com".into()),
            kind: IntegrationKind::Plex,
            secrets: HashMap::new(),
        }
    }

    const SESSIONS_XML: &str = r#"<MediaContainer size="2">
      <Video type="episode" title="Pilot" grandparentTitle="Show" parentTitle="Season 1" parentIndex="1" index="1"
             viewOffset="60000" duration="3600000">
        <Media container="mkv" bitrate="8000" audioCodec="aac">
          <Part><Stream streamType="1" width="1920" height="1080" frameRate="23.976"/><Stream streamType="2" channels="6" codec="eac3"/></Part>
        </Media>
        <User id="1" title="alice" thumb="https://plex.tv/a.png"/>
        <Player product="Plex Web" title="Firefox" state="paused"/>
        <Session id="sess1" bandwidth="4000" location="wan"/>
        <TranscodeSession videoDecision="transcode" audioDecision="copy" container="mp4" width="1280" height="720" videoCodec="h264" audioCodec="aac"/>
      </Video>
      <Video type="movie" title="Orphan"><Media/></Video>
      <Track type="track" title="Song" parentTitle="Album">
        <User id="2" title="bob"/><Player product="Plexamp" title="Phone" state="playing"/><Session id="sess2" location="lan"/>
      </Track>
    </MediaContainer>"#;

    #[test]
    fn maps_xml_sessions() {
        let root = xml::parse(SESSIONS_XML).unwrap();
        let sessions = map_sessions(&root);
        // The movie without a <Player> is dropped.
        assert_eq!(sessions.len(), 2);
        let first = &sessions[0];
        assert_eq!(first.session_id, "sess1");
        assert_eq!(first.session_name, "Plex Web (Firefox)");
        let playing = first.currently_playing.as_ref().unwrap();
        assert_eq!(playing.playing_type, PlayingType::Video);
        assert_eq!(playing.name, "Show");
        assert_eq!(playing.episode_name.as_deref(), Some("Pilot"));
        assert_eq!(playing.season_number, Some(1));
        assert_eq!(playing.playback.state, Some(PlaybackState::Paused));
        assert_eq!(playing.location, Some(PlaybackLocation::Wan));
        let metadata = playing.metadata.as_ref().unwrap();
        assert_eq!(metadata.bitrate_kbps, Some(4000.0));
        assert_eq!(metadata.audio.channel_count, Some(6));
        assert_eq!(metadata.audio.codec.as_deref(), Some("eac3"));
        assert!(!metadata.transcoding.is_video_direct);
        assert!(metadata.transcoding.is_audio_direct);
        assert!(metadata.transcoding.container_changed);

        let track = sessions[1].currently_playing.as_ref().unwrap();
        assert_eq!(track.playing_type, PlayingType::Audio);
        assert_eq!(track.album_name.as_deref(), Some("Album"));
        assert_eq!(sessions[1].user.profile_picture_url, None);
    }

    #[test]
    fn newest_episode_bumps_its_season_and_creates_missing_ones() {
        let items: Vec<RecentlyAddedItem> = serde_json::from_value(json!([
            { "key": "/library/metadata/10/children", "type": "season", "title": "Season 1",
              "parentTitle": "Show A", "addedAt": 100,
              "Image": [{ "type": "coverPoster", "url": "/p" }] }
        ]))
        .unwrap();
        let episodes: Vec<RecentlyAddedEpisode> = serde_json::from_value(json!([
            { "key": "/library/metadata/11", "title": "Ep 3", "index": 3, "parentIndex": 1,
              "parentKey": "/library/metadata/10", "grandparentKey": "/library/metadata/1",
              "grandparentTitle": "Show A", "addedAt": 500, "thumb": "/t" },
            { "key": "/library/metadata/21", "title": "Pilot", "index": 1, "parentIndex": 2,
              "parentTitle": "Season 2", "parentKey": "/library/metadata/20",
              "grandparentKey": "/library/metadata/2", "grandparentTitle": "Show B",
              "addedAt": 300, "grandparentThumb": "/gt" }
        ]))
        .unwrap();
        let merged = merge_recently_added_episodes(items, &episodes);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].added_at, 500);
        assert_eq!(merged[1].key, "/library/metadata/20/children");

        let releases = map_releases(&ctx(), "machine", merged);
        assert_eq!(releases[0].title, "Show A");
        assert_eq!(
            releases[0].subtitle.as_deref(),
            Some("S01E03 \u{2013} Ep 3")
        );
        assert_eq!(
            releases[0].href,
            "https://plex.example.com/web/index.html#!/server/machine/details?key=%2Flibrary%2Fmetadata%2F10"
        );
        assert_eq!(releases[1].title, "Show B");
        assert_eq!(releases[1].media_type, MediaType::Tv);
    }

    #[test]
    fn date_only_release_dates_are_flagged() {
        let items: Vec<RecentlyAddedItem> = serde_json::from_value(json!([
            { "key": "/library/metadata/1", "type": "movie", "title": "Film", "addedAt": 1,
              "originallyAvailableAt": "2024-05-01", "rating": 7.85, "duration": 5400000,
              "Image": [], "Genre": [{ "tag": "Drama" }], "tagline": "Tag" }
        ]))
        .unwrap();
        let releases = map_releases(&ctx(), "m", items);
        assert_eq!(releases[0].release_date_is_date_only, Some(true));
        assert_eq!(releases[0].rating.as_deref(), Some("7.9"));
        assert_eq!(releases[0].length, Some(5400.0));
        assert_eq!(releases[0].subtitle.as_deref(), Some("Tag"));
        assert_eq!(releases[0].tags, vec!["Drama".to_string()]);
    }
}
