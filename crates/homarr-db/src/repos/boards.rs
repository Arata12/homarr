use std::collections::HashMap;

use homarr_common::grid_algorithm::{GridAlgorithmItem, GridItemType};
use homarr_common::{
    create_id, deserialize_superjson, serialize_superjson, HomarrError, HomarrResult,
};
use homarr_definitions::board::{normalize_board_layout_roles, BoardLayoutNormInput, LayoutRole};
use homarr_definitions::section::{
    get_board_lane_column_count, project_root_sections, LaneGeometry, RootSectionLike,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{postgres::PgRow, sqlite::SqliteRow, Row};

use crate::models::{
    BoardRecord, FullBoard, GroupPermissionEntry, ItemLayoutRecord, ItemRecord, LayoutRecord,
    SectionLayoutRecord, SectionRecord, UserPermissionEntry,
};
use crate::pool::map_sqlx_err;
use crate::Database;

fn map_sqlite_board(row: &SqliteRow) -> Result<BoardRecord, sqlx::Error> {
    Ok(BoardRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        is_public: row.try_get("is_public")?,
        creator_id: row.try_get("creator_id")?,
        page_title: row.try_get("page_title")?,
        meta_title: row.try_get("meta_title")?,
        logo_image_url: row.try_get("logo_image_url")?,
        favicon_image_url: row.try_get("favicon_image_url")?,
        background_image_url: row.try_get("background_image_url")?,
        background_image_attachment: row.try_get("background_image_attachment")?,
        background_image_repeat: row.try_get("background_image_repeat")?,
        background_image_size: row.try_get("background_image_size")?,
        primary_color: row.try_get("primary_color")?,
        secondary_color: row.try_get("secondary_color")?,
        opacity: row.try_get("opacity")?,
        custom_css: row.try_get("custom_css")?,
        icon_color: row.try_get("icon_color")?,
        item_radius: row.try_get("item_radius")?,
        disable_status: row.try_get("disable_status")?,
    })
}

fn map_pg_board(row: &PgRow) -> Result<BoardRecord, sqlx::Error> {
    Ok(BoardRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        is_public: row.try_get("is_public")?,
        creator_id: row.try_get("creator_id")?,
        page_title: row.try_get("page_title")?,
        meta_title: row.try_get("meta_title")?,
        logo_image_url: row.try_get("logo_image_url")?,
        favicon_image_url: row.try_get("favicon_image_url")?,
        background_image_url: row.try_get("background_image_url")?,
        background_image_attachment: row.try_get("background_image_attachment")?,
        background_image_repeat: row.try_get("background_image_repeat")?,
        background_image_size: row.try_get("background_image_size")?,
        primary_color: row.try_get("primary_color")?,
        secondary_color: row.try_get("secondary_color")?,
        opacity: row.try_get("opacity")?,
        custom_css: row.try_get("custom_css")?,
        icon_color: row.try_get("icon_color")?,
        item_radius: row.try_get("item_radius")?,
        disable_status: row.try_get("disable_status")?,
    })
}

fn map_sqlite_layout(row: &SqliteRow) -> Result<LayoutRecord, sqlx::Error> {
    Ok(LayoutRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        board_id: row.try_get("board_id")?,
        column_count: row.try_get("column_count")?,
        left_gutter_column_count: row.try_get("left_gutter_column_count")?,
        right_gutter_column_count: row.try_get("right_gutter_column_count")?,
        breakpoint: row.try_get("breakpoint")?,
        role: row.try_get("role")?,
        resize_source: row.try_get("resize_source")?,
    })
}

fn map_pg_layout(row: &PgRow) -> Result<LayoutRecord, sqlx::Error> {
    let column_count: i16 = row.try_get("column_count")?;
    let left: i16 = row.try_get("left_gutter_column_count")?;
    let right: i16 = row.try_get("right_gutter_column_count")?;
    let breakpoint: i16 = row.try_get("breakpoint")?;
    Ok(LayoutRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        board_id: row.try_get("board_id")?,
        column_count: i32::from(column_count),
        left_gutter_column_count: i32::from(left),
        right_gutter_column_count: i32::from(right),
        breakpoint: i32::from(breakpoint),
        role: row.try_get("role")?,
        resize_source: row.try_get("resize_source")?,
    })
}

pub async fn list_all(db: &Database) -> HomarrResult<Vec<BoardRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "board" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_sqlite_board)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "board" ORDER BY lower("name") ASC"#)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_pg_board)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_by_id(db: &Database, id: &str) -> HomarrResult<Option<BoardRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "board" WHERE "id" = ?"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_sqlite_board)
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "board" WHERE "id" = $1"#)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_pg_board)
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_by_name(db: &Database, name: &str) -> HomarrResult<Option<BoardRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "board" WHERE "name" = ?"#)
                .bind(name)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_sqlite_board)
                .transpose()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let row = sqlx::query(r#"SELECT * FROM "board" WHERE "name" = $1"#)
                .bind(name)
                .fetch_optional(pool)
                .await
                .map_err(map_sqlx_err)?;
            row.as_ref()
                .map(map_pg_board)
                .transpose()
                .map_err(map_sqlx_err)
        }
    }
}

pub async fn get_full_board_by_id(
    db: &Database,
    board_id: &str,
) -> HomarrResult<Option<FullBoard>> {
    let Some(board) = get_by_id(db, board_id).await? else {
        return Ok(None);
    };
    load_full_board(db, board).await.map(Some)
}

pub async fn get_full_board_by_name(db: &Database, name: &str) -> HomarrResult<Option<FullBoard>> {
    let Some(board) = get_by_name(db, name).await? else {
        return Ok(None);
    };
    load_full_board(db, board).await.map(Some)
}

async fn load_full_board(db: &Database, board: BoardRecord) -> HomarrResult<FullBoard> {
    let board_id = &board.id;
    let layouts = match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "layout" WHERE "board_id" = ? ORDER BY "breakpoint" ASC"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_sqlite_layout)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT * FROM "layout" WHERE "board_id" = $1 ORDER BY "breakpoint" ASC"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(map_pg_layout)
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
    };

    let section_layouts = load_section_layouts_for_board(db, board_id).await?;
    let mut sl_by_section: HashMap<String, Vec<SectionLayoutRecord>> = HashMap::new();
    for sl in section_layouts {
        sl_by_section
            .entry(sl.section_id.clone())
            .or_default()
            .push(sl);
    }

    let sections = match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "section" WHERE "board_id" = ?"#)
                .bind(board_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let id: String = r.try_get("id")?;
                    let opts_raw: Option<String> = r.try_get("options")?;
                    let options = opts_raw
                        .as_deref()
                        .and_then(|s| deserialize_superjson::<Value>(s).ok())
                        .unwrap_or_else(|| Value::Object(Default::default()));
                    Ok(SectionRecord {
                        layouts: sl_by_section.remove(&id).unwrap_or_default(),
                        id,
                        board_id: r.try_get("board_id")?,
                        kind: r.try_get("kind")?,
                        x_offset: r.try_get("x_offset")?,
                        y_offset: r.try_get("y_offset")?,
                        name: r.try_get("name")?,
                        options,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "section" WHERE "board_id" = $1"#)
                .bind(board_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let id: String = r.try_get("id")?;
                    let opts_raw: Option<String> = r.try_get("options")?;
                    let options = opts_raw
                        .as_deref()
                        .and_then(|s| deserialize_superjson::<Value>(s).ok())
                        .unwrap_or_else(|| Value::Object(Default::default()));
                    Ok(SectionRecord {
                        layouts: sl_by_section.remove(&id).unwrap_or_default(),
                        id,
                        board_id: r.try_get("board_id")?,
                        kind: r.try_get("kind")?,
                        x_offset: r.try_get("x_offset")?,
                        y_offset: r.try_get("y_offset")?,
                        name: r.try_get("name")?,
                        options,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
    };

    let item_layouts = load_item_layouts_for_board(db, board_id).await?;
    let mut il_by_item: HashMap<String, Vec<ItemLayoutRecord>> = HashMap::new();
    for il in item_layouts {
        il_by_item.entry(il.item_id.clone()).or_default().push(il);
    }

    let integration_links = load_item_integrations_for_board(db, board_id).await?;
    let mut int_by_item: HashMap<String, Vec<String>> = HashMap::new();
    for (item_id, integration_id) in integration_links {
        int_by_item.entry(item_id).or_default().push(integration_id);
    }

    let items = match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "item" WHERE "board_id" = ?"#)
                .bind(board_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let id: String = r.try_get("id")?;
                    let opts_raw: String = r.try_get("options")?;
                    let adv_raw: String = r.try_get("advanced_options")?;
                    Ok(ItemRecord {
                        layouts: il_by_item.remove(&id).unwrap_or_default(),
                        integration_ids: int_by_item.remove(&id).unwrap_or_default(),
                        id,
                        board_id: r.try_get("board_id")?,
                        kind: r.try_get("kind")?,
                        options: deserialize_superjson::<Value>(&opts_raw)
                            .unwrap_or_else(|_| Value::Object(Default::default())),
                        advanced_options: deserialize_superjson::<Value>(&adv_raw)
                            .unwrap_or_else(|_| Value::Object(Default::default())),
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(r#"SELECT * FROM "item" WHERE "board_id" = $1"#)
                .bind(board_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    let id: String = r.try_get("id")?;
                    let opts_raw: String = r.try_get("options")?;
                    let adv_raw: String = r.try_get("advanced_options")?;
                    Ok(ItemRecord {
                        layouts: il_by_item.remove(&id).unwrap_or_default(),
                        integration_ids: int_by_item.remove(&id).unwrap_or_default(),
                        id,
                        board_id: r.try_get("board_id")?,
                        kind: r.try_get("kind")?,
                        options: deserialize_superjson::<Value>(&opts_raw)
                            .unwrap_or_else(|_| Value::Object(Default::default())),
                        advanced_options: deserialize_superjson::<Value>(&adv_raw)
                            .unwrap_or_else(|_| Value::Object(Default::default())),
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)?
        }
    };

    let (user_permissions, group_permissions) = get_permissions(db, board_id).await?;

    Ok(FullBoard {
        board,
        layouts,
        sections,
        items,
        user_permissions,
        group_permissions,
    })
}

async fn load_section_layouts_for_board(
    db: &Database,
    board_id: &str,
) -> HomarrResult<Vec<SectionLayoutRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT sl.* FROM "section_layout" sl
                   INNER JOIN "section" s ON s."id" = sl."section_id"
                   WHERE s."board_id" = ?"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(SectionLayoutRecord {
                        section_id: r.try_get("section_id")?,
                        layout_id: r.try_get("layout_id")?,
                        parent_section_id: r.try_get("parent_section_id")?,
                        x_offset: r.try_get("x_offset")?,
                        y_offset: r.try_get("y_offset")?,
                        width: r.try_get("width")?,
                        height: r.try_get("height")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT sl.* FROM "section_layout" sl
                   INNER JOIN "section" s ON s."id" = sl."section_id"
                   WHERE s."board_id" = $1"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(SectionLayoutRecord {
                        section_id: r.try_get("section_id")?,
                        layout_id: r.try_get("layout_id")?,
                        parent_section_id: r.try_get("parent_section_id")?,
                        x_offset: r.try_get("x_offset")?,
                        y_offset: r.try_get("y_offset")?,
                        width: r.try_get("width")?,
                        height: r.try_get("height")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

async fn load_item_layouts_for_board(
    db: &Database,
    board_id: &str,
) -> HomarrResult<Vec<ItemLayoutRecord>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT il.* FROM "item_layout" il
                   INNER JOIN "item" i ON i."id" = il."item_id"
                   WHERE i."board_id" = ?"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(ItemLayoutRecord {
                        item_id: r.try_get("item_id")?,
                        section_id: r.try_get("section_id")?,
                        layout_id: r.try_get("layout_id")?,
                        x_offset: r.try_get("x_offset")?,
                        y_offset: r.try_get("y_offset")?,
                        width: r.try_get("width")?,
                        height: r.try_get("height")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT il.* FROM "item_layout" il
                   INNER JOIN "item" i ON i."id" = il."item_id"
                   WHERE i."board_id" = $1"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| {
                    Ok(ItemLayoutRecord {
                        item_id: r.try_get("item_id")?,
                        section_id: r.try_get("section_id")?,
                        layout_id: r.try_get("layout_id")?,
                        x_offset: r.try_get("x_offset")?,
                        y_offset: r.try_get("y_offset")?,
                        width: r.try_get("width")?,
                        height: r.try_get("height")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)
        }
    }
}

async fn load_item_integrations_for_board(
    db: &Database,
    board_id: &str,
) -> HomarrResult<Vec<(String, String)>> {
    match db {
        Database::Sqlite(pool) => {
            let rows = sqlx::query(
                r#"SELECT ii."item_id", ii."integration_id" FROM "integration_item" ii
                   INNER JOIN "item" i ON i."id" = ii."item_id"
                   WHERE i."board_id" = ?"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("item_id")?, r.try_get("integration_id")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)
        }
        Database::Postgres(pool) => {
            let rows = sqlx::query(
                r#"SELECT ii."item_id", ii."integration_id" FROM "integration_item" ii
                   INNER JOIN "item" i ON i."id" = ii."item_id"
                   WHERE i."board_id" = $1"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            rows.iter()
                .map(|r| Ok((r.try_get("item_id")?, r.try_get("integration_id")?)))
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(map_sqlx_err)
        }
    }
}

pub struct CreateBoardInput {
    pub id: Option<String>,
    pub name: String,
    pub is_public: bool,
    pub creator_id: Option<String>,
    pub column_count: i32,
}

pub async fn create_board_with_defaults(
    db: &Database,
    input: CreateBoardInput,
) -> HomarrResult<FullBoard> {
    let board_id = input.id.unwrap_or_else(create_id);
    let mobile_layout_id = create_id();
    let base_layout_id = create_id();
    let root_section_id = create_id();
    let cols = input.column_count.clamp(1, 24);

    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(
                r#"INSERT INTO "board" ("id", "name", "is_public", "creator_id") VALUES (?, ?, ?, ?)"#,
            )
            .bind(&board_id)
            .bind(&input.name)
            .bind(input.is_public)
            .bind(&input.creator_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"INSERT INTO "layout" ("id", "name", "board_id", "column_count", "breakpoint", "role")
                   VALUES (?, 'Mobile', ?, 1, 0, 'mobile')"#,
            )
            .bind(&mobile_layout_id)
            .bind(&board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"INSERT INTO "layout" ("id", "name", "board_id", "column_count", "breakpoint", "role")
                   VALUES (?, 'Desktop', ?, ?, 1280, 'base')"#,
            )
            .bind(&base_layout_id)
            .bind(&board_id)
            .bind(cols)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"INSERT INTO "section" ("id", "board_id", "kind", "x_offset", "y_offset", "name")
                   VALUES (?, ?, 'empty', 0, 0, 'Main')"#,
            )
            .bind(&root_section_id)
            .bind(&board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(
                r#"INSERT INTO "board" ("id", "name", "is_public", "creator_id") VALUES ($1, $2, $3, $4)"#,
            )
            .bind(&board_id)
            .bind(&input.name)
            .bind(input.is_public)
            .bind(&input.creator_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"INSERT INTO "layout" ("id", "name", "board_id", "column_count", "breakpoint", "role")
                   VALUES ($1, 'Mobile', $2, 1, 0, 'mobile')"#,
            )
            .bind(&mobile_layout_id)
            .bind(&board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"INSERT INTO "layout" ("id", "name", "board_id", "column_count", "breakpoint", "role")
                   VALUES ($1, 'Desktop', $2, $3, 1280, 'base')"#,
            )
            .bind(&base_layout_id)
            .bind(&board_id)
            .bind(cols as i16)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"INSERT INTO "section" ("id", "board_id", "kind", "x_offset", "y_offset", "name")
                   VALUES ($1, $2, 'empty', 0, 0, 'Main')"#,
            )
            .bind(&root_section_id)
            .bind(&board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }

    get_full_board_by_id(db, &board_id)
        .await?
        .ok_or_else(|| HomarrError::Internal("Created board missing".to_string()))
}

pub async fn rename_board(db: &Database, board_id: &str, name: &str) -> HomarrResult<()> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"UPDATE "board" SET "name" = ? WHERE "id" = ?"#)
            .bind(name)
            .bind(board_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => {
            sqlx::query(r#"UPDATE "board" SET "name" = $1 WHERE "id" = $2"#)
                .bind(name)
                .bind(board_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected()
        }
    };
    if affected == 0 {
        return Err(HomarrError::NotFound(format!("Board {board_id} not found")));
    }
    Ok(())
}

pub async fn set_visibility(db: &Database, board_id: &str, is_public: bool) -> HomarrResult<()> {
    let affected = match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"UPDATE "board" SET "is_public" = ? WHERE "id" = ?"#)
                .bind(is_public)
                .bind(board_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected()
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"UPDATE "board" SET "is_public" = $1 WHERE "id" = $2"#)
                .bind(is_public)
                .bind(board_id)
                .execute(pool)
                .await
                .map_err(map_sqlx_err)?
                .rows_affected()
        }
    };
    if affected == 0 {
        return Err(HomarrError::NotFound(format!("Board {board_id} not found")));
    }
    Ok(())
}

/// Distinguishes an absent field (`None`) from an explicit `null` (`Some(None)`) so that
/// board settings can be cleared through PATCH requests.
fn double_option<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSettingsPatch {
    #[serde(default, deserialize_with = "double_option")]
    pub page_title: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub meta_title: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub logo_image_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub favicon_image_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub background_image_url: Option<Option<String>>,
    pub background_image_attachment: Option<String>,
    pub background_image_repeat: Option<String>,
    pub background_image_size: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub primary_color: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub secondary_color: Option<Option<String>>,
    pub opacity: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub custom_css: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub icon_color: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub item_radius: Option<Option<String>>,
    pub disable_status: Option<bool>,
}

pub async fn update_settings(
    db: &Database,
    board_id: &str,
    patch: BoardSettingsPatch,
) -> HomarrResult<BoardRecord> {
    let cur = get_by_id(db, board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound(format!("Board {board_id} not found")))?;

    let page_title = patch.page_title.unwrap_or(cur.page_title);
    let meta_title = patch.meta_title.unwrap_or(cur.meta_title);
    let logo_image_url = patch.logo_image_url.unwrap_or(cur.logo_image_url);
    let favicon_image_url = patch.favicon_image_url.unwrap_or(cur.favicon_image_url);
    let background_image_url = patch
        .background_image_url
        .unwrap_or(cur.background_image_url);
    let background_image_attachment = patch
        .background_image_attachment
        .unwrap_or(cur.background_image_attachment);
    let background_image_repeat = patch
        .background_image_repeat
        .unwrap_or(cur.background_image_repeat);
    let background_image_size = patch
        .background_image_size
        .unwrap_or(cur.background_image_size);
    let primary_color = patch.primary_color.unwrap_or(cur.primary_color);
    let secondary_color = patch.secondary_color.unwrap_or(cur.secondary_color);
    let opacity = patch.opacity.unwrap_or(cur.opacity).clamp(0, 100);
    let custom_css = patch.custom_css.unwrap_or(cur.custom_css);
    let icon_color = patch.icon_color.unwrap_or(cur.icon_color);
    let item_radius = patch.item_radius.unwrap_or(cur.item_radius);
    let disable_status = patch.disable_status.unwrap_or(cur.disable_status);

    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"UPDATE "board" SET
                   "page_title" = ?, "meta_title" = ?, "logo_image_url" = ?, "favicon_image_url" = ?,
                   "background_image_url" = ?, "background_image_attachment" = ?, "background_image_repeat" = ?,
                   "background_image_size" = ?, "primary_color" = ?, "secondary_color" = ?,
                   "opacity" = ?, "custom_css" = ?, "icon_color" = ?, "item_radius" = ?, "disable_status" = ?
                   WHERE "id" = ?"#,
            )
            .bind(&page_title)
            .bind(&meta_title)
            .bind(&logo_image_url)
            .bind(&favicon_image_url)
            .bind(&background_image_url)
            .bind(&background_image_attachment)
            .bind(&background_image_repeat)
            .bind(&background_image_size)
            .bind(&primary_color)
            .bind(&secondary_color)
            .bind(opacity)
            .bind(&custom_css)
            .bind(&icon_color)
            .bind(&item_radius)
            .bind(disable_status)
            .bind(board_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"UPDATE "board" SET
                   "page_title" = $1, "meta_title" = $2, "logo_image_url" = $3, "favicon_image_url" = $4,
                   "background_image_url" = $5, "background_image_attachment" = $6, "background_image_repeat" = $7,
                   "background_image_size" = $8, "primary_color" = $9, "secondary_color" = $10,
                   "opacity" = $11, "custom_css" = $12, "icon_color" = $13, "item_radius" = $14, "disable_status" = $15
                   WHERE "id" = $16"#,
            )
            .bind(&page_title)
            .bind(&meta_title)
            .bind(&logo_image_url)
            .bind(&favicon_image_url)
            .bind(&background_image_url)
            .bind(&background_image_attachment)
            .bind(&background_image_repeat)
            .bind(&background_image_size)
            .bind(&primary_color)
            .bind(&secondary_color)
            .bind(opacity)
            .bind(&custom_css)
            .bind(&icon_color)
            .bind(&item_radius)
            .bind(disable_status)
            .bind(board_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }

    get_by_id(db, board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board not found".to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBoardSectionInput {
    pub id: String,
    pub kind: String,
    pub x_offset: Option<i32>,
    pub y_offset: Option<i32>,
    pub name: Option<String>,
    #[serde(default)]
    pub options: Value,
    #[serde(default)]
    pub layouts: Vec<SectionLayoutRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveBoardItemInput {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub options: Value,
    #[serde(default)]
    pub advanced_options: Value,
    #[serde(default)]
    pub layouts: Vec<ItemLayoutRecord>,
    #[serde(default)]
    pub integration_ids: Vec<String>,
}

pub async fn save_board_content(
    db: &Database,
    board_id: &str,
    sections: &[SaveBoardSectionInput],
    items: &[SaveBoardItemInput],
) -> HomarrResult<FullBoard> {
    if get_by_id(db, board_id).await?.is_none() {
        return Err(HomarrError::NotFound(format!("Board {board_id} not found")));
    }

    let new_section_ids: std::collections::HashSet<&str> =
        sections.iter().map(|s| s.id.as_str()).collect();
    let new_item_ids: std::collections::HashSet<&str> =
        items.iter().map(|i| i.id.as_str()).collect();

    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;

            let old_item_ids: Vec<String> =
                sqlx::query_scalar(r#"SELECT "id" FROM "item" WHERE "board_id" = ?"#)
                    .bind(board_id)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
            let old_section_ids: Vec<String> =
                sqlx::query_scalar(r#"SELECT "id" FROM "section" WHERE "board_id" = ?"#)
                    .bind(board_id)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"DELETE FROM "item_layout" WHERE "item_id" IN (SELECT "id" FROM "item" WHERE "board_id" = ?)"#,
            )
            .bind(board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            sqlx::query(
                r#"DELETE FROM "integration_item" WHERE "item_id" IN (SELECT "id" FROM "item" WHERE "board_id" = ?)"#,
            )
            .bind(board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            sqlx::query(
                r#"DELETE FROM "section_layout" WHERE "section_id" IN (SELECT "id" FROM "section" WHERE "board_id" = ?)"#,
            )
            .bind(board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            // Delete only items and sections actually removed so kept rows preserve
            // `widget_secret`, `user_item_state`, and `section_collapse_state`.
            for old_id in &old_item_ids {
                if !new_item_ids.contains(old_id.as_str()) {
                    sqlx::query(r#"DELETE FROM "item" WHERE "id" = ? AND "board_id" = ?"#)
                        .bind(old_id)
                        .bind(board_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx_err)?;
                }
            }
            for old_id in &old_section_ids {
                if !new_section_ids.contains(old_id.as_str()) {
                    sqlx::query(r#"DELETE FROM "section" WHERE "id" = ? AND "board_id" = ?"#)
                        .bind(old_id)
                        .bind(board_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx_err)?;
                }
            }

            for s in sections {
                let opts = serialize_superjson(&s.options)?;
                let res = sqlx::query(
                    r#"INSERT INTO "section" ("id", "board_id", "kind", "x_offset", "y_offset", "name", "options")
                       VALUES (?, ?, ?, ?, ?, ?, ?)
                       ON CONFLICT ("id") DO UPDATE SET
                         "kind" = excluded."kind",
                         "x_offset" = excluded."x_offset",
                         "y_offset" = excluded."y_offset",
                         "name" = excluded."name",
                         "options" = excluded."options"
                       WHERE "section"."board_id" = excluded."board_id""#,
                )
                .bind(&s.id)
                .bind(board_id)
                .bind(&s.kind)
                .bind(s.x_offset)
                .bind(s.y_offset)
                .bind(&s.name)
                .bind(&opts)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                if res.rows_affected() != 1 {
                    return Err(HomarrError::Conflict(format!(
                        "Section '{}' belongs to a different board",
                        s.id
                    )));
                }
            }

            for s in sections {
                for l in &s.layouts {
                    sqlx::query(
                        r#"INSERT INTO "section_layout" ("section_id", "layout_id", "parent_section_id", "x_offset", "y_offset", "width", "height")
                           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                    )
                    .bind(&s.id)
                    .bind(&l.layout_id)
                    .bind(&l.parent_section_id)
                    .bind(l.x_offset)
                    .bind(l.y_offset)
                    .bind(l.width)
                    .bind(l.height)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }
            }

            for item in items {
                let opts = serialize_superjson(&item.options)?;
                let adv = serialize_superjson(&item.advanced_options)?;
                let res = sqlx::query(
                    r#"INSERT INTO "item" ("id", "board_id", "kind", "options", "advanced_options")
                       VALUES (?, ?, ?, ?, ?)
                       ON CONFLICT ("id") DO UPDATE SET
                         "kind" = excluded."kind",
                         "options" = excluded."options",
                         "advanced_options" = excluded."advanced_options"
                       WHERE "item"."board_id" = excluded."board_id""#,
                )
                .bind(&item.id)
                .bind(board_id)
                .bind(&item.kind)
                .bind(&opts)
                .bind(&adv)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                if res.rows_affected() != 1 {
                    return Err(HomarrError::Conflict(format!(
                        "Item '{}' belongs to a different board",
                        item.id
                    )));
                }

                for l in &item.layouts {
                    sqlx::query(
                        r#"INSERT INTO "item_layout" ("item_id", "section_id", "layout_id", "x_offset", "y_offset", "width", "height")
                           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                    )
                    .bind(&item.id)
                    .bind(&l.section_id)
                    .bind(&l.layout_id)
                    .bind(l.x_offset)
                    .bind(l.y_offset)
                    .bind(l.width)
                    .bind(l.height)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }

                for iid in &item.integration_ids {
                    sqlx::query(
                        r#"INSERT INTO "integration_item" ("item_id", "integration_id") VALUES (?, ?)"#,
                    )
                    .bind(&item.id)
                    .bind(iid)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }
            }

            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;

            let old_item_ids: Vec<String> =
                sqlx::query_scalar(r#"SELECT "id" FROM "item" WHERE "board_id" = $1"#)
                    .bind(board_id)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
            let old_section_ids: Vec<String> =
                sqlx::query_scalar(r#"SELECT "id" FROM "section" WHERE "board_id" = $1"#)
                    .bind(board_id)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;

            sqlx::query(
                r#"DELETE FROM "item_layout" WHERE "item_id" IN (SELECT "id" FROM "item" WHERE "board_id" = $1)"#,
            )
            .bind(board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            sqlx::query(
                r#"DELETE FROM "integration_item" WHERE "item_id" IN (SELECT "id" FROM "item" WHERE "board_id" = $1)"#,
            )
            .bind(board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;
            sqlx::query(
                r#"DELETE FROM "section_layout" WHERE "section_id" IN (SELECT "id" FROM "section" WHERE "board_id" = $1)"#,
            )
            .bind(board_id)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            for old_id in &old_item_ids {
                if !new_item_ids.contains(old_id.as_str()) {
                    sqlx::query(r#"DELETE FROM "item" WHERE "id" = $1 AND "board_id" = $2"#)
                        .bind(old_id)
                        .bind(board_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx_err)?;
                }
            }
            for old_id in &old_section_ids {
                if !new_section_ids.contains(old_id.as_str()) {
                    sqlx::query(r#"DELETE FROM "section" WHERE "id" = $1 AND "board_id" = $2"#)
                        .bind(old_id)
                        .bind(board_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx_err)?;
                }
            }

            for s in sections {
                let opts = serialize_superjson(&s.options)?;
                let res = sqlx::query(
                    r#"INSERT INTO "section" ("id", "board_id", "kind", "x_offset", "y_offset", "name", "options")
                       VALUES ($1, $2, $3, $4, $5, $6, $7)
                       ON CONFLICT ("id") DO UPDATE SET
                         "kind" = excluded."kind",
                         "x_offset" = excluded."x_offset",
                         "y_offset" = excluded."y_offset",
                         "name" = excluded."name",
                         "options" = excluded."options"
                       WHERE "section"."board_id" = excluded."board_id""#,
                )
                .bind(&s.id)
                .bind(board_id)
                .bind(&s.kind)
                .bind(s.x_offset)
                .bind(s.y_offset)
                .bind(&s.name)
                .bind(&opts)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                if res.rows_affected() != 1 {
                    return Err(HomarrError::Conflict(format!(
                        "Section '{}' belongs to a different board",
                        s.id
                    )));
                }
            }

            for s in sections {
                for l in &s.layouts {
                    sqlx::query(
                        r#"INSERT INTO "section_layout" ("section_id", "layout_id", "parent_section_id", "x_offset", "y_offset", "width", "height")
                           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                    )
                    .bind(&s.id)
                    .bind(&l.layout_id)
                    .bind(&l.parent_section_id)
                    .bind(l.x_offset)
                    .bind(l.y_offset)
                    .bind(l.width)
                    .bind(l.height)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }
            }

            for item in items {
                let opts = serialize_superjson(&item.options)?;
                let adv = serialize_superjson(&item.advanced_options)?;
                let res = sqlx::query(
                    r#"INSERT INTO "item" ("id", "board_id", "kind", "options", "advanced_options")
                       VALUES ($1, $2, $3, $4, $5)
                       ON CONFLICT ("id") DO UPDATE SET
                         "kind" = excluded."kind",
                         "options" = excluded."options",
                         "advanced_options" = excluded."advanced_options"
                       WHERE "item"."board_id" = excluded."board_id""#,
                )
                .bind(&item.id)
                .bind(board_id)
                .bind(&item.kind)
                .bind(&opts)
                .bind(&adv)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                if res.rows_affected() != 1 {
                    return Err(HomarrError::Conflict(format!(
                        "Item '{}' belongs to a different board",
                        item.id
                    )));
                }

                for l in &item.layouts {
                    sqlx::query(
                        r#"INSERT INTO "item_layout" ("item_id", "section_id", "layout_id", "x_offset", "y_offset", "width", "height")
                           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                    )
                    .bind(&item.id)
                    .bind(&l.section_id)
                    .bind(&l.layout_id)
                    .bind(l.x_offset)
                    .bind(l.y_offset)
                    .bind(l.width)
                    .bind(l.height)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }

                for iid in &item.integration_ids {
                    sqlx::query(
                        r#"INSERT INTO "integration_item" ("item_id", "integration_id") VALUES ($1, $2)"#,
                    )
                    .bind(&item.id)
                    .bind(iid)
                    .execute(&mut *tx)
                    .await
                    .map_err(map_sqlx_err)?;
                }
            }

            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }

    get_full_board_by_id(db, board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound("Board missing after save".to_string()))
}

pub async fn replace_layouts(
    db: &Database,
    board_id: &str,
    layouts: &[BoardLayoutNormInput],
) -> HomarrResult<Vec<LayoutRecord>> {
    let existing_board = get_full_board_by_id(db, board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound(format!("Board {board_id} not found")))?;
    let normalized = normalize_board_layout_roles(layouts);
    let existing = existing_board.layouts;
    let incoming_ids: std::collections::HashSet<&str> =
        normalized.iter().map(|l| l.id.as_str()).collect();

    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            for old in &existing {
                if !incoming_ids.contains(old.id.as_str()) {
                    sqlx::query(r#"DELETE FROM "layout" WHERE "id" = ? AND "board_id" = ?"#)
                        .bind(&old.id)
                        .bind(board_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx_err)?;
                }
            }
            for l in &normalized {
                let role_str = l.role.unwrap_or(LayoutRole::Custom).as_str();
                let res = sqlx::query(
                    r#"INSERT INTO "layout" ("id", "name", "board_id", "column_count", "left_gutter_column_count", "right_gutter_column_count", "breakpoint", "role", "resize_source")
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                       ON CONFLICT ("id") DO UPDATE SET
                       "name" = excluded."name", "column_count" = excluded."column_count",
                       "left_gutter_column_count" = excluded."left_gutter_column_count",
                       "right_gutter_column_count" = excluded."right_gutter_column_count",
                       "breakpoint" = excluded."breakpoint", "role" = excluded."role",
                       "resize_source" = excluded."resize_source"
                       WHERE "layout"."board_id" = excluded."board_id""#,
                )
                .bind(&l.id)
                .bind(&l.name)
                .bind(board_id)
                .bind(l.column_count)
                .bind(l.left_gutter_column_count)
                .bind(l.right_gutter_column_count)
                .bind(l.breakpoint)
                .bind(role_str)
                .bind(&l.resize_source)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                if res.rows_affected() != 1 {
                    return Err(HomarrError::Conflict(format!(
                        "Layout '{}' belongs to a different board",
                        l.id
                    )));
                }
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            for old in &existing {
                if !incoming_ids.contains(old.id.as_str()) {
                    sqlx::query(r#"DELETE FROM "layout" WHERE "id" = $1 AND "board_id" = $2"#)
                        .bind(&old.id)
                        .bind(board_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_sqlx_err)?;
                }
            }
            for l in &normalized {
                let role_str = l.role.unwrap_or(LayoutRole::Custom).as_str();
                let res = sqlx::query(
                    r#"INSERT INTO "layout" ("id", "name", "board_id", "column_count", "left_gutter_column_count", "right_gutter_column_count", "breakpoint", "role", "resize_source")
                       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                       ON CONFLICT ("id") DO UPDATE SET
                       "name" = excluded."name", "column_count" = excluded."column_count",
                       "left_gutter_column_count" = excluded."left_gutter_column_count",
                       "right_gutter_column_count" = excluded."right_gutter_column_count",
                       "breakpoint" = excluded."breakpoint", "role" = excluded."role",
                       "resize_source" = excluded."resize_source"
                       WHERE "layout"."board_id" = excluded."board_id""#,
                )
                .bind(&l.id)
                .bind(&l.name)
                .bind(board_id)
                .bind(l.column_count as i16)
                .bind(l.left_gutter_column_count as i16)
                .bind(l.right_gutter_column_count as i16)
                .bind(l.breakpoint as i16)
                .bind(role_str)
                .bind(&l.resize_source)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
                if res.rows_affected() != 1 {
                    return Err(HomarrError::Conflict(format!(
                        "Layout '{}' belongs to a different board",
                        l.id
                    )));
                }
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }

    Ok(get_full_board_by_id(db, board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound(format!("Board {board_id} not found")))?
        .layouts)
}

pub async fn reset_layout(
    db: &Database,
    board_id: &str,
    target_layout_id: &str,
    source_layout_id: Option<&str>,
) -> HomarrResult<FullBoard> {
    let board = get_full_board_by_id(db, board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound(format!("Board {board_id} not found")))?;

    let target_layout = board
        .layouts
        .iter()
        .find(|l| l.id == target_layout_id)
        .ok_or_else(|| HomarrError::NotFound("Target layout not found".to_string()))?;

    let source_layout = source_layout_id
        .and_then(|sid| board.layouts.iter().find(|l| l.id == sid))
        .or_else(|| board.layouts.iter().find(|l| l.role == "base"))
        .or_else(|| board.layouts.iter().find(|l| l.id != target_layout_id))
        .unwrap_or(target_layout);

    let root_sections: Vec<RootSectionLike> = board
        .sections
        .iter()
        .map(|s| RootSectionLike {
            id: s.id.clone(),
            kind: s.kind.clone(),
            x_offset: s.x_offset,
            y_offset: s.y_offset,
        })
        .collect();

    let mut elements = Vec::new();
    for s in &board.sections {
        if let Some(sl) = s.layouts.iter().find(|l| l.layout_id == source_layout.id) {
            if let Some(ref parent) = sl.parent_section_id {
                elements.push(GridAlgorithmItem {
                    id: s.id.clone(),
                    item_type: GridItemType::Section,
                    width: sl.width,
                    height: sl.height,
                    x_offset: sl.x_offset,
                    y_offset: sl.y_offset,
                    section_id: parent.clone(),
                });
            }
        }
    }
    for item in &board.items {
        if let Some(il) = item
            .layouts
            .iter()
            .find(|l| l.layout_id == source_layout.id)
        {
            elements.push(GridAlgorithmItem {
                id: item.id.clone(),
                item_type: GridItemType::Item,
                width: il.width,
                height: il.height,
                x_offset: il.x_offset,
                y_offset: il.y_offset,
                section_id: il.section_id.clone(),
            });
        }
    }

    let source_geom = LaneGeometry {
        column_count: source_layout.column_count,
        left_gutter_column_count: source_layout.left_gutter_column_count,
        right_gutter_column_count: source_layout.right_gutter_column_count,
        role: LayoutRole::from_str_opt(&source_layout.role).unwrap_or(LayoutRole::Custom),
    };
    let target_geom = LaneGeometry {
        column_count: target_layout.column_count,
        left_gutter_column_count: target_layout.left_gutter_column_count,
        right_gutter_column_count: target_layout.right_gutter_column_count,
        role: LayoutRole::from_str_opt(&target_layout.role).unwrap_or(LayoutRole::Custom),
    };
    let _ = get_board_lane_column_count(target_geom, homarr_definitions::BoardLane::Main);

    let projected = project_root_sections(&root_sections, &elements, source_geom, target_geom)
        .unwrap_or_default();

    let mut sections_input: Vec<SaveBoardSectionInput> = board
        .sections
        .iter()
        .map(|s| {
            let mut layouts: Vec<SectionLayoutRecord> = s
                .layouts
                .iter()
                .filter(|l| l.layout_id != target_layout_id)
                .cloned()
                .collect();
            if let Some(proj) = projected
                .iter()
                .find(|p| p.item_type == GridItemType::Section && p.id == s.id)
            {
                layouts.push(SectionLayoutRecord {
                    section_id: s.id.clone(),
                    layout_id: target_layout_id.to_string(),
                    parent_section_id: Some(proj.section_id.clone()),
                    x_offset: proj.x_offset,
                    y_offset: proj.y_offset,
                    width: proj.width,
                    height: proj.height,
                });
            }
            SaveBoardSectionInput {
                id: s.id.clone(),
                kind: s.kind.clone(),
                x_offset: s.x_offset,
                y_offset: s.y_offset,
                name: s.name.clone(),
                options: s.options.clone(),
                layouts,
            }
        })
        .collect();

    let mut items_input: Vec<SaveBoardItemInput> = board
        .items
        .iter()
        .map(|item| {
            let mut layouts: Vec<ItemLayoutRecord> = item
                .layouts
                .iter()
                .filter(|l| l.layout_id != target_layout_id)
                .cloned()
                .collect();
            if let Some(proj) = projected
                .iter()
                .find(|p| p.item_type == GridItemType::Item && p.id == item.id)
            {
                layouts.push(ItemLayoutRecord {
                    item_id: item.id.clone(),
                    section_id: proj.section_id.clone(),
                    layout_id: target_layout_id.to_string(),
                    x_offset: proj.x_offset,
                    y_offset: proj.y_offset,
                    width: proj.width,
                    height: proj.height,
                });
            }
            SaveBoardItemInput {
                id: item.id.clone(),
                kind: item.kind.clone(),
                options: item.options.clone(),
                advanced_options: item.advanced_options.clone(),
                layouts,
                integration_ids: item.integration_ids.clone(),
            }
        })
        .collect();

    // Ensure no duplicate layouts
    let _ = (&mut sections_input, &mut items_input);
    save_board_content(db, board_id, &sections_input, &items_input).await
}

pub async fn duplicate_board(
    db: &Database,
    source_board_id: &str,
    new_name: &str,
    creator_id: Option<&str>,
) -> HomarrResult<FullBoard> {
    let source = get_full_board_by_id(db, source_board_id)
        .await?
        .ok_or_else(|| HomarrError::NotFound(format!("Board {source_board_id} not found")))?;

    let new_board_id = create_id();
    let mut layout_map: HashMap<String, String> = HashMap::new();
    for l in &source.layouts {
        layout_map.insert(l.id.clone(), create_id());
    }
    let mut section_map: HashMap<String, String> = HashMap::new();
    for s in &source.sections {
        section_map.insert(s.id.clone(), create_id());
    }

    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "board" ("id", "name", "is_public", "creator_id", "page_title", "meta_title", "logo_image_url", "favicon_image_url", "background_image_url", "background_image_attachment", "background_image_repeat", "background_image_size", "primary_color", "secondary_color", "opacity", "custom_css", "icon_color", "item_radius", "disable_status")
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&new_board_id)
            .bind(new_name)
            .bind(source.board.is_public)
            .bind(creator_id.or(source.board.creator_id.as_deref()))
            .bind(&source.board.page_title)
            .bind(&source.board.meta_title)
            .bind(&source.board.logo_image_url)
            .bind(&source.board.favicon_image_url)
            .bind(&source.board.background_image_url)
            .bind(&source.board.background_image_attachment)
            .bind(&source.board.background_image_repeat)
            .bind(&source.board.background_image_size)
            .bind(&source.board.primary_color)
            .bind(&source.board.secondary_color)
            .bind(source.board.opacity)
            .bind(&source.board.custom_css)
            .bind(&source.board.icon_color)
            .bind(&source.board.item_radius)
            .bind(source.board.disable_status)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "board" ("id", "name", "is_public", "creator_id", "page_title", "meta_title", "logo_image_url", "favicon_image_url", "background_image_url", "background_image_attachment", "background_image_repeat", "background_image_size", "primary_color", "secondary_color", "opacity", "custom_css", "icon_color", "item_radius", "disable_status")
                   VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)"#,
            )
            .bind(&new_board_id)
            .bind(new_name)
            .bind(source.board.is_public)
            .bind(creator_id.or(source.board.creator_id.as_deref()))
            .bind(&source.board.page_title)
            .bind(&source.board.meta_title)
            .bind(&source.board.logo_image_url)
            .bind(&source.board.favicon_image_url)
            .bind(&source.board.background_image_url)
            .bind(&source.board.background_image_attachment)
            .bind(&source.board.background_image_repeat)
            .bind(&source.board.background_image_size)
            .bind(&source.board.primary_color)
            .bind(&source.board.secondary_color)
            .bind(source.board.opacity)
            .bind(&source.board.custom_css)
            .bind(&source.board.icon_color)
            .bind(&source.board.item_radius)
            .bind(source.board.disable_status)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }

    let norm_layouts: Vec<BoardLayoutNormInput> = source
        .layouts
        .iter()
        .map(|l| BoardLayoutNormInput {
            id: layout_map[&l.id].clone(),
            name: l.name.clone(),
            breakpoint: l.breakpoint,
            column_count: l.column_count,
            left_gutter_column_count: l.left_gutter_column_count,
            right_gutter_column_count: l.right_gutter_column_count,
            role: LayoutRole::from_str_opt(&l.role),
            resize_source: l.resize_source.clone(),
        })
        .collect();

    let sections: Vec<SaveBoardSectionInput> = source
        .sections
        .iter()
        .map(|s| {
            let new_sid = section_map[&s.id].clone();
            SaveBoardSectionInput {
                id: new_sid.clone(),
                kind: s.kind.clone(),
                x_offset: s.x_offset,
                y_offset: s.y_offset,
                name: s.name.clone(),
                options: s.options.clone(),
                layouts: s
                    .layouts
                    .iter()
                    .filter_map(|sl| {
                        Some(SectionLayoutRecord {
                            section_id: new_sid.clone(),
                            layout_id: layout_map.get(&sl.layout_id)?.clone(),
                            parent_section_id: sl
                                .parent_section_id
                                .as_ref()
                                .and_then(|p| section_map.get(p).cloned()),
                            x_offset: sl.x_offset,
                            y_offset: sl.y_offset,
                            width: sl.width,
                            height: sl.height,
                        })
                    })
                    .collect(),
            }
        })
        .collect();

    let items: Vec<SaveBoardItemInput> = source
        .items
        .iter()
        .map(|item| {
            let new_iid = create_id();
            SaveBoardItemInput {
                id: new_iid.clone(),
                kind: item.kind.clone(),
                options: item.options.clone(),
                advanced_options: item.advanced_options.clone(),
                layouts: item
                    .layouts
                    .iter()
                    .filter_map(|il| {
                        Some(ItemLayoutRecord {
                            item_id: new_iid.clone(),
                            section_id: section_map.get(&il.section_id)?.clone(),
                            layout_id: layout_map.get(&il.layout_id)?.clone(),
                            x_offset: il.x_offset,
                            y_offset: il.y_offset,
                            width: il.width,
                            height: il.height,
                        })
                    })
                    .collect(),
                integration_ids: item.integration_ids.clone(),
            }
        })
        .collect();

    let filled = match replace_layouts(db, &new_board_id, &norm_layouts).await {
        Ok(_) => save_board_content(db, &new_board_id, &sections, &items).await,
        Err(err) => Err(err),
    };
    if filled.is_err() {
        // The helpers commit separately; remove the half-built copy (cascades to its children).
        if let Err(err) = delete(db, &new_board_id).await {
            tracing::error!("Could not remove partial board copy {new_board_id}: {err}");
        }
    }
    filled
}

pub async fn create_board_item(
    db: &Database,
    board_id: &str,
    item: &SaveBoardItemInput,
) -> HomarrResult<()> {
    let opts = serialize_superjson(&item.options)?;
    let adv = serialize_superjson(&item.advanced_options)?;
    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(
                r#"INSERT INTO "item" ("id", "board_id", "kind", "options", "advanced_options") VALUES (?, ?, ?, ?, ?)"#,
            )
            .bind(&item.id)
            .bind(board_id)
            .bind(&item.kind)
            .bind(&opts)
            .bind(&adv)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            for l in &item.layouts {
                sqlx::query(
                    r#"INSERT INTO "item_layout" ("item_id", "section_id", "layout_id", "x_offset", "y_offset", "width", "height")
                       VALUES (?, ?, ?, ?, ?, ?, ?)"#,
                )
                .bind(&item.id)
                .bind(&l.section_id)
                .bind(&l.layout_id)
                .bind(l.x_offset)
                .bind(l.y_offset)
                .bind(l.width)
                .bind(l.height)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for iid in &item.integration_ids {
                sqlx::query(
                    r#"INSERT INTO "integration_item" ("item_id", "integration_id") VALUES (?, ?)"#,
                )
                .bind(&item.id)
                .bind(iid)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(
                r#"INSERT INTO "item" ("id", "board_id", "kind", "options", "advanced_options") VALUES ($1, $2, $3, $4, $5)"#,
            )
            .bind(&item.id)
            .bind(board_id)
            .bind(&item.kind)
            .bind(&opts)
            .bind(&adv)
            .execute(&mut *tx)
            .await
            .map_err(map_sqlx_err)?;

            for l in &item.layouts {
                sqlx::query(
                    r#"INSERT INTO "item_layout" ("item_id", "section_id", "layout_id", "x_offset", "y_offset", "width", "height")
                       VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                )
                .bind(&item.id)
                .bind(&l.section_id)
                .bind(&l.layout_id)
                .bind(l.x_offset)
                .bind(l.y_offset)
                .bind(l.width)
                .bind(l.height)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for iid in &item.integration_ids {
                sqlx::query(
                    r#"INSERT INTO "integration_item" ("item_id", "integration_id") VALUES ($1, $2)"#,
                )
                .bind(&item.id)
                .bind(iid)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_item_layout(
    db: &Database,
    item_id: &str,
    layout_id: &str,
    section_id: &str,
    x_offset: i32,
    y_offset: i32,
    width: i32,
    height: i32,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"UPDATE "item_layout" SET "section_id" = ?, "x_offset" = ?, "y_offset" = ?, "width" = ?, "height" = ?
                   WHERE "item_id" = ? AND "layout_id" = ?"#,
            )
            .bind(section_id)
            .bind(x_offset)
            .bind(y_offset)
            .bind(width)
            .bind(height)
            .bind(item_id)
            .bind(layout_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"UPDATE "item_layout" SET "section_id" = $1, "x_offset" = $2, "y_offset" = $3, "width" = $4, "height" = $5
                   WHERE "item_id" = $6 AND "layout_id" = $7"#,
            )
            .bind(section_id)
            .bind(x_offset)
            .bind(y_offset)
            .bind(width)
            .bind(height)
            .bind(item_id)
            .bind(layout_id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn get_item_by_id(db: &Database, item_id: &str) -> HomarrResult<Option<ItemRecord>> {
    let row_opt = match db {
        Database::Sqlite(pool) => sqlx::query(r#"SELECT * FROM "item" WHERE "id" = ?"#)
            .bind(item_id)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_err)?
            .map(|r| {
                let board_id: String = r.try_get("board_id").map_err(map_sqlx_err)?;
                let kind: String = r.try_get("kind").map_err(map_sqlx_err)?;
                let opts: String = r.try_get("options").map_err(map_sqlx_err)?;
                let adv: String = r.try_get("advanced_options").map_err(map_sqlx_err)?;
                Ok::<_, HomarrError>((board_id, kind, opts, adv))
            })
            .transpose()?,
        Database::Postgres(pool) => sqlx::query(r#"SELECT * FROM "item" WHERE "id" = $1"#)
            .bind(item_id)
            .fetch_optional(pool)
            .await
            .map_err(map_sqlx_err)?
            .map(|r| {
                let board_id: String = r.try_get("board_id").map_err(map_sqlx_err)?;
                let kind: String = r.try_get("kind").map_err(map_sqlx_err)?;
                let opts: String = r.try_get("options").map_err(map_sqlx_err)?;
                let adv: String = r.try_get("advanced_options").map_err(map_sqlx_err)?;
                Ok::<_, HomarrError>((board_id, kind, opts, adv))
            })
            .transpose()?,
    };
    let Some((board_id, kind, opts, adv)) = row_opt else {
        return Ok(None);
    };
    let int_ids: Vec<String> = match db {
        Database::Sqlite(pool) => {
            sqlx::query(r#"SELECT "integration_id" FROM "integration_item" WHERE "item_id" = ?"#)
                .bind(item_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?
                .iter()
                .map(|r| r.try_get("integration_id"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
        Database::Postgres(pool) => {
            sqlx::query(r#"SELECT "integration_id" FROM "integration_item" WHERE "item_id" = $1"#)
                .bind(item_id)
                .fetch_all(pool)
                .await
                .map_err(map_sqlx_err)?
                .iter()
                .map(|r| r.try_get("integration_id"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?
        }
    };
    Ok(Some(ItemRecord {
        id: item_id.to_string(),
        board_id,
        kind,
        options: deserialize_superjson::<Value>(&opts)
            .unwrap_or_else(|_| Value::Object(Default::default())),
        advanced_options: deserialize_superjson::<Value>(&adv)
            .unwrap_or_else(|_| Value::Object(Default::default())),
        layouts: Vec::new(),
        integration_ids: int_ids,
    }))
}

pub async fn get_permissions(
    db: &Database,
    board_id: &str,
) -> HomarrResult<(Vec<UserPermissionEntry>, Vec<GroupPermissionEntry>)> {
    match db {
        Database::Sqlite(pool) => {
            let urows = sqlx::query(
                r#"SELECT "user_id", "permission" FROM "boardUserPermission" WHERE "board_id" = ?"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            let grows = sqlx::query(
                r#"SELECT "group_id", "permission" FROM "boardGroupPermission" WHERE "board_id" = ?"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            let users = urows
                .iter()
                .map(|r| {
                    Ok(UserPermissionEntry {
                        user_id: r.try_get("user_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            let groups = grows
                .iter()
                .map(|r| {
                    Ok(GroupPermissionEntry {
                        group_id: r.try_get("group_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((users, groups))
        }
        Database::Postgres(pool) => {
            let urows = sqlx::query(
                r#"SELECT "user_id", "permission" FROM "boardUserPermission" WHERE "board_id" = $1"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            let grows = sqlx::query(
                r#"SELECT "group_id", "permission" FROM "boardGroupPermission" WHERE "board_id" = $1"#,
            )
            .bind(board_id)
            .fetch_all(pool)
            .await
            .map_err(map_sqlx_err)?;
            let users = urows
                .iter()
                .map(|r| {
                    Ok(UserPermissionEntry {
                        user_id: r.try_get("user_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            let groups = grows
                .iter()
                .map(|r| {
                    Ok(GroupPermissionEntry {
                        group_id: r.try_get("group_id")?,
                        permission: r.try_get("permission")?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlx_err)?;
            Ok((users, groups))
        }
    }
}

pub async fn set_permissions(
    db: &Database,
    board_id: &str,
    users: &[UserPermissionEntry],
    groups: &[GroupPermissionEntry],
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "boardUserPermission" WHERE "board_id" = ?"#)
                .bind(board_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "boardGroupPermission" WHERE "board_id" = ?"#)
                .bind(board_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            for u in users {
                sqlx::query(
                    r#"INSERT INTO "boardUserPermission" ("board_id", "user_id", "permission") VALUES (?, ?, ?)"#,
                )
                .bind(board_id)
                .bind(&u.user_id)
                .bind(&u.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for g in groups {
                sqlx::query(
                    r#"INSERT INTO "boardGroupPermission" ("board_id", "group_id", "permission") VALUES (?, ?, ?)"#,
                )
                .bind(board_id)
                .bind(&g.group_id)
                .bind(&g.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            let mut tx = pool.begin().await.map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "boardUserPermission" WHERE "board_id" = $1"#)
                .bind(board_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            sqlx::query(r#"DELETE FROM "boardGroupPermission" WHERE "board_id" = $1"#)
                .bind(board_id)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            for u in users {
                sqlx::query(
                    r#"INSERT INTO "boardUserPermission" ("board_id", "user_id", "permission") VALUES ($1, $2, $3)"#,
                )
                .bind(board_id)
                .bind(&u.user_id)
                .bind(&u.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            for g in groups {
                sqlx::query(
                    r#"INSERT INTO "boardGroupPermission" ("board_id", "group_id", "permission") VALUES ($1, $2, $3)"#,
                )
                .bind(board_id)
                .bind(&g.group_id)
                .bind(&g.permission)
                .execute(&mut *tx)
                .await
                .map_err(map_sqlx_err)?;
            }
            tx.commit().await.map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn set_section_collapse(
    db: &Database,
    user_id: &str,
    section_id: &str,
    collapsed: bool,
) -> HomarrResult<()> {
    match db {
        Database::Sqlite(pool) => {
            sqlx::query(
                r#"INSERT INTO "section_collapse_state" ("user_id", "section_id", "collapsed")
                   VALUES (?, ?, ?)
                   ON CONFLICT ("user_id", "section_id") DO UPDATE SET "collapsed" = excluded."collapsed""#,
            )
            .bind(user_id)
            .bind(section_id)
            .bind(collapsed)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
        Database::Postgres(pool) => {
            sqlx::query(
                r#"INSERT INTO "section_collapse_state" ("user_id", "section_id", "collapsed")
                   VALUES ($1, $2, $3)
                   ON CONFLICT ("user_id", "section_id") DO UPDATE SET "collapsed" = excluded."collapsed""#,
            )
            .bind(user_id)
            .bind(section_id)
            .bind(collapsed)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?;
        }
    }
    Ok(())
}

pub async fn get_section_collapse_states(
    db: &Database,
    user_id: &str,
) -> HomarrResult<HashMap<String, bool>> {
    let rows: Vec<(String, bool)> = match db {
        Database::Sqlite(pool) => sqlx::query(
            r#"SELECT "section_id", "collapsed" FROM "section_collapse_state" WHERE "user_id" = ?"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_err)?
        .iter()
        .map(|r| Ok((r.try_get("section_id")?, r.try_get("collapsed")?)))
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(map_sqlx_err)?,
        Database::Postgres(pool) => sqlx::query(
            r#"SELECT "section_id", "collapsed" FROM "section_collapse_state" WHERE "user_id" = $1"#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(map_sqlx_err)?
        .iter()
        .map(|r| Ok((r.try_get("section_id")?, r.try_get("collapsed")?)))
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(map_sqlx_err)?,
    };
    Ok(rows.into_iter().collect())
}

pub async fn delete(db: &Database, id: &str) -> HomarrResult<bool> {
    let affected = match db {
        Database::Sqlite(pool) => sqlx::query(r#"DELETE FROM "board" WHERE "id" = ?"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
        Database::Postgres(pool) => sqlx::query(r#"DELETE FROM "board" WHERE "id" = $1"#)
            .bind(id)
            .execute(pool)
            .await
            .map_err(map_sqlx_err)?
            .rows_affected(),
    };
    Ok(affected > 0)
}
