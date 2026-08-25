macro_rules! chat_error {
    ($messages:expr, $($arg:tt)*) => {
        $messages.push(Line::styled(format!("ERROR: {}", format!($($arg)*)), Style::default().fg(Color::LightRed)))
    }
}

pub(crate) use chat_error;
