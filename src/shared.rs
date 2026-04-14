use std::sync::{Mutex, atomic::AtomicBool};
use ratatui::{
    text::Line,
    style::Stylize
};

use crate::CHATTY_VERSION;
use crate::ChatColor;

pub struct Shared {
    pub messages: Mutex<Vec<Line<'static>>>,
    pub name: Mutex<String>,
    pub color: Mutex<ChatColor>,
    pub exit: AtomicBool,
    pub connection: AtomicBool
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            messages: Mutex::new(vec![
                Line::from(vec![
                    "Welcome to ".into(),
                    "ChaTTY ".yellow(),
                    CHATTY_VERSION.yellow(),
                    "!".into()
                ]),
                Line::from(vec![
                    "Use ".into(),
                    "UP".yellow(),
                    "/".into(),
                    "DOWN".yellow(),
                    " to scroll".into()
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
            ]),
            exit: AtomicBool::default(),
            color: Mutex::default(),
            name: Mutex::default(),
            connection: AtomicBool::default()
        }
    }
}
