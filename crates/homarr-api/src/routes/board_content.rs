//! Input validation for board editor payloads (layout, sections, items).

use std::collections::{HashMap, HashSet};

use homarr_auth::{check_integration_permission, AuthSession};
use homarr_common::{HomarrError, HomarrResult};
use homarr_db::{
    models::{ItemRecord, LayoutRecord},
    repos::boards::{SaveBoardItemInput, SaveBoardSectionInput},
    Database,
};
use homarr_definitions::{IntegrationPermission, WidgetKind};

const MAX_COORD: i32 = 32_767;
const MAX_NAME_LEN: usize = 64;
const MAX_CSS_CLASS_LEN: usize = 64;
const MAX_CSS_CLASSES: usize = 32;

fn bad(msg: impl Into<String>) -> HomarrError {
    HomarrError::BadRequest(msg.into())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_css_class(class: &str) -> bool {
    !class.is_empty()
        && class.len() <= MAX_CSS_CLASS_LEN
        && class
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// Validates the structure of a board content save. Integration permission checks are
/// done separately in [`validate_item_integrations`].
pub fn validate_board_content(
    layouts: &[LayoutRecord],
    sections: &[SaveBoardSectionInput],
    items: &[SaveBoardItemInput],
) -> HomarrResult<()> {
    let columns: HashMap<&str, i32> = layouts
        .iter()
        .map(|l| (l.id.as_str(), l.column_count))
        .collect();

    let mut section_ids = HashSet::new();
    for s in sections {
        if !valid_id(&s.id) {
            return Err(bad("Invalid section id"));
        }
        if !section_ids.insert(s.id.as_str()) {
            return Err(bad("Duplicate section id"));
        }
        if !matches!(s.kind.as_str(), "empty" | "category" | "container") {
            return Err(bad(format!("Unknown section kind '{}'", s.kind)));
        }
        if s.name
            .as_deref()
            .is_some_and(|n| n.chars().count() > MAX_NAME_LEN)
        {
            return Err(bad("Section name is too long"));
        }
        for off in [s.x_offset, s.y_offset].into_iter().flatten() {
            if !(-MAX_COORD..=MAX_COORD).contains(&off) {
                return Err(bad("Section offset out of range"));
            }
        }
        if !s.options.is_null() && !s.options.is_object() {
            return Err(bad("Section options must be an object"));
        }
    }
    for s in sections {
        let mut seen_layouts = HashSet::new();
        for l in &s.layouts {
            if !columns.contains_key(l.layout_id.as_str()) {
                return Err(bad("Section references an unknown layout"));
            }
            if !seen_layouts.insert(l.layout_id.as_str()) {
                return Err(bad("Duplicate section layout placement"));
            }
            if let Some(ref pid) = l.parent_section_id {
                if pid == &s.id || !section_ids.contains(pid.as_str()) {
                    return Err(bad("Section references an invalid parent section"));
                }
            }
            if l.width < 1 || l.height < 1 || l.x_offset < 0 || l.y_offset < 0 {
                return Err(bad("Invalid section layout bounds"));
            }
        }
    }

    let mut item_ids = HashSet::new();
    for item in items {
        if !valid_id(&item.id) {
            return Err(bad("Invalid item id"));
        }
        if !item_ids.insert(item.id.as_str()) {
            return Err(bad("Duplicate item id"));
        }
        if WidgetKind::from_str_opt(&item.kind).is_none() {
            return Err(bad(format!("Unknown widget kind '{}'", item.kind)));
        }
        if !item.options.is_null() && !item.options.is_object() {
            return Err(bad("Item options must be an object"));
        }
        validate_advanced_options(&item.advanced_options)?;

        let mut seen = HashSet::new();
        for l in &item.layouts {
            let Some(&cols) = columns.get(l.layout_id.as_str()) else {
                return Err(bad("Item references an unknown layout"));
            };
            if !section_ids.contains(l.section_id.as_str()) {
                return Err(bad("Item references an unknown section"));
            }
            if !seen.insert((l.layout_id.as_str(), l.section_id.as_str())) {
                return Err(bad("Duplicate item placement"));
            }
            if l.x_offset < 0 || l.y_offset < 0 || l.width < 1 || l.height < 1 {
                return Err(bad("Item placement must be positive"));
            }
            if l.x_offset > MAX_COORD
                || l.y_offset > MAX_COORD
                || l.width > MAX_COORD
                || l.height > MAX_COORD
            {
                return Err(bad("Item placement out of range"));
            }
            if l.x_offset + l.width > cols {
                return Err(bad(format!(
                    "Item placement exceeds the layout's {cols} columns"
                )));
            }
        }
        let mut unique_integrations = HashSet::new();
        for iid in &item.integration_ids {
            if !unique_integrations.insert(iid.as_str()) {
                return Err(bad("Duplicate integration id on item"));
            }
        }
    }
    Ok(())
}

fn validate_advanced_options(adv: &serde_json::Value) -> HomarrResult<()> {
    if adv.is_null() {
        return Ok(());
    }
    let Some(obj) = adv.as_object() else {
        return Err(bad("Advanced options must be an object"));
    };
    match obj.get("title") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::String(t)) if t.chars().count() <= MAX_NAME_LEN => {}
        _ => return Err(bad("Item title must be a string of at most 64 characters")),
    }
    match obj.get("customCssClasses") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Array(list)) if list.len() <= MAX_CSS_CLASSES => {
            for c in list {
                if !c.as_str().is_some_and(valid_css_class) {
                    return Err(bad(
                        "Custom CSS classes may only contain letters, digits, '-' and '_'",
                    ));
                }
            }
        }
        _ => return Err(bad("customCssClasses must be a list of class names")),
    }
    match obj.get("borderColor") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::String(c)) if c.is_empty() || valid_hex_color(c) => {}
        _ => return Err(bad("borderColor must be a hex color")),
    }
    Ok(())
}

/// Ensures every integration referenced by an item exists and the user may use any newly
/// attached integrations (`existing_items` allows board editors to keep existing widgets whose
/// integrations were configured by another user).
pub async fn validate_item_integrations(
    db: &Database,
    session: &AuthSession,
    items: &[SaveBoardItemInput],
    existing_items: &[ItemRecord],
) -> HomarrResult<()> {
    let known: HashSet<String> = homarr_db::repos::integrations::list_all(db)
        .await?
        .into_iter()
        .map(|i| i.id)
        .collect();
    let existing_by_item: HashMap<&str, HashSet<&str>> = existing_items
        .iter()
        .map(|it| {
            (
                it.id.as_str(),
                it.integration_ids.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let mut checked = HashSet::new();
    for item in items {
        let existing_for_item = existing_by_item.get(item.id.as_str());
        for iid in &item.integration_ids {
            if !known.contains(iid) {
                return Err(bad("Item references an unknown integration"));
            }
            let already_attached = existing_for_item.is_some_and(|set| set.contains(iid.as_str()));
            if !already_attached && checked.insert(iid.clone()) {
                check_integration_permission(db, session, iid, IntegrationPermission::Use).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_db::models::ItemLayoutRecord;
    use serde_json::json;

    fn layout(id: &str, cols: i32) -> LayoutRecord {
        LayoutRecord {
            id: id.into(),
            name: id.into(),
            board_id: "b".into(),
            column_count: cols,
            left_gutter_column_count: 0,
            right_gutter_column_count: 0,
            breakpoint: 0,
            role: "base".into(),
            resize_source: None,
        }
    }

    fn section(id: &str) -> SaveBoardSectionInput {
        SaveBoardSectionInput {
            id: id.into(),
            kind: "empty".into(),
            x_offset: Some(0),
            y_offset: Some(0),
            name: None,
            options: json!({}),
            layouts: vec![],
        }
    }

    fn item(x: i32, w: i32) -> SaveBoardItemInput {
        SaveBoardItemInput {
            id: "i1".into(),
            kind: "clock".into(),
            options: json!({}),
            advanced_options: json!({ "title": null, "customCssClasses": ["a-b"], "borderColor": "#fa5252" }),
            layouts: vec![ItemLayoutRecord {
                item_id: "i1".into(),
                section_id: "s1".into(),
                layout_id: "l1".into(),
                x_offset: x,
                y_offset: 0,
                width: w,
                height: 1,
            }],
            integration_ids: vec![],
        }
    }

    #[test]
    fn accepts_valid_and_rejects_out_of_bounds() {
        let layouts = [layout("l1", 4)];
        let sections = [section("s1")];
        assert!(validate_board_content(&layouts, &sections, &[item(0, 4)]).is_ok());
        assert!(validate_board_content(&layouts, &sections, &[item(2, 3)]).is_err());
        assert!(validate_board_content(&layouts, &sections, &[item(-1, 1)]).is_err());
    }

    #[test]
    fn rejects_bad_advanced_options() {
        let layouts = [layout("l1", 4)];
        let sections = [section("s1")];
        let mut it = item(0, 1);
        it.advanced_options = json!({ "customCssClasses": ["x\" onclick=\"y"] });
        assert!(validate_board_content(&layouts, &sections, &[it]).is_err());
        let mut it = item(0, 1);
        it.advanced_options = json!({ "borderColor": "red;} body{display:none" });
        assert!(validate_board_content(&layouts, &sections, &[it]).is_err());
    }
}
