use std::sync::{Mutex, atomic::AtomicBool};
use ratatui::{text::Line, style::Color};

use crate::ActivePopup;
use crate::MAX_MESSAGES;

#[derive(Default)]
pub struct Shared {
    pub messages: Mutex<Vec<Line<'static>>>,
    pub name: Mutex<String>,
    pub color: Mutex<Color>,
    pub exit: AtomicBool,
    pub connection: AtomicBool,
    pub popup: Mutex<Option<ActivePopup>>,
    pub clients: Mutex<Vec<(String, Color)>>
}

impl Shared {
    pub fn add_message(&self, line: Line<'static>) {
        let mut messages = self.messages.lock().unwrap();
        messages.push(line);

        let messages_len = messages.len();

        if messages_len > MAX_MESSAGES {
            messages.drain(..messages_len - MAX_MESSAGES);
        }
    }
}
