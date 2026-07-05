//! Diagnostic output formatters. Matches ty's `--output-format` choices.
//!
//! - `full`: multi-line with source context (deferred; we currently emit
//!   a single line per diagnostic, like `concise`, since the source
//!   snippet plumbing isn't wired yet).
//! - `concise`: `path:line:col: severity: [CODE] message`, one per line.
//! - `json`: a single JSON array of diagnostic objects.
//! - `github`: GitHub Actions workflow-command annotations.
//! - `gitlab`: GitLab Code Quality JSON report.
//! - `junit`: JUnit XML report for CI test aggregation.

use crate::{Diagnostic, Severity};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Full,
    Concise,
    Json,
    Github,
    Gitlab,
    Junit,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(Self::Full),
            "concise" => Some(Self::Concise),
            "json" => Some(Self::Json),
            "github" => Some(Self::Github),
            "gitlab" => Some(Self::Gitlab),
            "junit" => Some(Self::Junit),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonDiagnostic<'a> {
    code: &'a str,
    severity: &'a str,
    message: &'a str,
    path: &'a str,
    line: usize,
    column: usize,
}

/// Render the diagnostics to a string. `srcs` maps `path` -> source text
/// so we can compute line numbers and (eventually) source snippets.
pub fn render(
    diags: &[Diagnostic],
    format: OutputFormat,
    srcs: &std::collections::HashMap<String, String>,
) -> String {
    match format {
        OutputFormat::Concise => {
            let mut out = String::new();
            for d in diags {
                let (line, col) = line_col(d, srcs);
                use std::fmt::Write as _;
                let _ = writeln!(
                    out,
                    "{}:{}:{}: {}: [{}] {}",
                    d.path, line, col, d.severity, d.code, d.message
                );
            }
            out
        }
        OutputFormat::Full => {
            // Like `concise` but adds the source line and a caret under
            // the span. Falls back to the concise form
            // when the source text isn't available.
            let mut out = String::new();
            for d in diags {
                let (line, col) = line_col(d, srcs);
                use std::fmt::Write as _;
                let _ = writeln!(
                    out,
                    "{}:{}:{}: {}: [{}] {}",
                    d.path, line, col, d.severity, d.code, d.message
                );
                if let Some(src_line) = srcs
                    .get(&d.path)
                    .and_then(|src| line_containing(src, d.span.start))
                {
                    // The source line, then a caret line underlining the
                    // WHOLE span (`^~~~~`), not just a single `^`. Indent
                    // to the (1-based) char column; the underline width is
                    // the span's char width on this line (clamped to at
                    // least 1 and to the remainder of the line; multi-line
                    // spans underline to the line's end).
                    let _ = writeln!(out, "  {}", src_line);
                    let underline_width = span_char_width(d, src_line);
                    let tilde = "~".repeat(underline_width.saturating_sub(1));
                    let _ = writeln!(out, "  {}^{}", " ".repeat(col.saturating_sub(1)), tilde);
                }
            }
            out
        }
        OutputFormat::Json => {
            let items: Vec<JsonDiagnostic<'_>> = diags
                .iter()
                .map(|d| {
                    let (line, col) = line_col(d, srcs);
                    JsonDiagnostic {
                        code: d.code,
                        severity: d.severity.as_str(),
                        message: &d.message,
                        path: &d.path,
                        line,
                        column: col,
                    }
                })
                .collect();
            serde_json::to_string_pretty(&items)
                .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
        }
        OutputFormat::Github => {
            let mut out = String::new();
            for d in diags {
                let level = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "notice",
                };
                out.push_str(&format!(
                    "::{} file={},line={},col={}::{}: {}\n",
                    level,
                    d.path,
                    d.span.line + 1,
                    char_col_for(d, srcs),
                    d.code,
                    d.message
                ));
            }
            out
        }
        OutputFormat::Gitlab => {
            let entries: Vec<serde_json::Value> = diags
                .iter()
                .map(|d| {
                    let severity = match d.severity {
                        Severity::Error => "major",
                        Severity::Warning => "minor",
                        Severity::Info => "info",
                    };
                    serde_json::json!({
                        "description": d.message,
                        "check_name": d.code,
                        "fingerprint": format!("{}:{}:{}", d.path, d.span.start, d.code),
                        "severity": severity,
                        "location": {
                            "path": d.path,
                            "lines": { "begin": d.span.line + 1 }
                        }
                    })
                })
                .collect();
            serde_json::to_string_pretty(&entries).unwrap_or_default() + "\n"
        }
        OutputFormat::Junit => {
            let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
            let errors = diags
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .count();
            let warnings = diags
                .iter()
                .filter(|d| d.severity == Severity::Warning)
                .count();
            out.push_str("<testsuites name=\"ry\">\n");
            out.push_str(&format!(
                "  <testsuite name=\"ry-check\" errors=\"{}\" failures=\"{}\" tests=\"{}\">\n",
                errors,
                warnings,
                diags.len()
            ));
            for d in diags {
                let tag = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "failure",
                    Severity::Info => "system-out",
                };
                out.push_str(&format!(
                    "    <testcase name=\"{}: {}:{}\" classname=\"{}\">\n",
                    d.code,
                    d.path,
                    d.span.line + 1,
                    d.path
                ));
                // XML-escape the message.
                let escaped = d
                    .message
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;");
                out.push_str(&format!(
                    "      <{} message=\"{}\" type=\"{}\">\n",
                    tag, escaped, d.code
                ));
                out.push_str(&format!(
                    "        {}:{}:{}: {}: {}\n      </{}>\n    </testcase>\n",
                    d.path,
                    d.span.line + 1,
                    char_col_for(d, srcs),
                    d.code,
                    escaped,
                    tag
                ));
            }
            out.push_str("  </testsuite>\n</testsuites>\n");
            out
        }
    }
}

fn line_col(d: &Diagnostic, srcs: &std::collections::HashMap<String, String>) -> (usize, usize) {
    let line = srcs
        .get(&d.path)
        .and_then(|src| src.get(..d.span.start.min(src.len())))
        .map(|prefix| prefix.matches('\n').count() + 1)
        .unwrap_or_else(|| d.span.line + 1);
    let col = char_col_for(d, srcs);
    (line, col)
}

/// The char width of a diagnostic's span on its source line (at least
/// 1). Used to underline the whole span (`^~~~~`) rather than a single
/// caret. `d.span.end - d.span.start` is the byte width; we clamp to the
/// remainder of the line (multi-line spans underline to the line's end)
/// and convert bytes to chars via the line text.
fn span_char_width(d: &Diagnostic, src_line: &str) -> usize {
    // `d.span.col` is the byte column of the span start within the line.
    // The span's byte width is `end - start`; clamp to the line's end
    // (multi-line spans underline to the line's end) and convert to a
    // char count via the line slice. At least 1 so a zero-width span
    // still shows a single `^`.
    let start_byte = d.span.col.min(src_line.len());
    let raw_end = d.span.end.saturating_sub(d.span.start);
    let end_byte = (start_byte + raw_end).min(src_line.len());
    let width = if end_byte > start_byte {
        src_line[start_byte..end_byte].chars().count()
    } else {
        0
    };
    width.max(1)
}

/// Render a human-visible (1-based) character column for a diagnostic.
/// `Span::col` is a BYTE column; non-ASCII lines would otherwise show the
/// wrong column. When the source line is available, convert via
/// `byte_col_to_char_col`; otherwise fall back to the raw byte column
///.
fn char_col_for(d: &Diagnostic, srcs: &std::collections::HashMap<String, String>) -> usize {
    match srcs
        .get(&d.path)
        .and_then(|src| line_containing(src, d.span.start))
    {
        Some(line) => ry_core::parser::byte_col_to_char_col(line, d.span.col) + 1,
        None => d.span.col + 1,
    }
}

/// Borrow the single line of `src` that contains byte offset `pos`, as a
/// `&str` slice of the original source (no allocation).
fn line_containing(src: &str, pos: usize) -> Option<&str> {
    let bounded = pos.min(src.len());
    // Start of the line: byte after the preceding '\n' (or 0).
    let start = src[..bounded].rfind('\n').map(|i| i + 1).unwrap_or(0);
    // End of the line: the next '\n' (or end of source).
    let end = src[bounded..]
        .find('\n')
        .map(|i| bounded + i)
        .unwrap_or(src.len());
    src.get(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::Span;

    fn diag(code: &'static str, sev: Severity) -> Diagnostic {
        Diagnostic::new(sev, Span::new(0, 1, 0, 0), "x.R", code, "msg")
    }

    #[test]
    fn concise_format() {
        let d = vec![diag("RY040", Severity::Error)];
        let mut srcs = std::collections::HashMap::new();
        srcs.insert("x.R".to_string(), "y\n".to_string());
        let out = render(&d, OutputFormat::Concise, &srcs);
        assert!(out.contains("x.R:1:1: error: [RY040] msg"));
    }

    #[test]
    fn full_format_shows_source_line_and_caret() {
        // `full` = concise line + the source line + a
        // caret under the span's column. Span points at the second char
        // (byte col 1) of `ab`.
        let d = vec![Diagnostic::new(
            Severity::Error,
            Span::new(1, 2, 0, 1),
            "x.R",
            "RY040",
            "msg",
        )];
        let mut srcs = std::collections::HashMap::new();
        srcs.insert("x.R".to_string(), "ab\n".to_string());
        let out = render(&d, OutputFormat::Full, &srcs);
        assert!(
            out.contains("x.R:1:2: error: [RY040] msg"),
            "header line: {out}"
        );
        assert!(out.contains("  ab"), "source line: {out}");
        // Caret indented to column 2 (one space, then ^).
        assert!(out.contains("   ^"), "caret under col 2: {out}");
    }

    #[test]
    fn full_format_falls_back_when_source_absent() {
        // No srcs entry: behave like concise (no source/caret lines).
        let d = vec![diag("RY040", Severity::Error)];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Full, &srcs);
        assert!(out.contains("x.R:1:1: error: [RY040] msg"));
        assert!(!out.contains("  ^"), "no caret without source: {out}");
    }

    #[test]
    fn json_format_parses() {
        let d = vec![diag("RY001", Severity::Warning)];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Json, &srcs);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["code"], "RY001");
        assert_eq!(parsed[0]["severity"], "warning");
    }

    #[test]
    fn github_format_emits_workflow_command() {
        let d = vec![diag("RY040", Severity::Error)];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Github, &srcs);
        // Spans are 0-indexed; GitHub expects 1-indexed, so line/col are +1.
        assert!(out.contains("::error file=x.R,line=1,col=1::RY040: msg"));
    }

    #[test]
    fn github_format_severity_levels() {
        let d = vec![
            diag("RY040", Severity::Error),
            diag("RY010", Severity::Warning),
            diag("RY001", Severity::Info),
        ];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Github, &srcs);
        assert!(out.contains("::error file="));
        assert!(out.contains("::warning file="));
        assert!(out.contains("::notice file="));
    }

    #[test]
    fn gitlab_format_is_valid_json_array() {
        let d = vec![diag("RY040", Severity::Error)];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Gitlab, &srcs);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["description"], "msg");
        assert_eq!(parsed[0]["check_name"], "RY040");
        assert_eq!(parsed[0]["fingerprint"], "x.R:0:RY040");
        assert_eq!(parsed[0]["severity"], "major");
        assert_eq!(parsed[0]["location"]["path"], "x.R");
        assert_eq!(parsed[0]["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn gitlab_format_severity_mapping() {
        let d = vec![
            diag("RY040", Severity::Error),
            diag("RY010", Severity::Warning),
            diag("RY001", Severity::Info),
        ];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Gitlab, &srcs);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["severity"], "major");
        assert_eq!(parsed[1]["severity"], "minor");
        assert_eq!(parsed[2]["severity"], "info");
    }

    #[test]
    fn junit_format_is_valid_xml_envelope() {
        let d = vec![diag("RY040", Severity::Error)];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Junit, &srcs);
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
        assert!(out.contains("<testsuites name=\"ry\">"));
        assert!(
            out.contains("<testsuite name=\"ry-check\" errors=\"1\" failures=\"0\" tests=\"1\">")
        );
        assert!(out.contains("<testcase name=\"RY040: x.R:1\" classname=\"x.R\">"));
        assert!(out.contains("<error message=\"msg\" type=\"RY040\">"));
        assert!(out.contains("x.R:1:1: RY040: msg"));
        assert!(out.ends_with("</testsuites>\n"));
    }

    #[test]
    fn junit_format_escapes_xml_special_chars() {
        let d = vec![Diagnostic::new(
            Severity::Error,
            Span::new(0, 1, 0, 0),
            "x.R",
            "RY040",
            "a < b & c > \"d\"",
        )];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Junit, &srcs);
        // The message= attribute must have special chars escaped.
        assert!(out.contains("message=\"a &lt; b &amp; c &gt; &quot;d&quot;\""));
        // The raw message must NOT leak into the output unescaped.
        assert!(!out.contains("a < b & c > \"d\""));
    }

    #[test]
    fn junit_format_counts_warnings_as_failures() {
        let d = vec![
            diag("RY040", Severity::Error),
            diag("RY010", Severity::Warning),
        ];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Junit, &srcs);
        assert!(out.contains("errors=\"1\" failures=\"1\" tests=\"2\""));
        assert!(out.contains("<error "));
        assert!(out.contains("<failure "));
    }

    #[test]
    fn parse_accepts_ci_formats() {
        assert_eq!(OutputFormat::parse("github"), Some(OutputFormat::Github));
        assert_eq!(OutputFormat::parse("gitlab"), Some(OutputFormat::Gitlab));
        assert_eq!(OutputFormat::parse("junit"), Some(OutputFormat::Junit));
    }

    #[test]
    fn parse_rejects_unknown() {
        assert!(OutputFormat::parse("xml").is_none());
    }

    #[test]
    fn char_column_on_non_ascii_line() {
        // `Span::col` is a BYTE column. On a non-ASCII line
        // the printed column must be the CHARACTER column, not the byte
        // column. `café_x`: bytes c(0)a(1)f(2)é(3,4)_(5)x(6); the `_` is
        // at byte col 5 but char col 4, so the 1-indexed output is 5
        // (the raw byte+1 would wrongly print 6).
        let src = "café_x\n".to_string();
        // Span pointing at `_`: start=5, col=5 (byte col).
        let d = vec![Diagnostic::new(
            Severity::Error,
            Span::new(5, 6, 0, 5),
            "x.R",
            "RY040",
            "msg",
        )];
        let mut srcs = std::collections::HashMap::new();
        srcs.insert("x.R".to_string(), src);
        let out = render(&d, OutputFormat::Concise, &srcs);
        assert!(
            out.contains("x.R:1:5: error: [RY040] msg"),
            "expected char column 5, got: {out}"
        );
        assert!(
            !out.contains("x.R:1:6:"),
            "byte column leaked into output: {out}"
        );
        // Github format uses the same conversion.
        let gh = render(&d, OutputFormat::Github, &srcs);
        assert!(gh.contains("col=5::"), "github col should be 5, got: {gh}");
    }

    #[test]
    fn char_column_falls_back_when_source_absent() {
        // No srcs entry: fall back to the raw byte column (+1) rather
        // than panicking, so the renderer degrades gracefully.
        let d = vec![Diagnostic::new(
            Severity::Error,
            Span::new(5, 6, 0, 5),
            "x.R",
            "RY040",
            "msg",
        )];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Concise, &srcs);
        assert!(
            out.contains("x.R:1:6:"),
            "fallback should be byte col+1: {out}"
        );
    }
}
