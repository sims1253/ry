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
        | "RY102" | "RY105" => Confidence::High,
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
// The parser is deliberately tolerant of whitespace and case so
// `#RY:ignore[ry010]`, `# ry:ignore`, etc. all work. Rule codes are
// always uppercased `RYxxx` tokens; anything not starting with `RY` is
// dropped (so prose like `# ry: ignore this mess` suppresses all rules,
// matching ruff's "bare ignore" behavior).

/// A suppression directive parsed from a `# ry: ignore` or `# noqa`
/// comment.
#[derive(Debug, Clone)]
pub struct Suppression {
    /// Line number (0-indexed) of the code line the suppression applies
    /// to. For trailing comments this is the line they sit on; for
    /// standalone comments this is the next non-comment, non-blank
    /// line.
    pub line: usize,
    /// Rule codes to suppress. An empty vec means "suppress all rules".
    pub rules: Vec<String>,
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
    let mut suppressions = Vec::new();
    for c in comments {
        let Some(codes) = parse_ignore_comment_body(&c.body) else {
            continue;
        };
        if c.col == 0 || is_whitespace_only_prefix(&src_lines, c) {
            // Standalone: applies to the next non-comment, non-blank
            // line after this comment. If there is no such line (e.g.
            // the directive is the last thing in the file) there is
            // nothing to suppress, so the directive is dropped.
            if let Some(line) = next_code_line(&src_lines, c.line) {
                suppressions.push(Suppression { line, rules: codes });
            }
        } else {
            // Trailing: applies to this line.
            suppressions.push(Suppression {
                line: c.line,
                rules: codes,
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

/// Parse a comment body for an ignore directive. Returns `Some(codes)`
/// (empty vec = suppress all) when the body contains a recognized
/// directive, or `None` otherwise.
///
/// The body is the comment text AFTER the leading `#`; leading
/// whitespace is trimmed here. The marker must START the body, which
/// prevents false matches on prose like `# See docs for ry: ignore` or
/// `# TODO: add ry: ignore`.
///
/// Recognized forms (case-insensitive on the `ry:` / `noqa` markers):
///   - `# ry: ignore`
///   - `# ry:ignore`
///   - `# ry: ignore[RY040]`
///   - `# ry: ignore[RY040, RY010]`
///   - `# noqa`
///   - `# noqa: RY040`
///   - `# noqa[RY040]`
fn parse_ignore_comment_body(body: &str) -> Option<Vec<String>> {
    let body = body.trim_start();
    let body_lower = body.to_lowercase();

    // `# ry: ignore[...]` or `# ry:ignore[...]`
    for marker in ["ry: ignore", "ry:ignore"] {
        if let Some(rest) = body_lower.strip_prefix(marker) {
            // `ry: ignore-file` is a file-level directive, not a
            // line-level one; skip it here.
            if rest.starts_with("-file") {
                continue;
            }
            let after = &body[marker.len()..];
            return Some(parse_rule_codes(after));
        }
    }

    // `# noqa` / `# noqa: RY040` / `# noqa[RY040]`
    if body_lower.starts_with("noqa") {
        let after = &body["noqa".len()..];
        return Some(parse_rule_codes(after));
    }

    None
}

/// Parse rule codes from text like `[RY040]`, `[RY040, RY010]`,
/// `: RY040`, or empty. Returns an empty vec when no codes are found
/// (which means "suppress all"). Codes are uppercased so that
/// `ry010` and `RY010` are treated identically.
fn parse_rule_codes(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    // Stop at the closing `]`: anything after it is prose, not a code
    // (so `# ry: ignore[RY040] note` yields `RY040`, not a bogus
    // `RY040]` token that never matches).
    let text = match text.find(']') {
        Some(pos) => &text[..pos],
        None => text,
    };
    // Strip a single layer of surrounding brackets / leading colon.
    let text = text.trim_start_matches(['[', ':', ' ']);
    let text = text.trim_end();
    text.split([',', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_uppercase())
        .filter(|s| s.starts_with("RY"))
        .collect()
}

/// Returns `true` if any collected comment is a file-level suppression
/// directive (`# ry: ignore-file`). When true, every diagnostic in the
/// file should be suppressed. Working from the parser's collected
/// comments avoids mistaking a `#` inside a string literal for a
/// comment.
pub fn has_file_suppression_from_comments(comments: &[ry_core::ast::Comment]) -> bool {
    for c in comments {
        let body = c.body.trim_start();
        let lower = body.to_lowercase();
        if lower.starts_with("ry: ignore-file") || lower.starts_with("ry:ignore-file") {
            return true;
        }
    }
    false
}

/// Returns `true` if `diag` is covered by one of the given per-line
/// [`Suppression`] directives.
///
/// A suppression matches when:
///   - its `line` equals the diagnostic's line, AND
///   - its `rules` list is empty (suppress all) OR contains the
///     diagnostic's code.
pub fn is_suppressed(diag: &Diagnostic, suppressions: &[Suppression]) -> bool {
    suppressions.iter().any(|s| {
        s.line == diag.span.line && (s.rules.is_empty() || s.rules.iter().any(|r| r == diag.code))
    })
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
