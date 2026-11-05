use crate::colour::{strtonum, Colour};

const STYLE_DELIMITERS: [char; 3] = [' ', ',', '\n'];
const ATTRIBUTE_DELIMITERS: [char; 3] = [' ', ',', '|'];
const MAXIMUM_TOKEN_LENGTH: usize = 255;
const ATTRIBUTES: [&str; 15] = [
    "acs",
    "bright",
    "bold",
    "dim",
    "underscore",
    "blink",
    "reverse",
    "hidden",
    "italics",
    "strikethrough",
    "double-underscore",
    "curly-underscore",
    "dotted-underscore",
    "dashed-underscore",
    "overline",
];

pub(crate) fn is_valid_style(text: &str) -> bool {
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(STYLE_DELIMITERS);
        if rest.is_empty() {
            return true;
        }
        let end = rest.find(STYLE_DELIMITERS).unwrap_or(rest.len());
        if end > MAXIMUM_TOKEN_LENGTH || !is_valid_token(&rest[..end]) {
            return false;
        }
        rest = &rest[end..];
    }
}

pub(crate) fn is_valid_colour_text(text: &str) -> bool {
    text.is_empty() || Colour::parse(text).is_some()
}

fn is_valid_token(token: &str) -> bool {
    let end = token.len();
    let lower = token.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "default"
            | "ignore"
            | "noignore"
            | "push-default"
            | "pop-default"
            | "set-default"
            | "nolist"
            | "norange"
            | "noalign"
            | "none"
    ) {
        return true;
    }
    if let Some(list) = lower.strip_prefix("list=") {
        return matches!(list, "on" | "focus" | "left-marker" | "right-marker");
    }
    if end > 6 && lower.starts_with("range=") {
        return is_valid_range(&token[6..]);
    }
    if end > 6 && lower.starts_with("align=") {
        return matches!(&lower[6..], "left" | "centre" | "right" | "absolute-centre");
    }
    if end > 5 && lower.starts_with("fill=") {
        return Colour::parse(&token[5..]).is_some();
    }
    if end > 4 && lower.starts_with("dim=") {
        let value = token[4..].strip_suffix('%').unwrap_or(&token[4..]);
        return strtonum(value, 0, 100).is_some();
    }
    if end > 3 && lower.as_bytes()[1..].starts_with(b"g=") {
        return Colour::parse(&token[3..]).is_some()
            && matches!(lower.as_bytes()[0], b'f' | b'b');
    }
    if end > 3 && lower.starts_with("us=") {
        return Colour::parse(&token[3..]).is_some();
    }
    if end > 2 && lower.starts_with("no") {
        return matches!(&token[2..], "link" | "attr") || is_valid_attributes(&token[2..]);
    }
    if end > 6 && lower.starts_with("width=") {
        return match token[6..].strip_suffix('%').filter(|_| end > 7) {
            Some(percentage) => strtonum(percentage, 0, 100).is_some(),
            None => strtonum(&token[6..], 0, i64::from(u32::MAX)).is_some(),
        };
    }
    if end > 4 && lower.starts_with("pad=") {
        return strtonum(&token[4..], 0, i64::from(u32::MAX)).is_some();
    }
    if lower.starts_with("link=") {
        return true;
    }
    is_valid_attributes(token)
}

fn is_valid_range(range: &str) -> bool {
    let (name, argument) = match range.split_once('|') {
        Some((_, "")) => return false,
        Some((name, argument)) => (name, Some(argument)),
        None => (range, None),
    };
    let unsigned = |text: &str| strtonum(text, 0, i64::from(u32::MAX)).is_some();
    match (name.to_ascii_lowercase().as_str(), argument) {
        ("left" | "right", argument) => argument.is_none(),
        ("control", argument) => {
            argument.is_some_and(|argument| strtonum(argument, 0, 9).is_some())
        }
        ("pane", argument) => argument
            .and_then(|argument| argument.strip_prefix('%'))
            .is_some_and(|number| !number.is_empty() && unsigned(number)),
        ("window", argument) => argument.is_some_and(unsigned),
        ("session", argument) => argument
            .and_then(|argument| argument.strip_prefix('$'))
            .is_some_and(|number| !number.is_empty() && unsigned(number)),
        ("user", argument) => argument.is_some(),
        _ => true,
    }
}

fn is_valid_attributes(text: &str) -> bool {
    if text.is_empty()
        || text.starts_with(ATTRIBUTE_DELIMITERS)
        || text.ends_with(ATTRIBUTE_DELIMITERS)
    {
        return false;
    }
    if text.eq_ignore_ascii_case("default") || text.eq_ignore_ascii_case("none") {
        return true;
    }
    text.split(ATTRIBUTE_DELIMITERS)
        .filter(|name| !name.is_empty())
        .all(|name| ATTRIBUTES.iter().any(|attribute| attribute.eq_ignore_ascii_case(name)))
}
