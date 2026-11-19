//! Kinds of the group without specialized media capabilities: Audiobookshelf, Immich and Tracearr.

use homarr_common::HomarrResult;
use homarr_definitions::IntegrationKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::util::{send, send_json, url};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Deserialize)]
struct Ignored {}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImmichServerStats {
    pub user_count: u64,
    pub photo_count: u64,
    pub video_count: u64,
    pub total_library_usage_in_bytes: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImmichServerStatisticsResponse {
    photos: u64,
    videos: u64,
    usage: u64,
    #[serde(default)]
    usage_by_user: Option<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudiobookshelfDashboardData {
    pub library_count: usize,
    pub total_audiobooks: u64,
    pub total_podcasts: u64,
    pub total_listening_time_seconds: f64,
    pub active_sessions: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudiobookshelfLibrary {
    id: String,
    media_type: String,
}

#[derive(Deserialize)]
struct AudiobookshelfLibrariesResponse {
    libraries: Vec<AudiobookshelfLibrary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudiobookshelfLibraryStats {
    total_items: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudiobookshelfListeningStats {
    total_time: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudiobookshelfOnlineUsersResponse {
    open_sessions: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TracearrStats {
    pub active_streams: u64,
    pub total_users: u64,
    pub total_sessions: u64,
    pub recent_violations: u64,
}

pub async fn immich_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<ImmichServerStats> {
    let key = ctx.require_secret("apiKey")?;
    let client = rt.http.standard();
    let stats_fut = send_json::<ImmichServerStatisticsResponse>(
        ctx,
        "Immich server statistics",
        client
            .get(url(ctx, "/api/server/statistics", &[])?)
            .header("x-api-key", key),
    );
    let users_fut = send_json::<Vec<Value>>(
        ctx,
        "Immich users",
        client
            .get(url(ctx, "/api/users", &[])?)
            .header("x-api-key", key),
    );
    let (stats_res, users_res) = tokio::join!(stats_fut, users_fut);
    let stats = stats_res?;
    let user_count = match users_res {
        Ok(users) => users.len() as u64,
        Err(err) => match stats.usage_by_user {
            Some(by_user) => by_user.len() as u64,
            None => return Err(err),
        },
    };
    Ok(ImmichServerStats {
        user_count,
        photo_count: stats.photos,
        video_count: stats.videos,
        total_library_usage_in_bytes: stats.usage,
    })
}

pub async fn audiobookshelf_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<AudiobookshelfDashboardData> {
    let key = ctx.require_secret("apiKey")?;
    let auth = format!("Bearer {key}");
    let client = &rt.http.standard();
    let libraries_fut = async {
        let libs: AudiobookshelfLibrariesResponse = send_json(
            ctx,
            "Audiobookshelf libraries",
            client
                .get(url(ctx, "/api/libraries", &[])?)
                .header("Authorization", &auth),
        )
        .await?;
        let stats = futures_util::future::try_join_all(libs.libraries.into_iter().map(|lib| {
            let auth = auth.clone();
            async move {
                let encoded = percent_encoding::utf8_percent_encode(
                    &lib.id,
                    percent_encoding::NON_ALPHANUMERIC,
                );
                let lib_stats: AudiobookshelfLibraryStats = send_json(
                    ctx,
                    "Audiobookshelf library stats",
                    client
                        .get(url(ctx, &format!("/api/libraries/{encoded}/stats"), &[])?)
                        .header("Authorization", &auth),
                )
                .await?;
                HomarrResult::Ok((lib.media_type, lib_stats.total_items))
            }
        }))
        .await?;
        HomarrResult::Ok(stats)
    };
    let listening_fut = send_json::<AudiobookshelfListeningStats>(
        ctx,
        "Audiobookshelf listening stats",
        client
            .get(url(ctx, "/api/me/listening-stats", &[])?)
            .header("Authorization", &auth),
    );
    let online_fut = send_json::<AudiobookshelfOnlineUsersResponse>(
        ctx,
        "Audiobookshelf active sessions",
        client
            .get(url(ctx, "/api/users/online", &[])?)
            .header("Authorization", &auth),
    );
    let (libs, listening, online) = tokio::try_join!(libraries_fut, listening_fut, online_fut)?;
    let mut total_audiobooks = 0;
    let mut total_podcasts = 0;
    for (media_type, total) in &libs {
        match media_type.as_str() {
            "book" => total_audiobooks += *total,
            "podcast" => total_podcasts += *total,
            _ => {}
        }
    }
    Ok(AudiobookshelfDashboardData {
        library_count: libs.len(),
        total_audiobooks,
        total_podcasts,
        total_listening_time_seconds: listening.total_time,
        active_sessions: online.open_sessions.len(),
    })
}

pub async fn tracearr_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<TracearrStats> {
    let key = ctx.require_secret("apiKey")?;
    send_json(
        ctx,
        "Tracearr stats",
        rt.http
            .standard()
            .get(url(ctx, "/api/v1/public/stats", &[])?)
            .header("Authorization", format!("Bearer {key}")),
    )
    .await
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let client = rt.http.standard();
    match ctx.kind {
        IntegrationKind::Audiobookshelf => {
            let key = ctx.require_secret("apiKey")?;
            send(ctx, client.get(url(ctx, "/healthcheck", &[])?)).await?;
            let request = client
                .post(url(ctx, "/api/authorize", &[])?)
                .header("Authorization", format!("Bearer {key}"))
                .header("Accept", "application/json");
            send(ctx, request).await?;
        }
        IntegrationKind::Immich => {
            let key = ctx.require_secret("apiKey")?;
            let request = client
                .get(url(ctx, "/api/users/me", &[])?)
                .header("x-api-key", key);
            let _: Ignored = send_json(ctx, "the current user", request).await?;
        }
        IntegrationKind::Tracearr => {
            let key = ctx.require_secret("apiKey")?;
            let request = client
                .get(url(ctx, "/api/v1/public/health", &[])?)
                .header("Authorization", format!("Bearer {key}"));
            send(ctx, request).await?;
        }
        _ => {}
    }
    Ok(())
}
