//! Generated rule and capability documentation.
//!
//! P39-W7: Rule documentation and capability contracts are generated
//! from authoritative schemas rather than hand-copied.

/// Information about a ry rule.
#[derive(Debug, Clone)]
pub struct RuleInfo {
    pub code: String,
    pub title: String,
    pub description: String,
    pub default_severity: ry_core::Severity,
    pub default_confidence: ry_core::Confidence,
}

/// The complete registry of ry rules.
pub fn all_rules() -> Vec<RuleInfo> {
    vec![
        RuleInfo {
            code: "RY010".to_string(),
            title: "Undefined variable".to_string(),
            description: "A variable is used before it is assigned.".to_string(),
            default_severity: ry_core::Severity::Warning,
            default_confidence: ry_core::Confidence::Medium,
        },
        RuleInfo {
            code: "RY090".to_string(),
            title: "Unknown argument".to_string(),
            description: "An argument name is not in the function signature.".to_string(),
            default_severity: ry_core::Severity::Warning,
            default_confidence: ry_core::Confidence::High,
        },
        RuleInfo {
            code: "RY091".to_string(),
            title: "Missing required argument".to_string(),
            description: "A required argument is not provided.".to_string(),
            default_severity: ry_core::Severity::Warning,
            default_confidence: ry_core::Confidence::High,
        },
        RuleInfo {
            code: "RY097".to_string(),
            title: "Not R source".to_string(),
            description: "File does not appear to be R source.".to_string(),
            default_severity: ry_core::Severity::Info,
            default_confidence: ry_core::Confidence::Low,
        },
    ]
}

/// Generate a Markdown table of all rules.
pub fn rules_markdown_table() -> String {
    let mut out = String::from("| Code | Title | Severity | Confidence |\n");
    out.push_str("|------|-------|----------|------------|\n");
    for rule in all_rules() {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            rule.code,
            rule.title,
            rule.default_severity.as_str(),
            rule.default_confidence.as_str(),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_rules_have_unique_codes() {
        let rules = all_rules();
        let mut codes: Vec<String> = rules.iter().map(|r| r.code.clone()).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), rules.len(), "duplicate rule codes");
    }

    #[test]
    fn markdown_table_has_header() {
        let table = rules_markdown_table();
        assert!(table.starts_with("| Code |"));
        assert!(table.contains("RY010"));
    }
}
