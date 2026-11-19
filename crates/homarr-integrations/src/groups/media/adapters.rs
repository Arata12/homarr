//! Mapping of the capability types to the shape the current consumers (widgets / API) still use
//! (`crate::types`).

use chrono::{DateTime, Duration, Utc};
use homarr_common::{HomarrError, HomarrResult};

use super::seerr::media_type_str;
use crate::capabilities::calendar::CalendarEvent;
use crate::capabilities::indexer_manager::{Indexer, IndexerId};
use crate::capabilities::media_organizer::MissingMediaPage;
use crate::capabilities::media_requests::{MediaRequest, MediaRequestStatus, RequestStats};
use crate::capabilities::media_server::{PlaybackState, StreamSession};
use crate::capabilities::media_transcoding::{TdarrQueue, TdarrStatistics, TdarrWorker};
use crate::types::{
    CalendarEvent as LegacyCalendarEvent, IndexerInfo, MediaOrganizerItem, MediaRequestItem,
    MediaRequestStats, MediaServerSession, MediaTranscodingSummary,
};

/// Calendar range of the legacy API: optional ISO strings, defaulting to the last week / next month.
pub fn calendar_range(
    start: Option<&str>,
    end: Option<&str>,
) -> HomarrResult<(DateTime<Utc>, DateTime<Utc>)> {
    let parse = |value: &str| {
        DateTime::parse_from_rfc3339(value)
            .map(|date| date.with_timezone(&Utc))
            .map_err(|err| {
                HomarrError::BadRequest(format!("Invalid calendar date '{value}': {err}"))
            })
    };
    let now = Utc::now();
    let start = match start {
        Some(value) => parse(value)?,
        None => now - Duration::days(7),
    };
    let end = match end {
        Some(value) => parse(value)?,
        None => now + Duration::days(30),
    };
    Ok((start, end))
}

pub fn calendar_events(events: Vec<CalendarEvent>) -> Vec<LegacyCalendarEvent> {
    events
        .into_iter()
        .enumerate()
        .map(|(index, event)| LegacyCalendarEvent {
            id: format!("{}-{index}", event.start_date.timestamp()),
            title: event.title,
            sub_title: event.sub_title,
            description: event.description,
            start_date: event.start_date.to_rfc3339(),
            end_date: event.end_date.map(|date| date.to_rfc3339()),
            image_url: event.image.map(|image| image.src),
            link: event.links.into_iter().next().map(|link| link.href),
            indicator_color: event.indicator_color,
        })
        .collect()
}

pub fn indexers(indexers: Vec<Indexer>) -> Vec<IndexerInfo> {
    indexers
        .into_iter()
        .map(|indexer| IndexerInfo {
            id: match indexer.id {
                IndexerId::Number(id) => id.to_string(),
                IndexerId::Text(id) => id,
            },
            name: indexer.name,
            enabled: indexer.enabled,
            status: match indexer.status {
                Some(true) => "healthy",
                Some(false) => "unhealthy",
                None => "unknown",
            }
            .to_string(),
            protocol: "unknown".to_string(),
        })
        .collect()
}

pub fn media_missing(page: MissingMediaPage) -> Vec<MediaOrganizerItem> {
    page.items
        .into_iter()
        .map(|item| MediaOrganizerItem {
            id: item.id.to_string(),
            title: item.title,
            subtitle: item.series_title,
            release_date: None,
        })
        .collect()
}

pub fn media_sessions(sessions: Vec<StreamSession>) -> Vec<MediaServerSession> {
    sessions
        .into_iter()
        .filter_map(|session| {
            let playing = session.currently_playing?;
            let progress = match (playing.playback.position_ms, playing.playback.duration_ms) {
                (Some(position), Some(duration)) if duration > 0.0 => {
                    (position / duration * 100.0).clamp(0.0, 100.0)
                }
                _ => 0.0,
            };
            // The legacy shape names the episode as the item and the show as the series.
            let (item_name, series_name) = match playing.episode_name {
                Some(episode) => (episode, Some(playing.name)),
                None => (playing.name, None),
            };
            Some(MediaServerSession {
                id: session.session_id,
                username: session.user.username,
                item_name,
                series_name,
                client_name: session.session_name,
                is_paused: playing.playback.state == Some(PlaybackState::Paused),
                progress_percent: progress,
            })
        })
        .collect()
}

fn status_name(status: MediaRequestStatus) -> &'static str {
    match status {
        MediaRequestStatus::Pending => "pending",
        MediaRequestStatus::Approved => "approved",
        MediaRequestStatus::Declined => "declined",
        MediaRequestStatus::Failed => "failed",
        MediaRequestStatus::Completed => "completed",
    }
}

pub fn media_requests(
    requests: Vec<MediaRequest>,
    stats: RequestStats,
) -> (Vec<MediaRequestItem>, MediaRequestStats) {
    let items = requests
        .into_iter()
        .map(|request| MediaRequestItem {
            id: request.id.to_string(),
            title: request.name,
            media_type: media_type_str(request.media_type).to_string(),
            status: status_name(request.status).to_string(),
            requested_by: request
                .requested_by
                .map_or_else(|| "User".to_string(), |user| user.display_name),
            created_at: request.created_at.to_rfc3339(),
            poster_url: Some(request.poster_image_path),
        })
        .collect();
    let stats = MediaRequestStats {
        total: stats.total,
        pending: stats.pending,
        approved: stats.approved,
        available: stats.available,
        declined: stats.declined,
    };
    (items, stats)
}

pub fn transcoding_summary(
    statistics: &TdarrStatistics,
    workers: &[TdarrWorker],
    queue: &TdarrQueue,
) -> MediaTranscodingSummary {
    MediaTranscodingSummary {
        total_files: statistics.total_file_count,
        queue_count: queue.total_count,
        space_saved_bytes: statistics.total_saved_space.max(0.0) as u64,
        active_workers: workers.len() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::media_server::{CurrentlyPlaying, Playback, PlayingType, SessionUser};

    #[test]
    fn episode_sessions_use_the_episode_as_item_name() {
        let sessions = vec![
            StreamSession {
                session_id: "s1".into(),
                session_name: "Web (Firefox)".into(),
                user: SessionUser {
                    user_id: "u".into(),
                    username: "alice".into(),
                    profile_picture_url: None,
                },
                currently_playing: Some(CurrentlyPlaying {
                    playing_type: PlayingType::Video,
                    name: "Show".into(),
                    season_name: None,
                    season_number: None,
                    episode_name: Some("Pilot".into()),
                    episode_number: None,
                    album_name: None,
                    episode_count: None,
                    playback: Playback {
                        state: Some(PlaybackState::Paused),
                        position_ms: Some(50.0),
                        duration_ms: Some(200.0),
                    },
                    location: None,
                    metadata: None,
                }),
            },
            StreamSession {
                session_id: "idle".into(),
                session_name: "x".into(),
                user: SessionUser {
                    user_id: "u2".into(),
                    username: "bob".into(),
                    profile_picture_url: None,
                },
                currently_playing: None,
            },
        ];
        let legacy = media_sessions(sessions);
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].item_name, "Pilot");
        assert_eq!(legacy[0].series_name.as_deref(), Some("Show"));
        assert!(legacy[0].is_paused);
        assert_eq!(legacy[0].progress_percent, 25.0);
    }

    #[test]
    fn calendar_range_defaults_and_validation() {
        let (start, end) = calendar_range(None, None).unwrap();
        assert!(start < end);
        assert!(calendar_range(Some("not a date"), None).is_err());
    }
}
