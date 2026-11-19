use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::error::{HomarrError, HomarrResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GridItemType {
    Item,
    Section,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridAlgorithmItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: GridItemType,
    pub width: i32,
    pub height: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub section_id: String,
}

#[derive(Debug, Clone)]
pub struct GridAlgorithmInput<'a> {
    pub items: &'a [GridAlgorithmItem],
    pub width: i32,
    pub previous_width: i32,
    pub section_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridAlgorithmOutput {
    pub height: i32,
    pub items: Vec<GridAlgorithmItem>,
}

/// Responsive grid placement algorithm matching `packages/common/src/grid-algorithm.ts`.
pub fn generate_responsive_grid_for(
    input: GridAlgorithmInput<'_>,
) -> HomarrResult<GridAlgorithmOutput> {
    let mut items_by_section: HashMap<&str, Vec<&GridAlgorithmItem>> = HashMap::new();
    for item in input.items {
        items_by_section
            .entry(item.section_id.as_str())
            .or_default()
            .push(item);
    }

    let ancestors = HashSet::new();
    generate_responsive_grid_for_section(
        &items_by_section,
        input.previous_width,
        input.width,
        input.section_id,
        &ancestors,
    )
}

fn generate_responsive_grid_for_section(
    items_by_section: &HashMap<&str, Vec<&GridAlgorithmItem>>,
    previous_width: i32,
    width: i32,
    section_id: &str,
    ancestor_section_ids: &HashSet<String>,
) -> HomarrResult<GridAlgorithmOutput> {
    if ancestor_section_ids.contains(section_id) {
        return Ok(GridAlgorithmOutput {
            height: 0,
            items: Vec::new(),
        });
    }

    let mut next_ancestors = ancestor_section_ids.clone();
    next_ancestors.insert(section_id.to_string());

    let mut items_of_current_section = items_by_section
        .get(section_id)
        .cloned()
        .unwrap_or_default();

    if items_of_current_section.is_empty() {
        return Ok(GridAlgorithmOutput {
            height: 0,
            items: Vec::new(),
        });
    }

    items_of_current_section.sort_by(|a, b| {
        a.y_offset
            .cmp(&b.y_offset)
            .then_with(|| a.x_offset.cmp(&b.x_offset))
    });

    let column_count = width.max(1);
    let normalized_items: Vec<(GridAlgorithmItem, i32)> = items_of_current_section
        .iter()
        .map(|item| {
            let previous_item_width = item.width;
            let mut cloned = (*item).clone();
            cloned.width = cloned.width.min(column_count).max(1);
            (cloned, previous_item_width)
        })
        .collect();

    let mut new_items: Vec<GridAlgorithmItem> = Vec::new();
    let mut dynamic_section_height_map: HashMap<String, i32> = HashMap::new();

    for (dynamic_section, prev_w) in &normalized_items {
        if dynamic_section.item_type == GridItemType::Section {
            let result = generate_responsive_grid_for_section(
                items_by_section,
                *prev_w,
                dynamic_section.width,
                &dynamic_section.id,
                &next_ancestors,
            )?;
            new_items.extend(result.items);
            dynamic_section_height_map.insert(dynamic_section.id.clone(), result.height);
        }
    }

    if width >= previous_width {
        let height = items_of_current_section
            .iter()
            .map(|item| item.y_offset + item.height)
            .max()
            .unwrap_or(0);
        new_items.extend(normalized_items.into_iter().map(|(item, _)| item));
        return Ok(GridAlgorithmOutput {
            height,
            items: new_items,
        });
    }

    let mut occupied_2d: Vec<Vec<bool>> = Vec::new();
    for (mut item, _) in normalized_items {
        if item.item_type == GridItemType::Section {
            let dyn_height = dynamic_section_height_map
                .get(&item.id)
                .copied()
                .unwrap_or(1);
            item.height = dyn_height.max(item.height);
        }
        let pos = next_free_spot(&occupied_2d, &item, column_count)
            .ok_or_else(|| HomarrError::Internal("No free spot available in grid".to_string()))?;
        add_item_to_occupied(&mut occupied_2d, &item, pos, column_count);
        item.x_offset = pos.0;
        item.y_offset = pos.1;
        new_items.push(item);
    }

    Ok(GridAlgorithmOutput {
        height: occupied_2d.len() as i32,
        items: new_items,
    })
}

fn add_item_to_occupied(
    occupied_2d: &mut Vec<Vec<bool>>,
    item: &GridAlgorithmItem,
    position: (i32, i32),
    column_count: i32,
) {
    let height = item.height.max(1) as usize;
    let width = item.width.max(1) as usize;
    let pos_x = position.0.max(0) as usize;
    let pos_y = position.1.max(0) as usize;
    let cols = column_count.max(1) as usize;

    for y_offset in 0..height {
        let target_y = pos_y + y_offset;
        while occupied_2d.len() <= target_y {
            occupied_2d.push(vec![false; cols]);
        }
        for x_offset in 0..width {
            if pos_x + x_offset < cols {
                occupied_2d[target_y][pos_x + x_offset] = true;
            }
        }
    }
}

fn next_free_spot(
    occupied_2d: &[Vec<bool>],
    item: &GridAlgorithmItem,
    column_count: i32,
) -> Option<(i32, i32)> {
    for offset_y in 0..99_999 {
        for offset_x in 0..column_count {
            if offset_x + item.width <= column_count
                && is_free(occupied_2d, item, (offset_x, offset_y))
            {
                return Some((offset_x, offset_y));
            }
        }
    }
    None
}

fn is_free(occupied_2d: &[Vec<bool>], item: &GridAlgorithmItem, position: (i32, i32)) -> bool {
    let height = item.height.max(1) as usize;
    let width = item.width.max(1) as usize;
    let pos_x = position.0 as usize;
    let pos_y = position.1 as usize;

    for y_offset in 0..height {
        let Some(row) = occupied_2d.get(pos_y + y_offset) else {
            return true;
        };
        for x_offset in 0..width {
            if row.get(pos_x + x_offset).copied().unwrap_or(false) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflows_items_when_shrinking_columns() {
        let items = vec![
            GridAlgorithmItem {
                id: "a".into(),
                item_type: GridItemType::Item,
                width: 4,
                height: 1,
                x_offset: 0,
                y_offset: 0,
                section_id: "root".into(),
            },
            GridAlgorithmItem {
                id: "b".into(),
                item_type: GridItemType::Item,
                width: 4,
                height: 2,
                x_offset: 4,
                y_offset: 0,
                section_id: "root".into(),
            },
        ];

        let out = generate_responsive_grid_for(GridAlgorithmInput {
            items: &items,
            width: 4,
            previous_width: 10,
            section_id: "root",
        })
        .unwrap();

        assert_eq!(out.height, 3);
        let b = out.items.iter().find(|i| i.id == "b").unwrap();
        assert_eq!((b.x_offset, b.y_offset, b.width), (0, 1, 4));
    }

    #[test]
    fn guards_against_section_cycles() {
        let items = vec![GridAlgorithmItem {
            id: "sec1".into(),
            item_type: GridItemType::Section,
            width: 2,
            height: 1,
            x_offset: 0,
            y_offset: 0,
            section_id: "sec1".into(),
        }];

        let out = generate_responsive_grid_for(GridAlgorithmInput {
            items: &items,
            width: 2,
            previous_width: 4,
            section_id: "sec1",
        })
        .unwrap();

        assert_eq!(out.items.len(), 1);
    }
}
