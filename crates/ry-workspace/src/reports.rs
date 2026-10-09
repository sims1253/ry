//! Bounded, source-mapped static input for ordinary fenced R report chunks.
//! No report engine is invoked. Every retained R byte has its original
//! offset; all other non-newline bytes become ASCII spaces.

use std::ops::ControlFlow;
use std::path::Path;

use ry_core::ast::{Expr, InputIssue, Stmt};
use ry_core::parser::unquote_r_string;
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use ry_core::{ParseError, RParser, SourceFile, Span, Tree};

pub const MAX_REPORT_BYTES: usize = 2 * 1024 * 1024;
const MAX_CHUNKS: usize = 128;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADER_FIELDS: usize = 128;

#[derive(Clone, Copy)]
enum HeaderError {
    Budget,
    Unsupported,
}

/// Report extensions. Like knitr, matching ignores ASCII case, so
/// `report.RMD` and `report.Qmd` are reports too.
pub const REPORT_EXTENSIONS: &[&str] = &["rmd", "qmd"];

pub fn is_report_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            REPORT_EXTENSIONS
                .iter()
                .any(|report| ext.eq_ignore_ascii_case(report))
        })
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

fn is_quoted(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2
        && matches!(
            (bytes[0], bytes[bytes.len() - 1]),
            (b'`', b'`') | (b'"', b'"') | (b'\'', b'\'')
        )
}

/// A real reference to knitr's chunk-option object can change the execution
/// of later chunks. Only lowered R identifiers count, so strings, comments,
/// and similar names stay inert. Function bodies and assignment targets count
/// conservatively: a later call or replacement could use either reference.
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
                // Namespace components are already decoded by the parser;
                // only a standalone backtick name is decoded here.
                let is_options = if let Some((package, object)) = name.rsplit_once("::") {
                    package.trim_end_matches(':') == "knitr" && object == "opts_chunk"
                } else if is_quoted(name) {
                    unquote_r_string(name) == "opts_chunk"
                } else {
                    name == "opts_chunk"
                };
                if is_options {
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

/// The header text after an `{r` engine name. knitr lowercases the engine,
/// so `{R}` is an R chunk too. A missing `}` is kept so it can be refused.
fn r_engine_tail(info: &str) -> Option<&str> {
    let tail = info
        .strip_prefix("{r")
        .or_else(|| info.strip_prefix("{R"))?;
    (tail.is_empty() || tail.starts_with(['}', ',', ' ', '\t'])).then_some(tail)
}

/// Whether a backtick fence's info string opens a knitr chunk of any
/// engine, as knitr's `\{([a-zA-Z0-9_]+( *[ ,].*)?)\}` pattern reads it.
fn knitr_chunk(info: &str) -> bool {
    info.trim()
        .strip_prefix('{')
        .and_then(|info| info.strip_suffix('}'))
        .is_some_and(|inner| {
            let engine = inner
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                .count();
            engine > 0 && (engine == inner.len() || inner[engine..].starts_with([' ', ',']))
        })
}

fn simple_key(key: &str) -> Result<&str, ()> {
    let key = key.trim();
    if is_quoted(key) {
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
    // knitr reads a dashed YAML option name as the dotted R name.
    let key = simple_key(key)?;
    let key = if r_header {
        key.to_owned()
    } else {
        key.replace('-', ".")
    };
    // R headers need the uppercase literals; YAML booleans ignore case.
    let value = value.trim();
    let literal = |truth: &str| {
        if r_header {
            value == truth
        } else {
            value.eq_ignore_ascii_case(truth)
        }
    };
    let boolean = if literal("TRUE") {
        Ok(true)
    } else if literal("FALSE") {
        Ok(false)
    } else {
        Err(())
    };
    match key.as_str() {
        "child" | "ref.label" | "opts.label" | "engine" | "file" | "code" | "dependson" => Err(()),
        "eval" => boolean.map(Some),
        "include" | "echo" => boolean.map(|_| None),
        _ => Ok(None),
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

/// Whether `value` starts a scalar that continues on deeper lines: a block
/// scalar (`|`, `>-`, `|2`) or a quoted scalar left open on this line.
fn multiline_scalar(value: &str) -> bool {
    let value = value.trim();
    let mut chars = value.chars();
    match chars.next() {
        Some('|' | '>') => chars
            .as_str()
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .all(|indicator| matches!(indicator, '+' | '-' | '1'..='9')),
        Some(quote @ ('"' | '\'')) => value.len() == 1 || !value.ends_with(quote),
        _ => false,
    }
}

/// Whether this bounded reader can name `key`: it is non-empty and not an
/// explicit (`?`), tagged, anchored, aliased, merge, collection, or
/// sequence-entry form.
fn plain_key(key: &str) -> bool {
    !key.is_empty() && !key.starts_with(['?', '!', '&', '*', '[', '{', '<', '>', '|', '-'])
}

/// Document metadata keys that select or configure execution: the engine
/// (`engine`, `jupyter`) and its options. YAML merges can inherit them.
fn execution_key(key: &str) -> bool {
    matches!(
        key,
        "execute" | "knitr" | "eval" | "engine" | "jupyter" | "<<"
    )
}

/// The end of an R raw string (`r"(...)"`, `R'--[...]--'`) opening at
/// `pos`: `None` if no raw string opens there, `Some(None)` if it is never
/// closed. An `r` that continues a name (`xr"(a)"`) opens nothing.
fn raw_string_end(bytes: &[u8], pos: usize) -> Option<Option<usize>> {
    let continues_name = pos.checked_sub(1).is_some_and(|before| {
        let before = bytes[before];
        before.is_ascii_alphanumeric() || matches!(before, b'.' | b'_') || !before.is_ascii()
    });
    if !matches!(bytes[pos], b'r' | b'R') || continues_name {
        return None;
    }
    let quote = *bytes.get(pos + 1).filter(|&&b| b == b'"' || b == b'\'')?;
    let dashes = bytes[pos + 2..].iter().take_while(|&&b| b == b'-').count();
    let close = match bytes.get(pos + 2 + dashes)? {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        _ => return None,
    };
    let mut terminator = vec![close];
    terminator.extend(std::iter::repeat_n(b'-', dashes));
    terminator.push(quote);
    let content = pos + 3 + dashes;
    Some(
        bytes[content..]
            .windows(terminator.len())
            .position(|window| window == terminator)
            .map(|offset| content + offset + terminator.len()),
    )
}

/// Split only at header commas outside ordinary quotes and nested syntax.
/// With `r_syntax`, R raw strings are single values as well.
fn header_fields(inner: &str, r_syntax: bool) -> Result<Vec<&str>, HeaderError> {
    let bytes = inner.as_bytes();
    let mut fields = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut nesting = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if quote.is_none()
            && r_syntax
            && let Some(end) = raw_string_end(bytes, pos)
        {
            pos = end.ok_or(HeaderError::Unsupported)?;
            continue;
        }
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
                b'\'' | b'"' | b'`' => {
                    // A YAML quote opens only a whole scalar; inside a plain
                    // scalar (`a"b`) it is text, so refuse to guess fields.
                    let opens = r_syntax
                        || matches!(
                            inner[..pos].trim_end().bytes().next_back(),
                            None | Some(b'{' | b'[' | b',' | b':')
                        );
                    if !opens {
                        return Err(HeaderError::Unsupported);
                    }
                    quote = Some(byte);
                }
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
    for field in header_fields(inner, false).map_err(|_| ())? {
        let (key, _) = yaml_key_value(field)?.ok_or(())?;
        if !plain_key(key) {
            return Err(());
        }
        if execution_key(key) || key == "format" {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Parse a `{r ...}` header. Header metadata is evaluated as R before the
/// body, so a real reference to knitr's options object makes it unsupported
/// even when the option itself is just a caption or plot setting.
fn header_options(
    parser: &mut RParser,
    path: &str,
    rest: &str,
) -> Result<Option<bool>, HeaderError> {
    let header = rest.trim();
    let Some(inner) = r_engine_tail(header).and_then(|tail| tail.strip_suffix('}')) else {
        return Err(HeaderError::Unsupported);
    };
    if header.len() > MAX_HEADER_BYTES {
        return Err(HeaderError::Budget);
    }
    // knitr reads `{r<TAB>...}` as no chunk at all; refuse rather than run it.
    if !inner.is_empty() && !inner.starts_with([',', ' ']) {
        return Err(HeaderError::Unsupported);
    }
    r_options(parser, path, inner, true)
}

/// Read comma-separated `key = value` R options. A header may carry a chunk
/// label and bare fields; `#|` cell options must be plain assignments.
fn r_options(
    parser: &mut RParser,
    path: &str,
    fields: &str,
    header: bool,
) -> Result<Option<bool>, HeaderError> {
    let mut eval = None;
    for field in header_fields(fields, true)? {
        let (words, value) = match field.split_once('=') {
            Some(pair) => pair,
            None if header || field.trim().is_empty() => continue,
            None => return Err(HeaderError::Unsupported),
        };
        // A header's leading label (`setup eval = FALSE`) is not part of the key.
        let mut words = words.split_whitespace();
        let key = words.next_back().unwrap_or("");
        if !header && words.next().is_some() {
            return Err(HeaderError::Unsupported);
        }
        if let Some(value) =
            execution_option(key, value, true).map_err(|()| HeaderError::Unsupported)?
            && eval.replace(value).is_some()
        {
            return Err(HeaderError::Unsupported);
        }
        let expression = parser
            .parse(path, value.trim())
            .map_err(|_| HeaderError::Unsupported)?;
        if !expression.parse_errors.is_empty() || has_runtime_chunk_options(&expression.stmts) {
            return Err(HeaderError::Unsupported);
        }
    }
    Ok(eval)
}

/// Classify a cell's leading `#|` lines (without the `#|`) as knitr does:
/// YAML when the first line looks like `key:`, otherwise comma-separated R
/// options. Any line this reader cannot classify is an error at its index.
fn cell_options(
    parser: &mut RParser,
    path: &str,
    lines: &[&str],
) -> Result<Option<bool>, (usize, HeaderError)> {
    let Some(first) = lines.first() else {
        return Ok(None);
    };
    let first = first.strip_prefix(' ').unwrap_or(first);
    let yaml = first.split_once(':').is_some_and(|(key, rest)| {
        !key.is_empty() && !key.contains(' ') && rest.chars().next().is_none_or(char::is_whitespace)
    });
    if !yaml {
        let fields = lines.join(",");
        if fields.len() > MAX_HEADER_BYTES {
            return Err((0, HeaderError::Budget));
        }
        return r_options(parser, path, &fields, false).map_err(|error| (0, error));
    }
    let indent = |line: &str| line.len() - line.trim_start_matches(' ').len();
    let base = indent(lines[0]);
    let mut eval = None;
    for (index, line) in lines.iter().enumerate() {
        let content = line.trim();
        // Deeper lines continue the previous value; only root keys select options.
        if content.is_empty() || content.starts_with('#') || indent(line) > base {
            continue;
        }
        let unsupported = (index, HeaderError::Unsupported);
        let Ok(Some((key, value))) = yaml_key_value(content) else {
            return Err(unsupported);
        };
        if indent(line) < base || line.trim_start_matches(' ').starts_with('\t') || !plain_key(key)
        {
            return Err(unsupported);
        }
        if let Some(value) = execution_option(key, value, false).map_err(|()| unsupported)?
            && eval.replace(value).is_some()
        {
            return Err(unsupported);
        }
    }
    Ok(eval)
}

/// Parse `source` as plain R, or through the report mask when `path` names
/// a report. Reports return no tree: an option edit can mask or unmask text
/// outside the edited range, so a masked tree is never safe to reuse.
pub fn parse_source_with_tree(
    parser: &mut RParser,
    path: &str,
    source: &str,
    old_tree: Option<&Tree>,
) -> Result<(SourceFile, Option<Tree>), ParseError> {
    if is_report_path(Path::new(path)) {
        parse_report(parser, path, source).map(|file| (file, None))
    } else {
        let (file, tree) = parser.parse_with_tree(path, source, old_tree)?;
        Ok((file, Some(tree)))
    }
}

pub fn parse_source(
    parser: &mut RParser,
    path: &str,
    source: &str,
) -> Result<SourceFile, ParseError> {
    parse_source_with_tree(parser, path, source, None).map(|(file, _)| file)
}

fn parse_report(parser: &mut RParser, path: &str, source: &str) -> Result<SourceFile, ParseError> {
    if source.len() > MAX_REPORT_BYTES {
        let mut file = parser.parse(path, "")?;
        file.source = source.to_owned();
        file.input_issues.push(source_issue(
            source,
            0,
            0,
            "RY120",
            "report exceeds the 2 MiB static input limit",
        ));
        return Ok(file);
    }
    let mut report = Report {
        source,
        rows: lines(source),
        masked: source
            .bytes()
            .map(|byte| {
                if matches!(byte, b'\n' | b'\r') {
                    byte
                } else {
                    b' '
                }
            })
            .collect(),
        chunk_errors: Vec::new(),
    };
    let issue = match report.metadata_blocks() {
        Ok(blocks) => report.chunks(parser, path, &blocks)?,
        Err(issue) => Some(issue),
    };
    let masked = String::from_utf8(report.masked).expect("ASCII mask plus valid UTF-8 R chunks");
    let mut file = parser.parse(path, &masked)?;
    file.source = source.to_owned();
    file.input_issues.extend(issue);
    // Every admitted byte belongs to a chunk that parsed on its own, so a
    // failed chunk's errors are the report's. The combined parse only
    // echoes them, shifted: an unclosed string or bracket runs on through
    // later chunks, to a clean chunk or the end of the document.
    if !report.chunk_errors.is_empty() {
        file.parse_errors = report.chunk_errors;
    }
    file.parse_errors.sort_by_key(|span| (span.start, span.end));
    file.parse_errors.dedup();
    Ok(file)
}

struct Report<'a> {
    source: &'a str,
    rows: Vec<Line<'a>>,
    /// The source with only admitted R chunk bodies left unmasked.
    masked: Vec<u8>,
    /// Each admitted chunk also parses on its own, so syntax cannot
    /// continue through masked Markdown between chunks.
    chunk_errors: Vec<Span>,
}

impl Report<'_> {
    fn issue(&self, row: usize, code: &'static str, message: &str) -> InputIssue {
        source_issue(self.source, self.rows[row].offset, row, code, message)
    }

    fn blank(&self, row: usize) -> bool {
        self.rows[row].text.trim().is_empty()
    }

    fn closing_fence(&self, from: usize, kind: u8, width: usize) -> Option<usize> {
        (from..self.rows.len()).find(|&row| {
            fence(self.rows[row].text).is_some_and(|(close_kind, close_width, tail)| {
                close_kind == kind && close_width >= width && tail.trim().is_empty()
            })
        })
    }

    /// The first R chunk header among the skipped `rows` of a fence of
    /// `width` backticks. knitr does not see fences that open no chunk
    /// (tildes, plain info strings), and inside a chunk a header with the
    /// same backtick count starts a new one, so such a header may run.
    fn nested_r_chunk(
        &self,
        rows: std::ops::Range<usize>,
        width: usize,
        outer_chunk: bool,
    ) -> Option<usize> {
        rows.into_iter().find(|&row| {
            fence(self.rows[row].text).is_some_and(|(kind, inner_width, rest)| {
                kind == b'`'
                    && r_engine_tail(rest.trim()).is_some()
                    && (!outer_chunk || inner_width == width)
            })
        })
    }

    /// Row ranges of the YAML metadata blocks outside code fences. Pandoc
    /// reads every block, and each applies to the whole document, so all are
    /// checked before any chunk is admitted. A `---` line after text is a
    /// setext underline and one before a blank line is a horizontal rule.
    fn metadata_blocks(&self) -> Result<Vec<(usize, usize)>, InputIssue> {
        let mut blocks = Vec::new();
        let mut row = 0;
        while row < self.rows.len() {
            if let Some((kind, width, _)) = fence(self.rows[row].text) {
                row = self
                    .closing_fence(row + 1, kind, width)
                    .map_or(self.rows.len(), |close| close + 1);
            } else if self.rows[row].text.trim() == "---"
                && (row == 0 || self.blank(row - 1))
                && row + 1 < self.rows.len()
                && !self.blank(row + 1)
            {
                // Like Pandoc, an unclosed `---` after the start is a rule.
                let close = (row + 1..self.rows.len())
                    .find(|&close| matches!(self.rows[close].text.trim_end(), "---" | "..."));
                match close {
                    Some(close) => {
                        self.metadata_block(row, close)?;
                        blocks.push((row, close + 1));
                        row = close + 1;
                    }
                    None if (0..row).all(|row| self.blank(row)) => {
                        return Err(self.issue(row, "RY120", "unclosed report YAML front matter"));
                    }
                    None => row += 1,
                }
            } else {
                row += 1;
            }
        }
        Ok(blocks)
    }

    /// Check the metadata block between `open` and its unindented `---` or
    /// `...` at `close`. Execution settings change every chunk.
    fn metadata_block(&self, open: usize, close: usize) -> Result<(), InputIssue> {
        let unclassified = |row, what: &str| {
            self.issue(
                row,
                "RY121",
                &format!("{what} cannot be classified safely; no chunks are assumed executable"),
            )
        };
        let mut root_key = None;
        let mut root_indent = None;
        let mut scalar_indent = None;
        for (row, line) in self.rows.iter().enumerate().take(close).skip(open + 1) {
            let raw = line.text.trim_end_matches(['\r', '\n']);
            let content = raw.trim_start_matches(' ');
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            let indent = raw.len() - content.len();
            // Lines deeper than a multi-line scalar's key are its text.
            if scalar_indent.is_some_and(|key| indent > key) {
                continue;
            }
            scalar_indent = None;
            let root = *root_indent.get_or_insert(indent);
            if indent < root || content.starts_with('\t') {
                return Err(unclassified(row, "report YAML indentation"));
            }
            let at_root = indent == root;
            if at_root && content.starts_with('{') {
                if root_flow_execution(content) == Ok(false) {
                    continue;
                }
                return Err(unclassified(row, "root flow YAML execution settings"));
            }
            // Explicit, empty, tagged, anchored, and alias keys are refused at
            // any depth; below the root a key may follow a sequence dash.
            let (key, value) = match yaml_key_value(raw) {
                _ if content.starts_with('?') => {
                    return Err(unclassified(row, "report YAML key"));
                }
                Ok(Some((key, _))) if at_root && !plain_key(key) => {
                    return Err(unclassified(row, "report YAML root syntax"));
                }
                Ok(Some((key, _))) if !plain_key(key.trim_start_matches(['-', ' '])) => {
                    return Err(unclassified(row, "report YAML key"));
                }
                Ok(Some(entry)) => entry,
                Err(()) => return Err(unclassified(row, "report YAML key")),
                Ok(None) if at_root => return Err(unclassified(row, "report YAML root syntax")),
                Ok(None) => continue,
            };
            if at_root {
                root_key = Some(key);
            }
            let in_format = root_key == Some("format");
            if ((at_root || in_format) && execution_key(key))
                || (in_format && complex_format_value(value))
            {
                return Err(self.issue(
                    row,
                    "RY121",
                    "report-level execution options need a report engine; no chunks are assumed executable",
                ));
            }
            if multiline_scalar(value) {
                scalar_indent = Some(indent);
            }
        }
        Ok(())
    }

    /// Admit enabled R chunks in document order, skipping metadata `blocks`,
    /// and stop at the first boundary that makes later execution uncertain.
    fn chunks(
        &mut self,
        parser: &mut RParser,
        path: &str,
        blocks: &[(usize, usize)],
    ) -> Result<Option<InputIssue>, ParseError> {
        let mut r_chunks = 0;
        let mut row = 0;
        while row < self.rows.len() {
            if let Some(&(_, end)) = blocks.iter().find(|(start, _)| *start == row) {
                row = end;
                continue;
            }
            let Some((kind, width, rest)) = fence(self.rows[row].text) else {
                row += 1;
                continue;
            };
            // knitr and Quarto run backtick chunks only; `~~~{r}` is display.
            let r_chunk = kind == b'`' && r_engine_tail(rest.trim()).is_some();
            let (enabled, body_row) = if r_chunk {
                if r_chunks == MAX_CHUNKS {
                    return Ok(Some(self.issue(
                        row,
                        "RY120",
                        "report exceeds the 128 R chunk limit; later chunks are not analyzed",
                    )));
                }
                r_chunks += 1;
                match self.chunk_options(parser, path, row, rest) {
                    Ok(options) => options,
                    Err(issue) => return Ok(Some(issue)),
                }
            } else {
                (false, row + 1)
            };
            let close = self.closing_fence(body_row, kind, width);
            if !enabled {
                let outer_chunk = kind == b'`' && knitr_chunk(rest);
                let skipped = body_row..close.unwrap_or(self.rows.len());
                if let Some(nested) = self.nested_r_chunk(skipped, width, outer_chunk) {
                    return Ok(Some(self.issue(
                        nested,
                        "RY121",
                        "an R chunk inside another fence may run; later chunks are not analyzed",
                    )));
                }
            }
            let Some(close) = close else {
                return Ok(r_chunk.then(|| {
                    self.issue(
                        row,
                        "RY120",
                        "unclosed R chunk fence; its body is not analyzed",
                    )
                }));
            };
            if enabled {
                let body = self.rows[body_row].offset..self.rows[close].offset;
                let chunk = parser.parse(path, &self.source[body.clone()])?;
                self.chunk_errors
                    .extend(chunk.parse_errors.iter().map(|span| {
                        Span::new(
                            span.start + body.start,
                            span.end + body.start,
                            span.line + body_row,
                            span.col,
                        )
                    }));
                self.masked[body.clone()].copy_from_slice(&self.source.as_bytes()[body]);
                if has_runtime_chunk_options(&chunk.stmts) {
                    return Ok(Some(self.issue(
                        row,
                        "RY121",
                        "runtime chunk options may change later execution; later chunks are not analyzed",
                    )));
                }
            }
            row = close + 1;
        }
        Ok(None)
    }

    /// Read an R chunk's header and its leading `#|` cell options, which stay
    /// masked metadata. Returns whether the chunk runs and its first body row.
    fn chunk_options(
        &self,
        parser: &mut RParser,
        path: &str,
        row: usize,
        rest: &str,
    ) -> Result<(bool, usize), InputIssue> {
        let header_eval = match header_options(parser, path, rest) {
            Ok(eval) => eval,
            Err(HeaderError::Budget) => {
                return Err(self.issue(
                    row,
                    "RY120",
                    "R chunk header exceeds the 16 KiB or 128-field static input limit; later chunks are not analyzed",
                ));
            }
            Err(HeaderError::Unsupported) if rest.trim().ends_with('}') => {
                return Err(self.issue(
                    row,
                    "RY121",
                    "R chunk header has unsupported or conflicting execution options; later chunks are not analyzed",
                ));
            }
            Err(HeaderError::Unsupported) => {
                return Err(self.issue(
                    row,
                    "RY120",
                    "malformed R chunk header; later chunks are not analyzed",
                ));
            }
        };
        let options: Vec<&str> = self.rows[row + 1..]
            .iter()
            .map_while(|line| line.text.trim_start().strip_prefix("#|"))
            .map(str::trim_end)
            .collect();
        let body_row = row + 1 + options.len();
        let cell_eval = cell_options(parser, path, &options).map_err(|(index, error)| {
            let (code, message) = match error {
                HeaderError::Budget => (
                    "RY120",
                    "R chunk options exceed the 16 KiB or 128-field static input limit; later chunks are not analyzed",
                ),
                HeaderError::Unsupported => (
                    "RY121",
                    "R chunk has dynamic or conflicting execution options; later chunks are not analyzed",
                ),
            };
            self.issue(row + 1 + index, code, message)
        })?;
        match (header_eval, cell_eval) {
            (Some(header), Some(cell)) if header != cell => Err(self.issue(
                row,
                "RY121",
                "R chunk execution options conflict; later chunks are not analyzed",
            )),
            _ => Ok((cell_eval.or(header_eval).unwrap_or(true), body_row)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> SourceFile {
        parse_report(&mut RParser::new().unwrap(), "a.qmd", source).unwrap()
    }

    /// Assert the number of admitted statements and the boundary code, if any.
    #[track_caller]
    fn assert_report(source: &str, stmts: usize, code: Option<&str>) {
        let file = parse(source);
        let issue = file.input_issues.first().map(|issue| issue.code);
        assert_eq!(
            (file.stmts.len(), issue),
            (stmts, code),
            "{source}: {:?}",
            file.input_issues
        );
    }

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
        }
        assert_eq!(source_issue("", 0, 0, "RY120", "limit").span.end, 0);
        assert_eq!(source_issue("a", 0, 0, "RY120", "limit").span.end, 1);
    }

    #[test]
    fn report_mask_keeps_original_offsets_and_execution_boundaries() {
        let source = "é prose\r\n```{r}\r\nx <- 1L\r\n```\r\n~~~{r, eval = FALSE}\r\nx <- 2L\r\n~~~\r\n```{r, include = FALSE}\r\nx + \"s\"\r\n```\r\n";
        let file = parse(source);
        assert_eq!(file.source, source);
        assert_report(source, 2, None);
    }

    #[test]
    fn quarto_options_and_outer_fences_do_not_invent_execution() {
        let source = "````{verbatim}\n```{r}\nforeign <- 1L\n```\n````\n```{{r}}\nexample <- 1L\n```\n```r\nexample2 <- 1L\n```\n~~~{r}\ndisplayed <- 1L\n~~~\n```{r}\n#| eval: false\nremoved <- 1L\n```\n```{r}\n#| include: false\n#| echo: false\nretained <- 1L\n```\n";
        let file = parse(source);
        assert!(file.parse_errors.is_empty());
        assert!(file.comments.is_empty(), "cell options remain metadata");
        assert_report(source, 1, None);
    }

    #[test]
    fn knitr_chunk_headers_follow_knitr_fence_rules() {
        let later = "```{r}\nx + 1L\n```\n";
        for (source, stmts, code) in [
            // knitr lowercases the engine name.
            (format!("```{{R}}\nx <- 'a'\n```\n{later}"), 2, None),
            (
                format!("```{{R, eval=FALSE}}\nx <- 1L\n```\n{later}"),
                1,
                None,
            ),
            ("```{Rcpp}\nint x;\n```\n".to_owned(), 0, None),
            // knitr never runs a tilde fence, and `{r<TAB>` opens no chunk.
            (format!("~~~{{r}}\nx <- 'a'\n~~~\n{later}"), 1, None),
            (
                format!("```{{r\tlabel}}\nx <- 'a'\n```\n{later}"),
                0,
                Some("RY121"),
            ),
            // knitr does not see fences that open no chunk, so an inner
            // R chunk runs; inside a chunk only an equal fence starts one.
            (
                format!("~~~~python\n```{{r}}\nx <- 'a'\n```\n~~~~\n{later}"),
                0,
                Some("RY121"),
            ),
            (
                format!("````markdown\n```{{R}}\nx <- 'a'\n```\n````\n{later}"),
                0,
                Some("RY121"),
            ),
            (
                format!("```{{python}}\n```{{r}}\nx <- 'a'\n```\n{later}"),
                0,
                Some("RY121"),
            ),
            (
                format!("```{{r, eval=FALSE}}\n```{{r}}\nx <- 'a'\n```\n{later}"),
                0,
                Some("RY121"),
            ),
            (
                "~~~\nunclosed\n```{r}\nx <- 'a'\n```\n".to_owned(),
                0,
                Some("RY121"),
            ),
            (
                format!("````{{python}}\n```{{r}}\nx <- 'a'\n```\n````\n{later}"),
                1,
                None,
            ),
        ] {
            assert_report(&source, stmts, code);
        }
    }

    #[test]
    fn execution_boundaries_keep_earlier_evidence_and_their_own_row() {
        for (source, line) in [
            (
                "```{r}\nfirst <- 1L\n```\n```{r, eval=choose()}\nuncertain <- 1L\n```\n```{r}\nlater <- first\n```\n",
                3,
            ),
            (
                "```{r}\nfirst <- 1L\n```\n```{r, eval=TRUE}\n#| echo: false\n#| eval: false\nx <- 1L\n```\n",
                3,
            ),
        ] {
            let file = parse(source);
            assert_report(source, 1, Some("RY121"));
            let span = file.input_issues[0].span;
            assert_eq!(span.line, line);
            assert_eq!(
                source[..span.start].matches('\n').count(),
                line,
                "offset and line describe the same row"
            );
        }
    }

    #[test]
    fn an_unclosed_chunk_reports_only_its_own_syntax_errors() {
        for (source, lines) in [
            (
                "```{r}\nx <- \"abc\n```\nprose\n```{r}\ny <- 1L\n```\n",
                &[2][..],
            ),
            ("```{r}\nx <- (\n```\n```{r}\ny <- 1L\n```\n", &[1]),
            // A later chunk's own error is still reported.
            ("```{r}\nx <- r\"(abc\n```\n```{r}\ny <- )\n```\n", &[1, 4]),
        ] {
            let mut errors: Vec<_> = parse(source)
                .parse_errors
                .iter()
                .map(|span| span.line)
                .collect();
            errors.dedup();
            assert_eq!(errors, lines, "{source}");
        }
    }

    #[test]
    fn split_syntax_is_rejected_per_chunk_even_if_combined_mask_can_parse() {
        let source = "```{r}\nvalue <- (\n```\n```{r}\n1L)\n```\n";
        let file = parse(source);
        assert!(!file.parse_errors.is_empty());
        assert!(
            file.parse_errors
                .iter()
                .all(|span| span.start <= source.len())
        );
    }

    #[test]
    fn chunk_options_decide_execution() {
        for (source, stmts, code) in [
            ("```{r}\nx <- 1L\n", 0, Some("RY120")),
            ("```{r\nx <- 1L\n```\n", 0, Some("RY120")),
            (
                "```{r, child='other.Rmd'}\nx <- 1L\n```\n",
                0,
                Some("RY121"),
            ),
            (
                "```{r}\n#| code: external_code\nx <- 1L\n```\n",
                0,
                Some("RY121"),
            ),
            (
                "```{r}\n#| eval: choose()\nx <- 1L\n```\n",
                0,
                Some("RY121"),
            ),
            // Quoted keys are recognized; option names are case-sensitive.
            ("```{r, \"eval\"=FALSE}\nx <- 'a'\n```\n", 0, None),
            ("```{r}\n#| \"eval\": false\nx <- 'a'\n```\n", 0, None),
            ("```{r, Eval=FALSE}\n'x' + 1L\n```\n", 1, None),
            ("```{r}\n#| Eval: false\n'x' + 1L\n```\n", 1, None),
            // R headers need uppercase literals; Quarto accepts YAML booleans.
            ("```{r, eval=T}\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r, eval=t}\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r, eval=true}\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r, echo=F}\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r, include=f}\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r}\n#| eval: TRUE\nx <- 1L\n```\n", 1, None),
            // knitr also reads R-style `#|` options; anything else is refused.
            ("```{r}\n#| eval = FALSE\nx <- 1L\n```\n", 0, None),
            (
                "```{r}\n#| echo = FALSE, eval = TRUE\nx <- 1L\n```\n",
                1,
                None,
            ),
            (
                "```{r}\n#| eval = choose()\nx <- 1L\n```\n",
                0,
                Some("RY121"),
            ),
            ("```{r}\n#| eval = F\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r}\n#| eval FALSE\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r}\n#|   eval: false\nx <- 1L\n```\n", 0, Some("RY121")),
            ("```{r}\n#| {eval: false}\nx <- 1L\n```\n", 0, Some("RY121")),
            // Only one `#|` marker is an option prefix.
            ("```{r}\n#|#| eval: false\nx <- 1L\n```\n", 0, Some("RY121")),
            (
                "```{r}\n#| echo: false\n#| eval = FALSE\nx <- 1L\n```\n",
                0,
                Some("RY121"),
            ),
            (
                "```{r}\n#| fig-cap: |\n#|   A: caption\n#| eval: false\nx <- 1L\n```\n",
                0,
                None,
            ),
            // Inherited and referenced options; knitr reads `ref-label` as `ref.label`.
            (
                "```{r disabled, eval=FALSE}\nNULL\n```\n```{r run, opts.label=\"disabled\"}\nx <- 'a'\n```\n```{r}\nx + 1L\n```\n",
                0,
                Some("RY121"),
            ),
            (
                "```{r}\n#| opts-label: disabled\nx <- 1L\n```\n",
                0,
                Some("RY121"),
            ),
            (
                "```{r}\n#| ref-label: original\nx <- 1L\n```\n",
                0,
                Some("RY121"),
            ),
        ] {
            assert_report(source, stmts, code);
        }
    }

    #[test]
    fn header_fields_keep_quoted_commas_and_nested_expressions_together() {
        for header in [
            "{r, fig.cap=\"caption, eval=FALSE\"}",
            "{r, fig.cap=paste('a,b', c(1,2))}",
            "{r, fig.cap={\"caption\"}}",
            // A raw string is one value, whatever quotes and commas it holds.
            r#"{r, fig.cap=r"(a " caption, eval=FALSE)"}"#,
            r#"{r, fig.cap=R'--[a ' b, eval=FALSE]--', echo=FALSE}"#,
        ] {
            assert_report(
                &format!("```{header}\nx <- 'a'\n```\n```{{r}}\nx + 1L\n```\n"),
                2,
                None,
            );
        }
        assert_report(
            "```{r, fig.cap=r\"(a, b)\", eval=FALSE}\nx <- 'a'\n```\n",
            0,
            None,
        );
        for header in [
            "{r, fig.cap={knitr::opts_chunk$set(eval=FALSE); \"caption\"}}",
            r#"{r, fig.cap={`knitr`::`opts_\x63hunk`$set(eval=FALSE); "caption"}}"#,
            r#"{r, fig.cap={r"(knitr)"::opts_chunk$set(eval=FALSE); "caption"}}"#,
            r#"{r, fig.cap={R"--[knitr]--"::r"(opts_chunk)"$set(eval=FALSE); "caption"}}"#,
            r#"{r, fig.cap={r"{knitr}"::R"--{opts_chunk}--"$set(eval=FALSE); "caption"}}"#,
            r"{r, fig.cap={r'(knitr)'::opts_chunk$set(eval=FALSE); 'caption'}}",
            r#"{r, fig.cap=r"(unterminated, eval=FALSE}"#,
        ] {
            assert_report(
                &format!("```{header}\nNULL\n```\n```{{r}}\n'a' + 1L\n```\n"),
                0,
                Some("RY121"),
            );
        }
        for header in [
            format!("{{r, fig.cap=\"{}\"}}", "a".repeat(MAX_HEADER_BYTES)),
            format!("{{r, {}}}", "fig.cap=\"x\",".repeat(MAX_HEADER_FIELDS + 1)),
        ] {
            let file = parse(&format!("```{header}\nNULL\n```\n"));
            assert!(file.stmts.is_empty());
            assert_eq!(file.input_issues[0].code, "RY120");
            assert!(file.input_issues[0].message.contains("header exceeds"));
        }
    }

    #[test]
    fn runtime_options_use_r_identifier_identity() {
        for body in [
            "x <- 'opts_chunk$set(eval=FALSE)'\n# knitr::opts_chunk$set(eval=FALSE)",
            "my_opts_chunk_counter <- 1L",
            "opts_chunkish <- 1L",
            "value <- r\"(a \" opts_chunk x)\"",
            "value <- 'opts_chunk'",
            "`knitr::opts_chunk` <- 1L",
            "`r\"(knitr)\"::opts_chunk` <- 1L",
            "value <- r\"(knitr::opts_chunk$set(eval=FALSE))\"",
            r#""r\"(knitr)\""::opts_chunk$set(eval=FALSE)"#,
            r#"knitr::"r\"(opts_chunk)\""$set(eval=FALSE)"#,
        ] {
            let source = format!("```{{r}}\n{body}\nx <- 'a'\n```\n```{{r}}\nx + 1L\n```\n");
            assert!(parse(&source).parse_errors.is_empty(), "{body}");
            assert_report(&source, 3, None);
        }
        for body in [
            "knitr::opts_chunk$set(eval=FALSE)",
            "knitr::`opts_chunk`$set(eval=FALSE)",
            "change_options <- function() knitr::opts_chunk$set(eval=FALSE)",
            "knitr::opts_chunk$set <- function(...) NULL",
            "`knitr`::opts_chunk$set(eval=FALSE)",
            r"knitr::`opts_\x63hunk`$set(eval=FALSE)",
            r"`knitr`::`opts_\x63hunk`$set(eval=FALSE)",
            r#"r"(knitr)"::opts_chunk$set(eval=FALSE)"#,
            r#"R"--[knitr]--"::r"(opts_chunk)"$set(eval=FALSE)"#,
            r#"r"{knitr}"::R"--{opts_chunk}--"$set(eval=FALSE)"#,
            "knitr:::opts_chunk$set(eval=FALSE)",
            "knitr:::`opts_chunk`$set(eval=FALSE)",
            "r'(knitr)'::opts_chunk$set(eval=FALSE)",
            "knitr:::R'-[opts_chunk]-'$set(eval=FALSE)",
        ] {
            assert_report(
                &format!("```{{r}}\n{body}\n```\n```{{r}}\nx + 1L\n```\n"),
                1,
                Some("RY121"),
            );
        }
    }

    #[test]
    fn yaml_execution_keys_are_scoped_and_quoted_keys_are_recognized() {
        for front_matter in [
            "execute:\n  eval: false",
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
            // Engine selectors mean Quarto may run no R at all.
            "engine: markdown",
            "{engine: markdown}",
            "jupyter: python3",
            // A quote inside a plain flow scalar is text, not a delimiter.
            "{title: a\"b, execute: {eval: false}, note: c\"d}",
            // Explicit and empty keys are not read.
            "format:\n  html:\n    ? execute\n    : {eval: false}",
            "format:\n  html:\n    \"\": x",
            "title: x\n: y",
            "format:\n  html:\n    !!str execute:\n      !!str eval: false",
            "params:\n  &anchor alpha: 1",
        ] {
            assert_report(
                &format!("---\n{front_matter}\n---\n```{{r}}\n'a' + 1L\n```\n"),
                0,
                Some("RY121"),
            );
        }
        for front_matter in [
            "metadata:\n  eval: false",
            "metadata: {eval: false}",
            "  metadata:\n    eval: false",
            "  {title: \"test\", metadata: {execute: {eval: false}}}",
            "{title: \"test\", metadata: {execute: {eval: false}}}",
            "settings: &fmt\n  html:\n    execute:\n      eval: false\nmetadata:\n  default: *fmt",
            "format:\n  html:\n    toc: true",
            // Multi-line scalar text is not read as keys.
            "abstract: |\n  Why these results?\n  ? A rhetorical question\n  : a colon line",
            "description: >-\n  ? q\nauthor:\n  - name: X\n    affiliation: Y",
            "title: \"foo\n  ? bar\"",
        ] {
            assert_report(
                &format!("---\n{front_matter}\n---\n```{{r}}\n'a' + 1L\n```\n"),
                1,
                None,
            );
        }
    }

    #[test]
    fn every_metadata_block_is_found_and_bounded() {
        let chunk = "```{r}\n'a' + 1L\n```\n";
        let execute = "---\nexecute:\n  eval: false\n---\n";
        for (source, stmts, code) in [
            // Leading blank lines and later blocks still configure the report.
            (format!("\n{execute}{chunk}"), 0, Some("RY121")),
            (format!("{chunk}\n{execute}"), 0, Some("RY121")),
            (format!("{chunk}\n---\ntitle: x\n---\n"), 1, None),
            (format!("---\ntitle: x\n...\n{chunk}"), 1, None),
            // A horizontal rule or setext underline is not a metadata block.
            (format!("---\n\n{chunk}"), 1, None),
            (format!("Title\n---\n{chunk}"), 1, None),
            (format!("Intro\n\n---\nMore prose\n{chunk}"), 1, None),
            // Only an unindented delimiter closes a block.
            (
                format!("---\ntitle: |\n  ---\n  ```{{r}}\n  x <- 1L\n  ```\n---\n{chunk}"),
                1,
                None,
            ),
            ("---\ntitle: x\n  ---\n".to_owned(), 0, Some("RY120")),
        ] {
            assert_report(&source, stmts, code);
        }
    }

    #[test]
    fn source_and_chunk_limits_report_without_parsing_unbounded_r() {
        let chunks = "```{r}\nx <- 1L\n```\n".repeat(MAX_CHUNKS + 1);
        assert_report(&chunks, MAX_CHUNKS, Some("RY120"));
        let disabled = "```{r, eval=FALSE}\nx <- 1L\n```\n".repeat(MAX_CHUNKS + 1);
        assert_report(&disabled, 0, Some("RY120"));
        assert_report("---\ntitle: x\n", 0, Some("RY120"));

        let huge = "é".repeat(MAX_REPORT_BYTES / 2 + 1);
        assert_report(&huge, 0, Some("RY120"));
        assert_eq!(parse(&huge).input_issues[0].span.end, "é".len());
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
            let file = parse(&source);
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
