//! Diagnostic data types, severity overrides, and inline-suppression
//! parsing/filtering.
//!
//! Extracted from `lib.rs` without behavior change.
//! This module is self-contained: it depends only on `ry_core::Span`,
//! `ry_core::ast::Comment`, and the rule registry (`crate::rules`).

use ry_core::Span;

use crate::rules;

// ============================================================================
// Severity + Diagnostic
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub path: String,
    pub code: &'static str,
    pub message: String,
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

/// Scan source text for `# ry: ignore` / `# noqa` comments and return
/// one [`Suppression`] per directive found.
///
/// File-level directives (`# ry: ignore-file`) are NOT included here;
/// use [`has_file_suppression`] to detect those.
pub fn parse_suppressions(src: &str) -> Vec<Suppression> {
    // Legacy path: scan source text line-by-line. Used by callers that
    // don't have a parsed SourceFile handy (e.g. tests). Real callers
    // should prefer parse_suppressions_from_comments, which is lexical
    // (a `#` inside a string literal is NOT mistaken for a comment).
    let mut suppressions = Vec::new();
    // A standalone `# ry: ignore` line defers until the next code line.
    let mut pending: Option<Suppression> = None;

    for (line_num, line) in src.lines().enumerate() {
        let trimmed = line.trim();

        if let Some(codes) = parse_ignore_comment(trimmed) {
            // If the line has code before the comment, it's a trailing
            // suppression (applies to this line). Otherwise it's a
            // standalone comment that applies to the next code line.
            let code_before = line_before_comment(line);
            if code_before.trim().is_empty() {
                pending = Some(Suppression {
                    line: 0, // filled in when we reach the target line
                    rules: codes,
                });
            } else {
                suppressions.push(Suppression {
                    line: line_num,
                    rules: codes,
                });
            }
            continue;
        }

        // Resolve a pending standalone suppression against the next
        // non-blank, non-comment line. Blank lines and further comment
        // lines don't consume the pending suppression.
        if let Some(mut supp) = pending.take() {
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                supp.line = line_num;
                suppressions.push(supp);
            } else {
                pending = Some(supp);
            }
        }
    }

    suppressions
}

/// Lexical variant of `parse_suppressions`: consumes the parser's
/// collected `Comment` list (see `SourceFile::comments`) so that a `#`
/// appearing INSIDE a string literal is not mistaken for a suppression
/// directive (the legacy `parse_suppressions(&str)` scans source lines
/// for `#`, which falsely matches `x <- "# noqa"`).
///
/// Standalone-vs-trailing is decided by the comment's column: a comment
/// at column 0 (no code before it on the line) defers to the next code
/// line; a comment at column > 0 applies to its own line.
pub fn parse_suppressions_from_comments(comments: &[ry_core::ast::Comment]) -> Vec<Suppression> {
    let mut suppressions = Vec::new();
    let mut pending: Option<Suppression> = None;
    for c in comments {
        if let Some(codes) = parse_ignore_comment_body(&c.body) {
            if c.col == 0 {
                // Standalone: applies to the next code line. We don't
                // know which line that is from comments alone, so defer
                // and resolve against the next comment's line minus one
                // (a trailing comment on the target line) or the next
                // comment's line if none.
                pending = Some(Suppression {
                    line: 0,
                    rules: codes,
                });
            } else {
                // Trailing: applies to this line.
                suppressions.push(Suppression {
                    line: c.line,
                    rules: codes,
                });
            }
            continue;
        }
        // A non-directive comment resolves a pending standalone
        // suppression if it sits on a later line (heuristic: it marks a
        // line that has code, since trailing comments follow code).
        if let Some(mut supp) = pending.take() {
            if c.col > 0 && c.line > supp.line {
                supp.line = c.line;
                suppressions.push(supp);
            } else {
                pending = Some(supp);
            }
        }
    }
    if let Some(mut supp) = pending.take() {
        // File ended with an unresolved standalone directive: attach to
        // the line after the last comment as a best effort.
        supp.line = comments.last().map(|c| c.line + 1).unwrap_or(0);
        suppressions.push(supp);
    }
    suppressions
}

/// Parse a single comment line for an ignore directive. Returns
/// `Some(codes)` (empty vec = suppress all) when the line contains a
/// recognized directive, or `None` otherwise.
///
/// Recognized forms (case-insensitive on the `ry:` / `noqa` markers):
///   - `# ry: ignore`
///   - `# ry:ignore`
///   - `# ry: ignore[RY040]`
///   - `# ry: ignore[RY040, RY010]`
///   - `# noqa`
///   - `# noqa: RY040`
///   - `# noqa[RY040]`
fn parse_ignore_comment(line: &str) -> Option<Vec<String>> {
    let comment_start = line.find('#')?;
    let comment = &line[comment_start..];
    // Strip the '#' and whitespace so the marker must be at the START
    // of the comment body. This prevents false matches on prose like
    // `# See docs for ry: ignore` or `# TODO: add ry: ignore`.
    let body = comment[1..].trim_start();
    parse_ignore_comment_body(body)
}

/// Body-only variant: the comment text AFTER the leading `#` (already
/// trimmed of leading whitespace by the caller or here). Shared by the
/// legacy line-scanning parser and the lexical comment-based parser.
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
    // Strip a single layer of surrounding brackets / leading colon.
    let text = text.trim_start_matches(['[', ':', ' ']);
    let text = text.trim_end_matches(']');
    text.split([',', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_uppercase())
        .filter(|s| s.starts_with("RY"))
        .collect()
}

/// Return the portion of a line before its first `#` (the code part).
/// If the line has no comment, the whole line is returned.
fn line_before_comment(line: &str) -> &str {
    match line.find('#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

/// Returns `true` if the source contains a file-level suppression
/// directive (`# ry: ignore-file`). When true, every diagnostic in the
/// file should be suppressed.
pub fn has_file_suppression(src: &str) -> bool {
    // Legacy line-scanning path. The marker must START the comment body
    // (after the `#` and whitespace) -- NOT appear as a substring -- so
    // prose like `# see also ry: ignore-file` does not trigger a
    // file-wide suppression. Prefer has_file_suppression_from_comments
    // for callers with a parsed SourceFile (it also avoids mistaking
    // a `#` inside a string literal for a comment).
    for line in src.lines() {
        if let Some(hash_pos) = line.find('#') {
            let body = line[hash_pos + 1..].trim_start();
            let lower = body.to_lowercase();
            if lower.starts_with("ry: ignore-file") || lower.starts_with("ry:ignore-file") {
                return true;
            }
        }
    }
    false
}

/// Lexical variant of `has_file_suppression` using the parser's
/// collected comments. Avoids the string-literal `#` false positive.
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
/// running the checker.
pub fn filter_suppressed(diags: Vec<Diagnostic>, src: &str) -> Vec<Diagnostic> {
    if has_file_suppression(src) {
        return Vec::new();
    }
    let supps = parse_suppressions(src);
    diags
        .into_iter()
        .filter(|d| !is_suppressed(d, &supps))
        .collect()
}

/// Lexical variant of `filter_suppressed`: uses the parser's collected
/// comments so a `#` inside a string literal is not mistaken for a
/// suppression directive.
pub fn filter_suppressed_with_comments(
    diags: Vec<Diagnostic>,
    comments: &[ry_core::ast::Comment],
) -> Vec<Diagnostic> {
    if has_file_suppression_from_comments(comments) {
        return Vec::new();
    }
    let supps = parse_suppressions_from_comments(comments);
    diags
        .into_iter()
        .filter(|d| !is_suppressed(d, &supps))
        .collect()
}

/// Severity overrides that a caller (typically the CLI) wants to apply.
/// Matches ty's `--error` / `--warn` / `--ignore` semantics.
#[derive(Debug, Clone, Default)]
pub struct SeverityFilter {
    pub errors: Vec<String>,
    pub warns: Vec<String>,
    pub ignores: Vec<String>,
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

    /// Add a token (code / name / "all") to one of the buckets.
    pub fn add_error(&mut self, token: &str) {
        self.errors.push(token.to_string());
    }
    pub fn add_warn(&mut self, token: &str) {
        self.warns.push(token.to_string());
    }
    pub fn add_ignore(&mut self, token: &str) {
        self.ignores.push(token.to_string());
    }

    /// Returns the effective severity for a code, or None to suppress it.
    /// Precedence (highest to lowest): ignore > error > warn > default.
    pub fn effective(&self, code: &str, default: Severity) -> Option<Severity> {
        for tok in &self.ignores {
            if Self::expand(tok).contains(&code) {
                return None;
            }
        }
        for tok in &self.errors {
            if Self::expand(tok).contains(&code) {
                return Some(Severity::Error);
            }
        }
        for tok in &self.warns {
            if Self::expand(tok).contains(&code) {
                return Some(Severity::Warning);
            }
        }
        Some(default)
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
            d.severity = sev;
            out.push(d);
        }
    }
    *diagnostics = out;
}
