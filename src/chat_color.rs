use serde::{Serialize, Deserialize};
use ratatui::style::Color;
use std::fmt;

#[derive(Default, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ChatColor {
    // The default colors
    #[default]
    Reset,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,

    // Any RGB color
    Rgb(u8, u8, u8)
}

impl From<ChatColor> for Color {
    fn from(chat_color: ChatColor) -> Self {
        match chat_color {
            ChatColor::Reset => Color::Reset,
            ChatColor::Red => Color::Red,
            ChatColor::Green => Color::Green,
            ChatColor::Yellow => Color::Yellow,
            ChatColor::Blue => Color::Blue,
            ChatColor::Magenta => Color::Magenta,
            ChatColor::Cyan => Color::Cyan,
            ChatColor::LightRed => Color::LightRed,
            ChatColor::LightGreen => Color::LightGreen,
            ChatColor::LightYellow => Color::LightYellow,
            ChatColor::LightBlue => Color::LightBlue,
            ChatColor::LightMagenta => Color::LightMagenta,
            ChatColor::Rgb(r, g, b) => Color::Rgb(r, g, b)
        }
    }
}

impl From<Color> for ChatColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Reset => ChatColor::Reset,
            Color::Red => ChatColor::Red,
            Color::Green => ChatColor::Green,
            Color::Yellow => ChatColor::Yellow,
            Color::Blue => ChatColor::Blue,
            Color::Magenta => ChatColor::Magenta,
            Color::Cyan => ChatColor::Cyan,
            Color::LightRed => ChatColor::LightRed,
            Color::LightGreen => ChatColor::LightGreen,
            Color::LightYellow => ChatColor::LightYellow,
            Color::LightBlue => ChatColor::LightBlue,
            Color::LightMagenta => ChatColor::LightMagenta,
            Color::Rgb(r, g, b) => ChatColor::Rgb(r, g, b),
            _ => ChatColor::Reset
        }
    }
}

impl fmt::Display for ChatColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let color: Color = (*self).into();
        write!(f, "{}", color)
    }
}
