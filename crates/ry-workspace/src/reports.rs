//! Bounded, source-mapped static input for ordinary fenced R report chunks.
//! No report engine is invoked. Every retained R byte has its original
//! offset; all other non-newline bytes become ASCII spaces.

use std::borrow::Cow;
use std::ops::ControlFlow;

use ry_core::ast::{Expr, InputIssue, Stmt};
use ry_core::parser::unquote_r_string;
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use ry_core::{RParser, SourceFile, Span};
use tree_sitter::Tree;

pub const MAX_REPORT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHUNKS: usize = 128;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADER_FIELDS: usize = 128;

enum HeaderError {
    Budget,
    Unsupported,
}

pub fn is_report_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("Rmd" | "rmd" | "qmd")
    )
}

struct Line<'a> {
    offset: usize,
    text: &'a str,
}

fn lines(source: &str) -> Vec<Line<'_>> {
    let mut offset = 0;
    source
        .split_inclusive('\n')
        .map(|text| {
            let line = Line { offset, text };
            offset += text.len();
            line
        })
        .collect()
}

/// Anchor an input finding to one complete source character. The human
/// formatter slices the original line using this span, so a one-byte range
/// inside a multibyte character would panic.
fn source_issue(
    source: &str,
    offset: usize,
    line: usize,
    code: &'static str,
    message: &str,
) -> InputIssue {
    let mut start = offset.min(source.len());
    while !source.is_char_boundary(start) {
        start -= 1;
    }
    let end = source[start..]
        .chars()
        .next()
        .map_or(start, |character| start + character.len_utf8());
    InputIssue {
        span: Span::new(start, end, line, 0),
        code,
        message: message.to_owned(),
    }
}

fn mask(masked: &mut [u8], start: usize, end: usize) {
    for byte in &mut masked[start..end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn r_name_component(raw: &str) -> Cow<'_, str> {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2
        && matches!(
            (bytes[0], bytes[bytes.len() - 1]),
            (b'`', b'`') | (b'"', b'"') | (b'\'', b'\'')
        )
    {
        Cow::Owned(unquote_r_string(raw))
    } else {
        Cow::Borrowed(raw)
    }
}

/// A real reference to knitr's chunk-option object can change the execution
/// of later chunks. Inspect lowered R identifiers so raw strings, comments,
/// and similarly named variables cannot invent that boundary. We include
/// function bodies and assignment targets conservatively: without executing
/// the report, a later call or replacement could use either reference.
fn has_runtime_chunk_options(stmts: &[Stmt]) -> bool {
    let policy = Walk {
        assign_targets: true,
        assign_operands: true,
        dollar_args: false,
        fn_bodies: true,
        control_tests: true,
    };
    matches!(
        walk_stmts(stmts, policy, |node, _| {
            if let AstNode::Expr(Expr::Ident { name, .. }) = node {
                let (package, object) = name
                    .rsplit_once("::")
                    .map_or((None, name.as_str()), |(package, object)| {
                        (Some(package.trim_end_matches(':')), object)
                    });
                if package.is_none_or(|package| r_name_component(package) == "knitr")
                    && r_name_component(object) == "opts_chunk"
                {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(Descend::Into)
        }),
        ControlFlow::Break(())
    )
}

fn fence(text: &str) -> Option<(u8, usize, &str)> {
    let trimmed = text.trim_end_matches(['\r', '\n']);
    let rest = trimmed.trim_start_matches(' ');
    if trimmed.len() - rest.len() > 3 {
        return None;
    }
    let kind = *rest.as_bytes().first()?;
    if kind != b'`' && kind != b'~' {
        return None;
    }
    let count = rest.bytes().take_while(|byte| *byte == kind).count();
    (count >= 3).then_some((kind, count, &rest[count..]))
}

fn yaml_bool(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn r_bool(text: &str) -> Option<bool> {
    match text.trim() {
        "TRUE" => Some(true),
        "FALSE" => Some(false),
        _ => None,
    }
}

fn simple_key(key: &str) -> Result<&str, ()> {
    let key = key.trim();
    if key.len() >= 2
        && matches!(
            (key.as_bytes()[0], key.as_bytes()[key.len() - 1]),
            (b'"', b'"') | (b'\'', b'\'') | (b'`', b'`')
        )
    {
        // Encoded spellings can decode to execution keys. Without a full
        // R/YAML key decoder, make that uncertainty visible.
        if key.as_bytes().contains(&b'\\') {
            return Err(());
        }
        Ok(&key[1..key.len() - 1])
    } else if key.starts_with(['"', '\'', '`']) || key.ends_with(['"', '\'', '`']) {
        Err(())
    } else {
        Ok(key)
    }
}

fn execution_option(key: &str, value: &str, r_header: bool) -> Result<Option<bool>, ()> {
    let key = simple_key(key)?;
    let boolean = if r_header { r_bool } else { yaml_bool };
    if ["child", "ref.label", "engine", "file", "code", "dependson"].contains(&key) {
        return Err(());
    }
    if key == "eval" {
        boolean(value).map(Some).ok_or(())
    } else if key == "include" || key == "echo" {
        boolean(value).map(|_| None).ok_or(())
    } else {
        Ok(None)
    }
}

/// Extract a YAML key without mistaking a colon inside a quoted key or value
/// for a separator. This is a bounded key reader, not a YAML renderer.
fn yaml_key_value(line: &str) -> Result<Option<(&str, &str)>, ()> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let bytes = line.as_bytes();
    let mut quote = None;
    let mut pos = 0;
    while pos < bytes.len() {
        match (quote, bytes[pos]) {
            (Some(_), b'\\') => {
                pos = (pos + 2).min(bytes.len());
                continue;
            }
            (Some(delimiter), byte) if delimiter == byte => quote = None,
            (None, delimiter @ (b'\'' | b'"')) => quote = Some(delimiter),
            (None, b':') => {
                return simple_key(&line[..pos]).map(|key| Some((key, &line[pos + 1..])));
            }
            _ => {}
        }
        pos += 1;
    }
    if quote.is_some() {
        return Err(());
    }
    Ok(None)
}

fn complex_format_value(value: &str) -> bool {
    value
        .trim_start()
        .starts_with(['{', '[', '*', '&', '!', '|', '>'])
}

/// Split only at header commas outside ordinary quotes and nested R syntax.
fn header_fields(inner: &str) -> Result<Vec<&str>, HeaderError> {
    let bytes = inner.as_bytes();
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut nesting = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if let Some(delimiter) = quote {
            if byte == b'\\' {
                pos = (pos + 2).min(bytes.len());
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' | b'`' => quote = Some(byte),
                b'(' | b'[' | b'{' => nesting.push(byte),
                b')' | b']' | b'}' => {
                    let matches = matches!(
                        (nesting.pop(), byte),
                        (Some(b'('), b')') | (Some(b'['), b']') | (Some(b'{'), b'}')
                    );
                    if !matches {
                        return Err(HeaderError::Unsupported);
                    }
                }
                b',' if nesting.is_empty() => {
                    if fields.len() >= MAX_HEADER_FIELDS {
                        return Err(HeaderError::Budget);
                    }
                    fields.push(&inner[start..pos]);
                    start = pos + 1;
                }
                _ => {}
            }
        }
        pos += 1;
    }
    if quote.is_some() || !nesting.is_empty() {
        return Err(HeaderError::Unsupported);
    }
    if fields.len() >= MAX_HEADER_FIELDS {
        return Err(HeaderError::Budget);
    }
    fields.push(&inner[start..]);
    Ok(fields)
}

/// Classify only simple one-line root flow mappings. Nested values stay
/// opaque: a metadata value cannot promote its keys to document settings,
/// whereas a root execution or format key makes execution uncertain. Other
/// root flow syntax is refused rather than treated as ordinary block YAML.
fn root_flow_execution(line: &str) -> Result<bool, ()> {
    let inner = line
        .trim()
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .ok_or(())?;
    if inner.len() > MAX_HEADER_BYTES {
        return Err(());
    }
    for field in header_fields(inner).map_err(|_| ())? {
        let (key, _) = yaml_key_value(field)?.ok_or(())?;
        if key.is_empty() || key.starts_with(['?', '!', '&', '*', '[', '{', '<', '>']) {
            return Err(());
        }
        if matches!(key, "execute" | "knitr" | "eval" | "format" | "<<") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn header_options(
    parser: &mut RParser,
    path: &str,
    rest: &str,
) -> Result<Option<Option<bool>>, HeaderError> {
    let header = rest.trim();
    if !(header.starts_with("{r") && header.ends_with('}')) {
        return Ok(None);
    }
    if header.len() > MAX_HEADER_BYTES {
        return Err(HeaderError::Budget);
    }
    let inner = &header[2..header.len() - 1];
    if !inner.is_empty() && !inner.starts_with([',', ' ', '\t']) {
        return Ok(None);
    }
    let mut eval = None;
    for part in header_fields(inner)? {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        // A leading label such as `setup eval = FALSE` has no effect on
        // the option name. More than one assignment in the same segment
        // leaves the value dynamic and is refused below.
        let key = key.split_whitespace().last().unwrap_or("");
        if let Some(value) =
            execution_option(key, value, true).map_err(|()| HeaderError::Unsupported)?
            && eval.replace(value).is_some()
        {
            return Err(HeaderError::Unsupported);
        }
        // Chunk-header metadata is evaluated as R before the body. A real
        // reference to knitr's options object can change later chunks even
        // when the option's own key is just a caption or plot setting.
        let expression = parser
            .parse(path, value.trim())
            .map_err(|_| HeaderError::Unsupported)?;
        if !expression.parse_errors.is_empty() || has_runtime_chunk_options(&expression.stmts) {
            return Err(HeaderError::Unsupported);
        }
    }
    Ok(Some(eval))
}

/// Parse original report text through a byte-preserving R mask. The returned
/// tree is the masked whole-document tree and can be incrementally edited with
/// original-document byte and row deltas.
pub fn parse_report_with_tree(
    parser: &mut RParser,
    path: &str,
    source: &str,
    old_tree: Option<&Tree>,
) -> Result<(SourceFile, Tree), ry_core::parser::ParseError> {
    let issue = |offset, line, code, message| source_issue(source, offset, line, code, message);
    if source.len() > MAX_REPORT_BYTES {
        let (mut file, tree) = parser.parse_with_tree(path, "", None)?;
        file.source = source.to_owned();
        file.input_issues.push(issue(
            0,
            0,
            "RY120",
            "report exceeds the 2 MiB static input limit",
        ));
        return Ok((file, tree));
    }
    let rows = lines(source);
    let mut masked = source.as_bytes().to_vec();
    mask(&mut masked, 0, source.len());
    let mut issues = Vec::new();
    let mut chunks = Vec::new();
    let mut chunk_parse_errors = Vec::new();
    {
        // Global execution settings can change the meaning of every chunk.
        let mut first = 0;
        if rows.first().is_some_and(|line| line.text.trim() == "---") {
            first = 1;
            let mut root_key = None;
            let mut root_indent = None;
            while first < rows.len() && rows[first].text.trim() != "---" {
                let raw = rows[first].text.trim_end_matches(['\r', '\n']);
                let indent = raw.len() - raw.trim_start_matches(' ').len();
                let content = raw.trim_start_matches(' ');
                if content.is_empty() || content.starts_with('#') {
                    first += 1;
                    continue;
                }
                let root = *root_indent.get_or_insert(indent);
                if indent < root || content.starts_with('\t') {
                    issues.push(issue(rows[first].offset, first, "RY121", "report YAML indentation cannot be classified safely; no chunks are assumed executable"));
                    break;
                }
                let at_root = indent == root;
                if at_root && content.starts_with('{') {
                    match root_flow_execution(content) {
                        Ok(false) => {
                            first += 1;
                            continue;
                        }
                        Ok(true) | Err(()) => {
                            issues.push(issue(rows[first].offset, first, "RY121", "root flow YAML execution settings cannot be classified safely; no chunks are assumed executable"));
                            break;
                        }
                    }
                }
                if at_root && content.starts_with(['[', '-', '?', '!', '&', '*', '|', '>']) {
                    issues.push(issue(rows[first].offset, first, "RY121", "report YAML root syntax cannot be classified safely; no chunks are assumed executable"));
                    break;
                }
                match yaml_key_value(raw) {
                    Ok(Some((key, value))) => {
                        if at_root {
                            root_key = Some(key);
                        }
                        let execution_key = key == "execute" || key == "knitr";
                        let root_execution = at_root && (execution_key || key == "eval");
                        let format_execution =
                            !at_root && root_key == Some("format") && execution_key;
                        let format_inheritance = root_key == Some("format")
                            && (key == "<<" || complex_format_value(value));
                        if root_execution
                            || format_execution
                            || format_inheritance
                            || at_root && key == "<<"
                        {
                            issues.push(issue(rows[first].offset, first, "RY121", "report-level execution options need a report engine; no chunks are assumed executable"));
                            break;
                        }
                    }
                    Err(()) => {
                        issues.push(issue(rows[first].offset, first, "RY121", "report YAML key cannot be classified safely; no chunks are assumed executable"));
                        break;
                    }
                    Ok(None) => {}
                }
                first += 1;
            }
            if first == rows.len() {
                issues.push(issue(0, 0, "RY120", "unclosed report YAML front matter"));
            }
            first = first.saturating_add(1);
        }
        if issues.is_empty() {
            let mut open: Option<(u8, usize, bool, bool, usize, usize, usize)> = None;
            let mut seen_r_chunks = 0;
            let mut row = first;
            while row < rows.len() {
                let line = &rows[row];
                if let Some((kind, width, r_chunk, enabled, start_row, body_row, body_start)) = open
                {
                    if let Some((close_kind, close_width, tail)) = fence(line.text)
                        && close_kind == kind
                        && close_width >= width
                        && tail.trim().is_empty()
                    {
                        if r_chunk && enabled {
                            chunks.push((start_row, body_row, body_start, line.offset));
                            let chunk = parser.parse(path, &source[body_start..line.offset])?;
                            for span in chunk.parse_errors {
                                chunk_parse_errors.push(Span::new(
                                    span.start + body_start,
                                    span.end + body_start,
                                    span.line + body_row,
                                    span.col,
                                ));
                            }
                            if has_runtime_chunk_options(&chunk.stmts) {
                                issues.push(issue(rows[start_row].offset, start_row, "RY121", "runtime chunk options may change later execution; later chunks are not analyzed"));
                                break;
                            }
                        }
                        open = None;
                        row += 1;
                        continue;
                    }
                    row += 1;
                    continue;
                }
                let Some((kind, width, rest)) = fence(line.text) else {
                    row += 1;
                    continue;
                };
                let header = rest.trim();
                let r_chunk = ["{r}", "{r,", "{r ", "{r\t", "{r"].iter().any(|prefix| {
                    header == *prefix || (*prefix != "{r" && header.starts_with(prefix))
                });
                if r_chunk {
                    if seen_r_chunks >= MAX_CHUNKS {
                        issues.push(issue(
                            line.offset,
                            row,
                            "RY120",
                            "report exceeds the 128 R chunk limit; later chunks are not analyzed",
                        ));
                        break;
                    }
                    seen_r_chunks += 1;
                }
                let header_eval = if r_chunk {
                    match header_options(parser, path, rest) {
                        Ok(Some(value)) => value,
                        Err(HeaderError::Budget) => {
                            issues.push(issue(line.offset, row, "RY120", "R chunk header exceeds the 16 KiB or 128-field static input limit; later chunks are not analyzed"));
                            break;
                        }
                        _ => {
                            let (code, message) = if header.ends_with('}') {
                                (
                                    "RY121",
                                    "R chunk header has unsupported or conflicting execution options; later chunks are not analyzed",
                                )
                            } else {
                                (
                                    "RY120",
                                    "malformed R chunk header; later chunks are not analyzed",
                                )
                            };
                            issues.push(issue(line.offset, row, code, message));
                            break;
                        }
                    }
                } else {
                    None
                };
                let start_row = row;
                let mut enabled = header_eval.unwrap_or(true);
                // `#|` options at the start of a Quarto R cell are metadata.
                // Keep these lines masked, including their Unicode bytes.
                row += 1;
                if r_chunk {
                    let mut cell_eval = None;
                    while row < rows.len() && rows[row].text.trim_start().starts_with("#|") {
                        let option = rows[row].text.trim_start().trim_start_matches("#|").trim();
                        let parsed = option
                            .split_once(':')
                            .map_or(Ok(None), |(key, value)| execution_option(key, value, false));
                        match parsed {
                            Ok(Some(value)) if cell_eval.replace(value).is_none() => {}
                            Ok(Some(_)) | Err(()) => {
                                issues.push(issue(rows[row].offset, row, "RY121", "R chunk has dynamic or conflicting execution options; later chunks are not analyzed"));
                                break;
                            }
                            Ok(None) => {}
                        }
                        row += 1;
                    }
                    if !issues.is_empty() {
                        break;
                    }
                    if let Some(value) = cell_eval {
                        if header_eval.is_some() && value != enabled {
                            // A header and a cell option disagree; neither is a
                            // static execution certificate.
                            issues.push(issue(
                                line.offset,
                                row.saturating_sub(1),
                                "RY121",
                                "R chunk execution options conflict; later chunks are not analyzed",
                            ));
                            break;
                        }
                        enabled = value;
                    }
                }
                let body_start = if r_chunk && row < rows.len() {
                    rows[row].offset
                } else {
                    line.offset + line.text.len()
                };
                open = Some((kind, width, r_chunk, enabled, start_row, row, body_start));
            }
            if let Some((_, _, r_chunk, _, start_row, _, _)) = open
                && issues.is_empty()
                && r_chunk
            {
                issues.push(issue(
                    rows[start_row].offset,
                    start_row,
                    "RY120",
                    "unclosed R chunk fence; its body is not analyzed",
                ));
            }
        }
        // A chunk is only admitted after its matching close. Parse it on
        // its own as well, so syntax cannot accidentally continue through
        // masked Markdown between chunks.
        for &(_, _, start, end) in &chunks {
            masked[start..end].copy_from_slice(&source.as_bytes()[start..end]);
        }
    }
    let masked = String::from_utf8(masked).expect("ASCII mask plus valid UTF-8 R chunks");
    let (mut file, tree) = parser.parse_with_tree(path, &masked, old_tree)?;
    file.source = source.to_owned();
    file.input_issues = issues;
    file.parse_errors.extend(chunk_parse_errors);
    file.parse_errors.sort_by_key(|span| (span.start, span.end));
    file.parse_errors.dedup();
    Ok((file, tree))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_issue_spans_end_on_original_character_boundaries() {
        let source = "é🙂\n";
        for (offset, expected) in [
            (0, (0, 2)),
            (1, (0, 2)),
            (2, (2, 6)),
            (4, (2, 6)),
            (6, (6, 7)),
            (7, (7, 7)),
            (usize::MAX, (7, 7)),
        ] {
            let span = source_issue(source, offset, 0, "RY120", "limit").span;
            assert_eq!((span.start, span.end), expected, "offset {offset}");
            assert!(source.is_char_boundary(span.start));
            assert!(source.is_char_boundary(span.end));
        }
        assert_eq!(source_issue("", 0, 0, "RY120", "limit").span.end, 0);
        assert_eq!(source_issue("a", 0, 0, "RY120", "limit").span.end, 1);
    }

    #[test]
    fn report_mask_keeps_original_offsets_and_execution_boundaries() {
        let source = "é prose\r\n```{r}\r\nx <- 1L\r\n```\r\n~~~{r, eval = FALSE}\r\nx <- 2L\r\n~~~\r\n```{r, include = FALSE}\r\nx + \"s\"\r\n```\r\n";
        let mut parser = RParser::new().unwrap();
        let (file, _) = parse_report_with_tree(&mut parser, "a.qmd", source, None).unwrap();
        assert_eq!(file.source, source);
        assert_eq!(file.stmts.len(), 2);
        assert!(file.input_issues.is_empty());
    }

    #[test]
    fn quarto_options_and_outer_fences_do_not_invent_execution() {
        let source = "~~~~~python\n```{r}\nforeign <- 1L\n```\n~~~~~\n```{{r}}\nexample <- 1L\n```\n```r\nexample2 <- 1L\n```\n```{r}\n#| eval: false\nremoved <- 1L\n```\n```{r}\n#| include: false\n#| echo: false\nretained <- 1L\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", source, None).unwrap();
        assert!(file.input_issues.is_empty());
        assert!(file.parse_errors.is_empty());
        assert_eq!(file.stmts.len(), 1);
        assert!(file.comments.is_empty(), "cell options remain metadata");
    }

    #[test]
    fn dynamic_execution_stops_later_chunks_but_keeps_earlier_evidence() {
        let source = "```{r}\nfirst <- 1L\n```\n```{r, eval=choose()}\nuncertain <- 1L\n```\n```{r}\nlater <- first\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", source, None).unwrap();
        assert_eq!(file.stmts.len(), 1);
        assert_eq!(file.input_issues.len(), 1);
        assert_eq!(file.input_issues[0].code, "RY121");
        assert_eq!(file.input_issues[0].span.line, 3);
    }

    #[test]
    fn split_syntax_is_rejected_per_chunk_even_if_combined_mask_can_parse() {
        let source = "```{r}\nvalue <- (\n```\n```{r}\n1L)\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", source, None).unwrap();
        assert!(!file.parse_errors.is_empty());
        assert!(
            file.parse_errors
                .iter()
                .all(|span| span.start <= source.len())
        );
    }

    #[test]
    fn unclosed_fence_and_global_options_are_visible() {
        for source in [
            "```{r}\nx <- 1L\n",
            "---\nexecute:\n  eval: false\n---\n```{r}\nx <- 1L\n```\n",
        ] {
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", source, None)
                    .unwrap();
            assert!(file.stmts.is_empty());
            assert_eq!(file.input_issues.len(), 1);
            assert!(matches!(file.input_issues[0].code, "RY120" | "RY121"));
        }
    }

    #[test]
    fn child_and_dynamic_chunk_options_stop_static_execution() {
        for source in [
            "```{r, child='other.Rmd'}\nx <- 1L\n```\n",
            "```{r}\n#| code: external_code\nx <- 1L\n```\n",
            "```{r}\n#| eval: choose()\nx <- 1L\n```\n",
        ] {
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", source, None)
                    .unwrap();
            assert!(file.stmts.is_empty());
            assert_eq!(file.input_issues.len(), 1);
            assert_eq!(file.input_issues[0].code, "RY121");
        }
    }

    #[test]
    fn runtime_option_mentions_in_strings_and_comments_are_inert() {
        let source = "```{r}\nx <- 'opts_chunk$set(eval=FALSE)'\n# knitr::opts_chunk$set(eval=FALSE)\n```\n```{r}\ny <- 1L\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", source, None).unwrap();
        assert_eq!(file.stmts.len(), 2);
        assert!(file.input_issues.is_empty());

        let active = "```{r}\nknitr::opts_chunk$set(eval=FALSE)\n```\n```{r}\ny <- 1L\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", active, None).unwrap();
        assert_eq!(file.stmts.len(), 1);
        assert_eq!(file.input_issues[0].code, "RY121");
    }

    #[test]
    fn runtime_options_use_r_identifier_identity() {
        for body in [
            "my_opts_chunk_counter <- 1L",
            "opts_chunkish <- 1L",
            "value <- r\"(a \" opts_chunk x)\"",
            "value <- 'opts_chunk'",
        ] {
            let source = format!("```{{r}}\n{body}\nx <- 'a'\n```\n```{{r}}\nx + 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", &source, None)
                    .unwrap();
            assert_eq!(file.stmts.len(), 3, "{body}: {:?}", file.input_issues);
            assert!(file.input_issues.is_empty(), "{body}");
            assert!(
                file.parse_errors.is_empty(),
                "{body}: {:?}",
                file.parse_errors
            );
        }
        let source = "```{r}\nknitr::`opts_chunk`$set(eval=FALSE)\n```\n```{r}\nx + 1L\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", source, None).unwrap();
        assert_eq!(file.stmts.len(), 1);
        assert_eq!(file.input_issues[0].code, "RY121");

        for body in [
            "change_options <- function() knitr::opts_chunk$set(eval=FALSE)",
            "knitr::opts_chunk$set <- function(...) NULL",
            "`knitr`::opts_chunk$set(eval=FALSE)",
            r"knitr::`opts_\x63hunk`$set(eval=FALSE)",
            r"`knitr`::`opts_\x63hunk`$set(eval=FALSE)",
        ] {
            let source = format!("```{{r}}\n{body}\n```\n```{{r}}\nx + 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", &source, None)
                    .unwrap();
            assert_eq!(file.input_issues[0].code, "RY121", "{body}");
        }
    }

    #[test]
    fn header_fields_keep_quoted_commas_and_nested_expressions_together() {
        for header in [
            "{r, fig.cap=\"caption, eval=FALSE\"}",
            "{r, fig.cap=paste('a,b', c(1,2))}",
            "{r, fig.cap={\"caption\"}}",
        ] {
            let source = format!("```{header}\nx <- 'a'\n```\n```{{r}}\nx + 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", &source, None)
                    .unwrap();
            assert_eq!(file.stmts.len(), 2, "{header}: {:?}", file.input_issues);
            assert!(file.input_issues.is_empty(), "{header}");
        }
    }

    #[test]
    fn executable_header_metadata_cannot_hide_runtime_option_changes() {
        for header in [
            "{r, fig.cap={knitr::opts_chunk$set(eval=FALSE); \"caption\"}}",
            r#"{r, fig.cap={`knitr`::`opts_\x63hunk`$set(eval=FALSE); "caption"}}"#,
        ] {
            let source = format!("```{header}\nNULL\n```\n```{{r}}\n'a' + 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", &source, None)
                    .unwrap();
            assert_eq!(file.input_issues[0].code, "RY121", "{header}");
            assert_eq!(file.stmts.len(), 0, "{header}");
        }
    }

    #[test]
    fn parsed_header_metadata_has_a_visible_input_budget() {
        for header in [
            format!("{{r, fig.cap=\"{}\"}}", "a".repeat(MAX_HEADER_BYTES)),
            format!("{{r, {}}}", "fig.cap=\"x\",".repeat(MAX_HEADER_FIELDS + 1)),
        ] {
            let source = format!("```{header}\nNULL\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", &source, None)
                    .unwrap();
            assert!(file.stmts.is_empty());
            assert_eq!(file.input_issues[0].code, "RY120");
            assert!(file.input_issues[0].message.contains("header exceeds"));
        }
    }

    #[test]
    fn quoted_eval_keys_do_not_enable_disabled_chunks() {
        for source in [
            "```{r, \"eval\"=FALSE}\nx <- 'a'\n```\n",
            "```{r}\n#| \"eval\": false\nx <- 'a'\n```\n",
        ] {
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", source, None)
                    .unwrap();
            assert!(file.stmts.is_empty(), "{source}");
            assert!(file.input_issues.is_empty(), "{source}");
        }
    }

    #[test]
    fn execution_option_names_are_case_sensitive() {
        for header in ["{r, Eval=FALSE}", "{r}\n#| Eval: false"] {
            let source = format!("```{header}\n'x' + 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", &source, None)
                    .unwrap();
            assert_eq!(file.stmts.len(), 1, "{header}: {:?}", file.input_issues);
            assert!(file.input_issues.is_empty(), "{header}");
        }
    }

    #[test]
    fn r_header_booleans_require_unambiguous_literals() {
        for option in ["eval=T", "eval=t", "eval=true", "echo=F", "include=f"] {
            let source = format!("```{{r, {option}}}\nx <- 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.Rmd", &source, None)
                    .unwrap();
            assert!(file.stmts.is_empty(), "{option}");
            assert_eq!(file.input_issues[0].code, "RY121", "{option}");
        }
        let source = "```{r}\n#| eval: TRUE\nx <- 1L\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", source, None).unwrap();
        assert_eq!(file.stmts.len(), 1);
        assert!(file.input_issues.is_empty());
    }

    #[test]
    fn yaml_execution_keys_are_scoped_and_quoted_keys_are_recognized() {
        let benign = "---\nmetadata:\n  eval: false\n---\n```{r}\n'a' + 1L\n```\n";
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", benign, None).unwrap();
        assert_eq!(file.stmts.len(), 1);
        assert!(file.input_issues.is_empty());
        for front_matter in [
            "\"execute\":\n  \"eval\": false",
            "format:\n  html:\n    execute:\n      eval: false",
            "  {execute: {eval: false}}",
            "  {format: {html: {execute: {eval: false}}}}",
            "  execute:\n    eval: false",
            "  title: study\n  format:\n    html:\n      execute:\n        eval: false",
            "  !!map {execute: {eval: false}}",
            "# comment\n\n  {execute: {eval: false}}",
            "{execute: {eval: false}}",
            "{format: {html: {execute: {eval: false}}}}",
            "{\"exec\\u0075te\": {eval: false}}",
            "{format:\n  {html: {execute: {eval: false}}}}",
            "!!map {execute: {eval: false}}",
            "format: {html: {execute: {eval: false}}}",
            "settings: &fmt\n  html:\n    execute:\n      eval: false\nformat: *fmt",
            "settings: &fmt\n  html:\n    execute:\n      eval: false\nformat:\n  <<: *fmt",
            "settings: &fmt\n  execute:\n    eval: false\nformat:\n  html:\n    <<: *fmt",
            "format: {html: {\"e\\u0078ecute\": {eval: false}}}",
            "'knitr':\n  opts_chunk:\n    eval: false",
            "<<: *execution_defaults",
        ] {
            let source = format!("---\n{front_matter}\n---\n```{{r}}\n'a' + 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", &source, None)
                    .unwrap();
            assert!(file.stmts.is_empty(), "{front_matter}");
            assert_eq!(file.input_issues[0].code, "RY121", "{front_matter}");
        }

        for front_matter in [
            "metadata: {eval: false}",
            "  metadata:\n    eval: false",
            "  {title: \"test\", metadata: {execute: {eval: false}}}",
            "{title: \"test\", metadata: {execute: {eval: false}}}",
            "settings: &fmt\n  html:\n    execute:\n      eval: false\nmetadata:\n  default: *fmt",
            "format:\n  html:\n    toc: true",
        ] {
            let source = format!("---\n{front_matter}\n---\n```{{r}}\n'a' + 1L\n```\n");
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", &source, None)
                    .unwrap();
            assert_eq!(
                file.stmts.len(),
                1,
                "{front_matter}: {:?}",
                file.input_issues
            );
            assert!(file.input_issues.is_empty(), "{front_matter}");
        }
    }

    #[test]
    fn source_and_chunk_limits_report_without_parsing_unbounded_r() {
        let source = "```{r}\nx <- 1L\n```\n".repeat(MAX_CHUNKS + 1);
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", &source, None).unwrap();
        assert_eq!(file.stmts.len(), MAX_CHUNKS);
        assert_eq!(file.input_issues.len(), 1);
        assert_eq!(file.input_issues[0].code, "RY120");

        let disabled = "```{r, eval=FALSE}\nx <- 1L\n```\n".repeat(MAX_CHUNKS + 1);
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", &disabled, None).unwrap();
        assert!(file.stmts.is_empty());
        assert_eq!(file.input_issues[0].code, "RY120");

        let huge = "é".repeat(MAX_REPORT_BYTES / 2 + 1);
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", &huge, None).unwrap();
        assert!(file.stmts.is_empty());
        assert_eq!(file.input_issues[0].code, "RY120");
        assert_eq!(file.input_issues[0].span.end, "é".len());
    }

    #[test]
    fn bounded_random_fences_never_shift_original_coordinates() {
        let alphabet = [
            "é",
            "\r\n",
            "```",
            "~~~",
            "{r}",
            "#| eval: false",
            "x <- 1L",
            "\n",
        ];
        let mut seed = 0x1234_5678_u32;
        for _ in 0..256 {
            let mut source = String::new();
            for _ in 0..20 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                source.push_str(alphabet[(seed as usize) % alphabet.len()]);
            }
            let (file, _) =
                parse_report_with_tree(&mut RParser::new().unwrap(), "f.qmd", &source, None)
                    .unwrap();
            assert_eq!(file.source, source);
            assert!(file.input_issues.iter().all(|issue| {
                issue.span.start <= issue.span.end
                    && issue.span.end <= source.len()
                    && source.is_char_boundary(issue.span.start)
                    && source.is_char_boundary(issue.span.end)
            }));
            assert!(
                file.parse_errors
                    .iter()
                    .all(|span| span.start <= source.len())
            );
        }
    }
}
