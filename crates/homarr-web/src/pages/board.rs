use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use homarr_api::AppState;
use homarr_auth::{can_view_board, check_board_permission, AuthSession, OptionalAuth};
use homarr_db::{
    models::{BoardRecord, FullBoard, LayoutRecord, SectionRecord},
    repos::{boards, settings, users},
};
use homarr_definitions::{BoardPermission, WidgetKind};
use homarr_widgets::render_widget_html_with_origin;
use html_escape::{encode_double_quoted_attribute as encode_attr, encode_text};
use serde_json::{json, Value};

use super::board_editor::{
    app_list, cell_style, compute_placements, edit_bar_html, editor_blob, editor_strings,
    layout_css, script_json, sorted_layouts, usable_integrations, widget_catalog, ItemPlacement,
};
use crate::icons;
use crate::layout::{
    build_spotlight_items, render_page, urlencoding, PageContext, ServerUiSettings,
};
use crate::WebState;

pub(crate) async fn list_visible_boards(
    state: &AppState,
    session: Option<&AuthSession>,
) -> Vec<BoardRecord> {
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
        let ui = ServerUiSettings::load(&state.db).await;
        let is_admin = session.as_ref().is_some_and(AuthSession::is_admin);
        let spotlight = build_spotlight_items(state, &visible, is_admin, false).await;
        let body = if session.is_some() {
            r#"<div class="card empty-board-card">
  <h1 class="page-title">Welcome to Homarr</h1>
  <p class="muted">No boards exist yet. Create your first board to start customizing your dashboard.</p>
  <div class="btn-row-center">
    <a class="btn btn-primary" href="/manage/boards">Create Board</a>
  </div>
</div>"#
        } else {
            r#"<div class="card empty-board-card">
  <h1 class="page-title">Welcome to Homarr</h1>
  <p class="muted">No public boards are visible yet. Sign in to access or create your boards.</p>
  <div class="btn-row-center">
    <a class="btn btn-primary" href="/auth/login">Sign In</a>
  </div>
</div>"#
        };
        let mut ctx = PageContext {
            session: session.as_ref(),
            demo_mode: state.config.demo_mode,
            demo_read_only: state.config.demo_read_only,
            boards: &visible,
            spotlight_items: &spotlight,
            ..PageContext::minimal("Home", body, &locale, &web.i18n)
        };
        ui.apply(&mut ctx);
        return Html(render_page(ctx)).into_response();
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
        return Redirect::temporary(&format!("/boards/{}", urlencoding(&first.name)))
            .into_response();
    }
    let locale = web.i18n.negotiate(&headers);
    let ui = ServerUiSettings::load(&state.db).await;
    let body = r#"<div class="card empty-board-card"><h1 class="page-title">Boards</h1><p class="muted">No visible boards found.</p></div>"#;
    let mut ctx = PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible,
        ..PageContext::minimal("Boards", body, &locale, &web.i18n)
    };
    ui.apply(&mut ctx);
    Html(render_page(ctx)).into_response()
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
        return (StatusCode::NOT_FOUND, "Board not found").into_response();
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

    let select_opts = |current: &str, values: &[&str]| -> String {
        values
            .iter()
            .map(|v| {
                let sel = if *v == current { " selected" } else { "" };
                format!(r#"<option value="{v}"{sel}>{v}</option>"#)
            })
            .collect()
    };
    let attachment_opts = select_opts(&b.background_image_attachment, &["fixed", "scroll"]);
    let repeat_opts = select_opts(
        &b.background_image_repeat,
        &["no-repeat", "repeat", "repeat-x", "repeat-y"],
    );
    let size_opts = select_opts(&b.background_image_size, &["cover", "contain", "auto"]);

    let body = format!(
        r##"<div class="page-header">
  <div>
    <h1 class="page-title">Board Settings: {bname}</h1>
    <p class="muted" style="margin:4px 0 0">Customize layout appearance, background, card glass opacity, and access. Columns and responsive layouts are edited in the board editor (Edit mode, Layouts).</p>
  </div>
  <div style="display:flex;gap:8px">
    <a class="btn btn-sm" href="/boards/{bname_url}">Back to Board</a>
    <a class="btn btn-sm" href="/api/boards/{bid_attr}/export">Export JSON</a>
  </div>
</div>
<div class="card">
  <form class="form-grid" data-api-form="/api/boards/{bid_attr}" data-method="PATCH" data-clear-empty>
    <div class="form-row"><label>Board Name</label><input name="name" value="{bname_attr}" required /></div>
    <div class="form-row"><label>Page Title</label><input name="pageTitle" value="{page_title}" placeholder="Homarr" /></div>
    <div class="form-row">
      <label>Background Image URL</label>
      <div style="display:flex;gap:8px">
        <input id="bg-url-input" name="backgroundImageUrl" value="{bg_url}" placeholder="/images/demo-dashboard-background.svg" />
        <button type="button" class="btn btn-sm" style="white-space:nowrap" onclick="document.getElementById('bg-url-input').value='/images/demo-dashboard-background.svg'">Use Demo BG</button>
      </div>
    </div>
    <div class="form-row"><label>Meta Title</label><input name="metaTitle" value="{meta_title}" /></div>
    <div style="display:grid;grid-template-columns:1fr 1fr;gap:12px">
      <div class="form-row"><label>Logo Image URL</label><input name="logoImageUrl" value="{logo_url}" placeholder="/logo/homarr.svg" /></div>
      <div class="form-row"><label>Favicon URL</label><input name="faviconImageUrl" value="{favicon_url}" /></div>
    </div>
    <div style="display:grid;grid-template-columns:1fr 1fr 1fr;gap:12px">
      <div class="form-row"><label>Background Attachment</label><select name="backgroundImageAttachment">{attachment_opts}</select></div>
      <div class="form-row"><label>Background Repeat</label><select name="backgroundImageRepeat">{repeat_opts}</select></div>
      <div class="form-row"><label>Background Size</label><select name="backgroundImageSize">{size_opts}</select></div>
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
        bid_attr = encode_attr(&urlencoding(&b.id)),
        bname = encode_text(&b.name),
        bname_attr = encode_attr(&b.name),
        bname_url = encode_attr(&urlencoding(&b.name)),
        page_title = encode_attr(b.page_title.as_deref().unwrap_or("")),
        meta_title = encode_attr(b.meta_title.as_deref().unwrap_or("")),
        logo_url = encode_attr(b.logo_image_url.as_deref().unwrap_or("")),
        favicon_url = encode_attr(b.favicon_image_url.as_deref().unwrap_or("")),
        bg_url = encode_attr(b.background_image_url.as_deref().unwrap_or("")),
        opacity = b.opacity.clamp(10, 100),
        primary = encode_attr(b.primary_color.as_deref().unwrap_or("#fa5252")),
        secondary = encode_attr(b.secondary_color.as_deref().unwrap_or("#fd7e14")),
        custom_css = encode_text(b.custom_css.as_deref().unwrap_or("")),
    );

    let ui = ServerUiSettings::load(&state.db).await;
    let spotlight = build_spotlight_items(state, &visible, sess.is_admin(), false).await;
    let mut ctx = PageContext {
        session: Some(&sess),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible,
        active_board_name: Some(&b.name),
        spotlight_items: &spotlight,
        ..PageContext::minimal("Board Settings", &body, &locale, &web.i18n)
    };
    ui.apply(&mut ctx);
    Html(render_page(ctx)).into_response()
}

fn advanced_str<'a>(adv: &'a Value, key: &str) -> Option<&'a str> {
    adv.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn is_hex_color(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|h| {
        matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// Class names allowed on a grid item (letters, digits, `-` and `_` only).
fn safe_css_classes(adv: &Value) -> Vec<String> {
    adv.get("customCssClasses")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .filter(|c| {
                    !c.is_empty()
                        && c.len() <= 64
                        && c.chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Per-layout CSS for container sections: their inner row count follows the active breakpoint,
/// and in edit mode their cell follows the layout picked in the editor (items get that from
/// `board-editor.js` through `--ex`/`--ey`/...; containers are not editor items).
fn container_layout_css(layouts: &[&LayoutRecord]) -> String {
    let mut css = String::new();
    for (i, layout) in layouts.iter().enumerate() {
        let rule = format!(".board-container{{--r:var(--r{i})}}");
        if layout.breakpoint <= 0 || i == 0 {
            css.push_str(&rule);
        } else {
            css.push_str(&format!(
                "@media (min-width:{}px){{{rule}}}",
                layout.breakpoint
            ));
        }
        if layout
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            css.push_str(&format!(
                r#"body[data-edit-layout="{id}"] .board-container{{--ex:var(--x{i});--ey:var(--y{i});--ew:var(--w{i});--eh:var(--h{i});--r:var(--r{i})}}"#,
                id = layout.id,
            ));
        }
    }
    css
}

enum GridNode<'a> {
    Item(&'a str),
    Container(&'a SectionRecord),
}

impl<'a> GridNode<'a> {
    fn id(&self) -> &'a str {
        match self {
            Self::Item(id) => id,
            Self::Container(sec) => &sec.id,
        }
    }
}

/// Renders section grids, recursing into the container sections placed inside them.
struct ContainerRenderer<'a> {
    children: HashMap<&'a str, Vec<GridNode<'a>>>,
    item_html: HashMap<&'a str, String>,
    placements: &'a HashMap<String, ItemPlacement>,
    collapsed: &'a HashMap<String, bool>,
    /// Set for signed-in viewers: collapse toggles are then stored per user on the server.
    collapse_board_id: Option<&'a str>,
    open_all_label: String,
    untitled_label: String,
}

impl<'a> ContainerRenderer<'a> {
    fn add_child(&mut self, parent: &'a str, node: GridNode<'a>) {
        self.children.entry(parent).or_default().push(node);
    }

    /// Orders every grid by first-layout position, so DOM order follows the visual order.
    fn sort(&mut self) {
        let placements = self.placements;
        for list in self.children.values_mut() {
            list.sort_by_key(|n| {
                let (y, x) = placements
                    .get(n.id())
                    .and_then(|p| p.cells.first())
                    .map(|c| (c.y, c.x))
                    .unwrap_or((0, 0));
                (y, x, n.id())
            });
        }
    }

    fn item_count(&self, section_id: &str) -> usize {
        self.children.get(section_id).map_or(0, |list| {
            list.iter()
                .filter(|n| matches!(n, GridNode::Item(_)))
                .count()
        })
    }

    fn grid(&self, section_id: &str) -> String {
        let mut html = String::new();
        for node in self.children.get(section_id).into_iter().flatten() {
            match node {
                GridNode::Item(id) => {
                    if let Some(item) = self.item_html.get(id) {
                        html.push_str(item);
                    }
                }
                GridNode::Container(sec) => html.push_str(&self.container(sec)),
            }
        }
        html
    }

    /// A container section: a card placed in its parent grid with its own inner grid whose
    /// column count is the container's width (see `.container-grid` in homarr.css).
    fn container(&self, sec: &SectionRecord) -> String {
        let Some(pl) = self.placements.get(&sec.id) else {
            return String::new();
        };
        let opts = &sec.options;
        let flag =
            |key: &str, default: bool| opts.get(key).and_then(Value::as_bool).unwrap_or(default);
        let collapsible = flag("collapsible", false);
        let show_label = flag("showLabel", true);
        let show_open_all = flag("showOpenAll", false);
        let scrollable = flag("scrollable", false);
        let title = opts
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let label = title.unwrap_or(self.untitled_label.as_str());
        let collapsed = collapsible && self.collapsed.get(&sec.id).copied().unwrap_or(false);

        let kids = self.children.get(sec.id.as_str());
        let mut style = cell_style(&pl.cells);
        for (i, cell) in pl.cells.iter().enumerate() {
            let content_rows = kids
                .into_iter()
                .flatten()
                .filter_map(|n| self.placements.get(n.id())?.cells.get(i))
                .map(|c| c.y + c.h)
                .max()
                .unwrap_or(0);
            // Non-scrollable containers show all their content (rows shrink to fit the card).
            let rows = if scrollable {
                cell.h
            } else {
                cell.h.max(content_rows)
            };
            style.push_str(&format!("--r{i}:{rows};"));
        }
        if let Some(color) = advanced_str(opts, "borderColor").filter(|c| is_hex_color(c)) {
            style.push_str(&format!("border-color:{color};border-style:solid;"));
        }
        let mut classes = String::new();
        for c in safe_css_classes(opts) {
            classes.push(' ');
            classes.push_str(&c);
        }

        let sid = encode_attr(&sec.id);
        let label_text = encode_text(label);
        let label_attr = encode_attr(label);
        let expanded = if collapsed { "false" } else { "true" };
        let chev = icons::CHEVRON_DOWN;
        let toggle = if collapsible && show_label {
            format!(
                r#"<button type="button" class="board-container-header" data-container-toggle onclick="window.toggleCategory(this)" aria-expanded="{expanded}" aria-controls="board-container-{sid}-content" title="{label_attr}"><span class="board-container-title">{label_text}</span><span class="category-chevron">{chev}</span></button>"#
            )
        } else if collapsible {
            format!(
                r#"<button type="button" class="board-container-toggle" data-container-toggle onclick="window.toggleCategory(this)" aria-expanded="{expanded}" aria-controls="board-container-{sid}-content" aria-label="{label_attr}" title="{label_attr}"><span class="category-chevron">{chev}</span></button>"#
            )
        } else if show_label && title.is_some() {
            format!(
                r#"<span class="board-container-label" title="{label_attr}">{label_text}</span>"#
            )
        } else {
            String::new()
        };
        let open_all = if show_open_all {
            let name = encode_attr(&self.open_all_label.replace("{name}", label)).to_string();
            format!(
                r#"<button type="button" class="icon-btn icon-btn-sm board-container-open-all" onclick="window.openContainerApps(this)" title="{name}" aria-label="{name}">{icon}</button>"#,
                icon = icons::EXTERNAL_LINK,
            )
        } else {
            String::new()
        };
        let bar = if toggle.is_empty() && open_all.is_empty() {
            String::new()
        } else {
            format!(r#"<div class="board-container-bar">{toggle}{open_all}</div>"#)
        };

        format!(
            r#"<div class="grid-item board-section board-container{classes}" style="{style}" data-section-id="{sid}"{board_attr} data-collapsible="{collapsible}" data-collapsed="{collapsed}" data-scrollable="{scrollable}">
  {bar}
  <div class="board-container-body" id="board-container-{sid}-content">
    <div class="board-grid container-grid" data-layouts>{inner}</div>
  </div>
</div>"#,
            classes = encode_attr(&classes),
            style = encode_attr(&style),
            inner = self.grid(&sec.id),
            board_attr = self
                .collapse_board_id
                .filter(|_| collapsible)
                .map(|id| format!(r#" data-board-id="{}""#, encode_attr(id)))
                .unwrap_or_default(),
        )
    }
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
    let can_edit = !state.demo_read_only_active()
        && match session {
            Some(sess) => {
                check_board_permission(&state.db, Some(sess), &full.board, BoardPermission::Modify)
                    .await
                    .is_ok()
            }
            None => false,
        };
    let request_origin = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| state.config.base_url.clone());

    let layouts = sorted_layouts(&full);
    let placements = compute_placements(&full, &layouts);
    let col_count = layouts
        .iter()
        .map(|l| l.column_count)
        .max()
        .unwrap_or(12)
        .clamp(1, 24);
    let mut layout_style = layout_css(&layouts);
    if layouts.is_empty() {
        layout_style = format!(".board-grid{{--board-cols:{col_count}}}");
    }

    layout_style.push_str(&container_layout_css(&layouts));

    // Container sections are rendered inside their parent's grid, not as stacked sections.
    let mut ordered_sections: Vec<_> = full
        .sections
        .iter()
        .filter(|s| s.kind != "container")
        .collect();
    ordered_sections.sort_by_key(|s| (s.y_offset.unwrap_or(0), s.x_offset.unwrap_or(0)));
    let collapsed_sections: HashMap<String, bool> = match session {
        Some(sess) => boards::get_section_collapse_states(&state.db, &sess.user_id)
            .await
            .unwrap_or_default(),
        None => HashMap::new(),
    };

    let editor_strings = editor_strings(&web.i18n, &locale);
    let tr_attr = |key: &str| -> String {
        encode_attr(
            editor_strings
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(key),
        )
        .to_string()
    };

    // Resolve all placed widgets concurrently (cached in the resolver) instead of one by one.
    let placed_items: Vec<_> = full
        .items
        .iter()
        .filter_map(|item| placements.get(&item.id).map(|pl| (item, pl)))
        .collect();
    let resolved = state
        .widgets
        .resolve_many(
            placed_items
                .iter()
                .map(|(item, _)| {
                    (
                        WidgetKind::from_str_opt(&item.kind).unwrap_or(WidgetKind::Notebook),
                        item.options.clone(),
                        item.integration_ids.clone(),
                    )
                })
                .collect(),
        )
        .await;

    let mut item_html: HashMap<&str, String> = HashMap::new();
    for ((item, pl), result) in placed_items.into_iter().zip(resolved) {
        let kind = WidgetKind::from_str_opt(&item.kind).unwrap_or(WidgetKind::Notebook);
        let data = result.unwrap_or_else(|e| json!({ "error": e.to_string() }));

        let mut options = item.options.clone();
        if let (Some(title), Some(obj)) = (
            advanced_str(&item.advanced_options, "title"),
            options.as_object_mut(),
        ) {
            obj.insert("title".into(), Value::String(title.to_string()));
        }
        let rendered_card = render_widget_html_with_origin(
            kind,
            &item.id,
            &options,
            &data,
            request_origin.as_deref(),
        );

        let mut extra_classes = String::new();
        for c in safe_css_classes(&item.advanced_options) {
            extra_classes.push(' ');
            extra_classes.push_str(&c);
        }
        let mut extra_style = cell_style(&pl.cells);
        if let Some(color) =
            advanced_str(&item.advanced_options, "borderColor").filter(|c| is_hex_color(c))
        {
            extra_style.push_str(&format!("border-color:{color};border-style:solid;"));
        }

        let edit_controls = if can_edit {
            format!(
                r#"<div class="item-edit-overlay" data-editor-drag aria-hidden="true"></div>
  <div class="item-edit-controls">
<button type="button" class="icon-btn icon-btn-sm" title="{edit_t}" aria-label="{edit_t}" data-editor-action="edit-item" data-item-id="{iid}">{pencil}</button>
<button type="button" class="icon-btn icon-btn-sm" title="{dup_t}" aria-label="{dup_t}" data-editor-action="duplicate-item" data-item-id="{iid}">{copy}</button>
<button type="button" class="icon-btn icon-btn-sm icon-btn-danger" title="{rm_t}" aria-label="{rm_t}" data-editor-action="remove-item" data-item-id="{iid}">{trash}</button>
  </div>
  <div class="item-resize-handle" data-editor-resize title="Resize"></div>"#,
                iid = encode_attr(&item.id),
                edit_t = tr_attr("editItem"),
                dup_t = tr_attr("duplicateItem"),
                rm_t = tr_attr("removeItem"),
                pencil = icons::PENCIL,
                copy = icons::COPY,
                trash = icons::TRASH,
            )
        } else {
            String::new()
        };

        item_html.insert(
            &item.id,
            format!(
                r#"<div class="grid-item{extra_classes}" style="{extra_style}" data-item-id="{iid}" data-kind="{kind_attr}">
  {edit_controls}
  {rendered_card}
</div>"#,
                extra_classes = encode_attr(&extra_classes),
                extra_style = encode_attr(&extra_style),
                iid = encode_attr(&item.id),
                kind_attr = encode_attr(kind.as_str()),
            ),
        );
    }

    let mut grids = ContainerRenderer {
        children: HashMap::new(),
        item_html,
        placements: &placements,
        collapsed: &collapsed_sections,
        collapse_board_id: session.map(|_| full.board.id.as_str()),
        open_all_label: web.i18n.t(
            &locale,
            "section.action.openAllInNewTabsFor",
            "Open all apps in {name} in tabs",
        ),
        untitled_label: web
            .i18n
            .t(&locale, "section.container.untitled", "Untitled container"),
    };
    for item in &full.items {
        if let Some(pl) = placements.get(&item.id) {
            grids.add_child(&pl.section_id, GridNode::Item(&item.id));
        }
    }
    for sec in full.sections.iter().filter(|s| s.kind == "container") {
        if let Some(pl) = placements.get(&sec.id) {
            grids.add_child(&pl.section_id, GridNode::Container(sec));
        }
    }
    grids.sort();

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
        let item_count = grids.item_count(&sec.id);
        let grid_html = grids.grid(&sec.id);

        let sid = encode_attr(&sec.id);
        let section_edit_bar = if can_edit {
            format!(
                r#"<div class="section-edit-bar">
    <span class="section-edit-name">{name}</span>
    <span class="section-edit-actions">
      <button type="button" class="icon-btn icon-btn-sm" title="{up_t}" aria-label="{up_t}" data-editor-action="section-up" data-section-id="{sid}">{up}</button>
      <button type="button" class="icon-btn icon-btn-sm" title="{down_t}" aria-label="{down_t}" data-editor-action="section-down" data-section-id="{sid}">{down}</button>
      <button type="button" class="icon-btn icon-btn-sm" title="{edit_t}" aria-label="{edit_t}" data-editor-action="section-edit" data-section-id="{sid}">{pencil}</button>
      <button type="button" class="icon-btn icon-btn-sm icon-btn-danger" title="{rm_t}" aria-label="{rm_t}" data-editor-action="section-remove" data-section-id="{sid}">{trash}</button>
    </span>
  </div>"#,
                name = encode_text(&sec_label),
                up_t = tr_attr("moveUp"),
                down_t = tr_attr("moveDown"),
                edit_t = tr_attr("editSection"),
                rm_t = tr_attr("removeSection"),
                up = icons::ARROW_UP,
                down = icons::ARROW_DOWN,
                pencil = icons::PENCIL,
                trash = icons::TRASH,
            )
        } else {
            String::new()
        };
        if sec.kind == "category" {
            sections_html.push_str(&format!(
                r#"<section class="board-section category-section" data-section-id="{sid}" data-collapsed="false">
  {section_edit_bar}
  <button type="button" class="category-header" onclick="window.toggleCategory(this)" aria-expanded="true">
    <span class="category-title-wrap">
      <span class="category-chevron">{chev}</span>
      <span class="category-title">{title}</span>
      <span class="category-count">{item_count}</span>
    </span>
  </button>
  <div class="category-body">
    <div class="board-grid" data-layouts>{grid_html}</div>
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
  {section_edit_bar}
  {optional_header}
  <div class="board-grid" data-layouts>{grid_html}</div>
</section>"#,
            ));
        }
    }

    let (edit_mode_bar, editor_script) = if can_edit {
        let widgets = widget_catalog(&web.i18n, &locale);
        let app_list = app_list(&state.db).await;
        let integrations = match session {
            Some(sess) => usable_integrations(&state.db, sess).await,
            None => Vec::new(),
        };
        let blob = editor_blob(
            &full,
            &layouts,
            &placements,
            widgets,
            app_list,
            integrations,
            editor_strings.clone(),
        );
        (
            edit_bar_html(&full.board.name, &editor_strings),
            format!(
                r#"<div id="editor-modal-root"></div>
<script>window.__HOMARR_EDITOR__ = {};</script>"#,
                script_json(&blob)
            ),
        )
    } else {
        (String::new(), String::new())
    };

    let page_title = full
        .board
        .page_title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&full.board.name);

    let body = format!(
        r#"<style>{layout_style}</style>
{edit_mode_bar}
<div class="board-sections-stack">{sections_html}</div>
{editor_script}"#,
    );

    let ui = ServerUiSettings::load(&state.db).await;
    let is_admin = session.is_some_and(AuthSession::is_admin);
    let spotlight = build_spotlight_items(state, visible_boards, is_admin, can_edit).await;
    let mut ctx = PageContext {
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
        logo_image_url: full
            .board
            .logo_image_url
            .as_deref()
            .filter(|s| !s.trim().is_empty()),
        favicon_image_url: full.board.favicon_image_url.as_deref(),
        spotlight_items: &spotlight,
        can_edit_board: can_edit,
        is_board_page: true,
        hide_header: false,
        app_name: None,
        default_color_scheme: None,
        robots_meta: None,
    };
    ui.apply(&mut ctx);
    // Upstream shows the board's page title (falling back to the app name) next to the logo.
    if let Some(title) = full
        .board
        .page_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        ctx.app_name = Some(title);
    }
    Html(render_page(ctx))
}
