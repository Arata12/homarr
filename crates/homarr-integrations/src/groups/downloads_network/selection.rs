//! Port of `download-client/select-torrents.ts`: category filter, completed-torrent filter, sorting and limit
//! for torrent clients that cannot do this server side.

use std::cmp::Ordering;

use crate::capabilities::downloads::{
    DownloadClientCategory, DownloadClientItem, DownloadClientRatioItem, DownloadClientSelection,
};

/// A torrent already mapped to a [`DownloadClientItem`], plus the ratio the TS `fields.ratio` callback exposes
/// for sorting (it is not part of the item itself).
pub struct Candidate {
    pub item: DownloadClientItem,
    pub ratio: Option<f64>,
}

/// Fields of the TS `fields` map a selection can sort by.
const SORT_FIELDS: &[&str] = &[
    "name",
    "state",
    "category",
    "progress",
    "size",
    "index",
    "time",
    "upSpeed",
    "downSpeed",
    "added",
    "sent",
    "received",
    "ratio",
];

enum SortValue {
    Num(f64),
    Str(String),
    Many,
}

impl SortValue {
    fn rank(&self) -> u8 {
        match self {
            Self::Num(_) => 0,
            Self::Str(_) => 1,
            Self::Many => 2,
        }
    }
}

fn state_name(item: &DownloadClientItem) -> String {
    serde_json::to_value(item.state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn sort_value(candidate: &Candidate, field: &str) -> Option<SortValue> {
    let item = &candidate.item;
    let opt = |value: Option<f64>| Some(SortValue::Num(value.unwrap_or(0.0)));
    match field {
        "name" => Some(SortValue::Str(item.name.clone())),
        "state" => Some(SortValue::Str(state_name(item))),
        "category" => Some(match &item.category {
            None => SortValue::Num(0.0),
            Some(DownloadClientCategory::One(category)) => SortValue::Str(category.clone()),
            Some(DownloadClientCategory::Many(_)) => SortValue::Many,
        }),
        "progress" => Some(SortValue::Num(item.progress)),
        "size" => Some(SortValue::Num(item.size)),
        "index" => Some(SortValue::Num(item.index)),
        "time" => Some(SortValue::Num(item.time)),
        "upSpeed" => opt(item.up_speed),
        "downSpeed" => opt(item.down_speed),
        "added" => opt(item.added),
        "sent" => opt(item.sent),
        "received" => opt(item.received),
        "ratio" => opt(candidate.ratio),
        _ => None,
    }
}

fn locale_compare(left: &str, right: &str) -> Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

fn compare(left: &SortValue, right: &SortValue, multiplier: f64) -> Ordering {
    match (left, right) {
        (SortValue::Num(l), SortValue::Num(r)) => {
            if l == r {
                return Ordering::Equal;
            }
            let diff = (l - r) * multiplier;
            if diff > 0.0 {
                Ordering::Greater
            } else if diff < 0.0 {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }
        (SortValue::Str(l), SortValue::Str(r)) => {
            let ordering = locale_compare(l, r);
            if multiplier < 0.0 {
                ordering.reverse()
            } else {
                ordering
            }
        }
        (SortValue::Many, SortValue::Many) => Ordering::Equal,
        // The TS comparator answers 0 for mixed types; a fixed order keeps the sort total.
        (l, r) => l.rank().cmp(&r.rank()),
    }
}

fn is_eligible(item: &DownloadClientItem, selection: &DownloadClientSelection) -> bool {
    if !selection.category_filter.is_empty() {
        let categories: Vec<&str> = match &item.category {
            None => vec![""],
            Some(DownloadClientCategory::One(category)) => vec![category.as_str()],
            Some(DownloadClientCategory::Many(categories)) => {
                categories.iter().map(String::as_str).collect()
            }
        };
        let matches = selection
            .category_filter
            .iter()
            .any(|filter| categories.contains(&filter.as_str()));
        if matches != selection.filter_is_whitelist {
            return false;
        }
    }
    if item.progress < 1.0 {
        return true;
    }
    selection.show_completed_torrent
        && item.up_speed.unwrap_or(0.0) >= selection.active_torrent_threshold * 1024.0
}

pub fn select(
    candidates: Vec<Candidate>,
    limit: usize,
    selection: Option<&DownloadClientSelection>,
) -> Vec<DownloadClientItem> {
    let Some(selection) = selection else {
        return candidates
            .into_iter()
            .take(limit)
            .map(|candidate| candidate.item)
            .collect();
    };
    let mut eligible: Vec<Candidate> = candidates
        .into_iter()
        .filter(|candidate| is_eligible(&candidate.item, selection))
        .collect();

    if SORT_FIELDS.contains(&selection.sort.as_str()) {
        let multiplier = if selection.descending { -1.0 } else { 1.0 };
        let is_time = selection.sort == "time";
        let value_of = |candidate: &Candidate| {
            let value = sort_value(candidate, &selection.sort).unwrap_or(SortValue::Num(0.0));
            match value {
                SortValue::Num(number) if is_time && number == 0.0 => SortValue::Num(f64::INFINITY),
                other => other,
            }
        };
        eligible.sort_by(|left, right| compare(&value_of(left), &value_of(right), multiplier));
    }
    eligible
        .into_iter()
        .take(limit)
        .map(|candidate| candidate.item)
        .collect()
}

/// The first `limit` items as accounting entries, independent of the display selection.
pub fn ratio_items(items: &[DownloadClientItem], limit: usize) -> Vec<DownloadClientRatioItem> {
    items
        .iter()
        .take(limit)
        .map(|item| DownloadClientRatioItem {
            sent: item.sent,
            size: item.size,
            progress: item.progress,
            category: item.category.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::downloads::{DownloadClientItemState, DownloadClientType};

    fn item(id: &str, progress: f64, up_speed: f64, category: Option<&str>) -> Candidate {
        Candidate {
            item: DownloadClientItem {
                id: id.into(),
                index: 0.0,
                name: id.into(),
                item_type: DownloadClientType::Torrent,
                size: 10.0,
                sent: Some(1.0),
                received: None,
                down_speed: None,
                up_speed: Some(up_speed),
                time: 0.0,
                added: None,
                state: DownloadClientItemState::Seeding,
                progress,
                category: category.map(|c| DownloadClientCategory::One(c.into())),
            },
            ratio: None,
        }
    }

    fn selection() -> DownloadClientSelection {
        DownloadClientSelection {
            sort: "name".into(),
            descending: false,
            category_filter: vec![],
            filter_is_whitelist: false,
            show_completed_torrent: false,
            active_torrent_threshold: 0.0,
        }
    }

    fn ids(items: Vec<DownloadClientItem>) -> Vec<String> {
        items.into_iter().map(|i| i.id).collect()
    }

    #[test]
    fn without_selection_only_limits() {
        let out = select(
            vec![item("b", 1.0, 0.0, None), item("a", 0.5, 0.0, None)],
            1,
            None,
        );
        assert_eq!(ids(out), vec!["b"]);
    }

    #[test]
    fn hides_completed_torrents_unless_active_enough() {
        let mut sel = selection();
        let candidates = || {
            vec![
                item("done", 1.0, 5.0 * 1024.0, None),
                item("busy", 0.5, 0.0, None),
            ]
        };
        assert_eq!(ids(select(candidates(), 10, Some(&sel))), vec!["busy"]);
        sel.show_completed_torrent = true;
        sel.active_torrent_threshold = 4.0;
        assert_eq!(
            ids(select(candidates(), 10, Some(&sel))),
            vec!["busy", "done"]
        );
        sel.active_torrent_threshold = 6.0;
        assert_eq!(ids(select(candidates(), 10, Some(&sel))), vec!["busy"]);
    }

    #[test]
    fn category_whitelist_and_blacklist() {
        let mut sel = selection();
        sel.category_filter = vec!["tv".into()];
        sel.filter_is_whitelist = true;
        let candidates = || {
            vec![
                item("a", 0.1, 0.0, Some("tv")),
                item("b", 0.1, 0.0, Some("movies")),
                item("c", 0.1, 0.0, None),
            ]
        };
        assert_eq!(ids(select(candidates(), 10, Some(&sel))), vec!["a"]);
        sel.filter_is_whitelist = false;
        assert_eq!(ids(select(candidates(), 10, Some(&sel))), vec!["b", "c"]);
    }

    #[test]
    fn sorts_descending_and_limits() {
        let mut sel = selection();
        sel.sort = "name".into();
        sel.descending = true;
        let out = select(
            vec![
                item("a", 0.1, 0.0, None),
                item("c", 0.1, 0.0, None),
                item("b", 0.1, 0.0, None),
            ],
            2,
            Some(&sel),
        );
        assert_eq!(ids(out), vec!["c", "b"]);
    }

    #[test]
    fn time_sort_treats_zero_as_infinite() {
        let mut sel = selection();
        sel.sort = "time".into();
        let mut a = item("a", 0.1, 0.0, None);
        a.item.time = 0.0;
        let mut b = item("b", 0.1, 0.0, None);
        b.item.time = 5000.0;
        assert_eq!(ids(select(vec![a, b], 10, Some(&sel))), vec!["b", "a"]);
    }
}
