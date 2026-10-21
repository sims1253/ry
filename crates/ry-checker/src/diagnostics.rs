//! Diagnostic data types, severity overrides, and inline-suppression
//! parsing and filtering.

use std::borrow::Cow;

use ry_core::Span;

use crate::rules;

pub use ry_core::{Confidence, Severity};

/// The default confidence for a rule code.
pub(crate) fn default_confidence_for(code: &str) -> Confidence {
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
    /// Secondary locations backing the primary finding, at most
    /// [`MAX_RELATED`]. Never part of `message`, suppression, or baseline keys.
    pub related: Vec<Related>,
}

/// Renderers show at most this many related items per diagnostic.
pub const MAX_RELATED: usize = 3;

/// A secondary location the checker proved, such as the formal that a
/// call failed to supply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    /// Display path, in the same namespace as [`Diagnostic::path`].
    pub path: String,
    pub span: Span,
    pub message: String,
}

impl Related {
    /// The byte range in `source`, or `None` when the span does not fit
    /// this text (stale, truncated, or another file's). Renderers then omit
    /// the location; they never guess or clamp one.
    pub fn range_in(&self, source: &str) -> Option<std::ops::Range<usize>> {
        let (start, end) = (self.span.start, self.span.end);
        (start <= end
            && end <= source.len()
            && source.is_char_boundary(start)
            && source.is_char_boundary(end))
        .then_some(start..end)
    }
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
            related: Vec::new(),
        }
    }

    /// Attach related locations, keeping the first [`MAX_RELATED`].
    pub fn with_related(mut self, mut related: Vec<Related>) -> Self {
        related.truncate(MAX_RELATED);
        self.related = related;
        self
    }

    /// Look up the rule metadata for this diagnostic's code, if any.
    pub fn rule(&self) -> Option<&'static rules::Rule> {
        rules::find(self.code)
    }
}

// Inline suppression comments:
//
//     x <- bad  # ry: ignore                 # every rule on this line
//     x <- bad  # ry: ignore[RY010, RY040]   # the listed rules
//     x <- bad  # noqa: RY010                # flake8/ruff-compatible alias
//
//     # ry: ignore                           # standalone: the next code line
//     x <- bad
//
//     # ry: ignore-file                      # the whole file
//
// An invalid native directive reports RY112 and never hides another
// diagnostic.

/// A suppression directive parsed from a `# ry: ignore` or `# noqa`
/// comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// 0-indexed code line the suppression applies to: the comment's own
    /// line when trailing, the next code line when standalone.
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
        if !rules::suppressible(code) {
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

/// One [`Suppression`] per `# ry: ignore` or `# noqa` directive in the
/// parser's comment list. Reading comments, not source lines, keeps
/// `x <- "# noqa"` from suppressing anything. `src` is needed to find
/// the next code line for a standalone directive.
pub fn parse_suppressions_from_comments(
    comments: &[ry_core::ast::Comment],
    src: &str,
) -> Vec<Suppression> {
    let src_lines: Vec<&str> = src.lines().collect();
    let line_starts = line_starts(src);
    let mut suppressions = Vec::new();
    for c in comments {
        let Some(ParsedDirective::Line(kind, origin)) = parse_ignore_comment_body(&c.body) else {
            continue;
        };
        let standalone = c.col == 0 || is_whitespace_only_prefix(&src_lines, c);
        // A standalone directive applies to the next code line. With no
        // such line there is nothing to suppress, so only an invalid
        // directive stays, for its RY112 report.
        let line = match standalone.then(|| next_code_line(&src_lines, c.line)) {
            None => c.line,
            Some(Some(line)) => line,
            Some(None) if matches!(kind, SuppressionKind::Invalid(_)) => c.line,
            Some(None) => continue,
        };
        suppressions.push(Suppression {
            line,
            span: comment_span(c, src, &line_starts),
            origin,
            kind,
        });
    }
    suppressions
}

/// Whether only whitespace precedes the comment on its line, so an
/// indented comment is standalone too.
fn is_whitespace_only_prefix(src_lines: &[&str], c: &ry_core::ast::Comment) -> bool {
    src_lines
        .get(c.line)
        .and_then(|line| line.get(..c.col))
        .map(|prefix| prefix.trim().is_empty())
        .unwrap_or(false)
}

/// The first line after `start` that is neither blank nor comment-only.
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

/// A comment body parsed into a line or file directive. The marker must
/// start the body, so prose like `# TODO: add ry: ignore` does not match.
/// Markers are case-insensitive. Forms: `ry: ignore`, `ry:ignore`,
/// `ry: ignore[]` (all codes), `ry: ignore[RY040, RY010]`, `noqa`,
/// `noqa: RY040`, and `noqa[RY040]`.
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
    let list: Cow<'_, str> = if let Some(after) = text.strip_prefix('[') {
        let Some(close) = after.find(']') else {
            return SuppressionKind::Invalid("missing `]` in ignore list".into());
        };
        if !bracket_suffix_is_prose(&after[close + 1..]) {
            return SuppressionKind::Invalid("malformed suffix after ignore list".into());
        }
        Cow::Borrowed(&after[..close])
    } else if let Some(after) = text.strip_prefix(':') {
        Cow::Borrowed(after.trim())
    } else {
        // A bare ignore may carry prose. A code-like first word starts a
        // selective list, even when misspelled. Code-like groups and comma
        // separators continue it. The first ordinary word starts prose,
        // which may mention a rule code without adding an entry.
        let first_group = text.split_whitespace().next().unwrap_or("");
        let first = first_group.split(',').next().unwrap_or("");
        if first_group.starts_with(',') {
            return SuppressionKind::Invalid("empty entry in ignore list".into());
        }
        if !looks_like_rule_code(first) {
            return SuppressionKind::All;
        }
        let mut list = String::new();
        let mut comma_pending = false;
        for group in text.split_whitespace() {
            let mut word = group;
            if let Some(after_comma) = word.strip_prefix(',') {
                if list.is_empty() || comma_pending {
                    return SuppressionKind::Invalid("empty entry in ignore list".into());
                }
                list.push(',');
                comma_pending = true;
                word = after_comma;
                if word.is_empty() {
                    continue;
                }
            }
            if !word.split(',').next().is_some_and(looks_like_rule_code) {
                if comma_pending {
                    return SuppressionKind::Invalid("expected rule code after `,`".into());
                }
                break;
            }
            if !list.is_empty() && !comma_pending {
                list.push(' ');
            }
            list.push_str(word);
            comma_pending = word.ends_with(',');
        }
        if comma_pending {
            return SuppressionKind::Invalid("empty entry in ignore list".into());
        }
        Cow::Owned(list)
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

/// Byte offset of each line start, including one past a final newline.
pub(crate) fn line_starts(src: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(src.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
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
                rules::INVALID_IGNORE,
                format!("invalid ry ignore directive: {reason}"),
            ))
        })
        .collect()
}

/// Whether any comment is a `# ry: ignore-file` directive.
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

/// Whether one of the per-line [`Suppression`] directives covers `diag`.
/// Directive audit codes are always exempt.
pub fn is_suppressed(diag: &Diagnostic, suppressions: &[Suppression]) -> bool {
    suppressions
        .iter()
        .any(|s| s.line == diag.span.line && s.suppresses(diag.code))
}

/// Drop every diagnostic suppressed by a per-line directive or by
/// `# ry: ignore-file`.
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

/// Severity overrides with ty's `--error`, `--warn`, and `--ignore`
/// semantics. Each `add_*` expands its token into codes once, in the
/// private `expanded_*` caches.
#[derive(Debug, Clone, Default)]
pub struct SeverityFilter {
    expanded_errors: Vec<&'static str>,
    expanded_warns: Vec<&'static str>,
    expanded_ignores: Vec<&'static str>,
    selected: Option<Vec<&'static str>>,
    extended_selection: Vec<&'static str>,
    scoped: std::collections::HashMap<&'static str, Option<Severity>>,
}

impl SeverityFilter {
    /// Resolve a user-provided token (rule code, rule name, or "all")
    /// into the list of matching codes.
    pub(crate) fn expand(token: &str) -> Vec<&'static str> {
        if token == "all" {
            return rules::all_codes();
        }
        match rules::find(token) {
            Some(r) => vec![r.code],
            None => Vec::new(),
        }
    }

    /// Add a token (code, name, or "all") to one of the buckets.
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

    pub(crate) fn set_scoped(&mut self, code: &'static str, severity: Option<Severity>) {
        self.scoped.insert(code, severity);
    }

    /// Returns the effective severity for a code, or None to suppress it.
    /// Precedence (highest to lowest): global ignore, path-scoped choice,
    /// global error, global warn, default. Scoped choices never include
    /// codes protected by an explicit CLI or editor choice.
    pub fn effective(&self, code: &str, default: Severity) -> Option<Severity> {
        if self.expanded_ignores.contains(&code) {
            return None;
        }
        if let Some(severity) = self.scoped.get(code) {
            return *severity;
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

/// Apply a [`SeverityFilter`] in place, dropping ignored diagnostics.
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
