use homarr_common::{create_id, HomarrError, HomarrResult, SecretCrypto};
use homarr_definitions::board::{BoardLayoutNormInput, LayoutRole};
use homarr_definitions::settings::default_server_setting_json;
use serde_json::json;

use crate::models::{AppRecord, ItemLayoutRecord, SectionLayoutRecord};
use crate::repos::{apps, boards, groups, integrations, search_engines, settings, users};
use crate::Database;

/// Seeds mandatory default groups, onboarding record, default search engines, and server settings.
pub async fn seed_defaults(db: &Database) -> HomarrResult<()> {
    if groups::get_by_name(db, "everyone").await?.is_none() {
        let _ = groups::create(db, "everyone", None, 32_767).await?;
    }

    if settings::get_onboarding(db).await?.is_none() {
        let user_count = users::count(db).await?;
        let step = if user_count > 0 { "finish" } else { "start" };
        settings::set_onboarding(db, step, None).await?;
    }

    let existing_engines = search_engines::list_all(db).await?;
    if existing_engines.is_empty() {
        let defaults = [
            (
                "Google",
                "g",
                "https://www.google.com/search?q=%s",
                "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/google.svg",
                "Search the web with Google",
            ),
            (
                "DuckDuckGo",
                "ddg",
                "https://duckduckgo.com/?q=%s",
                "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/duckduckgo.svg",
                "Privacy-focused search with DuckDuckGo",
            ),
            (
                "Bing",
                "b",
                "https://www.bing.com/search?q=%s",
                "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/microsoft-bing.svg",
                "Search with Microsoft Bing",
            ),
            (
                "Wikipedia",
                "w",
                "https://en.wikipedia.org/wiki/Special:Search?search=%s",
                "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/wikipedia.svg",
                "Search Wikipedia articles",
            ),
            (
                "YouTube",
                "yt",
                "https://www.youtube.com/results?search_query=%s",
                "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/youtube.svg",
                "Search videos on YouTube",
            ),
        ];
        for (name, short, tpl, icon, desc) in defaults {
            let _ = search_engines::create(
                db,
                search_engines::SearchEngineInput {
                    id: None,
                    icon_url: icon.to_string(),
                    name: name.to_string(),
                    short: short.to_string(),
                    description: Some(desc.to_string()),
                    url_template: Some(tpl.to_string()),
                    kind: "generic".to_string(),
                    integration_id: None,
                },
            )
            .await?;
        }
    }

    for key in homarr_definitions::settings::DEFAULT_SERVER_SETTINGS_KEYS {
        if settings::get_setting_raw(db, key).await?.is_none() {
            if let Some(val) = default_server_setting_json(key) {
                settings::set_setting(db, key, &val).await?;
            }
        }
    }

    Ok(())
}

/// Seeds a demo admin user (`demo`/`demo`), sample apps, mock integration, and Home board when `DEMO_MODE=true`.
pub async fn seed_demo(db: &Database, _crypto: &SecretCrypto) -> HomarrResult<()> {
    seed_defaults(db).await?;

    let demo_user = match users::find_by_name_ci(db, "demo").await? {
        Some(existing) => existing,
        None => {
            let password_hash = tokio::task::spawn_blocking(|| bcrypt::hash("demo", 10))
                .await
                .map_err(|e| HomarrError::Internal(format!("Password hash task failed: {e}")))?
                .map_err(|e| HomarrError::Internal(format!("Bcrypt hash failed: {e}")))?;

            let user = users::create(
                db,
                users::CreateUserInput {
                    id: None,
                    name: "demo".to_string(),
                    email: Some("demo@homarr.dev".to_string()),
                    password_hash: Some(password_hash),
                    provider: "credentials".to_string(),
                    image: None,
                },
            )
            .await?;

            let admin_group = match groups::get_by_name(db, "administrators").await? {
                Some(g) => g,
                None => {
                    let g = groups::create(db, "administrators", Some(&user.id), 0).await?;
                    groups::set_permissions(db, &g.id, &["admin".to_string()]).await?;
                    g
                }
            };
            groups::add_member(db, &admin_group.id, &user.id).await?;
            if let Some(everyone) = groups::get_by_name(db, "everyone").await? {
                groups::add_member(db, &everyone.id, &user.id).await?;
            }
            user
        }
    };

    let existing_apps = apps::list_all(db).await?;
    let docs_app = ensure_demo_app(
        db,
        &existing_apps,
        "Homarr Docs",
        "Official Homarr documentation",
        "/logo/homarr.svg",
        "https://homarr.dev/docs/getting-started",
        Some("https://homarr.dev"),
    )
    .await?;
    let sonarr_app = ensure_demo_app(
        db,
        &existing_apps,
        "Sonarr",
        "TV series collection manager",
        "/images/apps/sonarr.svg",
        "https://sonarr.tv",
        None,
    )
    .await?;
    let radarr_app = ensure_demo_app(
        db,
        &existing_apps,
        "Radarr",
        "Movie collection manager",
        "/images/apps/radarr.svg",
        "https://radarr.video",
        None,
    )
    .await?;
    let nextcloud_app = ensure_demo_app(
        db,
        &existing_apps,
        "Nextcloud",
        "Self-hosted productivity platform",
        "/images/apps/nextcloud.svg",
        "https://nextcloud.com",
        None,
    )
    .await?;
    let truenas_app = ensure_demo_app(
        db,
        &existing_apps,
        "TrueNAS",
        "Open enterprise storage OS",
        "/images/apps/truenas.svg",
        "https://www.truenas.com",
        None,
    )
    .await?;
    let pihole_app = ensure_demo_app(
        db,
        &existing_apps,
        "Pi-hole",
        "Network-wide DNS ad blocker",
        "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/pi-hole.svg",
        "https://pi-hole.net",
        None,
    )
    .await?;

    let existing_integrations = integrations::list_all(db).await?;
    let mock_integration = match existing_integrations.into_iter().find(|i| i.kind == "mock") {
        Some(existing) => existing,
        None => {
            integrations::create(
                db,
                integrations::IntegrationInput {
                    id: None,
                    name: "Mock Homelab".to_string(),
                    url: "http://mock.local".to_string(),
                    external_url: None,
                    kind: "mock".to_string(),
                    app_id: None,
                },
                &[],
            )
            .await?
        }
    };

    let existing_home = boards::get_full_board_by_name(db, "Home").await?;
    let should_upgrade_legacy_mvp = existing_home.as_ref().is_some_and(|fb| {
        fb.board.background_image_url.is_none()
            && fb.sections.len() == 1
            && fb.sections[0].name.as_deref() == Some("Main")
            && fb.items.len() == 8
    });

    if existing_home.is_none() || should_upgrade_legacy_mvp {
        let board = match existing_home {
            Some(existing) => {
                let norm_layouts: Vec<BoardLayoutNormInput> = existing
                    .layouts
                    .iter()
                    .map(|l| BoardLayoutNormInput {
                        id: l.id.clone(),
                        name: l.name.clone(),
                        breakpoint: l.breakpoint,
                        column_count: if l.role == "base" { 12 } else { l.column_count },
                        left_gutter_column_count: l.left_gutter_column_count,
                        right_gutter_column_count: l.right_gutter_column_count,
                        role: LayoutRole::from_str_opt(&l.role),
                        resize_source: l.resize_source.clone(),
                    })
                    .collect();
                let updated_layouts =
                    boards::replace_layouts(db, &existing.board.id, &norm_layouts).await?;
                let mut refreshed = existing;
                refreshed.layouts = updated_layouts;
                refreshed
            }
            None => {
                boards::create_board_with_defaults(
                    db,
                    boards::CreateBoardInput {
                        id: None,
                        name: "Home".to_string(),
                        is_public: true,
                        creator_id: Some(demo_user.id.clone()),
                        column_count: 12,
                    },
                )
                .await?
            }
        };

        let _ = boards::update_settings(
            db,
            &board.board.id,
            boards::BoardSettingsPatch {
                page_title: Some(Some("Homarr".to_string())),
                background_image_url: Some(Some(
                    "/images/demo-dashboard-background.svg".to_string(),
                )),
                background_image_attachment: Some("fixed".to_string()),
                background_image_repeat: Some("no-repeat".to_string()),
                background_image_size: Some("cover".to_string()),
                primary_color: Some(Some("#fa5252".to_string())),
                secondary_color: Some(Some("#fd7e14".to_string())),
                opacity: Some(86),
                item_radius: Some(Some("lg".to_string())),
                ..Default::default()
            },
        )
        .await?;

        let mobile_id = board
            .layouts
            .iter()
            .find(|l| l.role == "mobile")
            .map(|l| l.id.clone())
            .unwrap_or_else(|| board.layouts[0].id.clone());
        let desktop_id = board
            .layouts
            .iter()
            .find(|l| l.role == "base")
            .map(|l| l.id.clone())
            .unwrap_or_else(|| board.layouts[0].id.clone());

        let root_section_id = board
            .sections
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_else(create_id);
        let apps_section_id = create_id();

        let sections_in = vec![
            boards::SaveBoardSectionInput {
                id: root_section_id.clone(),
                kind: "empty".to_string(),
                x_offset: Some(0),
                y_offset: Some(0),
                name: None,
                options: json!({}),
                layouts: Vec::<SectionLayoutRecord>::new(),
            },
            boards::SaveBoardSectionInput {
                id: apps_section_id.clone(),
                kind: "category".to_string(),
                x_offset: Some(0),
                y_offset: Some(1),
                name: Some("Apps & Services".to_string()),
                options: json!({}),
                layouts: Vec::<SectionLayoutRecord>::new(),
            },
        ];

        let mk_layouts =
            |sec_id: &str, item_id: &str, x: i32, y: i32, w: i32, h: i32, mob_y: i32| {
                vec![
                    ItemLayoutRecord {
                        item_id: item_id.to_string(),
                        section_id: sec_id.to_string(),
                        layout_id: desktop_id.clone(),
                        x_offset: x,
                        y_offset: y,
                        width: w,
                        height: h,
                    },
                    ItemLayoutRecord {
                        item_id: item_id.to_string(),
                        section_id: sec_id.to_string(),
                        layout_id: mobile_id.clone(),
                        x_offset: 0,
                        y_offset: mob_y,
                        width: 1,
                        height: h,
                    },
                ]
            };

        let i_clock = create_id();
        let i_weather = create_id();
        let i_dns = create_id();
        let i_downloads = create_id();
        let i_media = create_id();
        let i_health = create_id();

        let i_app_docs = create_id();
        let i_app_sonarr = create_id();
        let i_app_radarr = create_id();
        let i_app_nextcloud = create_id();
        let i_app_truenas = create_id();
        let i_app_pihole = create_id();
        let i_calendar = create_id();
        let i_bookmarks = create_id();
        let i_smart_home = create_id();

        let app_tile = |id: String, app_id: &str, x: i32, mob_y: i32| boards::SaveBoardItemInput {
            id: id.clone(),
            kind: "app".to_string(),
            options: json!({
                "appId": app_id,
                "openInNewTab": true,
                "showTitle": true,
                "pingEnabled": true
            }),
            advanced_options: json!({}),
            layouts: mk_layouts(&apps_section_id, &id, x, 0, 2, 1, mob_y),
            integration_ids: vec![],
        };

        let items_in = vec![
            // Section 1: Main telemetry canvas (12 columns)
            boards::SaveBoardItemInput {
                id: i_clock.clone(),
                kind: "clock".to_string(),
                options: json!({ "is24HourFormat": true, "showDate": true, "customTitle": "Homelab Time" }),
                advanced_options: json!({}),
                layouts: mk_layouts(&root_section_id, &i_clock, 0, 0, 3, 2, 0),
                integration_ids: vec![],
            },
            boards::SaveBoardItemInput {
                id: i_weather.clone(),
                kind: "weather".to_string(),
                options: json!({
                    "location": { "name": "Zurich", "latitude": 47.3769, "longitude": 8.5417 },
                    "isFormatFahrenheit": false
                }),
                advanced_options: json!({}),
                layouts: mk_layouts(&root_section_id, &i_weather, 3, 0, 3, 2, 2),
                integration_ids: vec![],
            },
            boards::SaveBoardItemInput {
                id: i_dns.clone(),
                kind: "dnsHoleSummary".to_string(),
                options: json!({ "layout": "grid", "usePiHoleColors": true }),
                advanced_options: json!({}),
                layouts: mk_layouts(&root_section_id, &i_dns, 6, 0, 6, 2, 4),
                integration_ids: vec![mock_integration.id.clone()],
            },
            boards::SaveBoardItemInput {
                id: i_downloads.clone(),
                kind: "downloads".to_string(),
                options: json!({ "showCompleted": true }),
                advanced_options: json!({}),
                layouts: mk_layouts(&root_section_id, &i_downloads, 0, 2, 4, 2, 6),
                integration_ids: vec![mock_integration.id.clone()],
            },
            boards::SaveBoardItemInput {
                id: i_media.clone(),
                kind: "mediaServer".to_string(),
                options: json!({}),
                advanced_options: json!({}),
                layouts: mk_layouts(&root_section_id, &i_media, 4, 2, 4, 2, 8),
                integration_ids: vec![mock_integration.id.clone()],
            },
            boards::SaveBoardItemInput {
                id: i_health.clone(),
                kind: "healthMonitoring".to_string(),
                options: json!({}),
                advanced_options: json!({}),
                layouts: mk_layouts(&root_section_id, &i_health, 8, 2, 4, 2, 10),
                integration_ids: vec![mock_integration.id.clone()],
            },
            // Section 2: Collapsible Category "Apps & Services"
            app_tile(i_app_docs, &docs_app.id, 0, 0),
            app_tile(i_app_sonarr, &sonarr_app.id, 2, 1),
            app_tile(i_app_radarr, &radarr_app.id, 4, 2),
            app_tile(i_app_nextcloud, &nextcloud_app.id, 6, 3),
            app_tile(i_app_truenas, &truenas_app.id, 8, 4),
            app_tile(i_app_pihole, &pihole_app.id, 10, 5),
            boards::SaveBoardItemInput {
                id: i_calendar.clone(),
                kind: "calendar".to_string(),
                options: json!({}),
                advanced_options: json!({}),
                layouts: mk_layouts(&apps_section_id, &i_calendar, 0, 1, 4, 2, 6),
                integration_ids: vec![mock_integration.id.clone()],
            },
            boards::SaveBoardItemInput {
                id: i_bookmarks.clone(),
                kind: "bookmarks".to_string(),
                options: json!({
                    "title": "Homarr Resources",
                    "layout": "grid",
                    "openNewTab": true,
                    "items": [
                        { "name": "Homarr Docs", "href": "https://homarr.dev/docs/getting-started", "iconUrl": "/logo/homarr.svg" },
                        { "name": "GitHub Repository", "href": "https://github.com/homarr-labs/homarr", "iconUrl": "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons@master/svg/github.svg" },
                        { "name": "Dashboard Icons", "href": "https://dashboardicons.com", "iconUrl": "/images/apps/imdb.svg" },
                        { "name": "OpenAPI Reference", "href": "/reference/api", "iconUrl": "/logo/homarr.svg" }
                    ]
                }),
                advanced_options: json!({}),
                layouts: mk_layouts(&apps_section_id, &i_bookmarks, 4, 1, 4, 2, 8),
                integration_ids: vec![],
            },
            boards::SaveBoardItemInput {
                id: i_smart_home.clone(),
                kind: "smartHome-controls".to_string(),
                options: json!({}),
                advanced_options: json!({}),
                layouts: mk_layouts(&apps_section_id, &i_smart_home, 8, 1, 4, 2, 10),
                integration_ids: vec![mock_integration.id.clone()],
            },
        ];

        boards::save_board_content(db, &board.board.id, &sections_in, &items_in).await?;

        let _ = users::update_preferences(
            db,
            &demo_user.id,
            users::UpdateUserPreferencesInput {
                home_board_id: Some(Some(board.board.id.clone())),
                mobile_home_board_id: Some(Some(board.board.id.clone())),
                ..Default::default()
            },
        )
        .await?;

        let mut board_settings = settings::get_setting(db, "board").await?;
        if let Some(obj) = board_settings.as_object_mut() {
            obj.insert("homeBoardId".to_string(), json!(board.board.id));
            obj.insert("mobileHomeBoardId".to_string(), json!(board.board.id));
        }
        settings::set_setting(db, "board", &board_settings).await?;
    }

    settings::set_onboarding(db, "finish", Some("setup")).await?;
    Ok(())
}

async fn ensure_demo_app(
    db: &Database,
    existing: &[AppRecord],
    name: &str,
    description: &str,
    icon_url: &str,
    href: &str,
    ping_url: Option<&str>,
) -> HomarrResult<AppRecord> {
    if let Some(found) = existing.iter().find(|a| a.name == name) {
        return Ok(found.clone());
    }
    apps::create(
        db,
        apps::AppInput {
            id: None,
            name: name.to_string(),
            description: Some(description.to_string()),
            icon_url: icon_url.to_string(),
            href: Some(href.to_string()),
            ping_url: ping_url.map(str::to_string),
        },
    )
    .await
}
