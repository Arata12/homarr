use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use homarr_common::{HomarrError, HomarrResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::{
    AppRecord, CronJobConfigRecord, CustomWidgetV2Record, FullBoard, GroupRecord,
    IntegrationRecord, MediaSummary, SearchEngineRecord, TrustedCertificateRecord, UserRecord,
};
use crate::repos::{
    apps, boards, custom_widgets, groups, integrations, medias, search_engines, settings, users,
};
use crate::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomarrBackupBundle {
    pub version: String,
    pub exported_at: String,
    pub apps: Vec<AppRecord>,
    pub boards: Vec<FullBoard>,
    pub integrations: Vec<IntegrationRecord>,
    pub search_engines: Vec<SearchEngineRecord>,
    pub groups: Vec<GroupRecord>,
    pub users: Vec<UserRecord>,
    pub server_settings: std::collections::HashMap<String, Value>,
    pub custom_widgets: Vec<CustomWidgetV2Record>,
    pub cron_jobs: Vec<CronJobConfigRecord>,
    pub trusted_certificates: Vec<TrustedCertificateRecord>,
    pub medias: Vec<BackupMediaEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupMediaEntry {
    pub summary: MediaSummary,
    pub content_base64: String,
}

pub async fn export_backup_json(db: &Database) -> HomarrResult<HomarrBackupBundle> {
    let all_apps = apps::list_all(db).await?;
    let board_list = boards::list_all(db).await?;
    let mut full_boards = Vec::with_capacity(board_list.len());
    for b in board_list {
        if let Some(full) = boards::get_full_board_by_id(db, &b.id).await? {
            full_boards.push(full);
        }
    }
    let all_integrations = integrations::list_all(db).await?;
    let all_search = search_engines::list_all(db).await?;
    let all_groups = groups::list_all(db).await?;
    let all_users = users::list_all(db).await?;
    let server_settings = settings::get_all_server_settings(db).await?;
    let custom_widgets = custom_widgets::list_v2(db).await?;
    let cron_jobs = settings::list_cron_configs(db).await?;
    let trusted_certificates = settings::list_trusted_hostnames(db).await?;

    let media_meta = medias::list_metadata(db).await?;
    let mut media_entries = Vec::with_capacity(media_meta.len());
    for meta in media_meta {
        if let Some(full) = medias::get_by_id(db, &meta.id).await? {
            media_entries.push(BackupMediaEntry {
                summary: meta,
                content_base64: B64.encode(&full.content),
            });
        }
    }

    Ok(HomarrBackupBundle {
        version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: Utc::now().to_rfc3339(),
        apps: all_apps,
        boards: full_boards,
        integrations: all_integrations,
        search_engines: all_search,
        groups: all_groups,
        users: all_users,
        server_settings,
        custom_widgets,
        cron_jobs,
        trusted_certificates,
        medias: media_entries,
    })
}

pub async fn import_backup_json(db: &Database, bundle: &HomarrBackupBundle) -> HomarrResult<()> {
    for (k, val) in &bundle.server_settings {
        settings::set_setting(db, k, val).await?;
    }

    for app in &bundle.apps {
        if apps::get_by_id(db, &app.id).await?.is_none() {
            apps::create(
                db,
                apps::AppInput {
                    id: Some(app.id.clone()),
                    name: app.name.clone(),
                    description: app.description.clone(),
                    icon_url: app.icon_url.clone(),
                    href: app.href.clone(),
                    ping_url: app.ping_url.clone(),
                },
            )
            .await?;
        }
    }

    for se in &bundle.search_engines {
        if search_engines::get_by_id(db, &se.id).await?.is_none() {
            let _ = search_engines::create(
                db,
                search_engines::SearchEngineInput {
                    id: Some(se.id.clone()),
                    icon_url: se.icon_url.clone(),
                    name: se.name.clone(),
                    short: se.short.clone(),
                    description: se.description.clone(),
                    url_template: se.url_template.clone(),
                    kind: se.kind.clone(),
                    integration_id: None,
                },
            )
            .await;
        }
    }

    for full in &bundle.boards {
        if boards::get_by_id(db, &full.board.id).await?.is_none()
            && boards::get_by_name(db, &full.board.name).await?.is_none()
        {
            let created = boards::create_board_with_defaults(
                db,
                boards::CreateBoardInput {
                    id: Some(full.board.id.clone()),
                    name: full.board.name.clone(),
                    is_public: full.board.is_public,
                    creator_id: None,
                    column_count: 10,
                },
            )
            .await?;

            let norm_layouts: Vec<homarr_definitions::board::BoardLayoutNormInput> = full
                .layouts
                .iter()
                .map(|l| homarr_definitions::board::BoardLayoutNormInput {
                    id: l.id.clone(),
                    name: l.name.clone(),
                    breakpoint: l.breakpoint,
                    column_count: l.column_count,
                    left_gutter_column_count: l.left_gutter_column_count,
                    right_gutter_column_count: l.right_gutter_column_count,
                    role: homarr_definitions::board::LayoutRole::from_str_opt(&l.role),
                    resize_source: l.resize_source.clone(),
                })
                .collect();
            if !norm_layouts.is_empty() {
                boards::replace_layouts(db, &created.board.id, &norm_layouts).await?;
            }

            let sections: Vec<boards::SaveBoardSectionInput> = full
                .sections
                .iter()
                .map(|s| boards::SaveBoardSectionInput {
                    id: s.id.clone(),
                    kind: s.kind.clone(),
                    x_offset: s.x_offset,
                    y_offset: s.y_offset,
                    name: s.name.clone(),
                    options: s.options.clone(),
                    layouts: s.layouts.clone(),
                })
                .collect();
            let items: Vec<boards::SaveBoardItemInput> = full
                .items
                .iter()
                .map(|i| boards::SaveBoardItemInput {
                    id: i.id.clone(),
                    kind: i.kind.clone(),
                    options: i.options.clone(),
                    advanced_options: i.advanced_options.clone(),
                    layouts: i.layouts.clone(),
                    integration_ids: vec![],
                })
                .collect();
            boards::save_board_content(db, &created.board.id, &sections, &items).await?;
        }
    }

    for cw in &bundle.custom_widgets {
        if custom_widgets::get_v2_by_id(db, &cw.id).await?.is_none() {
            custom_widgets::create_v2(
                db,
                custom_widgets::CustomWidgetV2Input {
                    id: Some(cw.id.clone()),
                    name: cw.name.clone(),
                    description: cw.description.clone(),
                    icon_url: cw.icon_url.clone(),
                    sources: cw.sources.clone(),
                    requests: cw.requests.clone(),
                    options: cw.options.clone(),
                    template: cw.template.clone(),
                    workshop: cw.workshop.clone(),
                    enabled: cw.enabled,
                    creator_id: None,
                },
            )
            .await?;
        }
    }

    for cron in &bundle.cron_jobs {
        settings::upsert_cron_config(db, &cron.name, &cron.cron_expression, cron.is_enabled)
            .await?;
    }

    for cert in &bundle.trusted_certificates {
        settings::upsert_trusted_hostname(db, &cert.hostname, &cert.thumbprint, &cert.certificate)
            .await?;
    }

    for media in &bundle.medias {
        if medias::get_by_id(db, &media.summary.id).await?.is_none() {
            let bytes = B64
                .decode(&media.content_base64)
                .map_err(|e| HomarrError::BadRequest(format!("Invalid media base64: {e}")))?;
            medias::create(
                db,
                &media.summary.name,
                &bytes,
                &media.summary.content_type,
                None,
            )
            .await?;
        }
    }

    Ok(())
}
