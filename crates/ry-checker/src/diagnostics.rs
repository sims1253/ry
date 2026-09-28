//! Diagnostic data types, severity overrides, and inline-suppression
//! parsing/filtering.
//!
//! This module is self-contained: it depends only on `ry_core::Span`,
//! `ry_core::ast::Comment`, and the rule registry (`crate::rules`).

use ry_core::Span;

use crate::rules;

// ============================================================================
// Severity + Diagnostic
// ============================================================================

// Severity and Confidence are defined in ry-core and re-exported here
// for backward compatibility.
pub use ry_core::{Confidence, Severity};

/// Determine the default confidence level for a rule code.
/// This is checker-specific and cannot live on the ry-core enum.
pub fn default_confidence_for(code: &str) -> Confidence {
    match code {
        "RY097" => Confidence::Low,
        "RY030" | "RY033" | "RY050" | "RY070" | "RY092" | "RY093" | "RY094" | "RY096" | "RY101"
        | "RY102" | "RY105" | "RY107" | "RY108" => Confidence::High,
        _ => Confidence::Medium,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub path: String,
    pub code: &'static str,
    pub message: String,
    pub confidence: Confidence,
}

impl ry_core::BaselineDiagnostic for Diagnostic {
    fn path(&self) -> &str {
        &self.path
    }
    fn code(&self) -> &str {
        self.code
    }
    fn message(&self) -> &str {
        &self.message
    }
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        span: Span,
        path: &str,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            span,
            path: path.to_string(),
            code,
            message: message.into(),
            confidence: if severity == Severity::Info {
                Confidence::Low
            } else {
                default_confidence_for(code)
            },
        }
    }

    /// Look up the rule metadata for this diagnostic's code, if any.
    pub fn rule(&self) -> Option<&'static rules::Rule> {
        rules::find(self.code)
    }
}

// ============================================================================
// Inline suppression comments (`# ry: ignore`, `# noqa`)
// ============================================================================
//
// Users can suppress false-positive diagnostics inline, mirroring the
// `# ruff: ignore` / `# noqa` conventions from the Python ecosystem:
//
//     x <- bad  # ry: ignore                 # suppress ALL rules on this line
//     x <- bad  # ry: ignore[RY010]          # suppress a specific rule
//     x <- bad  # ry: ignore[RY010, RY040]   # suppress multiple rules
//     x <- bad  # noqa: RY010                # flake8/ruff-compatible alias
//
//     # ry: ignore                           # standalone: suppresses the
//     x <- bad                               #   next non-comment, non-blank line
//
//     # ry: ignore-file                      # file-level: suppresses everything
//
// The parser is tolerant of whitespace and case, but distinguishes a bare
// ignore from an invalid or foreign-only explicit list. An invalid native
// directive reports RY112 and never hides another diagnostic.

/// A suppression directive parsed from a `# ry: ignore` or `# noqa`
/// comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// Line number (0-indexed) of the code line the suppression applies
    /// to. For trailing comments this is the line they sit on; for
    /// standalone comments this is the next non-comment, non-blank
    /// line.
    pub line: usize,
    /// The comment's source span, retained for directive diagnostics and audits.
    pub span: Span,
    pub origin: SuppressionOrigin,
    pub kind: SuppressionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionOrigin {
    Ry,
    Noqa,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressionKind {
    All,
    Selective(Vec<String>),
    /// A native directive whose spelling or code list cannot be honored.
    Invalid(String),
    /// An explicit `noqa` list containing only other tools' codes.
    Foreign,
}

impl Suppression {
    pub fn suppresses(&self, code: &str) -> bool {
        // Directive audits must be controlled with a severity override, not
        // with a blanket comment that can hide its own mistakes.
        if matches!(code, "RY112" | "RY113") {
            return false;
        }
        match &self.kind {
            SuppressionKind::All => true,
            SuppressionKind::Selective(codes) => codes.iter().any(|rule| rule == code),
            SuppressionKind::Invalid(_) | SuppressionKind::Foreign => false,
        }
    }

    pub fn valid_rules(&self) -> Option<&[String]> {
        match &self.kind {
            SuppressionKind::Selective(codes) => Some(codes),
            _ => None,
        }
    }
}

/// Scan the parser's collected `Comment` list (see
/// `SourceFile::comments`) for `# ry: ignore` / `# noqa` directives and
/// return one [`Suppression`] per directive found. Working from the
/// comment list rather than scanning source lines for `#` means a `#`
/// appearing INSIDE a string literal is not mistaken for a suppression
/// directive (so `x <- "# noqa"` does not suppress anything).
///
/// Standalone-vs-trailing is decided by the comment's column: a comment
/// at column 0 (no code before it on the line) defers to the next code
/// line; a comment at column > 0 applies to its own line.
///
/// Resolving a standalone directive to "the next code line" requires the
/// source text: blank lines and comment-only lines in between must be
/// skipped, which cannot be determined from the comment list alone. Pass
/// the full source so the resolution can find the target line.
pub fn parse_suppressions_from_comments(
    comments: &[ry_core::ast::Comment],
    src: &str,
) -> Vec<Suppression> {
    let src_lines: Vec<&str> = src.lines().collect();
    let mut line_starts = Vec::with_capacity(src_lines.len());
    let mut offset = 0;
    for line in src.split_inclusive('\n') {
        line_starts.push(offset);
        offset += line.len();
    }
    let mut suppressions = Vec::new();
    for c in comments {
        let Some(ParsedDirective::Line(kind, origin)) = parse_ignore_comment_body(&c.body) else {
            continue;
        };
        let span = comment_span(c, src, &line_starts);
        if c.col == 0 || is_whitespace_only_prefix(&src_lines, c) {
            // Standalone: applies to the next non-comment, non-blank
            // line after this comment. If there is no such line (e.g.
            // the directive is the last thing in the file) there is
            // nothing to suppress, so the directive is dropped.
            if let Some(line) = next_code_line(&src_lines, c.line) {
                suppressions.push(Suppression {
                    line,
                    span,
                    origin,
                    kind,
                });
            } else if matches!(kind, SuppressionKind::Invalid(_)) {
                suppressions.push(Suppression {
                    line: c.line,
                    span,
                    origin,
                    kind,
                });
            }
        } else {
            // Trailing: applies to this line.
            suppressions.push(Suppression {
                line: c.line,
                span,
                origin,
                kind,
            });
        }
    }
    suppressions
}

/// Whether everything on the comment's line before the `#` is
/// whitespace. A comment whose line is entirely whitespace up to the
/// `#` is standalone even when indented (`    # ry: ignore`), as opposed
/// to a trailing comment that follows code (`x <- 1  # ry: ignore`).
fn is_whitespace_only_prefix(src_lines: &[&str], c: &ry_core::ast::Comment) -> bool {
    src_lines
        .get(c.line)
        .and_then(|line| {
            // `c.col` is a BYTE column; only slice when it is within the
            // line's byte length.
            if c.col <= line.len() {
                Some(&line[..c.col])
            } else {
                None
            }
        })
        .map(|prefix| prefix.trim().is_empty())
        .unwrap_or(false)
}

/// Find the first line after `start` that is neither blank nor a
/// comment-only line (a line whose first non-whitespace character is
/// `#`). Used to resolve standalone `# ry: ignore` directives to their
/// target code line.
fn next_code_line(lines: &[&str], start: usize) -> Option<usize> {
    let mut line = start + 1;
    while line < lines.len() {
        let trimmed = lines[line].trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            return Some(line);
        }
        line += 1;
    }
    None
}

/// Parse a comment body into a line or file directive. An explicit list
/// with no valid ry codes is distinct from a bare ignore.
///
/// The body is the comment text AFTER the leading `#`; leading
/// whitespace is trimmed here. The marker must START the body, which
/// prevents false matches on prose like `# See docs for ry: ignore` or
/// `# TODO: add ry: ignore`.
///
/// Recognized forms (case-insensitive on the `ry:` / `noqa` markers):
///   - `# ry: ignore[]` (all codes, with optional trailing prose)
///   - `# ry: ignore`
///   - `# ry:ignore`
///   - `# ry: ignore[RY040]`
///   - `# ry: ignore[RY040, RY010]`
///   - `# noqa`
///   - `# noqa: RY040`
///   - `# noqa[RY040]`
#[derive(Debug)]
enum ParsedDirective {
    Line(SuppressionKind, SuppressionOrigin),
    File,
}

fn marker_boundary(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with(['[', ':']) || rest.starts_with(char::is_whitespace)
}

fn parse_ignore_comment_body(body: &str) -> Option<ParsedDirective> {
    let body = body.trim_start();
    let body_lower = body.to_ascii_lowercase();

    // `# ry: ignore[...]` or `# ry:ignore[...]`
    for marker in ["ry: ignore", "ry:ignore"] {
        if let Some(rest) = body_lower.strip_prefix(marker) {
            if let Some(file_rest) = rest.strip_prefix("-file") {
                if file_rest.is_empty() || file_rest.starts_with(char::is_whitespace) {
                    return Some(ParsedDirective::File);
                }
                return None;
            }
            if !marker_boundary(rest) {
                return None;
            }
            let after = &body[marker.len()..];
            return Some(ParsedDirective::Line(
                parse_native_codes(after),
                SuppressionOrigin::Ry,
            ));
        }
    }

    // `# noqa` / `# noqa: RY040` / `# noqa[RY040]`
    if body_lower.strip_prefix("noqa").is_some_and(marker_boundary) {
        let after = &body["noqa".len()..];
        return Some(ParsedDirective::Line(
            parse_noqa_codes(after),
            SuppressionOrigin::Noqa,
        ));
    }

    None
}

/// Parse native rule codes from bracketed or colon lists. A bare marker
/// (and the documented `[]` alias) is the only all-rules state.
fn parse_native_codes(text: &str) -> SuppressionKind {
    let text = text.trim();
    if text.is_empty() {
        return SuppressionKind::All;
    }
    let list = if let Some(after) = text.strip_prefix('[') {
        let Some(close) = after.find(']') else {
            return SuppressionKind::Invalid("missing `]` in ignore list".into());
        };
        if !bracket_suffix_is_prose(&after[close + 1..]) {
            return SuppressionKind::Invalid("malformed suffix after ignore list".into());
        }
        &after[..close]
    } else if let Some(after) = text.strip_prefix(':') {
        after.trim()
    } else {
        // Historic bare ignores may carry prose. A code-like first word
        // indicates an intended selective list, including a misspelling.
        let first_group = text.split_whitespace().next().unwrap_or("");
        let first = first_group.split(',').next().unwrap_or("");
        if !looks_like_rule_code(first) {
            return SuppressionKind::All;
        }
        first_group
    };
    if list.trim().is_empty() {
        return if text.starts_with('[') {
            SuppressionKind::All
        } else {
            SuppressionKind::Invalid("empty ignore list".into())
        };
    }
    if list.split(',').any(|part| part.trim().is_empty()) {
        return SuppressionKind::Invalid("empty entry in ignore list".into());
    }
    let mut codes = Vec::new();
    for token in list
        .split([',', ' ', '\t'])
        .filter(|token| !token.is_empty())
    {
        let code = token.to_ascii_uppercase();
        if rules::find(&code).is_none_or(|rule| rule.code != code) {
            return SuppressionKind::Invalid(format!("unknown ry rule code `{token}`"));
        }
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    SuppressionKind::Selective(codes)
}

fn parse_noqa_codes(text: &str) -> SuppressionKind {
    let text = text.trim();
    if text.is_empty() {
        return SuppressionKind::All;
    }
    let list = if let Some(after) = text.strip_prefix('[') {
        let Some(close) = after.find(']') else {
            return SuppressionKind::Foreign;
        };
        if !bracket_suffix_is_prose(&after[close + 1..]) {
            return SuppressionKind::Foreign;
        }
        &after[..close]
    } else {
        text.strip_prefix(':').unwrap_or(text).trim()
    };
    let codes: Vec<String> = list
        .split([',', ' ', '\t'])
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_uppercase)
        .filter(|code| rules::find(code).is_some_and(|rule| rule.code == code))
        .collect();
    if codes.is_empty() {
        SuppressionKind::Foreign
    } else {
        SuppressionKind::Selective(codes)
    }
}

fn looks_like_rule_code(token: &str) -> bool {
    let upper = token.to_ascii_uppercase();
    (upper.starts_with("RY") || upper.starts_with("RX"))
        && upper.bytes().any(|byte| byte.is_ascii_digit())
        && upper.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// A bracketed list may be followed by ordinary explanatory text, separated
/// by whitespace. Another bracket is a second (malformed) list, not prose.
fn bracket_suffix_is_prose(suffix: &str) -> bool {
    suffix.is_empty()
        || (suffix.starts_with(char::is_whitespace) && !suffix.trim_start().starts_with(['[', ']']))
}

fn comment_span(comment: &ry_core::ast::Comment, src: &str, line_starts: &[usize]) -> Span {
    let start = line_starts.get(comment.line).copied().unwrap_or(src.len()) + comment.col;
    Span {
        start,
        end: (start + 1 + comment.body.len()).min(src.len()),
        line: comment.line,
        col: comment.col,
    }
}

/// Invalid native directives are reported at their comments. RY112 is
/// emitted by the checker so CLI and LSP receive the same source finding.
pub fn invalid_suppression_diagnostics(
    comments: &[ry_core::ast::Comment],
    src: &str,
    path: &str,
) -> Vec<Diagnostic> {
    parse_suppressions_from_comments(comments, src)
        .into_iter()
        .filter_map(|directive| {
            let SuppressionKind::Invalid(reason) = directive.kind else {
                return None;
            };
            Some(Diagnostic::new(
                Severity::Warning,
                directive.span,
                path,
                "RY112",
                format!("invalid ry ignore directive: {reason}"),
            ))
        })
        .collect()
}

/// Returns `true` if any collected comment is a file-level suppression
/// directive (`# ry: ignore-file`). When true, every diagnostic in the
/// file should be suppressed. Working from the parser's collected
/// comments avoids mistaking a `#` inside a string literal for a
/// comment.
pub fn has_file_suppression_from_comments(comments: &[ry_core::ast::Comment]) -> bool {
    for c in comments {
        if matches!(
            parse_ignore_comment_body(&c.body),
            Some(ParsedDirective::File)
        ) {
            return true;
        }
    }
    false
}

/// Returns `true` if `diag` is covered by one of the given per-line
/// [`Suppression`] directives.
///
/// A suppression matches when its target line and valid kind cover the
/// diagnostic. Directive audit codes are always exempt.
pub fn is_suppressed(diag: &Diagnostic, suppressions: &[Suppression]) -> bool {
    suppressions
        .iter()
        .any(|s| s.line == diag.span.line && s.suppresses(diag.code))
}

/// Convenience: drop every diagnostic that is suppressed, either by a
/// per-line `# ry: ignore` / `# noqa` directive or by a file-level
/// `# ry: ignore-file`. This is the filter the CLI and LSP call after
/// running the checker. Uses the parser's collected comments so a `#`
/// inside a string literal is not mistaken for a suppression directive.
/// The source text is required to resolve standalone `# ry: ignore`
/// directives to their target code line.
pub fn filter_suppressed_with_comments(
    diags: Vec<Diagnostic>,
    comments: &[ry_core::ast::Comment],
    src: &str,
) -> Vec<Diagnostic> {
    if has_file_suppression_from_comments(comments) {
        return Vec::new();
    }
    let supps = parse_suppressions_from_comments(comments, src);
    diags
        .into_iter()
        .filter(|d| !is_suppressed(d, &supps))
        .collect()
}

/// Severity overrides that a caller (typically the CLI) wants to apply.
/// Matches ty's `--error` / `--warn` / `--ignore` semantics.
///
/// The `expanded_*` fields are private precomputed caches: each `add_*`
/// expands its token (rule name, code, or "all") into the concrete code
/// list once, so `effective` is O(codes) per diagnostic instead of
/// re-examining every token against the rule table on every call.
#[derive(Debug, Clone, Default)]
pub struct SeverityFilter {
    expanded_errors: Vec<&'static str>,
    expanded_warns: Vec<&'static str>,
    expanded_ignores: Vec<&'static str>,
    selected: Option<Vec<&'static str>>,
    extended_selection: Vec<&'static str>,
}

impl SeverityFilter {
    /// Resolve a user-provided token (rule code, rule name, or "all")
    /// into the list of matching codes.
    fn expand(token: &str) -> Vec<&'static str> {
        if token == "all" {
            return rules::all_codes();
        }
        match rules::find(token) {
            Some(r) => vec![r.code],
            None => Vec::new(),
        }
    }

    /// Add a token (code / name / "all") to one of the buckets,
    /// pre-expanding it into the cached code list.
    pub fn add_error(&mut self, token: &str) {
        self.expanded_errors.extend(Self::expand(token));
    }
    pub fn add_warn(&mut self, token: &str) {
        self.expanded_warns.extend(Self::expand(token));
    }
    pub fn add_ignore(&mut self, token: &str) {
        self.expanded_ignores.extend(Self::expand(token));
    }
    /// Replace the default-enabled set with an explicit selection.
    /// Calling this with no subsequent tokens intentionally selects no rules.
    pub fn begin_selection(&mut self) {
        self.selected.get_or_insert_with(Vec::new);
    }
    pub fn add_select(&mut self, token: &str) {
        self.begin_selection();
        self.selected
            .as_mut()
            .expect("selection initialized above")
            .extend(Self::expand(token));
    }
    /// Enable a rule in addition to the default or explicit selection.
    pub fn add_extend_select(&mut self, token: &str) {
        self.extended_selection.extend(Self::expand(token));
    }

    /// Returns the effective severity for a code, or None to suppress it.
    /// Precedence (highest to lowest): ignore > error > warn > default.
    pub fn effective(&self, code: &str, default: Severity) -> Option<Severity> {
        if self.expanded_ignores.contains(&code) {
            return None;
        }
        if self.expanded_errors.contains(&code) {
            return Some(Severity::Error);
        }
        if self.expanded_warns.contains(&code) {
            return Some(Severity::Warning);
        }
        let selected = self.selected.as_ref().map_or_else(
            || rules::enabled_by_default(code),
            |selected| selected.contains(&code),
        ) || self.extended_selection.contains(&code);
        selected.then_some(default)
    }
}

/// Apply a [`SeverityFilter`] to a vec of diagnostics in place:
/// re-severity each according to the filter, and drop the ones whose
/// effective severity is `None` (ignored).
pub fn apply_filter_to_diagnostics(diagnostics: &mut Vec<Diagnostic>, filter: &SeverityFilter) {
    let mut out: Vec<Diagnostic> = Vec::with_capacity(diagnostics.len());
    for d in diagnostics.drain(..) {
        let default = d
            .rule()
            .map(|r| r.default_severity)
            .unwrap_or(Severity::Warning);
        if let Some(sev) = filter.effective(d.code, default) {
            let mut d = d;
            // Severity overrides do not change the evidence supporting a
            // diagnostic. Preserve instance-specific confidence so
            // --min-confidence behaves the same with or without an override.
            d.severity = sev;
            out.push(d);
        }
    }
    *diagnostics = out;
}
