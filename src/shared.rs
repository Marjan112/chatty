use std::sync::{Mutex, atomic::AtomicBool};
use ratatui::text::Line;

use crate::ChatColor;
use crate::ActivePopup;

#[derive(Default)]
pub struct Shared {
    pub messages: Mutex<Vec<Line<'static>>>,
    pub name: Mutex<String>,
    pub color: Mutex<ChatColor>,
    pub exit: AtomicBool,
    pub connection: AtomicBool,
    pub popup: Mutex<Option<ActivePopup>>,
    pub clients: Mutex<Vec<(String, ChatColor)>>
}
