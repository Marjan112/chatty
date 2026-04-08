use std::sync::{Mutex, atomic::AtomicBool};
use ratatui::{
    text::Line,
    style::Stylize
};

use crate::CHATTY_VERSION;
use crate::ChatColor;

pub struct Shared {
    pub messages: Mutex<Vec<Line<'static>>>,
    pub after_disconnect_messages: Mutex<Vec<String>>,
    pub exit: AtomicBool,
    pub name: Mutex<String>,
    pub color: Mutex<ChatColor>
}

impl Shared {
    pub fn new(name: String) -> Self {
        Self {
            messages: Mutex::new(
                vec![
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
                ]
            ),
            after_disconnect_messages: Mutex::new(Vec::new()),
            exit: AtomicBool::new(false),
            name: Mutex::new(name),
            color: Mutex::new(ChatColor::Reset)
        }
    }
}