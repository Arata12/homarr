pub mod api_keys;
pub mod apps;
pub mod assistant;
pub mod boards;
pub mod custom_widgets;
pub mod groups;
pub mod icons;
pub mod integrations;
pub mod invites;
pub mod item_states;
pub mod medias;
pub mod search_engines;
pub mod sessions;
pub mod settings;
pub mod users;

use chrono::{DateTime, NaiveDateTime, Utc};

pub(crate) fn ms_to_naive(ms: i64) -> NaiveDateTime {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(Utc::now)
        .naive_utc()
}

pub(crate) fn secs_to_naive(secs: i64) -> NaiveDateTime {
    DateTime::<Utc>::from_timestamp(secs, 0)
        .unwrap_or_else(Utc::now)
        .naive_utc()
}

pub(crate) fn naive_to_ms(dt: NaiveDateTime) -> i64 {
    dt.and_utc().timestamp_millis()
}

pub(crate) fn naive_to_secs(dt: NaiveDateTime) -> i64 {
    dt.and_utc().timestamp()
}
