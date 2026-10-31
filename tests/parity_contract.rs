#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::Path;

const CONFIG: &str = include_str!("../src/config.rs");
const MATRIX: &str = include_str!("../docs/parity.md");

struct Row {
    item: String,
    test: String,
    status: String,
}

#[test]
fn parity_matrix_covers_every_compiled_option_and_effective_binding() {
    let options = compiled_options();
    let bindings = compiled_bindings();
    assert_eq!(options.len(), 27, "compiled option inventory changed");
    assert_eq!(bindings.len(), 27, "effective binding inventory changed");

    let option_rows = rows(section(MATRIX, "## Options", "## Bindings"));
    let binding_rows = rows(section(MATRIX, "## Bindings", "## Interactive modes"));
    assert_rows_cover("option", &options, &option_rows);
    assert_rows_cover("binding", &bindings, &binding_rows);

    for (name, start, end) in [
        (
            "interactive mode",
            "## Interactive modes",
            "## CLI and lifecycle",
        ),
        ("CLI and lifecycle", "## CLI and lifecycle", "## Baseline"),
    ] {
        let rows = rows(section(MATRIX, start, end));
        assert!(!rows.is_empty(), "{name} matrix is empty");
        validate_rows(&rows);
    }
}

fn assert_rows_cover(kind: &str, compiled: &[String], rows: &[Row]) {
    validate_rows(rows);
    let documented = rows
        .iter()
        .map(|row| row.item.as_str())
        .collect::<BTreeSet<_>>();
    let expected = compiled.iter().map(String::as_str).collect::<BTreeSet<_>>();
    assert_eq!(rows.len(), documented.len(), "duplicate {kind} rows");
    for item in &expected {
        assert!(
            documented.contains(item),
            "{kind} {item} has no parity row"
        );
    }
    for item in &documented {
        assert!(
            expected.contains(item),
            "{kind} row {item} is not in the compiled profile"
        );
    }
}

fn validate_rows(rows: &[Row]) {
    for row in rows {
        assert!(!row.item.is_empty(), "empty contract item");
        assert!(!row.test.is_empty(), "{} has no test", row.item);
        assert!(!row.test.contains("pending"), "{} has no test", row.item);
        assert!(
            matches!(row.status.as_str(), "pass" | "fail" | "blocked"),
            "{} has invalid status {}",
            row.item,
            row.status
        );
        if row.status == "pass" {
            for part in row.test.split(';') {
                let path = part
                    .trim()
                    .split("::")
                    .next()
                    .and_then(|value| value.split_whitespace().last())
                    .map(|value| value.trim_matches('`'))
                    .unwrap_or_default();
                assert!(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join(path).is_file(),
                    "{} claims pass with a missing test path {path}",
                    row.item
                );
            }
        }
    }
}

fn rows(section: &str) -> Vec<Row> {
    section
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with('|')
                && !line.starts_with("| Contract item")
                && !line.starts_with("| ---")
        })
        .map(|line| {
            let cells = line.split('|').map(str::trim).collect::<Vec<_>>();
            assert_eq!(cells.len(), 9, "matrix row has the wrong column count");
            Row {
                item: cells[1].trim_matches('`').to_owned(),
                test: cells[5].to_owned(),
                status: cells[6].to_owned(),
            }
        })
        .collect()
}

fn section<'a>(document: &'a str, start: &str, end: &str) -> &'a str {
    let start = document
        .find(start)
        .unwrap_or_else(|| panic!("missing matrix section {start}"));
    let rest = &document[start..];
    let end = rest
        .find(end)
        .unwrap_or_else(|| panic!("missing matrix section end {end}"));
    &rest[..end]
}

fn compiled_options() -> Vec<String> {
    let block = source_block("const COMPILED_OPTIONS", "];");
    let literals = string_literals(block);
    assert_eq!(literals.len() % 2, 0, "option tuple is malformed");
    literals
        .chunks(2)
        .map(|pair| pair[0].clone())
        .collect()
}

fn compiled_bindings() -> Vec<String> {
    let mut bindings = Vec::new();
    for marker in ["const VANILLA_BINDINGS", "const COMPILED_BINDINGS"] {
        for key in binding_keys(source_block(marker, "];")) {
            if !bindings.contains(&key) {
                bindings.push(key);
            }
        }
    }
    bindings
}

fn binding_keys(block: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = block;
    while let Some(index) = rest.find("key:") {
        rest = rest[index + "key:".len()..].trim_start();
        if let Some(key) = leading_string_literal(rest) {
            keys.push(key);
        } else {
            break;
        }
    }
    keys
}

fn source_block(marker: &str, end: &str) -> &'static str {
    let start = CONFIG
        .find(marker)
        .unwrap_or_else(|| panic!("missing compiled source marker {marker}"));
    let rest = &CONFIG[start..];
    let end = rest
        .find(end)
        .unwrap_or_else(|| panic!("missing compiled source terminator for {marker}"));
    &rest[..end]
}

fn leading_string_literal(value: &str) -> Option<String> {
    let mut chars = value.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' {
        return None;
    }
    let mut output = String::new();
    while let Some((_, character)) = chars.next() {
        match character {
            '"' => return Some(output),
            '\\' => {
                let (_, escaped) = chars.next()?;
                output.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '0' => '\0',
                    other => other,
                });
            }
            character => output.push(character),
        }
    }
    None
}

fn string_literals(value: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut chars = value.char_indices();
    while let Some((_, character)) = chars.next() {
        if character != '"' {
            continue;
        }
        let mut literal = String::new();
        let mut closed = false;
        while let Some((_, character)) = chars.next() {
            match character {
                '"' => {
                    closed = true;
                    break;
                }
                '\\' => {
                    let (_, escaped) = chars.next().expect("escaped literal byte");
                    literal.push(match escaped {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '0' => '\0',
                        other => other,
                    });
                }
                character => literal.push(character),
            }
        }
        assert!(closed, "unterminated string literal");
        literals.push(literal);
    }
    literals
}
