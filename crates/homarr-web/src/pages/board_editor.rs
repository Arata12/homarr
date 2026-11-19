//! Server-side helpers for the visual board editor: responsive placements, the editor data
//! blob consumed by `assets/static/board-editor.js`, and the edit-mode chrome.

use std::collections::HashMap;

use homarr_auth::{check_integration_permission, AuthSession};
use homarr_db::{
    models::{FullBoard, ItemRecord, LayoutRecord},
    repos::{apps, integrations},
    Database,
};
use homarr_definitions::IntegrationPermission;
use homarr_widgets::{all_widget_metadata, widget_option_fields};
use html_escape::{encode_double_quoted_attribute as encode_attr, encode_text};
use serde_json::{json, Map, Value};

use crate::i18n::I18nCatalog;
use crate::icons;
use crate::layout::urlencoding;

pub const MAX_ITEM_HEIGHT: i32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// `true` when the placement is stored in the database for that layout; `false` when it
    /// was derived by reflowing (first-fit packing).
    pub real: bool,
}

pub struct ItemPlacement {
    pub section_id: String,
    /// One cell per layout, in the order of the `layouts` slice given to [`compute_placements`].
    pub cells: Vec<Cell>,
}

/// Layouts ordered by breakpoint ascending (mobile first).
pub fn sorted_layouts(full: &FullBoard) -> Vec<&LayoutRecord> {
    let mut layouts: Vec<&LayoutRecord> = full.layouts.iter().collect();
    layouts.sort_by_key(|l| (l.breakpoint, l.column_count));
    layouts
}

fn base_layout_id(full: &FullBoard) -> Option<&str> {
    full.layouts
        .iter()
        .find(|l| l.role == "base")
        .or_else(|| full.layouts.first())
        .map(|l| l.id.as_str())
}

/// The section an item lives in: its base-layout placement, else the first stored placement.
pub fn item_section<'a>(item: &'a ItemRecord, base_id: Option<&str>) -> Option<&'a str> {
    item.layouts
        .iter()
        .find(|l| Some(l.layout_id.as_str()) == base_id)
        .or_else(|| item.layouts.first())
        .map(|l| l.section_id.as_str())
}

fn overlaps(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
}

/// First-fit packing of an `w x h` box into `cols` columns around already occupied rectangles.
pub fn first_free(occupied: &[(i32, i32, i32, i32)], cols: i32, w: i32, h: i32) -> (i32, i32) {
    let w = w.clamp(1, cols.max(1));
    for y in 0.. {
        for x in 0..=(cols - w).max(0) {
            if !occupied.iter().any(|o| overlaps((x, y, w, h), *o)) {
                return (x, y);
            }
        }
    }
    (0, 0)
}

/// Computes, for every item, its section and its cell in each layout. Missing placements are
/// reflowed around the stored ones so every layout renders every item.
pub fn compute_placements(
    full: &FullBoard,
    layouts: &[&LayoutRecord],
) -> HashMap<String, ItemPlacement> {
    let base_id = base_layout_id(full);
    let fallback_section = full.sections.first().map(|s| s.id.as_str());
    let mut by_section: HashMap<&str, Vec<&ItemRecord>> = HashMap::new();
    for item in &full.items {
        if let Some(sec) = item_section(item, base_id).or(fallback_section) {
            by_section.entry(sec).or_default().push(item);
        }
    }

    let mut out: HashMap<String, ItemPlacement> = HashMap::new();
    for (section_id, mut items) in by_section {
        let base_pos = |i: &ItemRecord| {
            i.layouts
                .iter()
                .find(|l| l.section_id == section_id && Some(l.layout_id.as_str()) == base_id)
                .map(|l| (l.y_offset, l.x_offset))
                .unwrap_or((i32::MAX, 0))
        };
        items.sort_by(|a, b| base_pos(a).cmp(&base_pos(b)).then_with(|| a.id.cmp(&b.id)));
        let base_size = |i: &ItemRecord| {
            i.layouts
                .iter()
                .find(|l| Some(l.layout_id.as_str()) == base_id)
                .or_else(|| i.layouts.first())
                .map(|l| (l.width, l.height))
                .unwrap_or((2, 1))
        };

        let mut cells: HashMap<&str, Vec<Cell>> = HashMap::new();
        for layout in layouts {
            let cols = layout.column_count.max(1);
            let mut occupied: Vec<(i32, i32, i32, i32)> = Vec::new();
            let mut staged: Vec<Option<Cell>> = Vec::new();
            for item in &items {
                let real = item
                    .layouts
                    .iter()
                    .find(|l| l.layout_id == layout.id && l.section_id == section_id);
                staged.push(real.map(|l| {
                    let w = l.width.clamp(1, cols);
                    Cell {
                        x: l.x_offset.clamp(0, cols - w),
                        y: l.y_offset.max(0),
                        w,
                        h: l.height.clamp(1, MAX_ITEM_HEIGHT),
                        real: true,
                    }
                }));
                if let Some(Some(c)) = staged.last() {
                    occupied.push((c.x, c.y, c.w, c.h));
                }
            }
            for (item, slot) in items.iter().zip(staged.iter_mut()) {
                if slot.is_none() {
                    let (bw, bh) = base_size(item);
                    let w = bw.clamp(1, cols);
                    let h = bh.clamp(1, MAX_ITEM_HEIGHT);
                    let (x, y) = first_free(&occupied, cols, w, h);
                    occupied.push((x, y, w, h));
                    *slot = Some(Cell {
                        x,
                        y,
                        w,
                        h,
                        real: false,
                    });
                }
            }
            for (item, slot) in items.iter().zip(staged) {
                cells
                    .entry(item.id.as_str())
                    .or_default()
                    .push(slot.expect("placement staged"));
            }
        }
        for item in &items {
            out.insert(
                item.id.clone(),
                ItemPlacement {
                    section_id: section_id.to_string(),
                    cells: cells.remove(item.id.as_str()).unwrap_or_default(),
                },
            );
        }
    }
    out
}

/// CSS that selects the grid column count and item placement variables per layout breakpoint.
pub fn layout_css(layouts: &[&LayoutRecord]) -> String {
    let mut css = String::new();
    for (i, layout) in layouts.iter().enumerate() {
        let rule = format!(
            ".board-grid{{--board-cols:{cols}}}.grid-item{{--x:var(--x{i});--y:var(--y{i});--w:var(--w{i});--h:var(--h{i})}}",
            cols = layout.column_count.clamp(1, 24),
        );
        if layout.breakpoint <= 0 || i == 0 {
            css.push_str(&rule);
        } else {
            css.push_str(&format!(
                "@media (min-width:{}px){{{rule}}}",
                layout.breakpoint
            ));
        }
    }
    css
}

pub fn cell_style(cells: &[Cell]) -> String {
    let mut s = String::new();
    for (i, c) in cells.iter().enumerate() {
        s.push_str(&format!(
            "--x{i}:{};--y{i}:{};--w{i}:{};--h{i}:{};",
            c.x, c.y, c.w, c.h
        ));
    }
    s
}

/// JSON safe to embed in an inline `<script>`.
pub fn script_json(value: &Value) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn humanize(key: &str) -> String {
    let mut out = String::new();
    for (i, c) in key.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// UI strings of the editor. The second element is the translation key looked up in the
/// locale catalog (when one exists), the third the English fallback.
const EDITOR_STRINGS: &[(&str, &str, &str)] = &[
    ("save", "common.action.save", "Save"),
    ("cancel", "common.action.cancel", "Cancel"),
    ("add", "common.action.add", "Add"),
    ("remove", "common.action.remove", "Remove"),
    ("edit", "common.action.edit", "Edit"),
    ("create", "common.action.create", "Create"),
    ("close", "common.action.close", "Close"),
    ("name", "common.field.name", "Name"),
    ("title", "common.field.title", "Title"),
    ("description", "common.field.description", "Description"),
    ("editItem", "item.action.edit", "Edit item"),
    ("duplicateItem", "item.action.duplicate", "Duplicate item"),
    ("removeItem", "item.action.remove", "Remove item"),
    (
        "advancedOptions",
        "item.edit.advancedOptions.label",
        "Advanced options",
    ),
    (
        "customCssClasses",
        "item.edit.field.customCssClasses.label",
        "Custom css classes",
    ),
    (
        "borderColor",
        "item.edit.field.borderColor.label",
        "Border color",
    ),
    (
        "integrations",
        "item.edit.field.integrations.label",
        "Integrations",
    ),
    (
        "columnCount",
        "layout.field.columnCount.label",
        "Column count",
    ),
    ("breakpoint", "layout.field.breakpoint.label", "Breakpoint"),
    ("newApp", "app.page.create.title", "New app"),
    ("editApp", "app.page.edit.title", "Edit app"),
    ("exitEdit", "board.action.edit.exit", "Exit edit mode"),
    ("saving", "", "Saving..."),
    ("saved", "", "All changes saved"),
    ("saveFailed", "", "Saving failed"),
    ("addWidget", "", "Add widget"),
    ("addSection", "", "Add section"),
    ("sectionName", "", "Section name"),
    (
        "sectionKindCategory",
        "",
        "Category (collapsible, with header)",
    ),
    ("sectionKindEmpty", "", "Plain (no header)"),
    ("editSection", "", "Edit section"),
    ("removeSection", "", "Remove section"),
    ("moveUp", "", "Move up"),
    ("moveDown", "", "Move down"),
    (
        "confirmRemoveItem",
        "",
        "Remove this widget from the board?",
    ),
    (
        "confirmRemoveSection",
        "",
        "Remove this section and all widgets inside it?",
    ),
    ("lastSection", "", "A board needs at least one section."),
    ("section", "", "Section"),
    ("width", "", "Width (columns)"),
    ("height", "", "Height (rows)"),
    ("layouts", "", "Layouts"),
    ("editingLayout", "", "Editing layout"),
    ("addLayout", "", "Add layout"),
    ("layoutName", "", "Layout name"),
    (
        "noIntegrations",
        "",
        "No compatible integrations have been created yet.",
    ),
    ("createIntegration", "", "Create integration"),
    ("chooseApp", "", "Choose app"),
    ("iconSearch", "", "Search icons"),
    ("iconUrl", "", "Icon URL"),
    ("appUrl", "", "URL"),
    ("pingUrl", "", "Ping URL (optional)"),
    ("yourApps", "", "Your apps"),
    ("widgets", "", "Widgets"),
    ("search", "", "Search"),
    ("locationName", "", "Place name"),
    ("latitude", "", "Latitude"),
    ("longitude", "", "Longitude"),
    ("onePerLine", "", "One entry per line"),
    ("jsonValue", "", "JSON value"),
    ("invalidJson", "", "Invalid JSON"),
    ("noAppSelected", "", "Select an app first"),
    ("clear", "", "Clear"),
    (
        "layoutsHint",
        "",
        "Each layout is used for screens at least as wide as its breakpoint (px).",
    ),
    (
        "mobileBaseProtected",
        "",
        "Base and mobile layouts can't be removed.",
    ),
];

pub fn editor_strings(i18n: &I18nCatalog, locale: &str) -> Value {
    let mut map = Map::new();
    for (key, dotted, fallback) in EDITOR_STRINGS {
        let text = if dotted.is_empty() {
            (*fallback).to_string()
        } else {
            i18n.t(locale, dotted, fallback)
        };
        map.insert((*key).to_string(), Value::String(text));
    }
    Value::Object(map)
}

pub fn widget_catalog(i18n: &I18nCatalog, locale: &str) -> Vec<Value> {
    all_widget_metadata()
        .into_iter()
        .map(|meta| {
            let kind = meta.kind.clone();
            let kind_enum = homarr_definitions::WidgetKind::from_str_opt(&kind);
            let fields: Vec<Value> = kind_enum
                .map(widget_option_fields)
                .unwrap_or_default()
                .into_iter()
                .map(|f| {
                    let label = i18n.t(
                        locale,
                        &format!("widget.{kind}.option.{}.label", f.key),
                        &humanize(&f.key),
                    );
                    let description = i18n.t(
                        locale,
                        &format!("widget.{kind}.option.{}.description", f.key),
                        "",
                    );
                    let mut v = serde_json::to_value(&f).unwrap_or(Value::Null);
                    if let Some(o) = v.as_object_mut() {
                        o.insert("label".into(), Value::String(label));
                        o.insert("description".into(), Value::String(description));
                        if let Some(Value::Array(choices)) = o.get_mut("choices") {
                            for c in choices {
                                let value = c.get("value").and_then(Value::as_str).unwrap_or("");
                                let translated = i18n.t(
                                    locale,
                                    &format!("widget.{kind}.option.{}.option.{value}", f.key),
                                    value,
                                );
                                c["label"] = Value::String(translated);
                            }
                        }
                    }
                    v
                })
                .collect();
            json!({
                "kind": kind,
                "name": i18n.t(locale, &format!("widget.{kind}.name"), &meta.name),
                "description": i18n.t(locale, &format!("widget.{kind}.description"), &meta.description),
                "category": meta.category,
                "defaultWidth": meta.default_width,
                "defaultHeight": meta.default_height,
                "supportedIntegrations": meta.supported_integrations,
                "defaultOptions": meta.default_options,
                "fields": fields,
            })
        })
        .collect()
}

/// Integrations the session may attach to widgets.
pub async fn usable_integrations(db: &Database, session: &AuthSession) -> Vec<Value> {
    let mut out = Vec::new();
    for i in integrations::list_all(db).await.unwrap_or_default() {
        if check_integration_permission(db, session, &i.id, IntegrationPermission::Use)
            .await
            .is_ok()
        {
            out.push(json!({ "id": i.id, "name": i.name, "kind": i.kind }));
        }
    }
    out
}

pub async fn app_list(db: &Database) -> Vec<Value> {
    apps::list_all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|a| serde_json::to_value(a).unwrap_or(Value::Null))
        .collect()
}

/// Everything the browser editor needs, as JSON for `window.__HOMARR_EDITOR__`.
#[allow(clippy::too_many_arguments)]
pub fn editor_blob(
    full: &FullBoard,
    layouts: &[&LayoutRecord],
    placements: &HashMap<String, ItemPlacement>,
    widgets: Vec<Value>,
    apps: Vec<Value>,
    integrations: Vec<Value>,
    strings: Value,
) -> Value {
    let items: Vec<Value> = full
        .items
        .iter()
        .filter_map(|item| {
            let pl = placements.get(&item.id)?;
            let mut p = Map::new();
            for (layout, c) in layouts.iter().zip(&pl.cells) {
                p.insert(
                    layout.id.clone(),
                    json!([c.x, c.y, c.w, c.h, u8::from(c.real)]),
                );
            }
            Some(json!({
                "id": item.id,
                "kind": item.kind,
                "options": item.options,
                "advancedOptions": item.advanced_options,
                "integrationIds": item.integration_ids,
                "sectionId": pl.section_id,
                "p": p,
            }))
        })
        .collect();
    let sections: Vec<Value> = full
        .sections
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "kind": s.kind,
                "name": s.name,
                "xOffset": s.x_offset,
                "yOffset": s.y_offset,
                "options": s.options,
                "layouts": s.layouts,
            })
        })
        .collect();
    json!({
        "boardId": full.board.id,
        "boardName": full.board.name,
        "layouts": layouts.iter().map(|l| json!({
            "id": l.id,
            "name": l.name,
            "columnCount": l.column_count,
            "breakpoint": l.breakpoint,
            "role": l.role,
            "leftGutterColumnCount": l.left_gutter_column_count,
            "rightGutterColumnCount": l.right_gutter_column_count,
        })).collect::<Vec<_>>(),
        "sections": sections,
        "items": items,
        "widgets": widgets,
        "apps": apps,
        "integrations": integrations,
        "i18n": strings,
        "icons": {
            "edit": icons::PENCIL,
            "copy": icons::COPY,
            "trash": icons::TRASH,
            "up": icons::ARROW_UP,
            "down": icons::ARROW_DOWN,
            "grip": icons::GRIP,
            "plus": icons::PLUS,
            "close": icons::X,
        },
    })
}

/// Edit-mode toolbar shown above the board.
pub fn edit_bar_html(board_name: &str, strings: &Value) -> String {
    let s = |k: &str| encode_text(strings.get(k).and_then(Value::as_str).unwrap_or(k)).to_string();
    let s_attr =
        |k: &str| encode_attr(strings.get(k).and_then(Value::as_str).unwrap_or(k)).to_string();
    format!(
        r#"<div class="edit-mode-bar">
  <div class="edit-mode-bar-left">
    <span class="edit-dot"></span>
    <strong>Editing: {board_name}</strong>
    <span id="editor-save-status" class="editor-save-status muted" role="status" aria-live="polite"></span>
  </div>
  <div class="edit-mode-bar-actions">
    <label class="editor-layout-picker">{editing_layout}
      <select id="editor-layout-select" aria-label="{editing_layout_attr}"></select>
    </label>
    <button type="button" class="btn btn-primary btn-sm" data-editor-action="add-widget">{plus}<span>{add_widget}</span></button>
    <button type="button" class="btn btn-sm" data-editor-action="add-section">{plus}<span>{add_section}</span></button>
    <button type="button" class="btn btn-sm" data-editor-action="layouts">{layouts_label}</button>
    <a class="btn btn-sm" href="/boards/{board_url}/settings">{gear}<span>Board Settings</span></a>
    <button type="button" class="btn btn-sm" data-editor-action="exit">{check}<span>Done</span></button>
  </div>
</div>"#,
        board_name = encode_text(board_name),
        board_url = encode_attr(&urlencoding(board_name)),
        editing_layout = s("editingLayout"),
        editing_layout_attr = s_attr("editingLayout"),
        add_widget = s("addWidget"),
        add_section = s("addSection"),
        layouts_label = s("layouts"),
        plus = icons::PLUS,
        gear = icons::SETTINGS,
        check = icons::CHECK,
    )
}
