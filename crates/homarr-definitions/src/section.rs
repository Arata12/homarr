use std::collections::HashMap;

use homarr_common::grid_algorithm::{
    generate_responsive_grid_for, GridAlgorithmInput, GridAlgorithmItem,
};
use serde::{Deserialize, Serialize};

use crate::board::LayoutRole;

pub const MAX_BOARD_TABS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionKind {
    Empty,
    Container,
}

impl SectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Container => "container",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "empty" => Some(Self::Empty),
            "container" => Some(Self::Container),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardLane {
    Left,
    Main,
    Right,
}

impl BoardLane {
    pub const ALL: [Self; 3] = [Self::Left, Self::Main, Self::Right];

    pub fn root_offset(self) -> i32 {
        match self {
            Self::Left => -1,
            Self::Main => 0,
            Self::Right => 1,
        }
    }
}

pub fn get_root_section_lane(x_offset: Option<i32>) -> BoardLane {
    match x_offset {
        Some(-1) => BoardLane::Left,
        Some(1) => BoardLane::Right,
        _ => BoardLane::Main,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSectionLike {
    pub id: String,
    pub kind: String,
    pub x_offset: Option<i32>,
    pub y_offset: Option<i32>,
}

pub fn compare_root_sections(a: &RootSectionLike, b: &RootSectionLike) -> std::cmp::Ordering {
    a.x_offset
        .unwrap_or(0)
        .cmp(&b.x_offset.unwrap_or(0))
        .then_with(|| a.y_offset.unwrap_or(0).cmp(&b.y_offset.unwrap_or(0)))
        .then_with(|| a.id.cmp(&b.id))
}

pub fn get_board_tab_sections(sections: &[RootSectionLike]) -> Vec<RootSectionLike> {
    let mut tabs: Vec<RootSectionLike> = sections
        .iter()
        .filter(|s| s.kind == "empty" && get_root_section_lane(s.x_offset) == BoardLane::Main)
        .cloned()
        .collect();
    tabs.sort_by(compare_root_sections);
    tabs
}

#[derive(Debug, Clone, Copy)]
pub struct LaneGeometry {
    pub column_count: i32,
    pub left_gutter_column_count: i32,
    pub right_gutter_column_count: i32,
    pub role: LayoutRole,
}

pub fn get_board_lane_column_count(layout: LaneGeometry, lane: BoardLane) -> i32 {
    let total = layout.column_count.max(1);
    let left = if layout.role == LayoutRole::Mobile {
        0
    } else {
        layout.left_gutter_column_count.max(0).min(total - 1)
    };
    let right = if layout.role == LayoutRole::Mobile {
        0
    } else {
        layout
            .right_gutter_column_count
            .max(0)
            .min(total - left - 1)
    };
    match lane {
        BoardLane::Left => left,
        BoardLane::Right => right,
        BoardLane::Main => total - left - right,
    }
}

pub fn project_root_sections(
    sections: &[RootSectionLike],
    elements: &[GridAlgorithmItem],
    source: LaneGeometry,
    target: LaneGeometry,
) -> Option<Vec<GridAlgorithmItem>> {
    let mut roots: Vec<RootSectionLike> = sections
        .iter()
        .filter(|s| s.kind == "empty")
        .cloned()
        .collect();
    roots.sort_by(compare_root_sections);

    let tabs: Vec<&RootSectionLike> = roots
        .iter()
        .filter(|r| get_root_section_lane(r.x_offset) == BoardLane::Main)
        .collect();
    let first_tab = *tabs.first()?;

    let mut rail_root_by_lane: HashMap<BoardLane, &RootSectionLike> = HashMap::new();
    for root in &roots {
        let lane = get_root_section_lane(root.x_offset);
        if lane != BoardLane::Main {
            rail_root_by_lane.entry(lane).or_insert(root);
        }
    }

    let lane_by_root_id: HashMap<&str, BoardLane> = roots
        .iter()
        .map(|r| (r.id.as_str(), get_root_section_lane(r.x_offset)))
        .collect();

    let get_target_lane = |lane: BoardLane| -> BoardLane {
        if lane != BoardLane::Main && get_board_lane_column_count(target, lane) == 0 {
            BoardLane::Main
        } else {
            lane
        }
    };

    let remapped_elements: Vec<GridAlgorithmItem> = elements
        .iter()
        .map(|el| {
            let Some(&source_lane) = lane_by_root_id.get(el.section_id.as_str()) else {
                return el.clone();
            };
            if source_lane == BoardLane::Main {
                return el.clone();
            }
            let target_lane = get_target_lane(source_lane);
            let target_root = if target_lane == BoardLane::Main {
                first_tab
            } else {
                rail_root_by_lane
                    .get(&target_lane)
                    .copied()
                    .unwrap_or(first_tab)
            };
            let mut cloned = el.clone();
            cloned.section_id = target_root.id.clone();
            cloned
        })
        .collect();

    let project_root =
        |root_id: &str, width: i32, source_lanes: &[BoardLane]| -> Vec<GridAlgorithmItem> {
            let active_lanes: Vec<BoardLane> = source_lanes
                .iter()
                .copied()
                .filter(|&l| get_board_lane_column_count(source, l) > 0)
                .collect();
            let previous_width = if active_lanes.len() == 1 {
                get_board_lane_column_count(source, active_lanes[0])
            } else {
                i32::MAX
            };
            generate_responsive_grid_for(GridAlgorithmInput {
                items: &remapped_elements,
                previous_width,
                width,
                section_id: root_id,
            })
            .map(|out| out.items)
            .unwrap_or_default()
        };

    let lanes_folding_into = |lane: BoardLane| -> Vec<BoardLane> {
        BoardLane::ALL
            .into_iter()
            .filter(|&src| {
                (src == BoardLane::Main || rail_root_by_lane.contains_key(&src))
                    && get_target_lane(src) == lane
            })
            .collect()
    };

    let mut result = Vec::new();
    for lane in BoardLane::ALL {
        let width = get_board_lane_column_count(target, lane);
        if width == 0 {
            continue;
        }
        if lane != BoardLane::Main {
            if let Some(root) = rail_root_by_lane.get(&lane) {
                result.extend(project_root(&root.id, width, &lanes_folding_into(lane)));
            }
        } else {
            for tab in &tabs {
                if tab.id == first_tab.id {
                    result.extend(project_root(
                        &tab.id,
                        width,
                        &lanes_folding_into(BoardLane::Main),
                    ));
                } else {
                    result.extend(project_root(&tab.id, width, &[BoardLane::Main]));
                }
            }
        }
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_common::grid_algorithm::GridItemType;

    #[test]
    fn projects_rail_into_main_when_mobile_has_no_gutters() {
        let sections = vec![
            RootSectionLike {
                id: "left-rail".into(),
                kind: "empty".into(),
                x_offset: Some(-1),
                y_offset: Some(0),
            },
            RootSectionLike {
                id: "main-tab".into(),
                kind: "empty".into(),
                x_offset: Some(0),
                y_offset: Some(0),
            },
        ];
        let elements = vec![
            GridAlgorithmItem {
                id: "item-left".into(),
                item_type: GridItemType::Item,
                width: 2,
                height: 1,
                x_offset: 0,
                y_offset: 0,
                section_id: "left-rail".into(),
            },
            GridAlgorithmItem {
                id: "item-main".into(),
                item_type: GridItemType::Item,
                width: 4,
                height: 2,
                x_offset: 0,
                y_offset: 0,
                section_id: "main-tab".into(),
            },
        ];
        let out = project_root_sections(
            &sections,
            &elements,
            LaneGeometry {
                column_count: 10,
                left_gutter_column_count: 2,
                right_gutter_column_count: 0,
                role: LayoutRole::Base,
            },
            LaneGeometry {
                column_count: 2,
                left_gutter_column_count: 0,
                right_gutter_column_count: 0,
                role: LayoutRole::Mobile,
            },
        )
        .unwrap();

        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|i| i.section_id == "main-tab"));
        assert!(out.iter().all(|i| i.width <= 2));
    }
}
