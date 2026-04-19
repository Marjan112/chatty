#![allow(dead_code)]

use ratatui::{text::Line, style::Stylize};
use chrono::{format::{DelayedFormat, StrftimeItems}, Local, TimeZone};

use crate::env::CHATTY_VERSION;

pub fn datetime_from_timestamp(secs: i64) -> DelayedFormat<StrftimeItems<'static>> {
    Local.timestamp_opt(secs, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M")
}

pub fn greet_message() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "Welcome to ".into(),
            "ChaTTY ".yellow(),
            CHATTY_VERSION.yellow(),
            "!".into()
        ]),
        Line::from(vec![
            "Type and press ".into(),
            "ENTER".yellow(),
            " to send".into()
        ]),
        Line::from(vec![
            "Type ".into(),
            "/help".yellow(),
            " for help".into()
        ]),
        Line::from(vec![
            "Press ".into(),
            "ESC".yellow(),
            " to exit".into()
        ])
    ]
}
