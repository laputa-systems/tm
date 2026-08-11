use std::fs;
use std::path::Path;

/// One logical command from a tmux configuration file. A binding may have
/// commands chained with `\;`; those are retained separately so quoted command
/// prompt templates survive tokenization.
#[derive(Debug, Clone)]
pub(crate) struct ConfigLine {
    pub tokens: Vec<String>,
    pub chained: Vec<Vec<String>>,
}

pub(crate) fn read(path: &Path) -> Result<Vec<ConfigLine>, String> {
    let contents =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(parse(&contents))
}

pub(crate) fn parse(contents: &str) -> Vec<ConfigLine> {
    contents
        .lines()
        .filter_map(|line| {
            let line = strip_comment(line);
            let commands = split_commands(&line)
                .into_iter()
                .map(|command| tokenize(&command))
                .filter(|tokens| !tokens.is_empty())
                .collect::<Vec<_>>();
            let mut commands = commands.into_iter();
            let tokens = commands.next()?;
            Some(ConfigLine {
                tokens,
                chained: commands.collect(),
            })
        })
        .collect()
}

/// Convert a tmux key name into the bytes a terminal normally sends for it.
/// The set is intentionally the set used by the user's configuration, with a
/// few common aliases retained for future config migrations.
pub(crate) fn key_bytes(value: &str) -> Option<Vec<u8>> {
    let value = value.strip_prefix("=").unwrap_or(value);
    match value {
        "Enter" | "C-m" | "C-j" => Some(vec![b'\r']),
        "Tab" | "C-i" => Some(vec![b'\t']),
        "BTab" => Some(b"\x1b[Z".to_vec()),
        "C-S-Tab" => Some(vec![b'\t']),
        "Space" => Some(vec![b' ']),
        "C-Space" => Some(vec![0]),
        "BSpace" => Some(vec![0x7f]),
        "Left" => Some(b"\x1b[D".to_vec()),
        "Right" => Some(b"\x1b[C".to_vec()),
        "Up" => Some(b"\x1b[A".to_vec()),
        "Down" => Some(b"\x1b[B".to_vec()),
        "C-Left" => Some(b"\x1b[1;5D".to_vec()),
        "C-Right" => Some(b"\x1b[1;5C".to_vec()),
        "C-Up" => Some(b"\x1b[1;5A".to_vec()),
        "C-Down" => Some(b"\x1b[1;5B".to_vec()),
        "Home" => Some(b"\x1b[1~".to_vec()),
        "End" => Some(b"\x1b[4~".to_vec()),
        "IC" | "Insert" => Some(b"\x1b[2~".to_vec()),
        "DC" | "Delete" => Some(b"\x1b[3~".to_vec()),
        "PPage" | "PageUp" | "PgUp" => Some(b"\x1b[5~".to_vec()),
        "NPage" | "PageDown" | "PgDn" => Some(b"\x1b[6~".to_vec()),
        "F1" => Some(b"\x1bOP".to_vec()),
        "F2" => Some(b"\x1bOQ".to_vec()),
        "F3" => Some(b"\x1bOR".to_vec()),
        "F4" => Some(b"\x1bOS".to_vec()),
        "F5" => Some(b"\x1b[15~".to_vec()),
        "F6" => Some(b"\x1b[17~".to_vec()),
        "F7" => Some(b"\x1b[18~".to_vec()),
        "F8" => Some(b"\x1b[19~".to_vec()),
        "F9" => Some(b"\x1b[20~".to_vec()),
        "F10" => Some(b"\x1b[21~".to_vec()),
        "F11" => Some(b"\x1b[23~".to_vec()),
        "F12" => Some(b"\x1b[24~".to_vec()),
        "Escape" | "Esc" => Some(vec![0x1b]),
        _ => {
            if let Some(value) = value.strip_prefix("C-") {
                let byte = value.as_bytes().first().copied()?;
                return Some(vec![byte.to_ascii_uppercase() & 0x1f]);
            }
            if let Some(value) = value.strip_prefix("M-C-") {
                let byte = value.as_bytes().first().copied()?;
                return Some(vec![0x1b, byte.to_ascii_uppercase() & 0x1f]);
            }
            if let Some(value) = value.strip_prefix("M-") {
                let byte = value.as_bytes().first().copied()?;
                return Some(vec![0x1b, byte]);
            }
            let bytes = value.as_bytes();
            (bytes.len() == 1).then(|| bytes.to_vec())
        }
    }
}

fn strip_comment(line: &str) -> String {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        match quote {
            Some(current) if current == character => quote = None,
            None if character == '\'' || character == '"' => quote = Some(character),
            None if character == '#' => return line[..index].to_owned(),
            _ => {}
        }
    }
    line.to_owned()
}

fn split_commands(line: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            if characters.peek() == Some(&';') {
                let _ = characters.next();
                output.push(std::mem::take(&mut current));
            } else {
                current.push(character);
                escaped = true;
            }
            continue;
        }
        if let Some(current_quote) = quote {
            if character == current_quote {
                quote = None;
            }
            current.push(character);
        } else if character == '\'' || character == '"' {
            quote = Some(character);
            current.push(character);
        } else {
            current.push(character);
        }
    }
    if !current.trim().is_empty() {
        output.push(current);
    }
    output
}

fn tokenize(command: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in command.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(current_quote) = quote {
            if character == current_quote {
                quote = None;
            } else {
                current.push(character);
            }
        } else if character == '\'' || character == '"' {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                output.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        output.push(current);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parser_preserves_chained_and_quoted_commands() {
        let lines = parse(
            r###"bind Enter source-file ~/.config/tmux/tmux.conf \; display 'configuration reloaded.'
bind r command-prompt -p "rename window:" "rename-window '%%'"
set -g status-left "#{?client_prefix,#[fg=yellow],}(#S) " # comment
"###,
        );
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].tokens[0], "bind");
        assert_eq!(lines[0].chained[0], ["display", "configuration reloaded."]);
        assert_eq!(lines[1].tokens[5], "rename-window '%%'");
        assert_eq!(lines[2].tokens[3], "#{?client_prefix,#[fg=yellow],}(#S) ");
    }

    #[test]
    fn config_key_names_cover_prefix_and_arrow_bindings() {
        assert_eq!(key_bytes("C-a"), Some(vec![1]));
        assert_eq!(key_bytes("C-n"), Some(vec![14]));
        assert_eq!(key_bytes("C-Left"), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key_bytes("\\"), Some(vec![b'\\']));
    }

    #[test]
    fn config_key_names_cover_common_terminal_keys() {
        assert_eq!(key_bytes("C-Space"), Some(vec![0]));
        assert_eq!(key_bytes("BSpace"), Some(vec![0x7f]));
        assert_eq!(key_bytes("Home"), Some(b"\x1b[1~".to_vec()));
        assert_eq!(key_bytes("End"), Some(b"\x1b[4~".to_vec()));
        assert_eq!(key_bytes("Delete"), Some(b"\x1b[3~".to_vec()));
        assert_eq!(key_bytes("PageUp"), Some(b"\x1b[5~".to_vec()));
        assert_eq!(key_bytes("PageDown"), Some(b"\x1b[6~".to_vec()));
        assert_eq!(key_bytes("F1"), Some(b"\x1bOP".to_vec()));
        assert_eq!(key_bytes("F12"), Some(b"\x1b[24~".to_vec()));
        assert_eq!(key_bytes("M-C-a"), Some(vec![0x1b, 1]));
    }
}
