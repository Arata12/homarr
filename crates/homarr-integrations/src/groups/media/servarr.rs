//! Servarr family: Sonarr, Sportarr, Radarr, Lidarr, Readarr, Bindery and Prowlarr's shared parts
//! (calendar, missing / queue, service version, test connection).
//!
//! Port of `media-organizer/*`, `base/servarr-service-version.ts`.

use chrono::{DateTime, NaiveDate, Utc};
use homarr_common::HomarrResult;
use homarr_definitions::IntegrationKind;
use reqwest::RequestBuilder;
use serde::Deserialize;

use super::util::{external_url, iso, q, send, send_json, url};
use crate::capabilities::calendar::{
    CalendarEvent, CalendarImage, CalendarImageAspectRatio, CalendarImageBadge, CalendarLink,
    CalendarMetadata, RadarrReleaseType,
};
use crate::capabilities::media_organizer::{
    MediaOrganizerItemType, MissingMediaItem, MissingMediaPage, QueuedMediaItem, QueuedMediaPage,
};
use crate::capabilities::service_version::ServiceVersion;
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Sonarr,
    Sportarr,
    Radarr,
    Lidarr,
    Readarr,
    Bindery,
    Prowlarr,
}

impl Flavor {
    pub fn of(kind: IntegrationKind) -> Option<Self> {
        Some(match kind {
            IntegrationKind::Sonarr => Self::Sonarr,
            IntegrationKind::Sportarr => Self::Sportarr,
            IntegrationKind::Radarr => Self::Radarr,
            IntegrationKind::Lidarr => Self::Lidarr,
            IntegrationKind::Readarr => Self::Readarr,
            IntegrationKind::Bindery => Self::Bindery,
            IntegrationKind::Prowlarr => Self::Prowlarr,
            _ => return None,
        })
    }

    fn is_sonarr_like(self) -> bool {
        matches!(self, Self::Sonarr | Self::Sportarr)
    }

    fn api_version(self) -> &'static str {
        match self {
            Self::Sonarr | Self::Sportarr | Self::Radarr => "v3",
            Self::Lidarr | Self::Readarr | Self::Bindery | Self::Prowlarr => "v1",
        }
    }

    pub fn supports_calendar(self) -> bool {
        matches!(
            self,
            Self::Sonarr | Self::Sportarr | Self::Radarr | Self::Lidarr | Self::Readarr
        )
    }

    pub fn supports_media_organizer(self) -> bool {
        matches!(
            self,
            Self::Sonarr | Self::Sportarr | Self::Radarr | Self::Bindery
        )
    }
}

/// Priority list that determines the quality of images using their order (earlier is better).
const MEDIA_ORGANIZER_PRIORITIES: [&str; 10] = [
    "cover",
    "poster",
    "banner",
    "disc",
    "logo",
    "fanart",
    "screenshot",
    "clearlogo",
    "headshot",
    "unknown",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Image {
    cover_type: String,
    #[serde(default)]
    remote_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

fn priority(image: &Image) -> i32 {
    // Like `indexOf`: unknown types sort first (-1).
    MEDIA_ORGANIZER_PRIORITIES
        .iter()
        .position(|name| *name == image.cover_type)
        .map_or(-1, |index| index as i32)
}

/// Stable sort by priority; the first image is the best one.
fn best_image(images: impl IntoIterator<Item = Image>) -> Option<Image> {
    let mut images: Vec<Image> = images.into_iter().collect();
    images.sort_by_key(priority);
    images.into_iter().next()
}

fn poster_url(images: &[Image]) -> Option<String> {
    images
        .iter()
        .find(|image| image.cover_type == "poster")
        .and_then(|image| image.remote_url.clone())
}

fn parse_date(ctx: &IntegrationContext, value: &str) -> HomarrResult<DateTime<Utc>> {
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Ok(date.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| date.and_utc())
        .ok_or_else(|| parse_error(ctx, "a date", format!("invalid date '{value}'")))
}

fn api_get(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
    query: &[(&str, Option<String>)],
) -> HomarrResult<RequestBuilder> {
    let key = ctx.require_secret("apiKey")?;
    Ok(rt
        .http
        .standard()
        .get(url(ctx, path, query)?)
        .header("X-Api-Key", key))
}

// ---- test connection & version ---------------------------------------------------------------

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    flavor: Flavor,
) -> HomarrResult<()> {
    // Bindery has no `/api` index, its status endpoint doubles as the connection test.
    let path = if flavor == Flavor::Bindery {
        "/api/v1/system/status"
    } else {
        "/api"
    };
    let _: serde_json::Value =
        send_json(ctx, "the API index", api_get(rt, ctx, path, &[])?).await?;
    Ok(())
}

#[derive(Deserialize)]
struct SystemStatus {
    version: String,
}

pub async fn service_version(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    flavor: Flavor,
) -> HomarrResult<ServiceVersion> {
    let path = format!("/api/{}/system/status", flavor.api_version());
    let status: SystemStatus =
        send_json(ctx, "the system status", api_get(rt, ctx, &path, &[])?).await?;
    Ok(ServiceVersion {
        version: status.version,
    })
}

// ---- calendar --------------------------------------------------------------------------------

pub async fn calendar_events(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    flavor: Flavor,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    include_unmonitored: bool,
) -> HomarrResult<Vec<CalendarEvent>> {
    let base_query = |extra: &[(&'static str, Option<String>)]| {
        let mut query = vec![
            ("start", q(iso(start))),
            ("end", q(iso(end))),
            ("unmonitored", q(include_unmonitored)),
        ];
        query.extend(extra.iter().cloned());
        query
    };
    let path = format!("/api/{}/calendar", flavor.api_version());
    match flavor {
        Flavor::Sonarr | Flavor::Sportarr => {
            let query = base_query(&[
                ("includeSeries", q(true)),
                ("includeEpisodeFile", q(true)),
                ("includeEpisodeImages", q(true)),
            ]);
            let events: Vec<SonarrCalendarEvent> =
                send_json(ctx, "the calendar", api_get(rt, ctx, &path, &query)?).await?;
            map_sonarr_calendar(ctx, flavor, events)
        }
        Flavor::Radarr => {
            let events: Vec<RadarrCalendarEvent> = send_json(
                ctx,
                "the calendar",
                api_get(rt, ctx, &path, &base_query(&[]))?,
            )
            .await?;
            map_radarr_calendar(ctx, events)
        }
        Flavor::Lidarr => {
            let events: Vec<LidarrCalendarEvent> = send_json(
                ctx,
                "the calendar",
                api_get(rt, ctx, &path, &base_query(&[]))?,
            )
            .await?;
            map_lidarr_calendar(ctx, events)
        }
        Flavor::Readarr => {
            let query = base_query(&[("includeAuthor", q(true))]);
            let events: Vec<ReadarrCalendarEvent> =
                send_json(ctx, "the calendar", api_get(rt, ctx, &path, &query)?).await?;
            map_readarr_calendar(ctx, events)
        }
        Flavor::Bindery | Flavor::Prowlarr => Ok(Vec::new()),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Series {
    #[serde(default)]
    id: Option<i64>,
    title: String,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    title_slug: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    imdb_id: Option<String>,
    #[serde(default)]
    images: Vec<Image>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SonarrCalendarEvent {
    title: String,
    air_date_utc: String,
    season_number: i64,
    episode_number: i64,
    #[serde(default)]
    series: Option<Series>,
    #[serde(default)]
    images: Vec<Image>,
}

fn sonarr_series_path(flavor: Flavor, series: &Series) -> String {
    if flavor == Flavor::Sportarr {
        // Sportarr shows a league at /leagues/{id}. It has no slug route.
        return series
            .id
            .map_or_else(|| "/leagues".to_string(), |id| format!("/leagues/{id}"));
    }
    format!("/series/{}", series.title_slug)
}

fn map_sonarr_calendar(
    ctx: &IntegrationContext,
    flavor: Flavor,
    events: Vec<SonarrCalendarEvent>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let (link_name, link_logo) = if flavor == Flavor::Sportarr {
        ("Sportarr", "/images/apps/sportarr.svg")
    } else {
        ("Sonarr", "/images/apps/sonarr.svg")
    };
    events
        .into_iter()
        .map(|event| {
            let start_date = parse_date(ctx, &event.air_date_utc)?;
            let image_src = best_image(
                event
                    .images
                    .iter()
                    .chain(event.series.iter().flat_map(|series| series.images.iter()))
                    .filter(|image| image.remote_url.is_some())
                    .cloned(),
            )
            .and_then(|image| image.remote_url);
            // Sportarr seasons are years ("S2026/E43" overflows the badge).
            let badge_content = if flavor == Flavor::Sportarr {
                format!("E{}", event.episode_number)
            } else {
                format!("S{}/E{}", event.season_number, event.episode_number)
            };
            let path = event
                .series
                .as_ref()
                .map(|series| sonarr_series_path(flavor, series));
            let mut links = vec![CalendarLink {
                href: external_url(ctx, path.as_deref().unwrap_or("/calendar")),
                name: link_name.to_string(),
                logo: Some(link_logo.to_string()),
                color: None,
                is_dark: true,
            }];
            if let Some(imdb_id) = event.series.as_ref().and_then(|s| s.imdb_id.as_ref()) {
                links.push(imdb_link(imdb_id));
            }
            Ok(CalendarEvent {
                title: event.title,
                sub_title: event.series.as_ref().map(|series| series.title.clone()),
                description: event.series.as_ref().and_then(|s| s.overview.clone()),
                start_date,
                end_date: None,
                is_all_day: None,
                image: image_src.map(|src| CalendarImage {
                    src,
                    aspect_ratio: Some(CalendarImageAspectRatio {
                        width: 7.0,
                        height: 12.0,
                    }),
                    badge: Some(CalendarImageBadge {
                        color: "red".to_string(),
                        content: badge_content,
                    }),
                }),
                location: None,
                metadata: None,
                indicator_color: "blue".to_string(),
                links,
            })
        })
        .collect()
}

fn imdb_link(imdb_id: &str) -> CalendarLink {
    CalendarLink {
        href: format!("https://www.imdb.com/title/{imdb_id}/"),
        name: "IMDb".to_string(),
        color: Some("#f5c518".to_string()),
        is_dark: false,
        logo: Some("/images/apps/imdb.svg".to_string()),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RadarrCalendarEvent {
    title: String,
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    in_cinemas: Option<String>,
    #[serde(default)]
    physical_release: Option<String>,
    #[serde(default)]
    digital_release: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    title_slug: String,
    #[serde(default)]
    images: Vec<Image>,
    #[serde(default)]
    imdb_id: Option<String>,
}

fn map_radarr_calendar(
    ctx: &IntegrationContext,
    events: Vec<RadarrCalendarEvent>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let mut result = Vec::new();
    for event in events {
        let image_src = best_image(
            event
                .images
                .iter()
                .filter(|image| image.remote_url.is_some())
                .cloned(),
        )
        .and_then(|image| image.remote_url);
        let mut links = vec![CalendarLink {
            href: external_url(ctx, &format!("/movie/{}", event.title_slug)),
            name: "Radarr".to_string(),
            logo: Some("/images/apps/radarr.svg".to_string()),
            color: None,
            is_dark: true,
        }];
        if let Some(imdb_id) = &event.imdb_id {
            links.push(imdb_link(imdb_id));
        }
        let releases = [
            (RadarrReleaseType::InCinemas, &event.in_cinemas),
            (RadarrReleaseType::DigitalRelease, &event.digital_release),
            (RadarrReleaseType::PhysicalRelease, &event.physical_release),
        ];
        for (release_type, date) in releases {
            let Some(date) = date else { continue };
            result.push(CalendarEvent {
                title: event.title.clone(),
                sub_title: event.original_title.clone(),
                description: event.overview.clone(),
                start_date: parse_date(ctx, date)?,
                end_date: None,
                is_all_day: None,
                image: image_src.clone().map(|src| CalendarImage {
                    src,
                    aspect_ratio: Some(CalendarImageAspectRatio {
                        width: 7.0,
                        height: 12.0,
                    }),
                    badge: None,
                }),
                location: None,
                metadata: Some(CalendarMetadata::Radarr { release_type }),
                indicator_color: "yellow".to_string(),
                links: links.clone(),
            });
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExternalLink {
    url: String,
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LidarrArtist {
    #[serde(default)]
    links: Vec<ExternalLink>,
    artist_name: String,
    #[serde(default)]
    title_slug: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LidarrCalendarEvent {
    title: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    images: Vec<Image>,
    artist: LidarrArtist,
    release_date: String,
}

fn lidarr_external_link(name: &str, href: &str) -> Option<CalendarLink> {
    let (name, color, logo) = match name {
        "vgmdb" => ("VgmDB", "#f5c518", "/images/apps/vgmdb.svg"),
        "imdb" => ("IMDb", "#f5c518", "/images/apps/imdb.png"),
        "last" => ("LastFM", "#cf222a", "/images/apps/lastfm.svg"),
        _ => return None,
    };
    Some(CalendarLink {
        name: name.to_string(),
        color: Some(color.to_string()),
        is_dark: false,
        logo: Some(logo.to_string()),
        href: href.to_string(),
    })
}

fn map_lidarr_calendar(
    ctx: &IntegrationContext,
    events: Vec<LidarrCalendarEvent>,
) -> HomarrResult<Vec<CalendarEvent>> {
    events
        .into_iter()
        .map(|event| {
            let image_src = best_image(
                event
                    .images
                    .iter()
                    .filter(|image| image.remote_url.is_some())
                    .cloned(),
            )
            .and_then(|image| image.remote_url);
            let mut links = Vec::new();
            // titleSlug is only present when Lidarr returns it.
            if let Some(slug) = &event.artist.title_slug {
                links.push(CalendarLink {
                    href: external_url(ctx, &format!("/artist/{slug}")),
                    name: "Lidarr".to_string(),
                    logo: Some("/images/apps/lidarr.svg".to_string()),
                    color: None,
                    is_dark: true,
                });
            }
            links.extend(
                event
                    .artist
                    .links
                    .iter()
                    .filter_map(|link| lidarr_external_link(&link.name, &link.url)),
            );
            Ok(CalendarEvent {
                title: event.title,
                sub_title: Some(event.artist.artist_name),
                description: event.overview,
                start_date: parse_date(ctx, &event.release_date)?,
                end_date: None,
                is_all_day: None,
                image: image_src.map(|src| CalendarImage {
                    src,
                    aspect_ratio: Some(CalendarImageAspectRatio {
                        width: 7.0,
                        height: 12.0,
                    }),
                    badge: None,
                }),
                location: None,
                metadata: None,
                indicator_color: "cyan".to_string(),
                links,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadarrAuthor {
    author_name: String,
    foreign_author_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadarrCalendarEvent {
    title: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    images: Vec<Image>,
    author: ReadarrAuthor,
    release_date: String,
}

/// Readarr appends a random `?lastWrite=<digits>` that has to be removed to load the image.
fn strip_last_write(url: &str) -> String {
    const MARKER: &str = "?lastWrite=";
    let Some(index) = url.find(MARKER) else {
        return url.to_string();
    };
    let rest = &url[index + MARKER.len()..];
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return url.to_string();
    }
    format!("{}{}", &url[..index], &rest[digits..])
}

fn map_readarr_calendar(
    ctx: &IntegrationContext,
    events: Vec<ReadarrCalendarEvent>,
) -> HomarrResult<Vec<CalendarEvent>> {
    events
        .into_iter()
        .map(|event| {
            let image_src = best_image(
                event
                    .images
                    .iter()
                    .filter(|image| image.url.is_some())
                    .cloned(),
            )
            .and_then(|image| image.url)
            .map(|path| external_url(ctx, &strip_last_write(&path)));
            Ok(CalendarEvent {
                title: event.title,
                sub_title: Some(event.author.author_name),
                description: event.overview,
                start_date: parse_date(ctx, &event.release_date)?,
                end_date: None,
                is_all_day: None,
                image: image_src.map(|src| CalendarImage {
                    src,
                    aspect_ratio: Some(CalendarImageAspectRatio {
                        width: 7.0,
                        height: 12.0,
                    }),
                    badge: None,
                }),
                location: None,
                metadata: None,
                indicator_color: "#f5c518".to_string(),
                links: vec![CalendarLink {
                    href: external_url(ctx, &format!("/author/{}", event.author.foreign_author_id)),
                    color: Some("#f5c518".to_string()),
                    is_dark: false,
                    logo: Some("/images/apps/readarr.svg".to_string()),
                    name: "Readarr".to_string(),
                }],
            })
        })
        .collect()
}

// ---- media organizer: missing & queue --------------------------------------------------------

const DEFAULT_PAGE_SIZE: usize = 10;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page<T> {
    total_records: u64,
    records: Vec<T>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MissingRecord {
    id: i64,
    title: String,
    // Sonarr: episode
    #[serde(default)]
    season_number: Option<i64>,
    #[serde(default)]
    episode_number: Option<i64>,
    #[serde(default)]
    series: Option<Series>,
    // Radarr: movie
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    title_slug: Option<String>,
    #[serde(default)]
    images: Vec<Image>,
}

pub async fn media_missing(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    flavor: Flavor,
    page_size: Option<usize>,
) -> HomarrResult<MissingMediaPage> {
    let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    match flavor {
        Flavor::Sonarr | Flavor::Sportarr => {
            let query = [
                ("page", q(1)),
                ("pageSize", q(page_size)),
                ("sortKey", q("airDateUtc")),
                ("sortDirection", q("ascending")),
                ("includeSeries", q(true)),
            ];
            let page: Page<MissingRecord> = send_json(
                ctx,
                "the missing episodes",
                api_get(rt, ctx, "/api/v3/wanted/missing", &query)?,
            )
            .await?;
            Ok(map_sonarr_missing(ctx, flavor, page))
        }
        Flavor::Radarr => {
            let query = [
                ("page", q(1)),
                ("pageSize", q(page_size)),
                ("sortKey", q("title")),
                ("sortDirection", q("ascending")),
            ];
            let page: Page<MissingRecord> = send_json(
                ctx,
                "the missing movies",
                api_get(rt, ctx, "/api/v3/wanted/missing", &query)?,
            )
            .await?;
            Ok(map_radarr_missing(ctx, page))
        }
        Flavor::Bindery => {
            let books: Vec<BinderyMissingBook> = send_json(
                ctx,
                "the missing books",
                api_get(rt, ctx, "/api/v1/wanted/missing", &[])?,
            )
            .await?;
            Ok(map_bindery_missing(ctx, books, page_size))
        }
        Flavor::Lidarr | Flavor::Readarr | Flavor::Prowlarr => {
            Err(crate::error::unsupported(ctx, "media organizer"))
        }
    }
}

fn map_sonarr_missing(
    ctx: &IntegrationContext,
    flavor: Flavor,
    page: Page<MissingRecord>,
) -> MissingMediaPage {
    // Sportarr shows its missing events on the activity page.
    let fallback = if flavor == Flavor::Sportarr {
        "/activity"
    } else {
        "/wanted/missing"
    };
    MissingMediaPage {
        total_count: page.total_records,
        items: page
            .records
            .into_iter()
            .map(|episode| MissingMediaItem {
                id: episode.id,
                title: episode.title,
                item_type: MediaOrganizerItemType::Episode,
                season_number: episode.season_number,
                episode_number: episode.episode_number,
                series_title: episode.series.as_ref().map(|series| series.title.clone()),
                year: episode.series.as_ref().and_then(|series| series.year),
                image_url: episode
                    .series
                    .as_ref()
                    .and_then(|series| poster_url(&series.images)),
                link: match episode.series.as_ref().filter(|s| !s.title_slug.is_empty()) {
                    Some(series) => external_url(ctx, &sonarr_series_path(flavor, series)),
                    None => external_url(ctx, fallback),
                },
            })
            .collect(),
    }
}

fn map_radarr_missing(ctx: &IntegrationContext, page: Page<MissingRecord>) -> MissingMediaPage {
    MissingMediaPage {
        total_count: page.total_records,
        items: page
            .records
            .into_iter()
            .map(|movie| MissingMediaItem {
                id: movie.id,
                title: movie.title,
                item_type: MediaOrganizerItemType::Movie,
                year: movie.year,
                season_number: None,
                episode_number: None,
                series_title: None,
                image_url: poster_url(&movie.images),
                link: external_url(
                    ctx,
                    &format!("/movie/{}", movie.title_slug.unwrap_or_default()),
                ),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinderyAuthor {
    author_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinderyMissingBook {
    id: i64,
    title: String,
    // Books with no known release date return `null`.
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    author: Option<BinderyAuthor>,
}

fn utc_year(value: &str) -> Option<i64> {
    use chrono::Datelike;
    let date = DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc).year())
        .or_else(|_| NaiveDate::parse_from_str(value, "%Y-%m-%d").map(|date| date.year()))
        .ok()?;
    Some(i64::from(date))
}

fn map_bindery_missing(
    ctx: &IntegrationContext,
    books: Vec<BinderyMissingBook>,
    page_size: usize,
) -> MissingMediaPage {
    MissingMediaPage {
        total_count: books.len() as u64,
        items: books
            .into_iter()
            .take(page_size)
            .map(|book| MissingMediaItem {
                id: book.id,
                link: external_url(ctx, &format!("/book/{}", book.id)),
                title: book.title,
                item_type: MediaOrganizerItemType::Book,
                year: book.release_date.as_deref().and_then(utc_year),
                season_number: None,
                episode_number: None,
                series_title: book.author.map(|author| author.author_name),
                // This endpoint returns a direct provider image URL. /api/v1/book's imageUrl is a
                // Bindery-proxied path that would 401 in the browser, so only absolute URLs are passed.
                image_url: book.image_url.filter(|url| url.starts_with("http")),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Episode {
    title: String,
    season_number: i64,
    episode_number: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueRecord {
    id: i64,
    title: String,
    status: String,
    #[serde(default)]
    timeleft: Option<String>,
    #[serde(default)]
    size: Option<f64>,
    #[serde(default)]
    sizeleft: Option<f64>,
    #[serde(default)]
    series: Option<Series>,
    #[serde(default)]
    episode: Option<Episode>,
    #[serde(default)]
    movie: Option<Series>,
}

fn percent_complete(size: Option<f64>, size_left: Option<f64>) -> f64 {
    let total = size.unwrap_or(0.0);
    if total > 0.0 {
        ((total - size_left.unwrap_or(0.0)) / total * 100.0).round()
    } else {
        0.0
    }
}

pub async fn media_queue(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    flavor: Flavor,
    page_size: Option<usize>,
) -> HomarrResult<QueuedMediaPage> {
    let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    match flavor {
        Flavor::Sonarr | Flavor::Sportarr => {
            let query = [
                ("page", q(1)),
                ("pageSize", q(page_size)),
                ("includeSeries", q(true)),
                ("includeEpisode", q(true)),
            ];
            let page: Page<QueueRecord> =
                send_json(ctx, "the queue", api_get(rt, ctx, "/api/v3/queue", &query)?).await?;
            Ok(map_sonarr_queue(ctx, flavor, page))
        }
        Flavor::Radarr => {
            let query = [
                ("page", q(1)),
                ("pageSize", q(page_size)),
                ("includeMovie", q(true)),
            ];
            let page: Page<QueueRecord> =
                send_json(ctx, "the queue", api_get(rt, ctx, "/api/v3/queue", &query)?).await?;
            Ok(map_radarr_queue(ctx, page))
        }
        Flavor::Bindery => {
            let data: BinderyQueue =
                send_json(ctx, "the queue", api_get(rt, ctx, "/api/v1/queue", &[])?).await?;
            Ok(map_bindery_queue(ctx, data.into_items(), page_size))
        }
        Flavor::Lidarr | Flavor::Readarr | Flavor::Prowlarr => {
            Err(crate::error::unsupported(ctx, "media organizer"))
        }
    }
}

fn map_sonarr_queue(
    ctx: &IntegrationContext,
    flavor: Flavor,
    page: Page<QueueRecord>,
) -> QueuedMediaPage {
    let fallback = if flavor == Flavor::Sportarr {
        "/activity"
    } else {
        "/activity/queue"
    };
    QueuedMediaPage {
        total_count: page.total_records,
        items: page
            .records
            .into_iter()
            .map(|item| QueuedMediaItem {
                id: item.id,
                title: item
                    .episode
                    .as_ref()
                    .map_or_else(|| item.title.clone(), |episode| episode.title.clone()),
                item_type: MediaOrganizerItemType::Episode,
                status: item.status.clone(),
                time_left: item.timeleft.clone(),
                percent_complete: percent_complete(item.size, item.sizeleft),
                year: None,
                season_number: item.episode.as_ref().map(|e| e.season_number),
                episode_number: item.episode.as_ref().map(|e| e.episode_number),
                series_title: item.series.as_ref().map(|series| series.title.clone()),
                image_url: item.series.as_ref().and_then(|s| poster_url(&s.images)),
                link: match item.series.as_ref().filter(|s| !s.title_slug.is_empty()) {
                    Some(series) => external_url(ctx, &sonarr_series_path(flavor, series)),
                    None => external_url(ctx, fallback),
                },
            })
            .collect(),
    }
}

fn map_radarr_queue(ctx: &IntegrationContext, page: Page<QueueRecord>) -> QueuedMediaPage {
    QueuedMediaPage {
        total_count: page.total_records,
        items: page
            .records
            .into_iter()
            .map(|item| QueuedMediaItem {
                id: item.id,
                title: item
                    .movie
                    .as_ref()
                    .map_or_else(|| item.title.clone(), |movie| movie.title.clone()),
                item_type: MediaOrganizerItemType::Movie,
                status: item.status.clone(),
                time_left: item.timeleft.clone(),
                percent_complete: percent_complete(item.size, item.sizeleft),
                year: item.movie.as_ref().and_then(|movie| movie.year),
                season_number: None,
                episode_number: None,
                series_title: None,
                image_url: item.movie.as_ref().and_then(|m| poster_url(&m.images)),
                link: match item.movie.as_ref().filter(|m| !m.title_slug.is_empty()) {
                    Some(movie) => external_url(ctx, &format!("/movie/{}", movie.title_slug)),
                    None => external_url(ctx, "/activity/queue"),
                },
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinderyQueueBook {
    #[serde(default)]
    id: Option<i64>,
    title: String,
    #[serde(default)]
    author_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BinderyQueueItem {
    id: i64,
    title: String,
    status: String,
    #[serde(default)]
    percentage: Option<String>,
    #[serde(default)]
    time_left: Option<String>,
    #[serde(default)]
    book: Option<BinderyQueueBook>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BinderyQueue {
    List(Vec<BinderyQueueItem>),
    Wrapped { items: Vec<BinderyQueueItem> },
}

impl BinderyQueue {
    fn into_items(self) -> Vec<BinderyQueueItem> {
        match self {
            Self::List(items) | Self::Wrapped { items } => items,
        }
    }
}

fn map_bindery_queue(
    ctx: &IntegrationContext,
    items: Vec<BinderyQueueItem>,
    page_size: usize,
) -> QueuedMediaPage {
    // Bindery's queue endpoint returns its entire historical log; "imported" rows are not in flight.
    let active: Vec<BinderyQueueItem> = items
        .into_iter()
        .filter(|item| item.status != "imported")
        .collect();
    QueuedMediaPage {
        total_count: active.len() as u64,
        items: active
            .into_iter()
            .take(page_size)
            .map(|item| {
                let percent = item
                    .percentage
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map_or(0.0, |value| value.parse::<f64>().unwrap_or(0.0).round());
                // A 100%-complete item has no meaningful time remaining.
                let time_left = if percent >= 100.0 {
                    None
                } else {
                    item.time_left
                };
                QueuedMediaItem {
                    id: item.id,
                    title: item
                        .book
                        .as_ref()
                        .map_or_else(|| item.title.clone(), |book| book.title.clone()),
                    item_type: MediaOrganizerItemType::Book,
                    status: item.status,
                    time_left,
                    percent_complete: percent,
                    year: None,
                    season_number: None,
                    episode_number: None,
                    series_title: item.book.as_ref().and_then(|book| book.author_name.clone()),
                    image_url: None,
                    link: match item.book.as_ref().and_then(|book| book.id) {
                        Some(id) => external_url(ctx, &format!("/book/{id}")),
                        None => external_url(ctx, "/queue"),
                    },
                }
            })
            .collect(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeriesStatsEntry {
    monitored: bool,
    #[serde(default)]
    statistics: Option<SeriesStatistics>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeriesStatistics {
    total_episode_count: u64,
    episode_file_count: u64,
    size_on_disk: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MovieStatsEntry {
    monitored: bool,
    has_file: bool,
    size_on_disk: u64,
}

#[derive(Debug, Deserialize)]
struct MonitoredOnlyEntry {
    monitored: bool,
}

pub enum LibraryStats {
    Series {
        shows: u64,
        monitored: u64,
        episodes: u64,
        downloaded: u64,
        storage: u64,
    },
    Movies {
        movies: u64,
        monitored: u64,
        downloaded: u64,
        storage: u64,
    },
    ArtistsOrBooks {
        total: u64,
        monitored: u64,
    },
}

pub async fn library_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    flavor: Flavor,
) -> HomarrResult<LibraryStats> {
    match flavor {
        Flavor::Sonarr | Flavor::Sportarr => {
            let list: Vec<SeriesStatsEntry> = send_json(
                ctx,
                "the series library",
                api_get(rt, ctx, "/api/v3/series", &[])?,
            )
            .await?;
            Ok(LibraryStats::Series {
                shows: list.len() as u64,
                monitored: list.iter().filter(|s| s.monitored).count() as u64,
                episodes: list
                    .iter()
                    .map(|s| s.statistics.as_ref().map_or(0, |st| st.total_episode_count))
                    .sum(),
                downloaded: list
                    .iter()
                    .map(|s| s.statistics.as_ref().map_or(0, |st| st.episode_file_count))
                    .sum(),
                storage: list
                    .iter()
                    .map(|s| s.statistics.as_ref().map_or(0, |st| st.size_on_disk))
                    .sum(),
            })
        }
        Flavor::Radarr => {
            let list: Vec<MovieStatsEntry> = send_json(
                ctx,
                "the movie library",
                api_get(rt, ctx, "/api/v3/movie", &[])?,
            )
            .await?;
            Ok(LibraryStats::Movies {
                movies: list.len() as u64,
                monitored: list.iter().filter(|m| m.monitored).count() as u64,
                downloaded: list.iter().filter(|m| m.has_file).count() as u64,
                storage: list.iter().map(|m| m.size_on_disk).sum(),
            })
        }
        Flavor::Lidarr | Flavor::Readarr => {
            let path = if flavor == Flavor::Lidarr {
                "/api/v1/artist"
            } else {
                "/api/v1/book"
            };
            let list: Vec<MonitoredOnlyEntry> =
                send_json(ctx, "the library", api_get(rt, ctx, path, &[])?).await?;
            Ok(LibraryStats::ArtistsOrBooks {
                total: list.len() as u64,
                monitored: list.iter().filter(|e| e.monitored).count() as u64,
            })
        }
        Flavor::Bindery | Flavor::Prowlarr => Err(crate::error::unsupported(ctx, "stats")),
    }
}

// ---- Prowlarr connection check helper ---------------------------------------------------------

/// Plain authenticated POST used by Prowlarr's "test all indexers".
pub async fn post_empty(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    path: &str,
) -> HomarrResult<()> {
    let key = ctx.require_secret("apiKey")?;
    let request = rt
        .http
        .standard()
        .post(url(ctx, path, &[])?)
        .header("X-Api-Key", key);
    send(ctx, request).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;

    fn ctx(kind: IntegrationKind) -> IntegrationContext {
        IntegrationContext {
            id: "i".into(),
            name: "Test".into(),
            url: "http://internal:8989".into(),
            external_url: Some("https://arr.example.com/".into()),
            kind,
            secrets: HashMap::new(),
        }
    }

    fn from_json<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn sonarr_calendar_uses_best_image_badge_and_links() {
        let events: Vec<SonarrCalendarEvent> = from_json(json!([{
            "title": "Pilot",
            "airDateUtc": "2024-05-01T01:00:00Z",
            "seasonNumber": 2,
            "episodeNumber": 5,
            "images": [{ "coverType": "fanart", "remoteUrl": "https://img/fanart.jpg" }],
            "series": {
                "id": 3, "title": "Show", "overview": "About", "titleSlug": "show",
                "imdbId": "tt1",
                "images": [{ "coverType": "poster", "remoteUrl": "https://img/poster.jpg" }]
            }
        }]));
        let mapped =
            map_sonarr_calendar(&ctx(IntegrationKind::Sonarr), Flavor::Sonarr, events).unwrap();
        let event = &mapped[0];
        assert_eq!(event.sub_title.as_deref(), Some("Show"));
        let image = event.image.as_ref().unwrap();
        assert_eq!(image.src, "https://img/poster.jpg");
        assert_eq!(image.badge.as_ref().unwrap().content, "S2/E5");
        assert_eq!(event.links[0].href, "https://arr.example.com/series/show");
        assert_eq!(event.links[1].href, "https://www.imdb.com/title/tt1/");
    }

    #[test]
    fn sportarr_uses_league_paths_and_short_badge() {
        let events: Vec<SonarrCalendarEvent> = from_json(json!([
            {
                "title": "Final", "airDateUtc": "2026-01-01T00:00:00Z",
                "seasonNumber": 2026, "episodeNumber": 43,
                "images": [{ "coverType": "poster", "remoteUrl": "https://img/p.jpg" }],
                "series": { "id": 7, "title": "League", "titleSlug": "league", "images": [] }
            },
            {
                "title": "Standalone", "airDateUtc": "2026-01-02T00:00:00Z",
                "seasonNumber": 2026, "episodeNumber": 1, "series": null, "images": []
            }
        ]));
        let mapped =
            map_sonarr_calendar(&ctx(IntegrationKind::Sportarr), Flavor::Sportarr, events).unwrap();
        assert_eq!(
            mapped[0]
                .image
                .as_ref()
                .unwrap()
                .badge
                .as_ref()
                .unwrap()
                .content,
            "E43"
        );
        assert_eq!(mapped[0].links[0].href, "https://arr.example.com/leagues/7");
        assert_eq!(mapped[0].links[0].name, "Sportarr");
        assert_eq!(mapped[1].links[0].href, "https://arr.example.com/calendar");
        assert!(mapped[1].image.is_none());
    }

    #[test]
    fn radarr_creates_one_event_per_release_date() {
        let events: Vec<RadarrCalendarEvent> = from_json(json!([{
            "title": "Movie", "originalTitle": "Film", "titleSlug": "movie-1",
            "inCinemas": "2024-01-01T00:00:00Z", "physicalRelease": "2024-03-01T00:00:00Z",
            "images": [{ "coverType": "poster", "remoteUrl": "https://img/p.jpg" }]
        }]));
        let mapped = map_radarr_calendar(&ctx(IntegrationKind::Radarr), events).unwrap();
        assert_eq!(mapped.len(), 2);
        assert_eq!(
            mapped[1].metadata,
            Some(CalendarMetadata::Radarr {
                release_type: RadarrReleaseType::PhysicalRelease
            })
        );
        assert_eq!(
            mapped[0].links[0].href,
            "https://arr.example.com/movie/movie-1"
        );
    }

    #[test]
    fn readarr_strips_last_write_from_image_urls() {
        assert_eq!(
            strip_last_write("/MediaCover/1/cover.jpg?lastWrite=638123"),
            "/MediaCover/1/cover.jpg"
        );
        assert_eq!(strip_last_write("/a.jpg"), "/a.jpg");
    }

    #[test]
    fn lidarr_maps_known_external_links_only() {
        let events: Vec<LidarrCalendarEvent> = from_json(json!([{
            "title": "Album", "releaseDate": "2024-02-02T00:00:00Z",
            "artist": {
                "artistName": "Artist", "titleSlug": "artist-slug",
                "links": [
                    { "name": "last", "url": "https://last.fm/x" },
                    { "name": "other", "url": "https://other" }
                ]
            },
            "images": []
        }]));
        let mapped = map_lidarr_calendar(&ctx(IntegrationKind::Lidarr), events).unwrap();
        assert_eq!(mapped[0].links.len(), 2);
        assert_eq!(mapped[0].links[1].name, "LastFM");
    }

    #[test]
    fn queue_percentage_and_links() {
        let page: Page<QueueRecord> = from_json(json!({
            "totalRecords": 2,
            "records": [
                { "id": 1, "title": "t", "status": "downloading", "timeleft": "00:10:00",
                  "size": 200.0, "sizeleft": 50.0,
                  "movie": { "title": "Movie", "year": 2020, "titleSlug": "movie", "images": [] } },
                { "id": 2, "title": "raw", "status": "queued" }
            ]
        }));
        let mapped = map_radarr_queue(&ctx(IntegrationKind::Radarr), page);
        assert_eq!(mapped.total_count, 2);
        assert_eq!(mapped.items[0].percent_complete, 75.0);
        assert_eq!(mapped.items[0].title, "Movie");
        assert_eq!(mapped.items[1].percent_complete, 0.0);
        assert_eq!(
            mapped.items[1].link,
            "https://arr.example.com/activity/queue"
        );
    }

    #[test]
    fn bindery_queue_skips_imported_and_clears_time_left_when_done() {
        let queue: BinderyQueue = from_json(json!({ "items": [
            { "id": 1, "title": "a", "status": "imported" },
            { "id": 2, "title": "b", "status": "blocked", "percentage": "100.0", "timeLeft": "721h" },
            { "id": 3, "title": "c", "status": "downloading", "percentage": "42.4", "timeLeft": "1h",
              "book": { "id": 9, "title": "Book", "authorName": "Author" } }
        ]}));
        let page = map_bindery_queue(&ctx(IntegrationKind::Bindery), queue.into_items(), 10);
        assert_eq!(page.total_count, 2);
        assert_eq!(page.items[0].time_left, None);
        assert_eq!(page.items[1].percent_complete, 42.0);
        assert_eq!(page.items[1].link, "https://arr.example.com/book/9");
        assert_eq!(page.items[1].series_title.as_deref(), Some("Author"));
    }

    #[test]
    fn bindery_missing_only_keeps_absolute_image_urls() {
        let books: Vec<BinderyMissingBook> = from_json(json!([
            { "id": 1, "title": "A", "releaseDate": "2019-04-02T00:00:00Z",
              "imageUrl": "https://assets/x.jpg", "author": { "authorName": "Au" } },
            { "id": 2, "title": "B", "releaseDate": null, "imageUrl": "/api/proxy/x.jpg" }
        ]));
        let page = map_bindery_missing(&ctx(IntegrationKind::Bindery), books, 10);
        assert_eq!(page.items[0].year, Some(2019));
        assert_eq!(
            page.items[0].image_url.as_deref(),
            Some("https://assets/x.jpg")
        );
        assert_eq!(page.items[1].year, None);
        assert_eq!(page.items[1].image_url, None);
    }
}
