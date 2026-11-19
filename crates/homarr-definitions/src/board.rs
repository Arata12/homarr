use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CornerStyle {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundImageAttachment {
    #[default]
    Fixed,
    Scroll,
}

impl BackgroundImageAttachment {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Scroll => "scroll",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BackgroundImageRepeat {
    #[serde(rename = "repeat")]
    Repeat,
    #[serde(rename = "repeat-x")]
    RepeatX,
    #[serde(rename = "repeat-y")]
    RepeatY,
    #[default]
    #[serde(rename = "no-repeat")]
    NoRepeat,
}

impl BackgroundImageRepeat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repeat => "repeat",
            Self::RepeatX => "repeat-x",
            Self::RepeatY => "repeat-y",
            Self::NoRepeat => "no-repeat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundImageSize {
    #[default]
    Cover,
    Contain,
}

impl BackgroundImageSize {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Contain => "contain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutRole {
    Mobile,
    Base,
    #[default]
    Custom,
}

impl LayoutRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mobile => "mobile",
            Self::Base => "base",
            Self::Custom => "custom",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "mobile" => Some(Self::Mobile),
            "base" => Some(Self::Base),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardLayoutNormInput {
    pub id: String,
    pub name: String,
    pub breakpoint: i32,
    pub column_count: i32,
    pub left_gutter_column_count: i32,
    pub right_gutter_column_count: i32,
    pub role: Option<LayoutRole>,
    pub resize_source: Option<String>,
}

const MAX_LAYOUT_BREAKPOINT: i32 = 32_767;

fn compare_layouts(a: &BoardLayoutNormInput, b: &BoardLayoutNormInput) -> std::cmp::Ordering {
    a.breakpoint
        .cmp(&b.breakpoint)
        .then_with(|| a.column_count.cmp(&b.column_count))
        .then_with(|| a.id.cmp(&b.id))
}

pub fn normalize_board_layout_roles(
    source_layouts: &[BoardLayoutNormInput],
) -> Vec<BoardLayoutNormInput> {
    let mobile_layouts: Vec<&BoardLayoutNormInput> = source_layouts
        .iter()
        .filter(|l| l.role == Some(LayoutRole::Mobile))
        .collect();
    let base_layouts: Vec<&BoardLayoutNormInput> = source_layouts
        .iter()
        .filter(|l| l.role == Some(LayoutRole::Base))
        .collect();

    let mobile_layout = if mobile_layouts.len() == 1 {
        Some(mobile_layouts[0])
    } else {
        None
    };
    let base_layout = if base_layouts.len() == 1 {
        Some(base_layouts[0])
    } else {
        None
    };
    let has_protected = mobile_layout.is_some() && base_layout.is_some();

    let mut ordered: Vec<BoardLayoutNormInput> =
        if let (true, Some(mobile)) = (has_protected, mobile_layout) {
            let mut rest: Vec<BoardLayoutNormInput> = source_layouts
                .iter()
                .filter(|l| l.id != mobile.id)
                .cloned()
                .collect();
            rest.sort_by(compare_layouts);
            let mut v = vec![mobile.clone()];
            v.extend(rest);
            v
        } else {
            let mut v = source_layouts.to_vec();
            v.sort_by(compare_layouts);
            v
        };

    let len = ordered.len();
    let mut previous_breakpoint = -1;
    for (index, layout) in ordered.iter_mut().enumerate() {
        let remaining = (len - index - 1) as i32;
        let breakpoint = if index == 0 {
            0
        } else {
            (MAX_LAYOUT_BREAKPOINT - remaining)
                .min((previous_breakpoint + 1).max(layout.breakpoint))
        };
        let role = if !has_protected {
            if index == 0 {
                LayoutRole::Mobile
            } else if index + 1 == len {
                LayoutRole::Base
            } else {
                LayoutRole::Custom
            }
        } else {
            layout.role.unwrap_or(LayoutRole::Custom)
        };
        layout.breakpoint = breakpoint;
        layout.role = Some(role);
        previous_breakpoint = breakpoint;
    }

    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_mobile_custom_and_base_layout_roles() {
        let input = vec![
            BoardLayoutNormInput {
                id: "l2".into(),
                name: "Desktop".into(),
                breakpoint: 1200,
                column_count: 10,
                left_gutter_column_count: 1,
                right_gutter_column_count: 1,
                role: None,
                resize_source: None,
            },
            BoardLayoutNormInput {
                id: "l1".into(),
                name: "Mobile".into(),
                breakpoint: 300,
                column_count: 2,
                left_gutter_column_count: 0,
                right_gutter_column_count: 0,
                role: None,
                resize_source: None,
            },
        ];
        let out = normalize_board_layout_roles(&input);
        assert_eq!(out[0].id, "l1");
        assert_eq!(out[0].breakpoint, 0);
        assert_eq!(out[0].role, Some(LayoutRole::Mobile));
        assert_eq!(out[1].id, "l2");
        assert_eq!(out[1].role, Some(LayoutRole::Base));
    }
}
