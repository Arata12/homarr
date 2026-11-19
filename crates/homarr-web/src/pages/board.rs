use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use homarr_api::AppState;
use homarr_auth::{can_view_board, check_board_permission, AuthSession, OptionalAuth};
use homarr_db::{
    models::{BoardRecord, FullBoard},
    repos::{apps, boards, search_engines, settings, users},
};
use homarr_definitions::{BoardPermission, WidgetKind};
use homarr_widgets::{all_widget_metadata, render_widget_html};
use html_escape::encode_text;
use serde_json::{json, Value};

use crate::icons;
use crate::layout::{render_page, PageContext};
use crate::WebState;

async fn list_visible_boards(state: &AppState, session: Option<&AuthSession>) -> Vec<BoardRecord> {
    let all = boards::list_all(&state.db).await.unwrap_or_default();
    let mut visible = Vec::new();
    for b in all {
        if can_view_board(&state.db, session, &b)
            .await
            .unwrap_or(false)
        {
            visible.push(b);
        }
    }
    visible
}

async fn build_spotlight_items(state: &AppState, visible_boards: &[BoardRecord]) -> Vec<Value> {
    let mut items = Vec::new();
    for b in visible_boards {
        items.push(json!({
            "label": b.name,
            "sub": "Board",
            "href": format!("/boards/{}", b.name),
        }));
    }
    for app in apps::list_all(&state.db).await.unwrap_or_default() {
        items.push(json!({
            "label": app.name,
            "sub": app.description.unwrap_or_else(|| "Application".to_string()),
            "href": app.href.unwrap_or_else(|| "#".to_string()),
        }));
    }
    for se in search_engines::list_all(&state.db)
        .await
        .unwrap_or_default()
    {
        items.push(json!({
            "label": format!("!{} ({})", se.short, se.name),
            "sub": "Search Engine",
            "href": se.url_template.unwrap_or_else(|| "https://duckduckgo.com".to_string()),
        }));
    }
    items.push(json!({ "label": "Manage Workspace", "sub": "Admin", "href": "/manage" }));
    items.push(json!({ "label": "API Reference", "sub": "Docs", "href": "/reference/api" }));
    items
}

pub async fn home_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let state = &web.app;
    if !state.config.demo_mode && users::count(&state.db).await.unwrap_or(0) == 0 {
        return Redirect::temporary("/init").into_response();
    }

    let visible = list_visible_boards(state, session.as_ref()).await;
    if visible.is_empty() {
        let locale = web.i18n.negotiate(&headers);
        let body = r#"<div class="card empty-board-card">
  <h1 class="page-title">Welcome to Homarr</h1>
  <p class="muted">No boards are visible yet. Sign in or create your first board under Manage.</p>
  <div class="btn-row-center">
    <a class="btn btn-primary" href="/auth/login">Sign In</a>
    <a class="btn" href="/manage/boards">Manage Boards</a>
  </div>
</div>"#;
        return Html(render_page(PageContext {
            session: session.as_ref(),
            demo_mode: state.config.demo_mode,
            demo_read_only: state.config.demo_read_only,
            boards: &visible,
            ..PageContext::minimal("Home", body, &locale, &web.i18n)
        }))
        .into_response();
    }

    let default_board_id = settings::get_setting(&state.db, "board")
        .await
        .ok()
        .and_then(|v| {
            v.get("homeBoardId")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        });

    let chosen = default_board_id
        .and_then(|id| visible.iter().find(|b| b.id == id))
        .or_else(|| visible.first())
        .expect("visible non-empty");

    let full = boards::get_full_board_by_id(&state.db, &chosen.id)
        .await
        .ok()
        .flatten();
    match full {
        Some(fb) => render_full_board(&web, session.as_ref(), &headers, &visible, fb)
            .await
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Board not found").into_response(),
    }
}

pub async fn boards_index_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let state = &web.app;
    let visible = list_visible_boards(state, session.as_ref()).await;
    if let Some(first) = visible.first() {
        return Redirect::temporary(&format!("/boards/{}", first.name)).into_response();
    }
    let locale = web.i18n.negotiate(&headers);
    let body = r#"<div class="card empty-board-card"><h1 class="page-title">Boards</h1><p class="muted">No visible boards found.</p></div>"#;
    Html(render_page(PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible,
        ..PageContext::minimal("Boards", body, &locale, &web.i18n)
    }))
    .into_response()
}

pub async fn named_board_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let state = &web.app;
    let visible = list_visible_boards(state, session.as_ref()).await;
    let Ok(Some(full)) = boards::get_full_board_by_name(&state.db, &name).await else {
        return (
            StatusCode::NOT_FOUND,
            Html("<h1>Board not found</h1>".to_string()),
        )
            .into_response();
    };
    if !can_view_board(&state.db, session.as_ref(), &full.board)
        .await
        .unwrap_or(false)
    {
        return Redirect::temporary("/auth/login").into_response();
    }
    render_full_board(&web, session.as_ref(), &headers, &visible, full)
        .await
        .into_response()
}

pub async fn board_settings_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let state = &web.app;
    let Some(sess) = session else {
        return Redirect::temporary("/auth/login").into_response();
    };
    let visible = list_visible_boards(state, Some(&sess)).await;
    let Ok(Some(full)) = boards::get_full_board_by_name(&state.db, &name).await else {
        return (StatusCode::NOT_FOUND, "Board not found").into_response();
    };
    if check_board_permission(&state.db, Some(&sess), &full.board, BoardPermission::Modify)
        .await
        .is_err()
    {
        return (StatusCode::FORBIDDEN, "Board modify permission required").into_response();
    }

    let locale = web.i18n.negotiate(&headers);
    let b = &full.board;
    let public_checked = if b.is_public { "checked" } else { "" };
    let disable_status_checked = if b.disable_status { "checked" } else { "" };
    let selected_radius = b.item_radius.as_deref().unwrap_or("lg");
    let mut radius_opts = String::new();
    for (val, label) in [
        ("xs", "Extra Small (4px)"),
        ("sm", "Small (6px)"),
        ("md", "Medium (8px)"),
        ("lg", "Large (12px)"),
        ("xl", "Extra Large (16px)"),
    ] {
        let sel = if selected_radius == val {
            " selected"
        } else {
            ""
        };
        radius_opts.push_str(&format!(r#"<option value="{val}"{sel}>{label}</option>"#));
    }

    let body = format!(
        r##"<div class="page-header">
  <div>
    <h1 class="page-title">Board Settings: {bname}</h1>
    <p class="muted" style="margin:4px 0 0">Customize layout appearance, background, card glass opacity, and access.</p>
  </div>
  <div style="display:flex;gap:8px">
    <a class="btn btn-sm" href="/boards/{bname}">Back to Board</a>
    <a class="btn btn-sm" href="/api/boards/{bid}/export">Export JSON</a>
  </div>
</div>
<div class="card">
  <form class="form-grid" data-api-form="/api/boards/{bid}" data-method="PATCH">
    <div class="form-row"><label>Board Name</label><input name="name" value="{bname}" required /></div>
    <div class="form-row"><label>Page Title</label><input name="pageTitle" value="{page_title}" placeholder="Homarr" /></div>
    <div class="form-row">
      <label>Background Image URL</label>
      <div style="display:flex;gap:8px">
        <input id="bg-url-input" name="backgroundImageUrl" value="{bg_url}" placeholder="/images/demo-dashboard-background.svg" />
        <button type="button" class="btn btn-sm" style="white-space:nowrap" onclick="document.getElementById('bg-url-input').value='/images/demo-dashboard-background.svg'">Use Demo BG</button>
      </div>
    </div>
    <div class="form-row">
      <label>Card Opacity (<span id="opacity-val">{opacity}%</span>)</label>
      <input type="range" name="opacity" min="10" max="100" value="{opacity}" oninput="document.getElementById('opacity-val').textContent=this.value+'%'" />
    </div>
    <div class="form-row">
      <label>Card Border Radius</label>
      <select name="itemRadius">{radius_opts}</select>
    </div>
    <div style="display:grid;grid-template-columns:1fr 1fr;gap:12px">
      <div class="form-row">
        <label>Primary Accent Color</label>
        <input type="color" name="primaryColor" value="{primary}" style="height:40px;padding:4px" />
      </div>
      <div class="form-row">
        <label>Secondary Accent Color</label>
        <input type="color" name="secondaryColor" value="{secondary}" style="height:40px;padding:4px" />
      </div>
    </div>
    <div class="form-row"><label>Custom CSS</label><textarea name="customCss" rows="5">{custom_css}</textarea></div>
    <div style="display:flex;gap:18px;flex-wrap:wrap">
      <label style="display:flex;align-items:center;gap:8px"><input type="checkbox" name="isPublic" style="width:auto" {public_checked} /> Public Board</label>
      <label style="display:flex;align-items:center;gap:8px"><input type="checkbox" name="disableStatus" style="width:auto" {disable_status_checked} /> Disable App Status Pings</label>
    </div>
    <div><button type="submit" class="btn btn-primary">Save Settings</button></div>
  </form>
</div>"##,
        bid = encode_text(&b.id),
        bname = encode_text(&b.name),
        page_title = encode_text(b.page_title.as_deref().unwrap_or("")),
        bg_url = encode_text(b.background_image_url.as_deref().unwrap_or("")),
        opacity = b.opacity.clamp(10, 100),
        primary = encode_text(b.primary_color.as_deref().unwrap_or("#fa5252")),
        secondary = encode_text(b.secondary_color.as_deref().unwrap_or("#fd7e14")),
        custom_css = encode_text(b.custom_css.as_deref().unwrap_or("")),
    );

    Html(render_page(PageContext {
        session: Some(&sess),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible,
        active_board_name: Some(&b.name),
        ..PageContext::minimal("Board Settings", &body, &locale, &web.i18n)
    }))
    .into_response()
}

async fn render_full_board(
    web: &WebState,
    session: Option<&AuthSession>,
    headers: &HeaderMap,
    visible_boards: &[BoardRecord],
    full: FullBoard,
) -> Html<String> {
    let state = &web.app;
    let locale = web.i18n.negotiate(headers);
    let can_edit = match session {
        Some(sess) => {
            check_board_permission(&state.db, Some(sess), &full.board, BoardPermission::Modify)
                .await
                .is_ok()
        }
        None => false,
    };

    let base_layout = full
        .layouts
        .iter()
        .find(|l| l.role == "base")
        .or_else(|| full.layouts.first());
    let col_count = base_layout
        .map(|l| l.column_count)
        .unwrap_or(12)
        .clamp(1, 24);
    let base_layout_id = base_layout.map(|l| l.id.as_str()).unwrap_or("");
    let first_section_id = full.sections.first().map(|s| s.id.as_str()).unwrap_or("");

    let mut ordered_sections: Vec<_> = full.sections.iter().collect();
    ordered_sections.sort_by_key(|s| (s.y_offset.unwrap_or(0), s.x_offset.unwrap_or(0)));

    let mut section_options = String::new();
    let mut sections_html = String::new();

    for sec in ordered_sections {
        let sec_label = sec
            .name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if sec.kind == "category" {
                    "Category".to_string()
                } else {
                    "Main Canvas".to_string()
                }
            });
        section_options.push_str(&format!(
            r#"<option value="{}">{}</option>"#,
            encode_text(&sec.id),
            encode_text(&sec_label),
        ));

        let mut placed_items = Vec::new();
        for item in &full.items {
            let placement = item
                .layouts
                .iter()
                .find(|l| {
                    l.section_id == sec.id
                        && (base_layout_id.is_empty() || l.layout_id == base_layout_id)
                })
                .or_else(|| item.layouts.iter().find(|l| l.section_id == sec.id));
            if let Some(pl) = placement {
                placed_items.push((pl.y_offset, pl.x_offset, item, pl));
            }
        }
        placed_items.sort_by_key(|(y, x, _, _)| (*y, *x));

        let item_count = placed_items.len();
        let mut grid_html = String::new();
        for (y, x, item, pl) in placed_items {
            let kind = WidgetKind::from_str_opt(&item.kind).unwrap_or(WidgetKind::Notebook);
            let data = state
                .widgets
                .resolve_widget_data(kind, &item.options, &item.integration_ids)
                .await
                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
            let rendered_card = render_widget_html(kind, &item.id, &item.options, &data);

            let w = pl.width.clamp(1, col_count);
            let x_clamped = x.clamp(0, (col_count - w).max(0));
            let y_clamped = y.max(0);
            let h = pl.height.clamp(1, 12);

            let edit_controls = if can_edit {
                format!(
                    r#"<div class="item-edit-controls">
  <button type="button" class="icon-btn icon-btn-danger" title="Remove widget" aria-label="Remove widget" onclick="window.deleteBoardItem('{bid}','{iid}')">{trash}</button>
</div>"#,
                    bid = encode_text(&full.board.id),
                    iid = encode_text(&item.id),
                    trash = icons::TRASH,
                )
            } else {
                String::new()
            };

            grid_html.push_str(&format!(
                r#"<div class="grid-item" style="--x:{x_clamped};--y:{y_clamped};--w:{w};--h:{h}" data-item-id="{iid}" data-kind="{kind_attr}">
  {edit_controls}
  {rendered_card}
</div>"#,
                iid = encode_text(&item.id),
                kind_attr = encode_text(kind.as_str()),
            ));
        }

        let sid = encode_text(&sec.id);
        if sec.kind == "category" {
            sections_html.push_str(&format!(
                r#"<section class="board-section category-section" data-section-id="{sid}" data-collapsed="false">
  <button type="button" class="category-header" onclick="window.toggleCategory(this)" aria-expanded="true">
    <span class="category-title-wrap">
      <span class="category-chevron">{chev}</span>
      <span class="category-title">{title}</span>
      <span class="category-count">{item_count}</span>
    </span>
  </button>
  <div class="category-body">
    <div class="board-grid" style="--board-cols:{col_count}">{grid_html}</div>
  </div>
</section>"#,
                chev = icons::CHEVRON_DOWN,
                title = encode_text(&sec_label),
            ));
        } else {
            let optional_header = match sec.name.as_deref().map(str::trim) {
                Some(name) if !name.is_empty() && name != "Main" => format!(
                    r#"<div class="section-header"><h2 class="section-title">{}</h2></div>"#,
                    encode_text(name)
                ),
                _ => String::new(),
            };
            sections_html.push_str(&format!(
                r#"<section class="board-section board-section-empty" data-section-id="{sid}">
  {optional_header}
  <div class="board-grid" style="--board-cols:{col_count}">{grid_html}</div>
</section>"#,
            ));
        }
    }

    let edit_mode_bar = if can_edit {
        format!(
            r#"<div class="edit-mode-bar">
  <div class="edit-mode-bar-left">
    <span class="edit-dot"></span>
    <strong>Editing: {board_name}</strong>
  </div>
  <div class="edit-mode-bar-actions">
    <button type="button" class="btn btn-primary btn-sm" onclick="window.openAddItemModal()">{plus}<span>Add Widget</span></button>
    <a class="btn btn-sm" href="/boards/{board_name}/settings">{gear}<span>Board Settings</span></a>
    <button type="button" class="btn btn-sm" onclick="window.toggleEditMode()">{check}<span>Done</span></button>
  </div>
</div>"#,
            board_name = encode_text(&full.board.name),
            plus = icons::PLUS,
            gear = icons::SETTINGS,
            check = icons::CHECK,
        )
    } else {
        String::new()
    };

    let all_meta = all_widget_metadata();
    let default_kind = all_meta.first().map(|m| m.kind.as_str()).unwrap_or("clock");
    let mut widget_cards = String::new();
    for (idx, meta) in all_meta.iter().enumerate() {
        let selected_cls = if idx == 0 { " is-selected" } else { "" };
        let kind_esc = encode_text(&meta.kind);
        let name_esc = encode_text(&meta.name);
        let desc_esc = encode_text(&meta.description);
        let search_raw = format!(
            "{} {} {}",
            meta.name.to_ascii_lowercase(),
            meta.kind.to_ascii_lowercase(),
            meta.description.to_ascii_lowercase()
        );
        let search_blob = encode_text(&search_raw);
        widget_cards.push_str(&format!(
            r#"<button type="button" class="widget-picker-card{selected_cls}" data-kind="{kind_esc}" data-search="{search_blob}" onclick="window.selectWidgetKind(this,'{kind_esc}')">
  <div class="widget-picker-top">
    <strong>{name_esc}</strong>
    <span class="widget-picker-kind">{kind_esc}</span>
  </div>
  <div class="widget-picker-desc">{desc_esc}</div>
</button>"#,
        ));
    }

    let mut app_options = String::from(r#"<option value="">-- Select App --</option>"#);
    for a in apps::list_all(&state.db).await.unwrap_or_default() {
        app_options.push_str(&format!(
            r#"<option value="{}">{}</option>"#,
            encode_text(&a.id),
            encode_text(&a.name)
        ));
    }

    let modal_html = if can_edit {
        format!(
            r#"<div id="add-item-modal" class="modal-backdrop" onclick="if(event.target===this)this.classList.remove('open')">
  <div class="modal-card modal-card-wide">
    <div class="modal-head">
      <h3>Add Widget to Board</h3>
      <button type="button" class="icon-btn icon-btn-sm" onclick="document.getElementById('add-item-modal').classList.remove('open')" aria-label="Close">{close_icon}</button>
    </div>
    <form class="form-grid" style="max-width:none" onsubmit="window.submitAddBoardItem(event,'{bid}','{sid}','{lid}')">
      <input type="hidden" name="kind" value="{default_kind}" />
      <div class="spotlight-input-row picker-search-row">
        {search_icon}
        <input id="widget-picker-search" type="search" placeholder="Filter all 67 Homarr widgets..." oninput="window.filterWidgetPicker(this.value)" autocomplete="off" />
      </div>
      <div class="widget-picker-grid" id="widget-picker-grid">{widget_cards}</div>
      <div id="picker-app-row" class="form-row" style="display:none">
        <label>Select Application</label>
        <select name="appId">{app_options}</select>
      </div>
      <div class="picker-config-row">
        <div class="form-row">
          <label>Section</label>
          <select name="sectionId">{section_options}</select>
        </div>
        <div class="form-row">
          <label>Width (cols)</label>
          <input type="number" name="width" min="1" max="{col_count}" value="2" />
        </div>
        <div class="form-row">
          <label>Height (rows)</label>
          <input type="number" name="height" min="1" max="8" value="1" />
        </div>
      </div>
      <div style="display:flex;gap:8px;justify-content:flex-end">
        <button type="button" class="btn" onclick="document.getElementById('add-item-modal').classList.remove('open')">Cancel</button>
        <button type="submit" class="btn btn-primary">{plus_icon}<span>Add to Board</span></button>
      </div>
    </form>
  </div>
</div>"#,
            close_icon = icons::X,
            search_icon = icons::SEARCH,
            plus_icon = icons::PLUS,
            bid = encode_text(&full.board.id),
            sid = encode_text(first_section_id),
            lid = encode_text(base_layout_id),
        )
    } else {
        String::new()
    };

    let page_title = full
        .board
        .page_title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&full.board.name);

    let body = format!(
        r#"{edit_mode_bar}
<div class="board-sections-stack">{sections_html}</div>
{modal_html}"#,
    );

    let spotlight = build_spotlight_items(state, visible_boards).await;
    Html(render_page(PageContext {
        title: page_title,
        body: &body,
        session,
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: visible_boards,
        active_board_name: Some(&full.board.name),
        locale: &locale,
        i18n: &web.i18n,
        custom_css: full.board.custom_css.as_deref(),
        primary_color: full.board.primary_color.as_deref(),
        secondary_color: full.board.secondary_color.as_deref(),
        background_image_url: full.board.background_image_url.as_deref(),
        background_image_attachment: Some(&full.board.background_image_attachment),
        background_image_repeat: Some(&full.board.background_image_repeat),
        background_image_size: Some(&full.board.background_image_size),
        opacity: Some(full.board.opacity),
        item_radius: full.board.item_radius.as_deref(),
        logo_image_url: full.board.logo_image_url.as_deref(),
        favicon_image_url: full.board.favicon_image_url.as_deref(),
        spotlight_items: &spotlight,
        can_edit_board: can_edit,
        is_board_page: true,
        hide_header: false,
    }))
}
