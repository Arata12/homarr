use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub user_id: String,
    pub username: String,
    pub profile_picture_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayingType {
    Audio,
    Video,
    Tv,
    Movie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackState {
    Playing,
    Paused,
    Buffering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackLocation {
    Lan,
    Wan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playback {
    pub state: Option<PlaybackState>,
    pub position_ms: Option<f64>,
    pub duration_ms: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub resolution: Option<Resolution>,
    pub frame_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMetadata {
    pub channel_count: Option<u32>,
    pub codec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodingTarget {
    pub audio_codec: Option<String>,
    pub video_codec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodingMetadata {
    pub container: Option<String>,
    pub resolution: Option<Resolution>,
    pub target: TranscodingTarget,
    pub is_video_direct: bool,
    pub is_audio_direct: bool,
    pub container_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    pub video: VideoMetadata,
    pub audio: AudioMetadata,
    pub transcoding: TranscodingMetadata,
    pub bitrate_kbps: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentlyPlaying {
    #[serde(rename = "type")]
    pub playing_type: PlayingType,
    pub name: String,
    pub season_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_number: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_count: Option<i64>,
    pub playback: Playback,
    pub location: Option<PlaybackLocation>,
    pub metadata: Option<SessionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSession {
    pub session_id: String,
    pub session_name: String,
    pub user: SessionUser,
    pub currently_playing: Option<CurrentlyPlaying>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentSessionsInput {
    pub show_only_playing: bool,
}
