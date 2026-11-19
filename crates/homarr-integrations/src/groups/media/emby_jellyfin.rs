//! Shared parts of the Jellyfin and Emby integrations: session mapping, location classification and the
//! media release helpers. Port of `jellyfin/jellyfin-integration.ts` and `emby/emby-integration.ts`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::util::external_url;
use crate::capabilities::media_releases::MediaType;
use crate::capabilities::media_server::{
    AudioMetadata, CurrentlyPlaying, Playback, PlaybackLocation, PlaybackState, PlayingType,
    Resolution, SessionMetadata, SessionUser, StreamSession, TranscodingMetadata,
    TranscodingTarget, VideoMetadata,
};
use crate::types::IntegrationContext;

/// JavaScript's `toFixed(1)` (ties round up).
pub fn to_fixed_1(value: f64) -> String {
    format!("{:.1}", (value * 10.0).round() / 10.0)
}

pub fn ticks_to_ms(ticks: Option<f64>) -> Option<f64> {
    ticks.map(|ticks| (ticks / 10_000.0).round())
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

fn is_private_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

/// Strips the port (and brackets) from a `RemoteEndPoint`.
pub fn extract_host(remote_end_point: &str) -> &str {
    if let Some(rest) = remote_end_point.strip_prefix('[') {
        return match rest.find(']') {
            Some(end) => &rest[..end],
            None => remote_end_point,
        };
    }
    // A bare IPv6 address (no brackets, no port) has more than one colon.
    if remote_end_point.split(':').count() == 2 {
        return &remote_end_point[..remote_end_point
            .rfind(':')
            .unwrap_or(remote_end_point.len())];
    }
    remote_end_point
}

/// Jellyfin and Emby only report the client's RemoteEndPoint, so lan/wan is classified here.
pub fn parse_location(remote_end_point: Option<&str>) -> Option<PlaybackLocation> {
    let host = extract_host(remote_end_point.filter(|value| !value.is_empty())?);
    let ip: IpAddr = host.parse().ok()?;
    let private = match ip {
        IpAddr::V4(ip) => is_private_v4(ip),
        // A dual-stack server can report an IPv4 client as an IPv4-mapped IPv6 address.
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => is_private_v4(mapped),
            None => is_private_v6(ip),
        },
    };
    Some(if private {
        PlaybackLocation::Lan
    } else {
        PlaybackLocation::Wan
    })
}

pub fn convert_type(kind: Option<&str>) -> PlayingType {
    match kind {
        Some("Audio" | "MusicVideo") => PlayingType::Audio,
        Some("Episode" | "Video") => PlayingType::Video,
        Some("Movie") => PlayingType::Movie,
        _ => PlayingType::Tv,
    }
}

pub fn map_release_type(kind: Option<&str>) -> MediaType {
    match kind {
        Some("Audio" | "AudioBook" | "MusicAlbum") => MediaType::Music,
        Some("Book") => MediaType::Book,
        Some("Episode" | "Series" | "Season") => MediaType::Tv,
        Some("Movie") => MediaType::Movie,
        Some("Video") => MediaType::Video,
        _ => MediaType::Unknown,
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaStream {
    #[serde(default)]
    pub bit_rate: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSource {
    #[serde(default)]
    pub bitrate: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NowPlayingItem {
    #[serde(default, rename = "Type")]
    pub item_type: Option<String>,
    #[serde(default)]
    pub series_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub season_name: Option<String>,
    #[serde(default)]
    pub parent_index_number: Option<i64>,
    #[serde(default)]
    pub episode_title: Option<String>,
    #[serde(default)]
    pub index_number: Option<i64>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub episode_count: Option<i64>,
    #[serde(default)]
    pub run_time_ticks: Option<f64>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub media_streams: Option<Vec<MediaStream>>,
    #[serde(default)]
    pub media_sources: Option<Vec<MediaSource>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayState {
    #[serde(default)]
    pub is_paused: Option<bool>,
    #[serde(default)]
    pub position_ticks: Option<f64>,
    #[serde(default)]
    pub play_method: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TranscodingInfo {
    #[serde(default)]
    pub bitrate: Option<f64>,
    #[serde(default)]
    pub framerate: Option<f64>,
    #[serde(default)]
    pub audio_channels: Option<f64>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub is_video_direct: Option<bool>,
    #[serde(default)]
    pub is_audio_direct: Option<bool>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SessionInfo {
    #[serde(default)]
    pub now_playing_item: Option<NowPlayingItem>,
    pub id: String,
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub remote_end_point: Option<String>,
    #[serde(default)]
    pub play_state: Option<PlayState>,
    #[serde(default)]
    pub transcoding_info: Option<TranscodingInfo>,
}

fn resolution(width: Option<f64>, height: Option<f64>) -> Option<Resolution> {
    match (width, height) {
        (Some(width), Some(height)) if width > 0.0 && height > 0.0 => Some(Resolution {
            width: width as u32,
            height: height as u32,
        }),
        _ => None,
    }
}

fn map_playing(session: &SessionInfo, item: &NowPlayingItem) -> CurrentlyPlaying {
    let transcoding = session.transcoding_info.as_ref();
    let is_episode = item.item_type.as_deref() == Some("Episode");

    // The server only reliably fills in MediaSources[].Bitrate for a transcoding session, so fall back
    // to the sum of the media streams' own bitrates for direct play.
    let streams_bitrate = item
        .media_streams
        .as_ref()
        .map(|streams| {
            streams
                .iter()
                .map(|s| s.bit_rate.unwrap_or(0.0))
                .sum::<f64>()
        })
        .filter(|sum| *sum != 0.0);
    let bitrate_bps = transcoding
        .and_then(|info| info.bitrate)
        .or_else(|| {
            item.media_sources
                .as_ref()
                .and_then(|sources| sources.first())
                .and_then(|source| source.bitrate)
        })
        .or(streams_bitrate);

    CurrentlyPlaying {
        playing_type: convert_type(item.item_type.as_deref()),
        name: item
            .series_name
            .clone()
            .or_else(|| item.name.clone())
            .unwrap_or_default(),
        season_name: Some(item.season_name.clone().unwrap_or_default()),
        season_number: is_episode.then_some(item.parent_index_number).flatten(),
        episode_name: if is_episode {
            item.episode_title.clone()
        } else {
            None
        },
        episode_number: is_episode.then_some(item.index_number).flatten(),
        album_name: Some(item.album.clone().unwrap_or_default()),
        episode_count: item.episode_count,
        playback: Playback {
            state: Some(
                if session.play_state.as_ref().and_then(|s| s.is_paused) == Some(true) {
                    PlaybackState::Paused
                } else {
                    PlaybackState::Playing
                },
            ),
            position_ms: ticks_to_ms(session.play_state.as_ref().and_then(|s| s.position_ticks)),
            duration_ms: ticks_to_ms(item.run_time_ticks),
        },
        location: parse_location(session.remote_end_point.as_deref()),
        metadata: Some(SessionMetadata {
            video: VideoMetadata {
                resolution: resolution(item.width, item.height),
                frame_rate: transcoding.and_then(|info| info.framerate),
            },
            audio: AudioMetadata {
                channel_count: transcoding
                    .and_then(|info| info.audio_channels)
                    .map(|count| count as u32),
                codec: transcoding.and_then(|info| info.audio_codec.clone()),
            },
            transcoding: TranscodingMetadata {
                resolution: transcoding.and_then(|info| resolution(info.width, info.height)),
                target: TranscodingTarget {
                    audio_codec: transcoding.and_then(|info| info.audio_codec.clone()),
                    video_codec: transcoding.and_then(|info| info.video_codec.clone()),
                },
                container: transcoding.and_then(|info| info.container.clone()),
                is_video_direct: transcoding
                    .and_then(|info| info.is_video_direct)
                    .unwrap_or(true),
                is_audio_direct: transcoding
                    .and_then(|info| info.is_audio_direct)
                    .unwrap_or(true),
                container_changed: session
                    .play_state
                    .as_ref()
                    .and_then(|s| s.play_method.as_deref())
                    == Some("DirectStream"),
            },
            bitrate_kbps: bitrate_bps.map(|bps| (bps / 1000.0).round()),
        }),
    }
}

/// Maps `/Sessions` to stream sessions. `own_device_id` is Homarr's own device (never listed).
pub fn map_sessions(
    ctx: &IntegrationContext,
    sessions: Vec<SessionInfo>,
    own_device_id: &str,
    show_only_playing: bool,
) -> Vec<StreamSession> {
    sessions
        .into_iter()
        .filter(|session| session.user_id.is_some())
        .filter(|session| session.device_id.as_deref() != Some(own_device_id))
        .filter(|session| !show_only_playing || session.now_playing_item.is_some())
        .map(|session| {
            let user_id = session.user_id.clone().unwrap_or_default();
            StreamSession {
                session_id: session.id.clone(),
                session_name: format!(
                    "{} ({})",
                    session.client.as_deref().unwrap_or("Unknown"),
                    session.device_name.as_deref().unwrap_or("Unknown")
                ),
                user: SessionUser {
                    profile_picture_url: Some(external_url(
                        ctx,
                        &format!("/Users/{user_id}/Images/Primary"),
                    )),
                    user_id,
                    username: session.user_name.clone().unwrap_or_default(),
                },
                currently_playing: session
                    .now_playing_item
                    .as_ref()
                    .map(|item| map_playing(&session, item)),
            }
        })
        .collect()
}

/// Release date of an item; the server reports ISO strings with up to 7 fractional digits.
pub fn parse_item_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
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
            name: "Jellyfin".into(),
            url: "http://jf:8096".into(),
            external_url: Some("https://jf.example.com".into()),
            kind: IntegrationKind::Jellyfin,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn extracts_hosts() {
        assert_eq!(extract_host("192.168.1.100:54321"), "192.168.1.100");
        assert_eq!(extract_host("8.8.8.8"), "8.8.8.8");
        assert_eq!(extract_host("[::1]:8096"), "::1");
        assert_eq!(extract_host("::1"), "::1");
    }

    #[test]
    fn classifies_locations() {
        use PlaybackLocation::{Lan, Wan};
        assert_eq!(parse_location(None), None);
        assert_eq!(parse_location(Some("")), None);
        assert_eq!(parse_location(Some("not-an-ip:1234")), None);
        for lan in [
            "192.168.1.100:54321",
            "10.0.0.5:1234",
            "172.16.5.5:1234",
            "127.0.0.1:8096",
            "[::1]:8096",
            "[fd12:3456:789a::1]:8096",
            "[::ffff:192.168.1.5]:8096",
            "169.254.1.5:8096",
            "[fe80::1]:8096",
        ] {
            assert_eq!(parse_location(Some(lan)), Some(Lan), "{lan}");
        }
        for wan in [
            "8.8.8.8:1234",
            "[2606:4700:4700::1111]:8096",
            "[::ffff:8.8.8.8]:8096",
        ] {
            assert_eq!(parse_location(Some(wan)), Some(Wan), "{wan}");
        }
    }

    #[test]
    fn maps_an_episode_session() {
        let sessions: Vec<SessionInfo> = serde_json::from_value(json!([
            {
                "Id": "s1", "Client": "Web", "DeviceName": "Firefox", "DeviceId": "dev",
                "UserId": "u1", "UserName": "alice", "RemoteEndPoint": "8.8.8.8",
                "PlayState": { "IsPaused": true, "PositionTicks": 600000000.0, "PlayMethod": "DirectStream" },
                "NowPlayingItem": {
                    "Type": "Episode", "SeriesName": "Show", "Name": "Pilot", "EpisodeTitle": "Pilot",
                    "ParentIndexNumber": 2, "IndexNumber": 4, "RunTimeTicks": 36000000000.0,
                    "Width": 1920, "Height": 1080,
                    "MediaStreams": [{ "BitRate": 4000000 }, { "BitRate": 128000 }]
                }
            },
            { "Id": "own", "DeviceId": "homarr", "UserId": "u2" },
            { "Id": "idle", "DeviceId": "x", "UserId": "u3" },
            { "Id": "nouser", "DeviceId": "y" }
        ]))
        .unwrap();
        let mapped = map_sessions(&ctx(), sessions.clone(), "homarr", true);
        assert_eq!(mapped.len(), 1);
        let session = &mapped[0];
        assert_eq!(session.session_name, "Web (Firefox)");
        assert_eq!(
            session.user.profile_picture_url.as_deref(),
            Some("https://jf.example.com/Users/u1/Images/Primary")
        );
        let playing = session.currently_playing.as_ref().unwrap();
        assert_eq!(playing.name, "Show");
        assert_eq!(playing.season_number, Some(2));
        assert_eq!(playing.episode_number, Some(4));
        assert_eq!(playing.playback.state, Some(PlaybackState::Paused));
        assert_eq!(playing.playback.position_ms, Some(60_000.0));
        assert_eq!(playing.playback.duration_ms, Some(3_600_000.0));
        assert_eq!(playing.location, Some(PlaybackLocation::Wan));
        let metadata = playing.metadata.as_ref().unwrap();
        assert_eq!(metadata.bitrate_kbps, Some(4128.0));
        assert_eq!(
            metadata.video.resolution,
            Some(Resolution {
                width: 1920,
                height: 1080
            })
        );
        assert!(metadata.transcoding.container_changed);
        assert!(metadata.transcoding.is_video_direct);

        // Without the playing filter idle sessions are listed too (own device and users without id are not).
        assert_eq!(map_sessions(&ctx(), sessions, "homarr", false).len(), 2);
    }

    #[test]
    fn parses_server_dates_with_seven_fraction_digits() {
        assert!(parse_item_date("2024-05-01T10:20:30.1234567Z").is_some());
    }
}
