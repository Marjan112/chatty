macro_rules! chat_error {
    ($messages:expr, $($arg:tt)*) => {
        $messages.add_message(Line::styled(format!("ERROR: {}", format!($($arg)*)), Style::default().fg(Color::LightRed)))
    }
}

macro_rules! chat_warn {
    ($messages:expr, $($arg:tt)*) => {
        $messages.add_message(Line::styled(format!("WARNING: {}", format!($($arg)*)), Style::default().fg(Color::Yellow)))
    }
}

pub(crate) use chat_error;
pub(crate) use chat_warn;
