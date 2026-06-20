//! Diagnostic output formatters. Matches ty's `--output-format` choices.
//!
//! - `full`: multi-line with source context (deferred; we currently emit
//!   a single line per diagnostic, like `concise`, since the source
//!   snippet plumbing isn't wired yet).
//! - `concise`: `path:line:col: severity: [CODE] message`, one per line.
//! - `json`: a single JSON array of diagnostic objects.

use crate::Diagnostic;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Full,
    Concise,
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(Self::Full),
            "concise" => Some(Self::Concise),
            "json" => Some(Self::Json),
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
        OutputFormat::Full | OutputFormat::Concise => {
            let mut out = String::new();
            for d in diags {
                let (line, col) = line_col(d, srcs);
                use std::fmt::Write as _;
                let _ = writeln!(
                    out,
                    "{}:{}:{}: {}: [{}] {}",
                    d.path,
                    line,
                    col,
                    d.severity,
                    d.code,
                    d.message
                );
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
            serde_json::to_string_pretty(&items).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
        }
    }
}

fn line_col(d: &Diagnostic, srcs: &std::collections::HashMap<String, String>) -> (usize, usize) {
    let line = srcs
        .get(&d.path)
        .and_then(|src| src.get(..d.span.start.min(src.len())))
        .map(|prefix| prefix.matches('\n').count() + 1)
        .unwrap_or_else(|| d.span.line + 1);
    (line, d.span.col + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;
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
    fn json_format_parses() {
        let d = vec![diag("RY001", Severity::Warning)];
        let srcs = std::collections::HashMap::new();
        let out = render(&d, OutputFormat::Json, &srcs);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["code"], "RY001");
        assert_eq!(parsed[0]["severity"], "warning");
    }

    #[test]
    fn parse_rejects_unknown() {
        assert!(OutputFormat::parse("xml").is_none());
    }
}
