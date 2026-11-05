#[path = "colour/x11.rs"]
mod x11;

use std::fmt;

const THEME_NAMES: [&str; 10] = [
    "themeblack",
    "themewhite",
    "themelightgrey",
    "themedarkgrey",
    "themegreen",
    "themeyellow",
    "themered",
    "themeblue",
    "themecyan",
    "thememagenta",
];

const BASIC_NAMES: [(&str, u8); 16] = [
    ("black", 0),
    ("red", 1),
    ("green", 2),
    ("yellow", 3),
    ("blue", 4),
    ("magenta", 5),
    ("cyan", 6),
    ("white", 7),
    ("brightblack", 90),
    ("brightred", 91),
    ("brightgreen", 92),
    ("brightyellow", 93),
    ("brightblue", 94),
    ("brightmagenta", 95),
    ("brightcyan", 96),
    ("brightwhite", 97),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Colour {
    Default,
    Terminal,
    Basic(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
    Theme(u8),
}

impl Colour {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.first() == Some(&b'#') && bytes.len() == 7 {
            let digits = &text[1..];
            if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            let channel = |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).ok();
            return Some(Self::Rgb(channel(0)?, channel(2)?, channel(4)?));
        }
        for prefix in ["colour", "color"] {
            if starts_with_ignore_case(text, prefix) {
                let index = strtonum(&text[prefix.len()..], 0, 255)?;
                return Some(Self::Indexed(u8::try_from(index).ok()?));
            }
        }
        let lower = text.to_ascii_lowercase();
        match lower.as_str() {
            "default" => return Some(Self::Default),
            "terminal" => return Some(Self::Terminal),
            _ => {}
        }
        if let Some(index) = THEME_NAMES.iter().position(|name| *name == lower) {
            return Some(Self::Theme(u8::try_from(index).ok()?));
        }
        for (name, code) in BASIC_NAMES {
            if lower == name || text == code.to_string() {
                return Some(Self::Basic(code));
            }
        }
        Self::from_x11_name(&lower)
    }

    fn from_x11_name(lower: &str) -> Option<Self> {
        let shade = lower.strip_prefix("grey").or_else(|| lower.strip_prefix("gray"));
        if let Some(shade) = shade {
            if shade.is_empty() {
                return Some(Self::Rgb(0xbe, 0xbe, 0xbe));
            }
            let percent = strtonum(shade, 0, 100)?;
            let level = u8::try_from((2.55 * percent as f64).round() as i64).ok()?;
            return Some(Self::Rgb(level, level, level));
        }
        let index = x11::X11_COLOURS
            .binary_search_by_key(&lower, |(name, _)| name)
            .ok()?;
        let rgb = x11::X11_COLOURS[index].1;
        Some(Self::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
    }
}

impl fmt::Display for Colour {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => formatter.write_str("default"),
            Self::Terminal => formatter.write_str("terminal"),
            Self::Basic(code) => formatter.write_str(
                BASIC_NAMES
                    .iter()
                    .find(|(_, candidate)| candidate == code)
                    .map_or("invalid", |(name, _)| name),
            ),
            Self::Indexed(index) => write!(formatter, "colour{index}"),
            Self::Rgb(red, green, blue) => write!(formatter, "#{red:02x}{green:02x}{blue:02x}"),
            Self::Theme(index) => formatter.write_str(THEME_NAMES[usize::from(*index)]),
        }
    }
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

pub(crate) fn strtonum(text: &str, minimum: i64, maximum: i64) -> Option<i64> {
    let trimmed = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude = digits.parse::<i128>().ok()?;
    let number = if negative { -magnitude } else { magnitude };
    i64::try_from(number)
        .ok()
        .filter(|number| (minimum..=maximum).contains(number))
}
