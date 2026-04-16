use std::sync::{Mutex, atomic::AtomicBool};
use ratatui::text::Line;

use crate::ChatColor;
use crate::ActivePopup;
use crate::greet_message;

pub struct Shared {
    pub messages: Mutex<Vec<Line<'static>>>,
    pub name: Mutex<String>,
    pub color: Mutex<ChatColor>,
    pub exit: AtomicBool,
    pub connection: AtomicBool,
    pub popup: Mutex<Option<ActivePopup>>
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            messages: Mutex::new(greet_message()),
            exit: AtomicBool::default(),
            color: Mutex::default(),
            name: Mutex::default(),
            connection: AtomicBool::default(),
            popup: Mutex::default()
        }
    }
}
