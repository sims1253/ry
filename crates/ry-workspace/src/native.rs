//! Literal native registration tables from a package's C and C++ sources.

use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    RegistrationMacro,
    String(String),
    Punct(char),
    Barrier,
}

impl Token {
    fn word(&self) -> Option<&str> {
        match self {
            Self::Word(value) => Some(value),
            _ => None,
        }
    }
}

/// Read ordinary source files only. Headers, generated sources, and custom
/// build steps may supply more routines; this inventory is never exhaustive.
pub(crate) fn registered_symbols(root: &Path, library: &str) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(root.join("src")) else {
        return HashSet::new();
    };
    let mut paths: Vec<_> = entries
        .take(4096)
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && matches!(
                    entry.path().extension().and_then(|s| s.to_str()),
                    Some("c" | "cc" | "cpp" | "cxx")
                )
        })
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    let mut remaining = 8 * 1024 * 1024;
    let mut found = HashSet::new();
    for path in paths.into_iter().take(256) {
        let Ok(size) = path.metadata().map(|m| m.len()) else {
            continue;
        };
        if size > 2 * 1024 * 1024 || size > remaining {
            continue;
        }
        // Bound the read even if the file grows after metadata was read.
        use std::io::Read;
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file.take(size + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 != size {
            continue;
        }
        remaining -= size;
        if let Ok(source) = std::str::from_utf8(&bytes) {
            found.extend(source_symbols(source, library));
        }
    }
    found
}

fn lex(source: &str) -> Vec<Token> {
    let mut chars = source.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Vec::new();
            }
        } else if ch == '"' || ch == '\'' {
            let mut value = String::new();
            let mut valid = ch == '"';
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == ch {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    match chars.next() {
                        Some(c @ ('\\' | '"')) => value.push(c),
                        Some(_) => valid = false,
                        None => break,
                    }
                } else {
                    value.push(c);
                }
            }
            if !closed {
                return Vec::new();
            }
            tokens.push(if valid {
                Token::String(value)
            } else {
                Token::Barrier
            });
        } else if ch.is_ascii_alphanumeric() || ch == '_' {
            let mut value = ch.to_string();
            while chars
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
            {
                value.push(chars.next().unwrap());
            }
            tokens.push(Token::Word(value));
        } else {
            tokens.push(Token::Punct(ch));
        }
    }
    tokens
}

fn strip_comments(source: &str) -> Option<String> {
    let mut chars = source.chars().peekable();
    let mut clean = String::new();
    while let Some(ch) = chars.next() {
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            clean.push(' ');
            for c in chars.by_ref() {
                if c == '\n' {
                    clean.push(c);
                    break;
                }
            }
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            clean.push(' ');
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '\n' {
                    clean.push(c);
                }
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    closed = true;
                    break;
                }
            }
            if !closed {
                return None;
            }
        } else if ch == 'R' && chars.peek() == Some(&'"') {
            // Raw C++ strings need a delimiter parser; leave this file opaque.
            return None;
        } else if ch == '"' || ch == '\'' {
            clean.push(ch);
            let mut closed = false;
            while let Some(c) = chars.next() {
                clean.push(c);
                if c == ch {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    clean.push(chars.next()?);
                }
            }
            if !closed {
                return None;
            }
        } else {
            clean.push(ch);
        }
    }
    Some(clean)
}

fn append_code_tokens(code: &mut String, macros: &HashSet<String>, tokens: &mut Vec<Token>) {
    tokens.extend(lex(code).into_iter().map(|token| {
        if token.word().is_some_and(|name| macros.contains(name)) {
            Token::RegistrationMacro
        } else {
            token
        }
    }));
    code.clear();
}

fn source_tokens(source: &str) -> Vec<Token> {
    let joined = source.replace("\\\r\n", "").replace("\\\n", "");
    let Some(joined) = strip_comments(&joined) else {
        return Vec::new();
    };
    let mut macros = HashSet::new();
    let mut conditional_depth = 0usize;
    let mut code = String::new();
    let mut output = Vec::new();
    // Comment stripping preserves strings and directive line boundaries.
    for line in joined.lines() {
        let trimmed = line.trim_start();
        if let Some(directive) = trimmed.strip_prefix('#') {
            append_code_tokens(&mut code, &macros, &mut output);
            let tokens = lex(directive);
            let kind = tokens.first().and_then(Token::word);
            match kind {
                Some("if" | "ifdef" | "ifndef") => conditional_depth += 1,
                Some("endif") => conditional_depth = conditional_depth.saturating_sub(1),
                Some("define") => {
                    if let Some(name) = tokens.get(1).and_then(Token::word) {
                        macros.remove(name);
                        if conditional_depth == 0 && registration_macro(&tokens[2..]) {
                            macros.insert(name.to_string());
                        }
                    }
                }
                Some("undef") => {
                    if let Some(name) = tokens.get(1).and_then(Token::word) {
                        macros.remove(name);
                    }
                }
                _ => {}
            }
            code.push_str(" ; ");
        } else if conditional_depth == 0 {
            code.push_str(line);
        } else {
            output.push(Token::Barrier);
            code.push_str(" ; ");
        }
        code.push('\n');
    }
    append_code_tokens(&mut code, &macros, &mut output);
    output
}

/// Accept only the common two-parameter `#name, (DL_FUNC)&name, count` macro.
fn registration_macro(tokens: &[Token]) -> bool {
    let [
        Token::Punct('('),
        Token::Word(name),
        Token::Punct(','),
        Token::Word(count),
        Token::Punct(')'),
        rest @ ..,
    ] = tokens
    else {
        return false;
    };
    rest == lex(&format!("{{ #{name}, (DL_FUNC)&{name}, {count} }}"))
}

fn balanced_end(tokens: &[Token], start: usize, open: char, close: char) -> Option<usize> {
    if tokens.get(start) != Some(&Token::Punct(open)) {
        return None;
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if token == &Token::Punct(open) {
            depth += 1;
        }
        if token == &Token::Punct(close) {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn table_symbols(tokens: &[Token]) -> HashSet<String> {
    let mut found = HashSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == Token::Punct('{') {
            let Some(end) = balanced_end(tokens, index, '{', '}') else {
                break;
            };
            if let [
                Token::Punct('{'),
                Token::String(name),
                Token::Punct(','),
                rest @ ..,
            ] = &tokens[index..end]
                && !name.is_empty()
                && !rest.is_empty()
            {
                found.insert(name.clone());
            } else {
                // A null name ends registration. An unknown name may be null.
                break;
            }
            index = end + 1;
        } else if tokens[index] == Token::RegistrationMacro {
            if let Some(
                [
                    Token::Punct('('),
                    Token::Word(name),
                    Token::Punct(','),
                    Token::Word(count),
                    Token::Punct(')'),
                ],
            ) = tokens.get(index + 1..index + 6)
                && count.parse::<u32>().is_ok()
            {
                found.insert(name.clone());
                index += 6;
            } else {
                index += 1;
            }
        } else if tokens[index] == Token::Barrier {
            break;
        } else {
            index += 1;
        }
    }
    found
}

fn source_symbols(source: &str, library: &str) -> HashSet<String> {
    let tokens = source_tokens(source);
    let mut tables = HashMap::new();
    let mut brace_depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Punct('{') => brace_depth += 1,
            Token::Punct('}') => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        if brace_depth != 0 {
            continue;
        }
        if !matches!(
            token.word(),
            Some("R_CallMethodDef" | "R_CMethodDef" | "R_FortranMethodDef" | "R_ExternalMethodDef")
        ) {
            continue;
        }
        let Some(
            [
                Token::Word(name),
                Token::Punct('['),
                Token::Punct(']'),
                Token::Punct('='),
                Token::Punct('{'),
            ],
        ) = tokens.get(index + 1..index + 6)
        else {
            continue;
        };
        let start = index + 5;
        if let Some(end) = balanced_end(&tokens, start, '{', '}') {
            tables.insert(name.clone(), table_symbols(&tokens[start + 1..end]));
        }
    }
    let init = format!("R_init_{}", library.replace('.', "_"));
    let mut found = HashSet::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.word() != Some(&init) {
            continue;
        }
        let Some(params_end) = balanced_end(&tokens, index + 1, '(', ')') else {
            continue;
        };
        let body_start = params_end + 1;
        let Some(body_end) = balanced_end(&tokens, body_start, '{', '}') else {
            continue;
        };
        let shadowed_tables: HashSet<_> = tokens[body_start + 1..body_end]
            .windows(2)
            .filter(|pair| {
                matches!(
                    pair[0].word(),
                    Some(
                        "R_CallMethodDef"
                            | "R_CMethodDef"
                            | "R_FortranMethodDef"
                            | "R_ExternalMethodDef"
                    )
                )
            })
            .filter_map(|pair| pair[1].word())
            .collect();
        let mut position = body_start + 1;
        while position < body_end {
            // Only direct statements in this initializer establish inventory.
            let start = position;
            while position < body_end && !matches!(tokens[position], Token::Punct(';' | '{')) {
                position += 1;
            }
            if position < body_end && tokens[position] == Token::Punct('{') {
                position =
                    balanced_end(&tokens, position, '{', '}').map_or(body_end, |end| end + 1);
                continue;
            }
            let statement = &tokens[start..position];
            if let [
                Token::Word(call),
                Token::Punct('('),
                Token::Word(_dll),
                Token::Punct(','),
                Token::Word(c),
                Token::Punct(','),
                Token::Word(call_table),
                Token::Punct(','),
                Token::Word(fortran),
                Token::Punct(','),
                Token::Word(external),
                Token::Punct(')'),
            ] = statement
                && call == "R_registerRoutines"
            {
                for table in [c, call_table, fortran, external] {
                    if !shadowed_tables.contains(table.as_str())
                        && let Some(names) = tables.get(table)
                    {
                        found.extend(names.iter().cloned());
                    }
                }
            }
            position += 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(table: &str, init: &str) -> String {
        format!(
            "static const R_CallMethodDef calls[] = {{ {table}, {{NULL, NULL, 0}} }};\nvoid R_init_example(DllInfo *dll) {{ {init} }}"
        )
    }

    #[test]
    fn registered_literal_and_local_macro_rows_supply_names() {
        let source = format!(
            "#define CALLDEF(name, n) \\\n {{ #name, (DL_FUNC)&name, n }}\n{}",
            source(
                r#"{"alias", (DL_FUNC)&implementation, 1}, CALLDEF(entry, 2)"#,
                "R_registerRoutines(dll, NULL, calls, NULL, NULL);",
            )
        );
        assert_eq!(
            source_symbols(&source, "example"),
            HashSet::from(["alias".into(), "entry".into()])
        );
        assert!(source_symbols(&source, "other").is_empty());
    }

    #[test]
    fn macro_definitions_apply_at_each_table_row() {
        let source = r#"
#define CALLDEF(name, n) { #name, (DL_FUNC)&name, n }
static const R_CallMethodDef calls[] = {
    CALLDEF(before, 1),
#undef CALLDEF
    CALLDEF(after_undef, 1),
#define CALLDEF(name, n) { #name, (DL_FUNC)&name, n }
    CALLDEF(redefined, 1),
#if UNKNOWN
#define CALLDEF(name, n) custom(name, n)
#endif
    CALLDEF(after_conditional, 1),
    {NULL, NULL, 0}
};
void R_init_example(DllInfo *dll) { R_registerRoutines(dll, NULL, calls, NULL, NULL); }
#undef CALLDEF
"#;
        assert_eq!(
            source_symbols(source, "example"),
            HashSet::from(["before".into(), "redefined".into()])
        );
    }

    #[test]
    fn terminated_or_uncertain_rows_stop_registration() {
        for row in [
            "{NULL, NULL, 0}",
            "{0, 0, 0}",
            "{nullptr, nullptr, 0}",
            "{dynamic_name, (DL_FUNC)&entry, 0}",
            "\n#if UNKNOWN\n{NULL, NULL, 0},\n#endif\n",
        ] {
            let source = source(
                &format!(
                    "{{\"before\", (DL_FUNC)&entry, 0}}, {row}, {{\"after\", (DL_FUNC)&entry, 0}}"
                ),
                "R_registerRoutines(dll, NULL, calls, NULL, NULL);",
            );
            assert_eq!(
                source_symbols(&source, "example"),
                HashSet::from(["before".into()]),
                "{source}"
            );
        }
    }

    #[test]
    fn local_table_does_not_resolve_through_a_shadowed_global_table() {
        let source = source(
            "{\"global_entry\", (DL_FUNC)&entry, 0}",
            "const R_CallMethodDef calls[] = { {NULL, NULL, 0} }; R_registerRoutines(dll, NULL, calls, NULL, NULL);",
        );
        assert!(source_symbols(&source, "example").is_empty());
    }

    #[test]
    fn file_inventory_reads_registered_tables() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        let source = format!(
            "#define CALLDEF(name, n) {{ #name, (DL_FUNC)&name, n }}\n{}",
            source(
                "CALLDEF(entry, 1)",
                "R_registerRoutines(dll, NULL, calls, NULL, NULL);"
            )
        );
        std::fs::write(dir.path().join("src/init.c"), source).unwrap();
        assert_eq!(
            registered_symbols(dir.path(), "example"),
            HashSet::from(["entry".into()])
        );
    }

    #[test]
    fn workspace_context_receives_the_native_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::create_dir(root.join("R")).unwrap();
        std::fs::write(root.join("DESCRIPTION"), "Package: example\n").unwrap();
        std::fs::write(
            root.join("NAMESPACE"),
            "useDynLib(example, .registration = TRUE)\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/init.c"),
            source(
                "{\"entry\", (DL_FUNC)&entry, 0}",
                "R_registerRoutines(dll, NULL, calls, NULL, NULL);",
            ),
        )
        .unwrap();
        let path = root.join("R/use.R").to_string_lossy().into_owned();
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse(&path, "f <- function() entry\n").unwrap();
        let config = ry_config::Config::default();
        let files = [file];
        let stubs = std::collections::BTreeMap::new();
        let context = crate::resolve_workspace_context(
            root,
            &config,
            crate::ResolutionEnvironment {
                files: files.iter().collect(),
                user_stubs: &stubs,
            },
        )
        .unwrap();
        assert!(
            context.external_bindings[&path].contains("entry"),
            "{:?}",
            context.external_bindings
        );
    }

    #[test]
    fn macro_can_be_the_first_table_entry() {
        let source = r#"
#define CALLDEF(name, n) { #name, (DL_FUNC)&name, n }
static const R_CallMethodDef calls[] = { CALLDEF(entry, 1), {"alias", (DL_FUNC)&implementation, 0}, {NULL, NULL, 0} };
void R_init_example(DllInfo *dll) { R_registerRoutines(dll, NULL, calls, NULL, NULL); }
"#;
        assert_eq!(
            source_symbols(source, "example"),
            HashSet::from(["entry".into(), "alias".into()])
        );
    }

    #[test]
    fn unregistered_conditional_and_dynamic_rows_do_not_supply_names() {
        for source in [
            source(
                r#"{"entry", (DL_FUNC)&entry, 0}"#,
                "R_registerRoutines(dll, NULL, other, NULL, NULL);",
            ),
            source(
                "{dynamic_name, (DL_FUNC)&entry, 0}",
                "R_registerRoutines(dll, NULL, calls, NULL, NULL);",
            ),
            source(
                r#"{"entry", (DL_FUNC)&entry, 0}"#,
                "if (enabled) R_registerRoutines(dll, NULL, calls, NULL, NULL);",
            ),
            source(
                r#"{"entry", (DL_FUNC)&entry, 0}"#,
                "if (enabled) { R_registerRoutines(dll, NULL, calls, NULL, NULL); }",
            ),
            source(
                r#"{"entry", (DL_FUNC)&entry, 0}"#,
                "\n#ifdef ENABLE\nR_registerRoutines(dll, NULL, calls, NULL, NULL);\n#endif\n",
            ),
            format!(
                "/*\n#define CALLDEF(name,n) {{ #name, (DL_FUNC)&name, n }}\n*/\n{}",
                source(
                    "CALLDEF(entry, 0)",
                    "R_registerRoutines(dll, NULL, calls, NULL, NULL);"
                )
            ),
            format!(
                "#define CALLDEF(name,n) {{ \"different\", (DL_FUNC)&name, n }}\n{}",
                source(
                    "CALLDEF(entry, 0)",
                    "R_registerRoutines(dll, NULL, calls, NULL, NULL);"
                )
            ),
        ] {
            assert!(source_symbols(&source, "example").is_empty(), "{source}");
        }
    }

    #[test]
    fn comments_and_string_contents_are_not_registration_code() {
        let declaration = source(
            r#"{"entry", (DL_FUNC)&entry, 0}"#,
            "R_registerRoutines(dll, NULL, calls, NULL, NULL);",
        );
        assert!(source_symbols(&format!("/* {declaration} */"), "example").is_empty());
        assert!(source_symbols(&format!("const char *s = {declaration:?};"), "example").is_empty());
        assert!(
            source_symbols(
                &format!("const char *s = R\"tag({declaration})tag\";"),
                "example"
            )
            .is_empty()
        );
        let commented =
            declaration.replace("R_registerRoutines", "/* ignored */ R_registerRoutines");
        assert_eq!(
            source_symbols(&commented, "example"),
            HashSet::from(["entry".into()])
        );
    }

    #[test]
    fn conditional_rows_preserve_prior_proven_registration() {
        let source = source(
            "{\"always\", (DL_FUNC)&always, 0},\n#ifdef ENABLE\n{\"optional\", (DL_FUNC)&optional, 0},\n#endif\n{\"later\", (DL_FUNC)&later, 0}",
            "R_registerRoutines(dll, NULL, calls, NULL, NULL);",
        );
        assert_eq!(
            source_symbols(&source, "example"),
            HashSet::from(["always".into()])
        );
    }
}
