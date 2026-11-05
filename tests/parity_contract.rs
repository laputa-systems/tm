#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const CONFIG: &str = include_str!("../src/config.rs");
const MATRIX: &str = include_str!("../docs/parity.md");

struct Row {
    item: String,
    test: String,
    evidence: String,
    status: String,
}

struct Sources {
    files: BTreeMap<String, String>,
}

struct Citation {
    path: String,
    function: Option<String>,
}

const REFERENCE_CONSTRUCTOR: &str = "Mux::reference";
const REFERENCE_DIRECTORY: &str = "tests/reference/";
const REJECTION_MESSAGE: &str = "unsupported flag";

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
        ("CLI and lifecycle", "## CLI and lifecycle", "## CLI flags"),
    ] {
        let rows = rows(section(MATRIX, start, end));
        assert!(!rows.is_empty(), "{name} matrix is empty");
        assert_valid(&rows);
    }
}

#[test]
fn cli_flag_rows_are_classified_and_cited() {
    let rows = flag_rows(section(MATRIX, "## CLI flags", "## Baseline"));
    assert!(!rows.is_empty(), "CLI flag matrix is empty");
    let errors = validate_flag_rows(&rows, &Sources::load());
    assert!(errors.is_empty(), "CLI flag matrix is invalid:\n{}", errors.join("\n"));
}

struct FlagRow {
    command: String,
    flag: String,
    classification: String,
    test: String,
    evidence: String,
}

fn flag_rows(section: &str) -> Vec<FlagRow> {
    section
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with('|') && !line.starts_with("| Command") && !line.starts_with("| ---")
        })
        .map(|line| {
            let cells = line.split('|').map(str::trim).collect::<Vec<_>>();
            assert_eq!(cells.len(), 8, "CLI flag row has the wrong column count: {line}");
            FlagRow {
                command: cells[1].trim_matches('`').to_owned(),
                flag: cells[2].trim_matches('`').to_owned(),
                classification: cells[3].to_owned(),
                test: cells[5].to_owned(),
                evidence: cells[6].to_owned(),
            }
        })
        .collect()
}

fn rejects_flag(body: &str, _: &Sources) -> bool {
    body.contains(REJECTION_MESSAGE)
}

fn validate_flag_rows(rows: &[FlagRow], sources: &Sources) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    let mut generic = Vec::new();
    for row in rows {
        let name = format!("{} {}", row.command, row.flag);
        if !seen.insert(name.clone()) {
            errors.push(format!("row `{name}` is listed twice"));
        }
        if !matches!(row.classification.as_str(), "match" | "rejected" | "tm-only") {
            errors.push(format!(
                "row `{name}` has invalid classification {:?}",
                row.classification
            ));
        }
        if row.classification == "match" && row.evidence != "differential" {
            errors.push(format!("row `{name}` is match without differential evidence"));
        }
        if row.classification == "rejected" {
            let rejects = parse_citations(&row.test).is_ok_and(|citations| {
                citations.iter().any(|citation| {
                    citation.function.as_deref().is_some_and(|function| {
                        reaches_from(sources, &citation.path, function, rejects_flag)
                    })
                })
            });
            if !rejects {
                errors.push(format!(
                    "row `{name}` is rejected but no cited test asserts {REJECTION_MESSAGE:?}"
                ));
            }
        }
        generic.push(Row {
            item: name,
            test: row.test.clone(),
            evidence: row.evidence.clone(),
            status: if row.classification == "match" { "pass" } else { "fail" }.to_owned(),
        });
    }
    errors.extend(validate_rows(&generic, sources));
    errors
}

fn assert_rows_cover(kind: &str, compiled: &[String], rows: &[Row]) {
    assert_valid(rows);
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

fn assert_valid(rows: &[Row]) {
    let errors = validate_rows(rows, &Sources::load());
    assert!(errors.is_empty(), "parity matrix is invalid:\n{}", errors.join("\n"));
}

fn validate_rows(rows: &[Row], sources: &Sources) -> Vec<String> {
    let mut errors = Vec::new();
    for row in rows {
        let name = format!("row `{}`", row.item);
        if row.item.is_empty() {
            errors.push("a row has an empty contract item".to_owned());
        }
        if !matches!(row.status.as_str(), "pass" | "fail" | "blocked") {
            errors.push(format!("{name} has invalid status {:?}", row.status));
        }
        if !matches!(row.evidence.as_str(), "differential" | "recorded" | "internal") {
            errors.push(format!("{name} has invalid evidence {:?}", row.evidence));
        }
        if row.status == "pass" && row.evidence == "internal" {
            errors.push(format!("{name} is pass with internal evidence"));
        }
        let citations = match parse_citations(&row.test) {
            Ok(citations) if !citations.is_empty() => citations,
            Ok(_) => {
                errors.push(format!("{name} cites no test"));
                continue;
            }
            Err(message) => {
                errors.push(format!("{name}: {message}"));
                continue;
            }
        };
        let mut resolved = Vec::new();
        for citation in &citations {
            let Some(source) = sources.files.get(&citation.path) else {
                errors.push(format!("{name} cites missing file {}", citation.path));
                continue;
            };
            let Some(function) = &citation.function else {
                continue;
            };
            if !is_test_function(source, function) {
                errors.push(format!(
                    "{name} cites {}::{function}, which is not a test function in that file",
                    citation.path
                ));
                continue;
            }
            resolved.push(citation);
        }
        let reaches = |wanted: fn(&str, &Sources) -> bool| {
            resolved.iter().any(|citation| {
                let function = citation.function.as_deref().unwrap_or_default();
                reaches_from(sources, &citation.path, function, wanted)
            })
        };
        match row.evidence.as_str() {
            "differential" if !reaches(constructs_reference) => errors.push(format!(
                "{name} is differential but no cited test reaches {REFERENCE_CONSTRUCTOR}"
            )),
            "recorded" if !reaches(reads_recorded_observation) => errors.push(format!(
                "{name} is recorded but no cited test reads a file under {REFERENCE_DIRECTORY}"
            )),
            _ => {}
        }
    }
    errors
}

fn constructs_reference(body: &str, _: &Sources) -> bool {
    body.contains(REFERENCE_CONSTRUCTOR)
}

fn reads_recorded_observation(body: &str, sources: &Sources) -> bool {
    sources
        .files
        .keys()
        .filter(|path| path.starts_with(REFERENCE_DIRECTORY))
        .filter_map(|path| path.strip_prefix("tests/"))
        .any(|relative| body.contains(relative) || body.contains(&format!("../{relative}")))
}

fn reaches_from(
    sources: &Sources,
    path: &str,
    function: &str,
    wanted: fn(&str, &Sources) -> bool,
) -> bool {
    let mut visited = BTreeSet::new();
    let mut pending = vec![(path.to_owned(), function.to_owned())];
    while let Some((path, function)) = pending.pop() {
        if !visited.insert((path.clone(), function.clone())) {
            continue;
        }
        let Some(body) = sources
            .files
            .get(&path)
            .and_then(|source| function_body(source, &function))
        else {
            continue;
        };
        if wanted(body, sources) {
            return true;
        }
        for callee in called_names(body) {
            for candidate in sources.helper_paths(&path) {
                if sources
                    .files
                    .get(&candidate)
                    .is_some_and(|source| function_body(source, &callee).is_some())
                {
                    pending.push((candidate, callee.clone()));
                }
            }
        }
    }
    false
}

impl Sources {
    fn load() -> Self {
        let mut files = BTreeMap::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for directory in ["src", "tests"] {
            collect_sources(root, &root.join(directory), &mut files);
        }
        Self { files }
    }

    fn helper_paths(&self, path: &str) -> Vec<String> {
        self.files
            .keys()
            .filter(|candidate| candidate.as_str() == path || candidate.starts_with("tests/support/"))
            .cloned()
            .collect()
    }
}

fn collect_sources(root: &Path, directory: &Path, files: &mut BTreeMap<String, String>) {
    for entry in std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
    {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect_sources(root, &path, files);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            let relative = path.strip_prefix(root).expect("path under manifest root");
            files.insert(relative.to_string_lossy().into_owned(), text);
        }
    }
}

fn parse_citations(cell: &str) -> Result<Vec<Citation>, String> {
    let mut citations = Vec::new();
    let mut current_path: Option<String> = None;
    let mut spans = cell.split('`');
    spans.next();
    while let Some(span) = spans.next() {
        spans.next();
        if let Some((path, rest)) = span.split_once("::") {
            let function = rest.rsplit("::").next().unwrap_or(rest);
            current_path = Some(path.to_owned());
            citations.push(Citation {
                path: path.to_owned(),
                function: Some(function.to_owned()),
            });
        } else if span.ends_with(".rs") {
            current_path = Some(span.to_owned());
            citations.push(Citation {
                path: span.to_owned(),
                function: None,
            });
        } else {
            let path = current_path
                .clone()
                .ok_or_else(|| format!("citation `{span}` follows no file path"))?;
            citations.push(Citation {
                path,
                function: Some(span.to_owned()),
            });
        }
    }
    Ok(citations)
}

fn is_test_function(source: &str, name: &str) -> bool {
    let Some(start) = function_start(source, name) else {
        return false;
    };
    source[..start]
        .split('\n')
        .rev()
        .skip(1)
        .take_while(|line| line.trim_start().starts_with("#["))
        .any(|line| line.trim() == "#[test]")
}

fn function_start(source: &str, name: &str) -> Option<usize> {
    let needle = format!("fn {name}(");
    source.match_indices(&needle).find_map(|(index, _)| {
        let boundary = source[..index]
            .chars()
            .next_back()
            .is_none_or(|character| !(character.is_alphanumeric() || character == '_'));
        boundary.then_some(index)
    })
}

fn function_body<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let start = function_start(source, name)?;
    let bytes = source.as_bytes();
    let mut index = start;
    while index < bytes.len() && bytes[index] != b'{' {
        index = skip_literal(bytes, index);
    }
    let body_start = index;
    let mut depth = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[body_start..=index]);
                }
            }
            _ => {}
        }
        index = skip_literal(bytes, index);
    }
    None
}

fn skip_literal(bytes: &[u8], index: usize) -> usize {
    match bytes[index] {
        b'"' => skip_string(bytes, index + 1),
        b'r' if raw_string_hashes(bytes, index).is_some() => {
            let hashes = raw_string_hashes(bytes, index).unwrap_or_default();
            let mut closing = vec![b'"'];
            closing.extend(std::iter::repeat_n(b'#', hashes));
            let from = index + 2 + hashes;
            bytes[from..]
                .windows(closing.len())
                .position(|window| window == closing.as_slice())
                .map_or(bytes.len(), |at| from + at + closing.len())
        }
        b'/' if bytes.get(index + 1) == Some(&b'/') => bytes[index..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |at| index + at),
        b'\'' => match (bytes.get(index + 1), bytes.get(index + 2), bytes.get(index + 3)) {
            (Some(b'\\'), _, _) => bytes[index + 3..]
                .iter()
                .position(|byte| *byte == b'\'')
                .map_or(bytes.len(), |at| index + 3 + at + 1),
            (Some(_), Some(b'\''), _) => index + 3,
            _ => index + 1,
        },
        _ => index + 1,
    }
}

fn raw_string_hashes(bytes: &[u8], index: usize) -> Option<usize> {
    let before_is_identifier = index > 0
        && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_');
    if before_is_identifier {
        return None;
    }
    let hashes = bytes[index + 1..]
        .iter()
        .take_while(|byte| **byte == b'#')
        .count();
    (bytes.get(index + 1 + hashes) == Some(&b'"')).then_some(hashes)
}

fn skip_string(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn called_names(body: &str) -> BTreeSet<String> {
    let bytes = body.as_bytes();
    let mut names = BTreeSet::new();
    let mut index = 0;
    while index < bytes.len() {
        let next = skip_literal(bytes, index);
        if next != index + 1 || !(bytes[index].is_ascii_alphabetic() || bytes[index] == b'_') {
            index = next;
            continue;
        }
        let end = index
            + bytes[index..]
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_')
                .count();
        if bytes.get(end) == Some(&b'(') {
            names.insert(body[index..end].to_owned());
        }
        index = end;
    }
    names
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
            assert_eq!(cells.len(), 10, "matrix row has the wrong column count");
            Row {
                item: cells[1].trim_matches('`').to_owned(),
                test: cells[5].to_owned(),
                evidence: cells[6].to_owned(),
                status: cells[7].to_owned(),
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

#[cfg(test)]
mod validator {
    use super::*;

    const DIRECT: &str = "#[test]\nfn direct() {\n    let r = Mux::reference(&s, \"r\");\n}\n";
    const HELPED: &str = "#[test]\nfn helped() {\n    build(\"}\");\n}\n";
    const HELPER: &str = "pub fn build(name: &str) {\n    let _ = Mux::reference(name);\n}\n";
    const ISOLATED: &str = "#[test]\nfn isolated() {\n    let tm = Mux::tm(&s, \"tm\");\n}\n";
    const REJECTS: &str = "#[test]\nfn rejects() {\n    let _ = format!(\"tm: unsupported flag {}\", f);\n}\n";
    const REPLAY: &str = "#[test]\nfn replay() {\n    let _ = include_str!(\"reference/capture.txt\");\n}\n";

    fn sources() -> Sources {
        let files = [
            ("tests/a.rs", format!("{DIRECT}\n{HELPED}\n{ISOLATED}\n{REPLAY}\n{REJECTS}\nfn not_a_test() {{}}\n")),
            ("tests/support/h.rs", HELPER.to_owned()),
            ("tests/reference/capture.txt", String::new()),
        ];
        Sources {
            files: files
                .into_iter()
                .map(|(path, text)| (path.to_owned(), text))
                .collect(),
        }
    }

    fn errors(test: &str, evidence: &str, status: &str) -> Vec<String> {
        let row = Row {
            item: "item".to_owned(),
            test: test.to_owned(),
            evidence: evidence.to_owned(),
            status: status.to_owned(),
        };
        validate_rows(&[row], &sources())
    }

    #[test]
    fn well_formed_rows_pass() {
        assert_eq!(errors("X `tests/a.rs::direct`", "differential", "pass"), Vec::<String>::new());
        assert_eq!(
            errors("X `tests/a.rs::helped`; `tests/a.rs::isolated`", "differential", "pass"),
            Vec::<String>::new()
        );
        assert_eq!(errors("X `tests/a.rs::replay`", "recorded", "pass"), Vec::<String>::new());
        assert_eq!(errors("X `tests/a.rs::isolated`", "internal", "fail"), Vec::<String>::new());
        assert_eq!(
            errors("X `tests/a.rs::isolated`, `direct`", "differential", "fail"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_missing_function_is_rejected() {
        let found = errors("X `tests/a.rs::vanished`", "differential", "pass");
        assert!(found.iter().any(|message| message.contains("vanished")), "{found:?}");
    }

    #[test]
    fn a_function_that_is_not_a_test_is_rejected() {
        let found = errors("X `tests/a.rs::not_a_test`", "internal", "fail");
        assert!(found.iter().any(|message| message.contains("not_a_test")), "{found:?}");
    }

    #[test]
    fn a_missing_file_is_rejected() {
        let found = errors("X `tests/gone.rs::direct`", "differential", "pass");
        assert!(found.iter().any(|message| message.contains("tests/gone.rs")), "{found:?}");
    }

    #[test]
    fn a_differential_row_that_never_reaches_the_reference_is_rejected() {
        let found = errors("X `tests/a.rs::isolated`", "differential", "pass");
        assert!(found.iter().any(|message| message.contains("no cited test reaches")), "{found:?}");
    }

    #[test]
    fn a_recorded_row_without_a_recorded_observation_is_rejected() {
        let found = errors("X `tests/a.rs::isolated`", "recorded", "pass");
        assert!(found.iter().any(|message| message.contains("tests/reference/")), "{found:?}");
    }

    #[test]
    fn a_pass_row_cannot_be_internal() {
        let found = errors("X `tests/a.rs::isolated`", "internal", "pass");
        assert!(found.iter().any(|message| message.contains("internal")), "{found:?}");
    }

    #[test]
    fn unknown_evidence_and_status_are_rejected() {
        assert_eq!(errors("X `tests/a.rs::direct`", "guess", "pass").len(), 1);
        assert_eq!(errors("X `tests/a.rs::direct`", "differential", "ok").len(), 1);
    }

    fn flag_errors(classification: &str, test: &str, evidence: &str) -> Vec<String> {
        let row = FlagRow {
            command: "cmd".to_owned(),
            flag: "-x".to_owned(),
            classification: classification.to_owned(),
            test: test.to_owned(),
            evidence: evidence.to_owned(),
        };
        validate_flag_rows(&[row], &sources())
    }

    #[test]
    fn well_formed_flag_rows_pass() {
        assert_eq!(flag_errors("match", "X `tests/a.rs::direct`", "differential"), Vec::<String>::new());
        assert_eq!(flag_errors("rejected", "X `tests/a.rs::rejects`", "internal"), Vec::<String>::new());
        assert_eq!(flag_errors("tm-only", "X `tests/a.rs::isolated`", "internal"), Vec::<String>::new());
    }

    #[test]
    fn a_match_flag_row_needs_differential_evidence() {
        let found = flag_errors("match", "X `tests/a.rs::isolated`", "internal");
        assert!(found.iter().any(|message| message.contains("without differential")), "{found:?}");
    }

    #[test]
    fn a_rejected_flag_row_must_cite_a_rejection_test() {
        let found = flag_errors("rejected", "X `tests/a.rs::direct`", "differential");
        assert!(found.iter().any(|message| message.contains("unsupported flag")), "{found:?}");
    }

    #[test]
    fn an_unknown_flag_classification_is_rejected() {
        let found = flag_errors("accepted", "X `tests/a.rs::isolated`", "internal");
        assert!(found.iter().any(|message| message.contains("invalid classification")), "{found:?}");
    }
}
