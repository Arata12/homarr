//! Server-side helpers for the visual board editor: responsive placements, the editor data
//! blob consumed by `assets/static/board-editor.js`, and the edit-mode chrome.

use std::collections::{HashMap, HashSet};

use homarr_auth::{check_integration_permission, AuthSession};
use homarr_db::{
    models::{FullBoard, ItemRecord, LayoutRecord, SectionRecord},
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

/// A stored placement of an item or container in one layout.
#[derive(Debug, Clone, Copy)]
pub struct StoredCell<'a> {
    pub layout_id: &'a str,
    pub parent_id: &'a str,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Something placed on a section grid: a widget item or a nested container section.
#[derive(Debug, Clone)]
pub struct GridChild<'a> {
    pub id: &'a str,
    pub is_container: bool,
    pub stored: Vec<StoredCell<'a>>,
}

fn base_cell<'c, 'a>(c: &'c GridChild<'a>, base_id: Option<&str>) -> Option<&'c StoredCell<'a>> {
    c.stored
        .iter()
        .find(|s| Some(s.layout_id) == base_id)
        .or_else(|| c.stored.first())
}

/// Computes, for every item and every container section, its parent section and its cell in
/// each layout. Containers are placed inside their parent; their own children are then placed
/// in a grid whose column count is the container's width in that layout (as upstream does).
/// Missing placements are reflowed around the stored ones so every layout renders everything.
pub fn compute_placements(
    full: &FullBoard,
    layouts: &[&LayoutRecord],
) -> HashMap<String, ItemPlacement> {
    let mut roots: Vec<&SectionRecord> = full
        .sections
        .iter()
        .filter(|s| s.kind != "container")
        .collect();
    roots.sort_by_key(|s| (s.y_offset.unwrap_or(0), s.x_offset.unwrap_or(0)));
    let root_ids: Vec<&str> = roots.iter().map(|s| s.id.as_str()).collect();

    let base_id = base_layout_id(full);
    let mut children: Vec<GridChild> = full
        .items
        .iter()
        .map(|item| GridChild {
            id: &item.id,
            is_container: false,
            stored: item
                .layouts
                .iter()
                .map(|l| StoredCell {
                    layout_id: &l.layout_id,
                    parent_id: &l.section_id,
                    x: l.x_offset,
                    y: l.y_offset,
                    w: l.width,
                    h: l.height,
                })
                .collect(),
        })
        .collect();
    children.extend(
        full.sections
            .iter()
            .filter(|s| s.kind == "container")
            .map(|s| GridChild {
                id: &s.id,
                is_container: true,
                stored: s
                    .layouts
                    .iter()
                    .filter_map(|l| {
                        Some(StoredCell {
                            layout_id: &l.layout_id,
                            parent_id: l.parent_section_id.as_deref()?,
                            x: l.x_offset,
                            y: l.y_offset,
                            w: l.width,
                            h: l.height,
                        })
                    })
                    .collect(),
            }),
    );
    place_tree(&root_ids, &children, layouts, base_id)
}

/// Placement core of [`compute_placements`], free of database records so it can be tested.
pub fn place_tree(
    root_ids: &[&str],
    children: &[GridChild<'_>],
    layouts: &[&LayoutRecord],
    base_id: Option<&str>,
) -> HashMap<String, ItemPlacement> {
    let mut out: HashMap<String, ItemPlacement> = HashMap::new();
    let Some(&fallback) = root_ids.first() else {
        return out;
    };
    let containers: HashSet<&str> = children
        .iter()
        .filter(|c| c.is_container)
        .map(|c| c.id)
        .collect();
    let mut parent: HashMap<&str, &str> = children
        .iter()
        .map(|c| {
            let p = base_cell(c, base_id)
                .map(|s| s.parent_id)
                .filter(|p| *p != c.id && (root_ids.contains(p) || containers.contains(p)))
                .unwrap_or(fallback);
            (c.id, p)
        })
        .collect();
    // A container nested (indirectly) inside itself would never render: move it to the root.
    for c in children.iter().filter(|c| c.is_container) {
        let mut cur = parent[c.id];
        let mut steps = 0;
        while cur != c.id && containers.contains(cur) && steps <= containers.len() {
            cur = parent[cur];
            steps += 1;
        }
        if cur == c.id {
            parent.insert(c.id, fallback);
        }
    }

    let mut by_parent: HashMap<&str, Vec<&GridChild>> = HashMap::new();
    for c in children {
        by_parent.entry(parent[c.id]).or_default().push(c);
    }

    let root_cols: Vec<i32> = layouts.iter().map(|l| l.column_count.max(1)).collect();
    let mut queue: Vec<(&str, Vec<i32>)> =
        root_ids.iter().map(|r| (*r, root_cols.clone())).collect();
    while let Some((section_id, cols_per_layout)) = queue.pop() {
        let Some(mut list) = by_parent.remove(section_id) else {
            continue;
        };
        let base_pos = |c: &GridChild| {
            c.stored
                .iter()
                .find(|s| s.parent_id == section_id && Some(s.layout_id) == base_id)
                .map(|s| (s.y, s.x))
                .unwrap_or((i32::MAX, 0))
        };
        list.sort_by(|&a, &b| base_pos(a).cmp(&base_pos(b)).then_with(|| a.id.cmp(b.id)));

        let max_h = |c: &GridChild| {
            if c.is_container {
                i32::MAX
            } else {
                MAX_ITEM_HEIGHT
            }
        };
        let mut cells: Vec<Vec<Cell>> = vec![Vec::with_capacity(layouts.len()); list.len()];
        for (layout, &cols) in layouts.iter().zip(&cols_per_layout) {
            let cols = cols.max(1);
            let staged: Vec<Option<Cell>> = list
                .iter()
                .map(|&c| {
                    c.stored
                        .iter()
                        .find(|s| s.layout_id == layout.id && s.parent_id == section_id)
                        .map(|s| {
                            let w = s.w.clamp(1, cols);
                            Cell {
                                x: s.x.clamp(0, cols - w),
                                y: s.y.max(0),
                                w,
                                h: s.h.clamp(1, max_h(c)),
                                real: true,
                            }
                        })
                })
                .collect();
            let mut occupied: Vec<(i32, i32, i32, i32)> = staged
                .iter()
                .flatten()
                .map(|c| (c.x, c.y, c.w, c.h))
                .collect();
            for (i, (&c, slot)) in list.iter().zip(staged).enumerate() {
                let cell = slot.unwrap_or_else(|| {
                    let (bw, bh) = base_cell(c, base_id).map(|s| (s.w, s.h)).unwrap_or((2, 1));
                    let w = bw.clamp(1, cols);
                    let h = bh.clamp(1, max_h(c));
                    let (x, y) = first_free(&occupied, cols, w, h);
                    occupied.push((x, y, w, h));
                    Cell {
                        x,
                        y,
                        w,
                        h,
                        real: false,
                    }
                });
                cells[i].push(cell);
            }
        }
        for (c, cells) in list.into_iter().zip(cells) {
            if c.is_container {
                queue.push((c.id, cells.iter().map(|cell| cell.w).collect()));
            }
            out.insert(
                c.id.to_string(),
                ItemPlacement {
                    section_id: section_id.to_string(),
                    cells,
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
            // Stored placements outside the rendered section (e.g. a different section on
            // mobile): the editor saves them back unchanged so they are not lost.
            let mut other = Map::new();
            for l in item
                .layouts
                .iter()
                .filter(|l| l.section_id != pl.section_id)
            {
                other.insert(l.layout_id.clone(), json!(l));
            }
            Some(json!({
                "id": item.id,
                "kind": item.kind,
                "options": item.options,
                "advancedOptions": item.advanced_options,
                "integrationIds": item.integration_ids,
                "sectionId": pl.section_id,
                "p": p,
                "o": other,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(id: &str, cols: i32) -> LayoutRecord {
        LayoutRecord {
            id: id.into(),
            name: id.into(),
            board_id: "b".into(),
            column_count: cols,
            left_gutter_column_count: 0,
            right_gutter_column_count: 0,
            breakpoint: 0,
            role: id.into(),
            resize_source: None,
        }
    }

    fn child<'a>(
        id: &'a str,
        is_container: bool,
        stored: &[(&'a str, i32, i32, i32, i32)],
    ) -> GridChild<'a> {
        GridChild {
            id,
            is_container,
            stored: stored
                .iter()
                .map(|&(parent_id, x, y, w, h)| StoredCell {
                    layout_id: "base",
                    parent_id,
                    x,
                    y,
                    w,
                    h,
                })
                .collect(),
        }
    }

    #[test]
    fn nests_containers_and_uses_container_width_as_columns() {
        let base = layout("base", 12);
        let mobile = layout("mobile", 2);
        let layouts = [&mobile, &base];
        let children = vec![
            child("c1", true, &[("root", 2, 0, 4, 3)]),
            child("c2", true, &[("c1", 0, 0, 3, 2)]),
            child("a", false, &[("c2", 1, 0, 5, 1)]),
            child("r", false, &[("root", 0, 0, 2, 1)]),
            child("n", false, &[]),
            // Cycle: neither may vanish.
            child("x", true, &[("y", 0, 0, 1, 1)]),
            child("y", true, &[("x", 0, 0, 1, 1)]),
        ];
        let out = place_tree(&["root"], &children, &layouts, Some("base"));
        let cell = |id: &str, li: usize| {
            let c = out[id].cells[li];
            (c.x, c.y, c.w, c.h)
        };

        assert_eq!(out["c1"].section_id, "root");
        assert_eq!(out["c2"].section_id, "c1");
        assert_eq!(out["a"].section_id, "c2");
        assert_eq!(cell("c1", 1), (2, 0, 4, 3));
        assert_eq!(cell("c2", 1), (0, 0, 3, 2));
        // Inside c2 the grid has 3 columns, so the 5-wide item is clamped.
        assert_eq!(cell("a", 1), (0, 0, 3, 1));
        // Reflowed root items avoid the container's cells.
        assert_eq!(cell("n", 1), (6, 0, 2, 1));
        // Mobile has no stored rows: everything reflows within its parent's width.
        assert_eq!(cell("c1", 0).2, 2);
        assert_eq!(cell("a", 0).2, 2);
        assert!(!out["a"].cells[0].real && out["a"].cells[1].real);
        assert!(out.contains_key("x") && out.contains_key("y"));
    }
}
