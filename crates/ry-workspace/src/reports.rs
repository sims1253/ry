//! Bounded, source-mapped static input for ordinary fenced R report chunks.
//! No report engine is invoked. Every retained R byte has its original
//! offset; all other non-newline bytes become ASCII spaces.

use ry_core::ast::InputIssue;
use ry_core::{RParser, SourceFile, Span};
use tree_sitter::Tree;

pub const MAX_REPORT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHUNKS: usize = 128;

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

fn issue(offset: usize, line: usize, code: &'static str, message: &str) -> InputIssue {
    InputIssue {
        span: Span::new(offset, offset.saturating_add(1), line, 0),
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

/// Detect the common runtime chunk-option object in executable R tokens.
/// A mention inside a string, backtick name, or comment is inert.
fn has_runtime_chunk_options(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        match bytes[pos] {
            b'#' => {
                pos += 1;
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
            }
            quote @ (b'\'' | b'"' | b'`') => {
                pos += 1;
                while pos < bytes.len() {
                    if bytes[pos] == b'\\' {
                        pos = (pos + 2).min(bytes.len());
                    } else if bytes[pos] == quote {
                        pos += 1;
                        break;
                    } else {
                        pos += 1;
                    }
                }
            }
            _ => {
                if bytes[pos..].starts_with(b"opts_chunk") {
                    return true;
                }
                pos += 1;
            }
        }
    }
    false
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

fn option_value(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn execution_option(key: &str, value: &str) -> Result<Option<bool>, ()> {
    let key = key.trim();
    if ["child", "ref.label", "engine", "file", "code", "dependson"]
        .iter()
        .any(|name| key.eq_ignore_ascii_case(name))
    {
        return Err(());
    }
    if key.eq_ignore_ascii_case("eval") {
        option_value(value).map(Some).ok_or(())
    } else if key.eq_ignore_ascii_case("include") || key.eq_ignore_ascii_case("echo") {
        option_value(value).map(|_| None).ok_or(())
    } else {
        Ok(None)
    }
}

fn header_options(rest: &str) -> Result<Option<Option<bool>>, ()> {
    let header = rest.trim();
    if !(header.starts_with("{r") && header.ends_with('}')) {
        return Ok(None);
    }
    let inner = &header[2..header.len() - 1];
    if !inner.is_empty() && !inner.starts_with([',', ' ', '\t']) {
        return Ok(None);
    }
    let mut eval = None;
    for part in inner.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        // A leading label such as `setup eval = FALSE` has no effect on
        // the option name. More than one assignment in the same segment
        // leaves the value dynamic and is refused below.
        let key = key.split_whitespace().last().unwrap_or("");
        if let Some(value) = execution_option(key, value)? {
            if eval.replace(value).is_some() {
                return Err(());
            }
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
            while first < rows.len() && rows[first].text.trim() != "---" {
                let yaml = rows[first].text.trim();
                if yaml.starts_with("execute:")
                    || yaml.starts_with("knitr:")
                    || yaml.starts_with("eval:")
                {
                    issues.push(issue(rows[first].offset, first, "RY121", "report-level execution options need a report engine; no chunks are assumed executable"));
                    break;
                }
                first += 1;
            }
            if first == rows.len() {
                issues.push(issue(0, 0, "RY120", "unclosed report YAML front matter"));
            }
            first = first.saturating_add(1);
        }
        if issues.is_empty() {
            let mut open: Option<(u8, usize, bool, bool, usize, usize)> = None;
            let mut row = first;
            while row < rows.len() {
                let line = &rows[row];
                if let Some((kind, width, r_chunk, enabled, start_row, body_start)) = open {
                    if let Some((close_kind, close_width, tail)) = fence(line.text)
                        && close_kind == kind
                        && close_width >= width
                        && tail.trim().is_empty()
                    {
                        if r_chunk && enabled {
                            chunks.push((start_row, body_start, line.offset));
                            if has_runtime_chunk_options(&source[body_start..line.offset]) {
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
                if r_chunk && chunks.len() >= MAX_CHUNKS {
                    issues.push(issue(
                        line.offset,
                        row,
                        "RY120",
                        "report exceeds the 128 R chunk limit; later chunks are not analyzed",
                    ));
                    break;
                }
                let header_eval = if r_chunk {
                    match header_options(rest) {
                        Ok(Some(value)) => value,
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
                            .map_or(Ok(None), |(key, value)| execution_option(key, value));
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
                open = Some((kind, width, r_chunk, enabled, start_row, body_start));
            }
            if let Some((_, _, r_chunk, _, start_row, _)) = open
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
        for &(start_row, start, end) in &chunks {
            let chunk = parser.parse(path, &source[start..end])?;
            for span in chunk.parse_errors {
                chunk_parse_errors.push(Span::new(
                    span.start + start,
                    span.end + start,
                    span.line + start_row + 1,
                    span.col,
                ));
            }
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
    fn source_and_chunk_limits_report_without_parsing_unbounded_r() {
        let source = "```{r}\nx <- 1L\n```\n".repeat(MAX_CHUNKS + 1);
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", &source, None).unwrap();
        assert_eq!(file.stmts.len(), MAX_CHUNKS);
        assert_eq!(file.input_issues.len(), 1);
        assert_eq!(file.input_issues[0].code, "RY120");

        let huge = "é".repeat(MAX_REPORT_BYTES / 2 + 1);
        let (file, _) =
            parse_report_with_tree(&mut RParser::new().unwrap(), "a.qmd", &huge, None).unwrap();
        assert!(file.stmts.is_empty());
        assert_eq!(file.input_issues[0].code, "RY120");
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
            assert!(
                file.input_issues
                    .iter()
                    .all(|issue| issue.span.start <= source.len())
            );
            assert!(
                file.parse_errors
                    .iter()
                    .all(|span| span.start <= source.len())
            );
        }
    }
}
