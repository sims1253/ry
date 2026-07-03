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
        OutputFormat::Full | OutputFormat::Concise => {
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
                    d.span.col + 1,
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
                    d.span.col + 1,
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
    (line, d.span.col + 1)
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
}
