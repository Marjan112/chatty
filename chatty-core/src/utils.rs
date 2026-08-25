use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};

pub const MAX_MESSAGES: usize = 200;

pub fn datetime_from_timestamp(secs: i64) -> DelayedFormat<StrftimeItems<'static>> {
    Local.timestamp_opt(secs, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M")
}
