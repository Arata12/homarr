use std::collections::HashSet;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::Utc;
use homarr_common::{serialize_superjson, HomarrError, HomarrResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::{
    AppRecord, CronJobConfigRecord, CustomWidgetV2Record, FullBoard, GroupRecord,
    IntegrationRecord, MediaSummary, SearchEngineRecord, TrustedCertificateRecord, UserRecord,
};
use crate::pool::map_sqlx_err;
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCount {
    pub imported: usize,
    pub skipped: usize,
}

/// What an import actually did; entities that already exist are skipped, never overwritten.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub apps: ImportCount,
    pub search_engines: ImportCount,
    /// Imported without password (the export carries none); existing names are skipped.
    pub users: ImportCount,
    /// Name and position only: the export carries no group permissions or members.
    pub groups: ImportCount,
    /// Imported without secrets (the export carries none).
    pub integrations: ImportCount,
    pub boards: ImportCount,
    pub custom_widgets: ImportCount,
    pub medias: ImportCount,
    /// Item-to-integration links dropped because the integration does not exist after import.
    pub dropped_integration_links: usize,
    pub server_settings: usize,
    pub cron_jobs: usize,
    pub trusted_certificates: usize,
}

/// Rows inserted by an import, removed again if a later step fails.
#[derive(Debug)]
enum Created {
    App(String),
    SearchEngine(String),
    User(String),
    Group(String),
    Integration(String),
    Board(String),
    CustomWidget(String),
    Media(String),
}

/// All-or-nothing on error: everything is validated before the first write, entity inserts are
/// undone if any later step fails, and settings/cron/certificate overwrites run last in a single
/// transaction. Not crash-safe: a process crash mid-import can still leave inserted entities.
pub async fn import_backup_json(
    db: &Database,
    bundle: &HomarrBackupBundle,
) -> HomarrResult<ImportSummary> {
    let media_bytes = bundle
        .medias
        .iter()
        .map(|m| {
            B64.decode(&m.content_base64)
                .map_err(|e| HomarrError::BadRequest(format!("Invalid media base64: {e}")))
        })
        .collect::<HomarrResult<Vec<_>>>()?;
    let encoded_settings = bundle
        .server_settings
        .iter()
        .map(|(k, v)| Ok::<_, HomarrError>((k.as_str(), serialize_superjson(v)?)))
        .collect::<HomarrResult<Vec<_>>>()?;

    let mut summary = ImportSummary::default();
    let mut created = Vec::new();
    let mut result = import_entities(db, bundle, &media_bytes, &mut summary, &mut created).await;
    if result.is_ok() {
        result = import_settings(db, bundle, &encoded_settings).await;
    }
    if let Err(err) = result {
        rollback(db, created).await;
        return Err(err);
    }

    summary.server_settings = encoded_settings.len();
    summary.cron_jobs = bundle.cron_jobs.len();
    summary.trusted_certificates = bundle.trusted_certificates.len();
    Ok(summary)
}

async fn rollback(db: &Database, created: Vec<Created>) {
    for entry in created.into_iter().rev() {
        let res = match &entry {
            Created::App(id) => apps::delete(db, id).await,
            Created::SearchEngine(id) => search_engines::delete(db, id).await,
            Created::User(id) => users::delete(db, id).await,
            Created::Group(id) => groups::delete(db, id).await,
            Created::Integration(id) => integrations::delete(db, id).await,
            Created::Board(id) => boards::delete(db, id).await,
            Created::CustomWidget(id) => custom_widgets::delete_v2(db, id).await,
            Created::Media(id) => medias::delete(db, id).await,
        };
        if let Err(err) = res {
            tracing::error!("Backup import rollback could not remove {entry:?}: {err}");
        }
    }
}

async fn import_entities(
    db: &Database,
    bundle: &HomarrBackupBundle,
    media_bytes: &[Vec<u8>],
    summary: &mut ImportSummary,
    created: &mut Vec<Created>,
) -> HomarrResult<()> {
    for user in &bundle.users {
        let Some(name) = user.name.as_deref() else {
            summary.users.skipped += 1;
            continue;
        };
        if users::find_by_name_ci(db, name).await?.is_some()
            || users::find_by_id(db, &user.id).await?.is_some()
        {
            summary.users.skipped += 1;
            continue;
        }
        users::create(
            db,
            users::CreateUserInput {
                id: Some(user.id.clone()),
                name: name.to_string(),
                email: user.email.clone(),
                password_hash: None,
                provider: user.provider.clone(),
                image: user.image.clone(),
            },
        )
        .await?;
        created.push(Created::User(user.id.clone()));
        summary.users.imported += 1;
    }

    for group in &bundle.groups {
        if groups::get_by_name(db, &group.name).await?.is_some() {
            summary.groups.skipped += 1;
            continue;
        }
        let owner_id = match group.owner_id.as_deref() {
            Some(id) => users::find_by_id(db, id).await?.map(|_| id),
            None => None,
        };
        let new_group = groups::create(db, &group.name, owner_id, group.position).await?;
        created.push(Created::Group(new_group.id));
        summary.groups.imported += 1;
    }

    for app in &bundle.apps {
        if apps::get_by_id(db, &app.id).await?.is_some() {
            summary.apps.skipped += 1;
            continue;
        }
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
        created.push(Created::App(app.id.clone()));
        summary.apps.imported += 1;
    }

    for integration in &bundle.integrations {
        if integrations::get_by_id(db, &integration.id)
            .await?
            .is_some()
        {
            summary.integrations.skipped += 1;
            continue;
        }
        let app_id = match integration.app_id.as_deref() {
            Some(id) => apps::get_by_id(db, id).await?.map(|_| id.to_string()),
            None => None,
        };
        integrations::create(
            db,
            integrations::IntegrationInput {
                id: Some(integration.id.clone()),
                name: integration.name.clone(),
                url: integration.url.clone(),
                external_url: integration.external_url.clone(),
                kind: integration.kind.clone(),
                app_id,
            },
            &[],
        )
        .await?;
        created.push(Created::Integration(integration.id.clone()));
        summary.integrations.imported += 1;
    }

    let known_integrations: HashSet<String> = integrations::list_all(db)
        .await?
        .into_iter()
        .map(|i| i.id)
        .collect();

    for se in &bundle.search_engines {
        if search_engines::get_by_id(db, &se.id).await?.is_some() {
            summary.search_engines.skipped += 1;
            continue;
        }
        // Invalid legacy entries are skipped instead of aborting the whole import.
        let result = search_engines::create(
            db,
            search_engines::SearchEngineInput {
                id: Some(se.id.clone()),
                icon_url: se.icon_url.clone(),
                name: se.name.clone(),
                short: se.short.clone(),
                description: se.description.clone(),
                url_template: se.url_template.clone(),
                kind: se.kind.clone(),
                integration_id: se
                    .integration_id
                    .clone()
                    .filter(|id| known_integrations.contains(id)),
            },
        )
        .await;
        match result {
            Ok(_) => {}
            Err(HomarrError::BadRequest(_)) => {
                summary.search_engines.skipped += 1;
                continue;
            }
            Err(err) => return Err(err),
        }
        created.push(Created::SearchEngine(se.id.clone()));
        summary.search_engines.imported += 1;
    }

    for full in &bundle.boards {
        if boards::get_by_id(db, &full.board.id).await?.is_some()
            || boards::get_by_name(db, &full.board.name).await?.is_some()
        {
            summary.boards.skipped += 1;
            continue;
        }
        let column_count = full
            .layouts
            .iter()
            .find(|l| l.role == "base")
            .map_or(10, |l| l.column_count);
        let new_board = boards::create_board_with_defaults(
            db,
            boards::CreateBoardInput {
                id: Some(full.board.id.clone()),
                name: full.board.name.clone(),
                is_public: full.board.is_public,
                creator_id: None,
                column_count,
            },
        )
        .await?;
        created.push(Created::Board(new_board.board.id.clone()));
        summary.boards.imported += 1;

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
            boards::replace_layouts(db, &new_board.board.id, &norm_layouts).await?;
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
        summary.dropped_integration_links += full
            .items
            .iter()
            .flat_map(|i| &i.integration_ids)
            .filter(|id| !known_integrations.contains(*id))
            .count();
        let items: Vec<boards::SaveBoardItemInput> = full
            .items
            .iter()
            .map(|i| boards::SaveBoardItemInput {
                id: i.id.clone(),
                kind: i.kind.clone(),
                options: i.options.clone(),
                advanced_options: i.advanced_options.clone(),
                layouts: i.layouts.clone(),
                integration_ids: i
                    .integration_ids
                    .iter()
                    .filter(|id| known_integrations.contains(*id))
                    .cloned()
                    .collect(),
            })
            .collect();
        boards::save_board_content(db, &new_board.board.id, &sections, &items).await?;
    }

    for cw in &bundle.custom_widgets {
        if custom_widgets::get_v2_by_id(db, &cw.id).await?.is_some() {
            summary.custom_widgets.skipped += 1;
            continue;
        }
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
        created.push(Created::CustomWidget(cw.id.clone()));
        summary.custom_widgets.imported += 1;
    }

    for (media, bytes) in bundle.medias.iter().zip(media_bytes) {
        if medias::get_by_id(db, &media.summary.id).await?.is_some() {
            summary.medias.skipped += 1;
            continue;
        }
        let new_media = medias::create(
            db,
            &media.summary.name,
            bytes,
            &media.summary.content_type,
            None,
        )
        .await?;
        created.push(Created::Media(new_media.id));
        summary.medias.imported += 1;
    }

    Ok(())
}

/// Overwrites existing values, so it runs last and in one transaction.
async fn import_settings(
    db: &Database,
    bundle: &HomarrBackupBundle,
    encoded_settings: &[(&str, String)],
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            for (key, value) in encoded_settings {
                sqlx::query(
                    r#"INSERT INTO "serverSetting" ("setting_key", "value") VALUES (?, ?)
                       ON CONFLICT ("setting_key") DO UPDATE SET "value" = excluded."value""#,
                )
                .bind(*key)
                .bind(value)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for cron in &bundle.cron_jobs {
                sqlx::query(
                    r#"INSERT INTO "cron_job_configuration" ("name", "cron_expression", "is_enabled")
                       VALUES (?, ?, ?)
                       ON CONFLICT ("name") DO UPDATE SET "cron_expression" = excluded."cron_expression", "is_enabled" = excluded."is_enabled""#,
                )
                .bind(&cron.name)
                .bind(&cron.cron_expression)
                .bind(cron.is_enabled)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for cert in &bundle.trusted_certificates {
                sqlx::query(
                    r#"INSERT INTO "trusted_certificate_hostname" ("hostname", "thumbprint", "certificate")
                       VALUES (?, ?, ?)
                       ON CONFLICT ("hostname", "thumbprint") DO UPDATE SET "certificate" = excluded."certificate""#,
                )
                .bind(&cert.hostname)
                .bind(&cert.thumbprint)
                .bind(&cert.certificate)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            for (key, value) in encoded_settings {
                sqlx::query(
                    r#"INSERT INTO "serverSetting" ("setting_key", "value") VALUES ($1, $2)
                       ON CONFLICT ("setting_key") DO UPDATE SET "value" = excluded."value""#,
                )
                .bind(*key)
                .bind(value)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for cron in &bundle.cron_jobs {
                sqlx::query(
                    r#"INSERT INTO "cron_job_configuration" ("name", "cron_expression", "is_enabled")
                       VALUES ($1, $2, $3)
                       ON CONFLICT ("name") DO UPDATE SET "cron_expression" = excluded."cron_expression", "is_enabled" = excluded."is_enabled""#,
                )
                .bind(&cron.name)
                .bind(&cron.cron_expression)
                .bind(cron.is_enabled)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for cert in &bundle.trusted_certificates {
                sqlx::query(
                    r#"INSERT INTO "trusted_certificate_hostname" ("hostname", "thumbprint", "certificate")
                       VALUES ($1, $2, $3)
                       ON CONFLICT ("hostname", "thumbprint") DO UPDATE SET "certificate" = excluded."certificate""#,
                )
                .bind(&cert.hostname)
                .bind(&cert.thumbprint)
                .bind(&cert.certificate)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use homarr_core::DatabaseDriver;
    use serde_json::json;

    use super::*;
    use crate::models::ItemLayoutRecord;

    async fn fresh_db() -> Database {
        let db = Database::connect(DatabaseDriver::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        db.migrate().await.unwrap();
        db
    }

    /// Exports a db holding a user, a group, an app, an integration and a 7-column board
    /// whose single item is linked to that integration.
    async fn source_bundle() -> HomarrBackupBundle {
        let db = fresh_db().await;
        users::create(
            &db,
            users::CreateUserInput {
                id: None,
                name: "alice".to_string(),
                email: None,
                password_hash: Some("hash".to_string()),
                provider: "credentials".to_string(),
                image: None,
            },
        )
        .await
        .unwrap();
        groups::create(&db, "Admins", None, 1).await.unwrap();
        apps::create(
            &db,
            apps::AppInput {
                id: None,
                name: "App".to_string(),
                description: None,
                icon_url: "icon.png".to_string(),
                href: None,
                ping_url: None,
            },
        )
        .await
        .unwrap();
        let integration = integrations::create(
            &db,
            integrations::IntegrationInput {
                id: None,
                name: "Sonarr".to_string(),
                url: "http://sonarr.local".to_string(),
                external_url: None,
                kind: "sonarr".to_string(),
                app_id: None,
            },
            &[],
        )
        .await
        .unwrap();
        let board = boards::create_board_with_defaults(
            &db,
            boards::CreateBoardInput {
                id: None,
                name: "Main".to_string(),
                is_public: false,
                creator_id: None,
                column_count: 7,
            },
        )
        .await
        .unwrap();
        let base = board.layouts.iter().find(|l| l.role == "base").unwrap();
        let sections: Vec<boards::SaveBoardSectionInput> = board
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
        let item = boards::SaveBoardItemInput {
            id: "item-1".to_string(),
            kind: "app".to_string(),
            options: json!({}),
            advanced_options: json!({}),
            layouts: vec![ItemLayoutRecord {
                item_id: "item-1".to_string(),
                section_id: board.sections[0].id.clone(),
                layout_id: base.id.clone(),
                x_offset: 0,
                y_offset: 0,
                width: 1,
                height: 1,
            }],
            integration_ids: vec![integration.id],
        };
        boards::save_board_content(&db, &board.board.id, &sections, &[item])
            .await
            .unwrap();
        export_backup_json(&db).await.unwrap()
    }

    #[tokio::test]
    async fn sqlite_import_round_trip_keeps_column_count_and_integration_links() {
        let bundle = source_bundle().await;
        let db = fresh_db().await;

        let summary = import_backup_json(&db, &bundle).await.unwrap();

        assert_eq!(summary.users.imported, 1);
        assert_eq!(summary.groups.imported, 1);
        assert_eq!(summary.integrations.imported, 1);
        assert_eq!(summary.boards.imported, 1);
        assert_eq!(summary.dropped_integration_links, 0);
        let user = users::find_by_name_ci(&db, "alice").await.unwrap().unwrap();
        assert!(user.password.is_none());
        assert!(groups::get_by_name(&db, "Admins").await.unwrap().is_some());

        let board = boards::get_full_board_by_name(&db, "Main")
            .await
            .unwrap()
            .unwrap();
        let base = board.layouts.iter().find(|l| l.role == "base").unwrap();
        assert_eq!(base.column_count, 7);
        assert_eq!(
            board.items[0].integration_ids,
            vec![bundle.integrations[0].id.clone()]
        );

        // Re-importing skips everything that already exists.
        let again = import_backup_json(&db, &bundle).await.unwrap();
        assert_eq!(again.boards.imported, 0);
        assert_eq!(again.boards.skipped, 1);
        assert_eq!(again.users.skipped, 1);
    }

    #[tokio::test]
    async fn sqlite_import_failure_leaves_no_partial_data() {
        let mut bundle = source_bundle().await;
        // Item layout pointing at a missing section fails the board content write (FK).
        bundle.boards[0].items[0].layouts[0].section_id = "missing-section".to_string();
        let setting_key = bundle.server_settings.keys().next().unwrap().clone();
        let db = fresh_db().await;
        let setting_before = settings::get_setting_raw(&db, &setting_key).await.unwrap();

        assert!(import_backup_json(&db, &bundle).await.is_err());

        assert!(users::find_by_name_ci(&db, "alice")
            .await
            .unwrap()
            .is_none());
        assert!(groups::get_by_name(&db, "Admins").await.unwrap().is_none());
        assert!(apps::list_all(&db).await.unwrap().is_empty());
        assert!(integrations::list_all(&db).await.unwrap().is_empty());
        assert!(boards::list_all(&db).await.unwrap().is_empty());
        assert_eq!(
            settings::get_setting_raw(&db, &setting_key).await.unwrap(),
            setting_before
        );
    }
}
