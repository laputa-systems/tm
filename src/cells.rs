use unicode_width::UnicodeWidthChar;

pub(crate) fn char_width(character: char, current: usize) -> usize {
    if character == '\t' {
        8 - current % 8
    } else {
        UnicodeWidthChar::width(character).unwrap_or(0)
    }
}

pub(crate) fn display_width(value: &str) -> usize {
    display_width_of(value.chars())
}

pub(crate) fn display_width_of(chars: impl Iterator<Item = char>) -> usize {
    chars.fold(0, |width, character| {
        width.saturating_add(char_width(character, width))
    })
}

pub(crate) fn is_wide(character: char) -> bool {
    UnicodeWidthChar::width(character).is_some_and(|width| width >= 2)
}

pub(crate) fn display_column_to_char_index(line: &str, column: usize) -> usize {
    let mut display_column = 0;
    for (index, character) in line.chars().enumerate() {
        let width = char_width(character, display_column);
        if column < display_column.saturating_add(width) {
            return index;
        }
        display_column = display_column.saturating_add(width);
    }
    line.chars().count()
}

pub(crate) fn char_index_to_display_column(line: &str, index: usize) -> usize {
    display_width_of(line.chars().take(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_combining_and_tab_widths_share_one_model() {
        assert_eq!(display_width("界"), 2);
        assert_eq!(display_width("e\u{301}"), 1);
        assert_eq!(display_width("a\tb"), 9);
        assert_eq!(display_width("🙂"), 2);
        assert!(is_wide('界'));
        assert!(is_wide('🙂'));
        assert!(!is_wide('e'));
    }

    #[test]
    fn display_columns_map_to_logical_characters() {
        assert_eq!(display_column_to_char_index("a界b", 0), 0);
        assert_eq!(display_column_to_char_index("a界b", 1), 1);
        assert_eq!(display_column_to_char_index("a界b", 2), 1);
        assert_eq!(display_column_to_char_index("a界b", 3), 2);
        assert_eq!(display_column_to_char_index("e\u{301}x", 1), 2);
        assert_eq!(char_index_to_display_column("a界b", 2), 3);
    }
}
