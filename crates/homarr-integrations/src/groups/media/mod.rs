//! Media group: servarr family (Sonarr, Radarr, ...), indexer managers, media servers, media requests,
//! transcoding and Tracearr.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use homarr_common::{HomarrError, HomarrResult};
use homarr_definitions::IntegrationKind;
use serde_json::Value;

use super::{Handled, IntegrationGroup};
use crate::capabilities::calendar::CalendarEvent;
use crate::capabilities::indexer_manager::Indexer;
use crate::capabilities::media_organizer::{MissingMediaPage, QueuedMediaPage};
use crate::capabilities::media_releases::MediaRelease;
use crate::capabilities::media_requests::{
    MediaInformation, MediaRequest, MediaRequestMediaType, RequestStats, RequestUser,
};
use crate::capabilities::media_server::{CurrentSessionsInput, StreamSession};
use crate::capabilities::media_transcoding::{TdarrQueue, TdarrStatistics, TdarrWorker};
use crate::capabilities::search::SearchResult;
use crate::capabilities::service_version::ServiceVersion;
use crate::capabilities::stats::{StatsMetric, StatsUnit, StatsValue, StatsValues};
use crate::error::unsupported;
use crate::runtime::IntegrationRuntime;
use crate::types::{
    CalendarEvent as LegacyCalendarEvent, IndexerInfo, IntegrationContext, MediaOrganizerItem,
    MediaRequestItem, MediaRequestStats, MediaServerSession, MediaTranscodingSummary,
};

mod adapters;
mod bazarr;
mod emby;
mod emby_jellyfin;
mod jackett;
mod jellyfin;
mod navidrome;
mod plex;
mod prowlarr;
mod seerr;
mod servarr;
mod simple;
mod tdarr;
mod util;
mod xml;

use servarr::Flavor;

pub struct MediaGroup;

const KINDS: &[IntegrationKind] = &[
    IntegrationKind::Sonarr,
    IntegrationKind::Radarr,
    IntegrationKind::Lidarr,
    IntegrationKind::Readarr,
    IntegrationKind::Sportarr,
    IntegrationKind::Bindery,
    IntegrationKind::Prowlarr,
    IntegrationKind::Jackett,
    IntegrationKind::Bazarr,
    IntegrationKind::Jellyfin,
    IntegrationKind::Emby,
    IntegrationKind::Plex,
    IntegrationKind::Navidrome,
    IntegrationKind::Audiobookshelf,
    IntegrationKind::Immich,
    IntegrationKind::Jellyseerr,
    IntegrationKind::Seerr,
    IntegrationKind::Overseerr,
    IntegrationKind::Tdarr,
    IntegrationKind::Tracearr,
];

fn is_request_kind(kind: IntegrationKind) -> bool {
    matches!(
        kind,
        IntegrationKind::Overseerr | IntegrationKind::Jellyseerr | IntegrationKind::Seerr
    )
}

fn is_tdarr(ctx: &IntegrationContext) -> bool {
    ctx.kind == IntegrationKind::Tdarr
}

#[async_trait]
impl IntegrationGroup for MediaGroup {
    fn kinds(&self) -> &'static [IntegrationKind] {
        KINDS
    }

    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        use IntegrationKind as K;
        if let Some(flavor) = Flavor::of(ctx.kind) {
            return Some(servarr::test_connection(rt, ctx, flavor).await);
        }
        Some(match ctx.kind {
            K::Bazarr => bazarr::test_connection(rt, ctx).await,
            K::Jackett => jackett::test_connection(rt, ctx).await,
            K::Jellyfin => jellyfin::test_connection(rt, ctx).await,
            K::Emby => emby::test_connection(rt, ctx).await,
            K::Plex => plex::test_connection(rt, ctx).await,
            K::Navidrome => navidrome::test_connection(rt, ctx).await,
            K::Audiobookshelf | K::Immich | K::Tracearr => simple::test_connection(rt, ctx).await,
            K::Tdarr => tdarr::test_connection(rt, ctx).await,
            kind if is_request_kind(kind) => seerr::test_connection(rt, ctx).await,
            _ => return None,
        })
    }

    async fn service_version(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<ServiceVersion> {
        let flavor = Flavor::of(ctx.kind)?;
        Some(servarr::service_version(rt, ctx, flavor).await)
    }

    async fn calendar_events(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        include_unmonitored: bool,
    ) -> Handled<Vec<CalendarEvent>> {
        let flavor = Flavor::of(ctx.kind).filter(|flavor| flavor.supports_calendar())?;
        Some(servarr::calendar_events(rt, ctx, flavor, start, end, include_unmonitored).await)
    }

    async fn indexers(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<Indexer>> {
        match ctx.kind {
            IntegrationKind::Prowlarr => Some(prowlarr::indexers(rt, ctx).await),
            IntegrationKind::Jackett => Some(jackett::indexers(rt, ctx).await),
            _ => None,
        }
    }

    async fn indexers_test_all(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        match ctx.kind {
            IntegrationKind::Prowlarr => Some(prowlarr::test_all(rt, ctx).await),
            IntegrationKind::Jackett => Some(jackett::test_all(rt, ctx).await),
            _ => None,
        }
    }

    async fn media_missing(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        page_size: Option<usize>,
    ) -> Handled<MissingMediaPage> {
        let flavor = Flavor::of(ctx.kind).filter(|flavor| flavor.supports_media_organizer())?;
        Some(servarr::media_missing(rt, ctx, flavor, page_size).await)
    }

    async fn media_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        page_size: Option<usize>,
    ) -> Handled<QueuedMediaPage> {
        let flavor = Flavor::of(ctx.kind).filter(|flavor| flavor.supports_media_organizer())?;
        Some(servarr::media_queue(rt, ctx, flavor, page_size).await)
    }

    async fn media_releases(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaRelease>> {
        match ctx.kind {
            IntegrationKind::Jellyfin => Some(jellyfin::media_releases(rt, ctx).await),
            IntegrationKind::Emby => Some(emby::media_releases(rt, ctx).await),
            IntegrationKind::Plex => Some(plex::media_releases(rt, ctx).await),
            _ => None,
        }
    }

    async fn media_request_series_information(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        media_type: MediaRequestMediaType,
        id: i64,
    ) -> Handled<MediaInformation> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::series_information(rt, ctx, media_type, id).await)
    }

    async fn media_request_create(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        media_type: MediaRequestMediaType,
        id: i64,
        seasons: Option<&[i64]>,
    ) -> Handled<()> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::create_request(rt, ctx, media_type, id, seasons).await)
    }

    async fn media_requests(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaRequest>> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::requests(rt, ctx).await)
    }

    async fn media_request_stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<RequestStats> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::stats(rt, ctx).await)
    }

    async fn media_request_users(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<RequestUser>> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::users(rt, ctx).await)
    }

    async fn media_request_approve(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        request_id: i64,
    ) -> Handled<()> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::decide_request(rt, ctx, request_id, true).await)
    }

    async fn media_request_decline(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        request_id: i64,
    ) -> Handled<()> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::decide_request(rt, ctx, request_id, false).await)
    }

    async fn media_server_sessions(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        options: CurrentSessionsInput,
    ) -> Handled<Vec<StreamSession>> {
        match ctx.kind {
            IntegrationKind::Jellyfin => Some(jellyfin::sessions(rt, ctx, options).await),
            IntegrationKind::Emby => Some(emby::sessions(rt, ctx, options).await),
            IntegrationKind::Plex => Some(plex::sessions(rt, ctx, options).await),
            IntegrationKind::Navidrome => Some(navidrome::sessions(rt, ctx).await),
            _ => None,
        }
    }

    async fn transcoding_statistics(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<TdarrStatistics> {
        is_tdarr(ctx).then_some(())?;
        Some(tdarr::statistics(rt, ctx).await)
    }

    async fn transcoding_workers(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<TdarrWorker>> {
        is_tdarr(ctx).then_some(())?;
        Some(tdarr::workers(rt, ctx).await)
    }

    async fn transcoding_queue(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        first_item_index: usize,
        page_size: usize,
    ) -> Handled<TdarrQueue> {
        is_tdarr(ctx).then_some(())?;
        Some(tdarr::queue(rt, ctx, first_item_index, page_size).await)
    }

    async fn search(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        query: &str,
    ) -> Handled<Vec<SearchResult>> {
        is_request_kind(ctx.kind).then_some(())?;
        Some(seerr::search(rt, ctx, query).await)
    }

    async fn integration_data(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        operation: &str,
        _input: &Value,
    ) -> Handled<Value> {
        fn to_json(value: impl serde::Serialize) -> HomarrResult<Value> {
            serde_json::to_value(value)
                .map_err(|err| HomarrError::Internal(format!("Serialization error: {err}")))
        }
        match ctx.kind {
            IntegrationKind::Immich
            | IntegrationKind::Audiobookshelf
            | IntegrationKind::Bazarr
            | IntegrationKind::Tracearr
            | IntegrationKind::Navidrome => {}
            _ => return None,
        }
        Some(
            async {
                match (ctx.kind, operation) {
                    (IntegrationKind::Immich, "getServerStats") => {
                        to_json(simple::immich_stats(rt, ctx).await?)
                    }
                    (IntegrationKind::Audiobookshelf, "getDashboardData") => {
                        to_json(simple::audiobookshelf_stats(rt, ctx).await?)
                    }
                    (IntegrationKind::Bazarr, "getBadges") => {
                        to_json(bazarr::badges(rt, ctx).await?)
                    }
                    (IntegrationKind::Tracearr, "getStats") => {
                        to_json(simple::tracearr_stats(rt, ctx).await?)
                    }
                    (IntegrationKind::Navidrome, "getDashboardData") => {
                        to_json(navidrome::dashboard_data(rt, ctx).await?)
                    }
                    _ => Err(unsupported(ctx, operation)),
                }
            }
            .await,
        )
    }

    fn stats_metrics(&self, kind: IntegrationKind) -> Vec<StatsMetric> {
        use super::{count_metric, stat_metric};
        use IntegrationKind as K;
        match kind {
            K::Jackett => vec![count_metric("configured", "Configured indexers")],
            K::Prowlarr => vec![count_metric("indexers", "Indexers")],
            K::Plex | K::Jellyfin | K::Emby => {
                vec![count_metric("sessions", "Playback sessions")]
            }
            K::Jellyseerr | K::Seerr | K::Overseerr => vec![
                count_metric("pending", "Pending requests"),
                count_metric("approved", "Approved requests"),
                count_metric("available", "Available requests"),
                count_metric("processing", "Processing requests"),
            ],
            K::Tdarr => vec![
                count_metric("totalFileCount", "Files"),
                count_metric("totalTranscodeCount", "Transcodes"),
                count_metric("failedTranscodeCount", "Failed transcodes"),
            ],
            K::Bazarr => vec![
                count_metric("episodes", "Missing episode subtitles"),
                count_metric("movies", "Missing movie subtitles"),
                count_metric("providers", "Providers"),
            ],
            K::Immich => vec![
                count_metric("userCount", "Users"),
                count_metric("photoCount", "Photos"),
                count_metric("videoCount", "Videos"),
                stat_metric("totalLibraryUsageInBytes", "Storage", StatsUnit::Bytes),
            ],
            K::Audiobookshelf => vec![
                count_metric("libraryCount", "Libraries"),
                count_metric("totalAudiobooks", "Audiobooks"),
                count_metric("totalPodcasts", "Podcasts"),
                stat_metric(
                    "totalListeningTimeSeconds",
                    "Listening time",
                    StatsUnit::Seconds,
                ),
                count_metric("activeSessions", "Active sessions"),
            ],
            K::Navidrome => vec![
                count_metric("artistCount", "Artists"),
                count_metric("albumCount", "Albums"),
                count_metric("songCount", "Songs"),
            ],
            K::Tracearr => vec![
                count_metric("activeStreams", "Active streams"),
                count_metric("totalUsers", "Users"),
                count_metric("totalSessions", "Sessions"),
                count_metric("recentViolations", "Recent violations"),
            ],
            K::Sonarr => vec![
                count_metric("shows", "Shows"),
                count_metric("monitored", "Monitored"),
                count_metric("downloaded", "Downloaded episodes"),
                stat_metric("storage", "Storage", StatsUnit::Bytes),
                count_metric("missing", "Missing"),
                count_metric("queued", "Queued"),
                count_metric("episodes", "Episodes"),
            ],
            K::Sportarr => vec![
                count_metric("shows", "Leagues"),
                count_metric("monitored", "Monitored"),
                count_metric("downloaded", "Downloaded events"),
                stat_metric("storage", "Storage", StatsUnit::Bytes),
                count_metric("missing", "Missing"),
                count_metric("queued", "Queued"),
                count_metric("episodes", "Events"),
            ],
            K::Radarr => vec![
                count_metric("movies", "Movies"),
                count_metric("monitored", "Monitored"),
                count_metric("downloaded", "Downloaded movies"),
                stat_metric("storage", "Storage", StatsUnit::Bytes),
                count_metric("missing", "Missing"),
                count_metric("queued", "Queued"),
            ],
            K::Lidarr => vec![
                count_metric("artists", "Artists"),
                count_metric("monitored", "Monitored"),
                count_metric("upcoming", "Events in the next 7 days"),
            ],
            K::Readarr => vec![
                count_metric("books", "Books"),
                count_metric("monitored", "Monitored"),
                count_metric("upcoming", "Events in the next 7 days"),
            ],
            _ => Vec::new(),
        }
    }

    async fn stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsValues> {
        use IntegrationKind as K;
        Some(match ctx.kind {
            K::Jackett => jackett::indexers(rt, ctx).await.map(|list| {
                StatsValues::from([("configured".into(), StatsValue::Number(list.len() as f64))])
            }),
            K::Prowlarr => prowlarr::indexers(rt, ctx).await.map(|list| {
                StatsValues::from([("indexers".into(), StatsValue::Number(list.len() as f64))])
            }),
            K::Plex | K::Jellyfin | K::Emby => self
                .media_server_sessions(
                    rt,
                    ctx,
                    CurrentSessionsInput {
                        show_only_playing: false,
                    },
                )
                .await?
                .map(|sessions| {
                    StatsValues::from([(
                        "sessions".into(),
                        StatsValue::Number(sessions.len() as f64),
                    )])
                }),
            k if is_request_kind(k) => seerr::stats(rt, ctx).await.map(|s| {
                StatsValues::from([
                    ("pending".into(), StatsValue::Number(s.pending as f64)),
                    ("approved".into(), StatsValue::Number(s.approved as f64)),
                    ("available".into(), StatsValue::Number(s.available as f64)),
                    ("processing".into(), StatsValue::Number(s.processing as f64)),
                ])
            }),
            K::Tdarr => tdarr::statistics(rt, ctx).await.map(|s| {
                StatsValues::from([
                    (
                        "totalFileCount".into(),
                        StatsValue::Number(s.total_file_count as f64),
                    ),
                    (
                        "totalTranscodeCount".into(),
                        StatsValue::Number(s.total_transcode_count as f64),
                    ),
                    (
                        "failedTranscodeCount".into(),
                        StatsValue::Number(s.failed_transcode_count as f64),
                    ),
                ])
            }),
            K::Bazarr => bazarr::badges(rt, ctx).await.map(|b| {
                StatsValues::from([
                    ("episodes".into(), StatsValue::Number(b.episodes as f64)),
                    ("movies".into(), StatsValue::Number(b.movies as f64)),
                    ("providers".into(), StatsValue::Number(b.providers as f64)),
                ])
            }),
            K::Immich => simple::immich_stats(rt, ctx).await.map(|s| {
                StatsValues::from([
                    ("userCount".into(), StatsValue::Number(s.user_count as f64)),
                    (
                        "photoCount".into(),
                        StatsValue::Number(s.photo_count as f64),
                    ),
                    (
                        "videoCount".into(),
                        StatsValue::Number(s.video_count as f64),
                    ),
                    (
                        "totalLibraryUsageInBytes".into(),
                        StatsValue::Number(s.total_library_usage_in_bytes as f64),
                    ),
                ])
            }),
            K::Audiobookshelf => simple::audiobookshelf_stats(rt, ctx).await.map(|d| {
                StatsValues::from([
                    (
                        "libraryCount".into(),
                        StatsValue::Number(d.library_count as f64),
                    ),
                    (
                        "totalAudiobooks".into(),
                        StatsValue::Number(d.total_audiobooks as f64),
                    ),
                    (
                        "totalPodcasts".into(),
                        StatsValue::Number(d.total_podcasts as f64),
                    ),
                    (
                        "totalListeningTimeSeconds".into(),
                        StatsValue::Number(d.total_listening_time_seconds),
                    ),
                    (
                        "activeSessions".into(),
                        StatsValue::Number(d.active_sessions as f64),
                    ),
                ])
            }),
            K::Navidrome => navidrome::dashboard_data(rt, ctx).await.map(|d| {
                StatsValues::from([
                    (
                        "artistCount".into(),
                        StatsValue::Number(d.artist_count as f64),
                    ),
                    (
                        "albumCount".into(),
                        StatsValue::Number(d.album_count as f64),
                    ),
                    ("songCount".into(), StatsValue::Number(d.song_count as f64)),
                ])
            }),
            K::Tracearr => simple::tracearr_stats(rt, ctx).await.map(|s| {
                StatsValues::from([
                    (
                        "activeStreams".into(),
                        StatsValue::Number(s.active_streams as f64),
                    ),
                    (
                        "totalUsers".into(),
                        StatsValue::Number(s.total_users as f64),
                    ),
                    (
                        "totalSessions".into(),
                        StatsValue::Number(s.total_sessions as f64),
                    ),
                    (
                        "recentViolations".into(),
                        StatsValue::Number(s.recent_violations as f64),
                    ),
                ])
            }),
            K::Sonarr | K::Sportarr | K::Radarr => {
                async {
                    let flavor = Flavor::of(ctx.kind).expect("checked kind");
                    let (library, missing, queued) = tokio::try_join!(
                        servarr::library_stats(rt, ctx, flavor),
                        servarr::media_missing(rt, ctx, flavor, Some(1)),
                        servarr::media_queue(rt, ctx, flavor, Some(1)),
                    )?;
                    Ok(match library {
                        servarr::LibraryStats::Series {
                            shows,
                            monitored,
                            episodes,
                            downloaded,
                            storage,
                        } => StatsValues::from([
                            ("shows".into(), StatsValue::Number(shows as f64)),
                            ("monitored".into(), StatsValue::Number(monitored as f64)),
                            ("episodes".into(), StatsValue::Number(episodes as f64)),
                            ("downloaded".into(), StatsValue::Number(downloaded as f64)),
                            ("storage".into(), StatsValue::Number(storage as f64)),
                            (
                                "missing".into(),
                                StatsValue::Number(missing.total_count as f64),
                            ),
                            (
                                "queued".into(),
                                StatsValue::Number(queued.total_count as f64),
                            ),
                        ]),
                        servarr::LibraryStats::Movies {
                            movies,
                            monitored,
                            downloaded,
                            storage,
                        } => StatsValues::from([
                            ("movies".into(), StatsValue::Number(movies as f64)),
                            ("monitored".into(), StatsValue::Number(monitored as f64)),
                            ("downloaded".into(), StatsValue::Number(downloaded as f64)),
                            ("storage".into(), StatsValue::Number(storage as f64)),
                            (
                                "missing".into(),
                                StatsValue::Number(missing.total_count as f64),
                            ),
                            (
                                "queued".into(),
                                StatsValue::Number(queued.total_count as f64),
                            ),
                        ]),
                        servarr::LibraryStats::ArtistsOrBooks { .. } => unreachable!(),
                    })
                }
                .await
            }
            K::Lidarr | K::Readarr => {
                async {
                    let flavor = Flavor::of(ctx.kind).expect("checked kind");
                    let now = Utc::now();
                    let (library, events) = tokio::try_join!(
                        servarr::library_stats(rt, ctx, flavor),
                        servarr::calendar_events(
                            rt,
                            ctx,
                            flavor,
                            now,
                            now + Duration::days(7),
                            true
                        ),
                    )?;
                    let (total, monitored) = match library {
                        servarr::LibraryStats::ArtistsOrBooks { total, monitored } => {
                            (total, monitored)
                        }
                        _ => unreachable!(),
                    };
                    let count_key = if ctx.kind == K::Lidarr {
                        "artists"
                    } else {
                        "books"
                    };
                    Ok(StatsValues::from([
                        (count_key.into(), StatsValue::Number(total as f64)),
                        ("monitored".into(), StatsValue::Number(monitored as f64)),
                        ("upcoming".into(), StatsValue::Number(events.len() as f64)),
                    ]))
                }
                .await
            }
            _ => return None,
        })
    }

    // ---- adapters for the current consumers ----------------------------------------------------

    async fn legacy_calendar_events(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        start: Option<&str>,
        end: Option<&str>,
    ) -> Handled<Vec<LegacyCalendarEvent>> {
        Flavor::of(ctx.kind).filter(|flavor| flavor.supports_calendar())?;
        let (start, end) = match adapters::calendar_range(start, end) {
            Ok(range) => range,
            Err(err) => return Some(Err(err)),
        };
        let events = self.calendar_events(rt, ctx, start, end, true).await?;
        Some(events.map(adapters::calendar_events))
    }

    async fn legacy_indexers(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<IndexerInfo>> {
        Some(self.indexers(rt, ctx).await?.map(adapters::indexers))
    }

    async fn legacy_media_missing(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaOrganizerItem>> {
        Some(
            self.media_missing(rt, ctx, Some(20))
                .await?
                .map(adapters::media_missing),
        )
    }

    async fn legacy_service_version(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Option<String>> {
        Some(
            self.service_version(rt, ctx)
                .await?
                .map(|service| Some(service.version)),
        )
    }

    async fn legacy_media_sessions(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<MediaServerSession>> {
        let options = CurrentSessionsInput {
            show_only_playing: true,
        };
        Some(
            self.media_server_sessions(rt, ctx, options)
                .await?
                .map(adapters::media_sessions),
        )
    }

    async fn legacy_media_requests(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<(Vec<MediaRequestItem>, MediaRequestStats)> {
        is_request_kind(ctx.kind).then_some(())?;
        let result = async {
            let (requests, stats) =
                futures_util::future::join(seerr::requests(rt, ctx), seerr::stats(rt, ctx)).await;
            Ok(adapters::media_requests(requests?, stats?))
        };
        Some(result.await)
    }

    async fn legacy_media_request_action(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        request_id: &str,
        approve: bool,
    ) -> Handled<()> {
        is_request_kind(ctx.kind).then_some(())?;
        let Ok(request_id) = request_id.parse::<i64>() else {
            return Some(Err(homarr_common::HomarrError::BadRequest(format!(
                "Invalid request id '{request_id}'"
            ))));
        };
        Some(seerr::decide_request(rt, ctx, request_id, approve).await)
    }

    async fn legacy_media_transcoding(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<MediaTranscodingSummary> {
        is_tdarr(ctx).then_some(())?;
        let result = async {
            let (statistics, workers, queue) = futures_util::future::join3(
                tdarr::statistics(rt, ctx),
                tdarr::workers(rt, ctx),
                tdarr::queue(rt, ctx, 0, 1),
            )
            .await;
            Ok(adapters::transcoding_summary(
                &statistics?,
                &workers?,
                &queue?,
            ))
        };
        Some(result.await)
    }
}
