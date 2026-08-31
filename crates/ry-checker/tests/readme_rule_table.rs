//! #107: keep the README rule table honest against the rule registry.
//!
//! The "## Rules" table in README.md is hand-maintained and has drifted
//! before (#83: RY003, RY102, RY103, and RY105 disappeared from the table
//! while all four stayed active). This test parses the table out of README.md
//! and asserts exact parity with [`ry_checker::rules::RULES`] on code, name,
//! and severity, so a registry change without a matching README edit (or vice
//! versa) fails `cargo test --workspace` — the check rides the default PR
//! workflow with no CI wiring of its own.
//!
//! Summaries are deliberately not compared verbatim: the table lightly edits
//! several registry summaries for presentation (RY033, RY093, RY097, RY100,
//! and RY101 reword theirs; RY031/RY032 escape `|` as `&#124;` and `>` as
//! `\>`). The stable contract is code, name, and severity, plus a non-empty
//! summary cell so a truncated row cannot pass.
//!
//! The same guard covers the curated rule table in the "## Default profile
//! policy" section of docs/editor-defaults.md. That table drifted too
//! (RY020, RY030, RY040, and RY090 carried wrong names, and RY032 was
//! documented as a disabled "test fixture" rule while the registry ships it
//! enabled). Unlike the README table, it lists a curated subset of rules, so
//! the test asserts row correctness — code, name, severity, and
//! enabled-by-default — against the registry, not full parity.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ry_checker::rules::{RULES, enabled_by_default};

const EXPECTED_COLUMNS: [&str; 4] = ["code", "name", "severity", "summary"];

/// One data row of the README "## Rules" table.
struct Row {
    code: String,
    name: String,
    severity: String,
    summary: String,
    /// 1-based README.md line, for actionable failure messages.
    line: usize,
}

/// Split a table line `| a | b |` into trimmed cells.
fn cells(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix('|').unwrap_or(trimmed);
    trimmed.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_alignment_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let dashes = c.trim_matches(':');
            !dashes.is_empty() && dashes.chars().all(|ch| ch == '-')
        })
}

/// Parse the rule table out of the "## Rules" section of README.md.
fn rule_table_rows() -> Vec<Row> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md");
    let readme =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let lines: Vec<&str> = readme.lines().collect();

    let heading = lines
        .iter()
        .position(|l| l.trim() == "## Rules")
        .unwrap_or_else(|| panic!("README.md has no '## Rules' section"));

    let section_end = lines[heading + 1..]
        .iter()
        .position(|l| l.starts_with("## "))
        .map(|i| heading + 1 + i)
        .unwrap_or(lines.len());

    // The rule table is the first contiguous run of `|`-prefixed lines in
    // the section: a second table later in the same section (examples,
    // tier tables, …) is legal prose and must not be parsed as rule rows,
    // so collection stops at the table's first non-table line.
    let mut table: Vec<(usize, &str)> = Vec::new();
    for (offset, line) in lines[heading + 1..section_end].iter().enumerate() {
        if line.starts_with('|') {
            table.push((heading + 2 + offset, line));
        } else if !table.is_empty() {
            break; // the first contiguous table has ended
        }
    }

    assert!(
        table.len() >= 3,
        "README '## Rules' section has no rule table (expected header, alignment, and data rows)"
    );

    let header = cells(table[0].1);
    assert_eq!(
        header, EXPECTED_COLUMNS,
        "unexpected column header for the README rule table"
    );
    assert!(
        is_alignment_row(&cells(table[1].1)),
        "expected an alignment row right after the README rule table header"
    );

    table[2..]
        .iter()
        .map(|(line, raw)| {
            let cells = cells(raw);
            assert_eq!(
                cells.len(),
                4,
                "README rule table row at line {line} has {} cells (code, name, severity, summary expected); an unescaped '|' in a cell splits the row",
                cells.len()
            );
            Row {
                code: cells[0].clone(),
                name: cells[1].clone(),
                severity: cells[2].clone(),
                summary: cells[3].clone(),
                line: *line,
            }
        })
        .collect()
}

#[test]
fn readme_rule_table_matches_rule_registry() {
    let rows = rule_table_rows();
    let mut problems: Vec<String> = Vec::new();

    // Registry -> README: every registered rule needs a matching row.
    for rule in RULES {
        let Some(row) = rows.iter().find(|r| r.code == rule.code) else {
            problems.push(format!(
                "missing row: {} ({}) is registered in ry_checker::rules::RULES \
                 but the README '## Rules' table has no row for it; add one",
                rule.code, rule.name
            ));
            continue;
        };
        if row.name != rule.name {
            problems.push(format!(
                "name mismatch: README line {} calls {} `{}` but the registry \
                 calls it `{}`",
                row.line, row.code, row.name, rule.name
            ));
        }
        let severity = rule.default_severity.as_str();
        if row.severity != severity {
            problems.push(format!(
                "severity mismatch: README line {} lists {} as `{}` but its \
                 registry default severity is `{}`",
                row.line, row.code, row.severity, severity
            ));
        }
        if row.summary.is_empty() {
            problems.push(format!(
                "empty summary: README line {} ({}) has no summary text",
                row.line, row.code
            ));
        }
    }

    // README -> registry: no row may document an unregistered rule, and a
    // pasted-duplicate row must be caught even though the registry -> README
    // loop only ever looks at the first row per code.
    let mut seen: HashSet<&str> = HashSet::new();
    for row in &rows {
        if !RULES.iter().any(|r| r.code == row.code) {
            problems.push(format!(
                "unknown row: README line {} documents {} (`{}`) but no rule \
                 with that code is registered; remove the row or fix the code",
                row.line, row.code, row.name
            ));
            continue;
        }
        if !seen.insert(row.code.as_str()) {
            problems.push(format!(
                "duplicate row: README line {} repeats {} ({})",
                row.line, row.code, row.name
            ));
        }
    }

    // The registry keeps codes lexicographic; the table must follow it.
    // Only checked when membership is exact and duplicate-free, so a
    // missing/unknown row does not also produce a noisy order diff on top
    // of its own message — and a duplicate row paired with a missing one
    // (which preserves the row count) cannot slip through either.
    // `seen` holds every distinct *registered* row code (unknown rows are
    // skipped above), so equal row and `seen` lengths mean no duplicate or
    // unknown rows, and `seen` equal to the registry codes means nothing
    // is missing.
    let readme_order: Vec<&str> = rows.iter().map(|r| r.code.as_str()).collect();
    let registry_order: Vec<&str> = RULES.iter().map(|r| r.code).collect();
    let registry_codes: HashSet<&str> = RULES.iter().map(|r| r.code).collect();
    // The registry's own order is part of the contract: if RULES and the
    // table drifted to the same non-lexicographic order together, the
    // comparison below would still pass while the invariant above fails.
    // Strict `<` also rejects duplicate codes in the registry itself.
    if !registry_order.windows(2).all(|pair| pair[0] < pair[1]) {
        problems.push(format!(
            "registry order: rule codes are not lexicographically ordered: \
             {registry_order:?}"
        ));
    }
    let same_membership = rows.len() == seen.len() && seen == registry_codes;
    if same_membership && readme_order != registry_order {
        problems.push(format!(
            "row order: README rows are ordered {readme_order:?} but the \
             registry order is {registry_order:?}; keep the table in registry \
             (lexicographic code) order"
        ));
    }

    assert!(
        problems.is_empty(),
        "README.md rule table (## Rules) is out of sync with \
         ry_checker::rules::RULES ({} problem(s)):\n  - {}",
        problems.len(),
        problems.join("\n  - ")
    );
}

/// Expected columns of the docs/editor-defaults.md rule table.
const EDITOR_DEFAULTS_COLUMNS: [&str; 5] = ["Rule", "Severity", "Default", "Verdict", "Evidence"];

/// One data row of the docs/editor-defaults.md rule table.
struct EditorDefaultsRow {
    code: String,
    name: String,
    severity: String,
    default_enabled: String,
    /// 1-based docs/editor-defaults.md line, for actionable failure messages.
    line: usize,
}

/// Parse the curated rule table out of the "## Default profile policy"
/// section of docs/editor-defaults.md.
///
/// The first cell is `RY010 (unbound-variable)`: the registry code, then the
/// registry name in parentheses. Every row must be a rule row — the table
/// holds no config rows — so a row that does not fit the shape fails here
/// instead of slipping past the registry comparison.
fn editor_defaults_rule_rows() -> Vec<EditorDefaultsRow> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/editor-defaults.md");
    let doc = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let lines: Vec<&str> = doc.lines().collect();

    let heading = lines
        .iter()
        .position(|l| l.trim() == "## Default profile policy")
        .unwrap_or_else(|| {
            panic!("docs/editor-defaults.md has no '## Default profile policy' section")
        });

    let section_end = lines[heading + 1..]
        .iter()
        .position(|l| l.starts_with('#'))
        .map(|i| heading + 1 + i)
        .unwrap_or(lines.len());

    // The rule table is the first contiguous run of `|`-prefixed lines in
    // the section; collection stops at the table's first non-table line.
    let mut table: Vec<(usize, &str)> = Vec::new();
    for (offset, line) in lines[heading + 1..section_end].iter().enumerate() {
        if line.starts_with('|') {
            table.push((heading + 2 + offset, line));
        } else if !table.is_empty() {
            break; // the first contiguous table has ended
        }
    }

    assert!(
        table.len() >= 3,
        "docs/editor-defaults.md '## Default profile policy' section has no rule table \
         (expected header, alignment, and data rows)"
    );

    let header = cells(table[0].1);
    assert_eq!(
        header, EDITOR_DEFAULTS_COLUMNS,
        "unexpected column header for the docs/editor-defaults.md rule table"
    );
    assert!(
        is_alignment_row(&cells(table[1].1)),
        "expected an alignment row right after the docs/editor-defaults.md rule table header"
    );

    table[2..]
        .iter()
        .map(|(line, raw)| {
            let cells = cells(raw);
            assert_eq!(
                cells.len(),
                5,
                "docs/editor-defaults.md rule table row at line {line} has {} cells \
                 (Rule, Severity, Default, Verdict, Evidence expected); an unescaped '|' \
                 in a cell splits the row",
                cells.len()
            );
            let (code, name) = split_rule_cell(&cells[0], *line);
            EditorDefaultsRow {
                code,
                name,
                severity: cells[1].clone(),
                default_enabled: cells[2].clone(),
                line: *line,
            }
        })
        .collect()
}

/// Split a `RY010 (unbound-variable)` rule cell into code and name.
fn split_rule_cell(cell: &str, line: usize) -> (String, String) {
    let (code, rest) = cell.split_once(' ').unwrap_or_else(|| {
        panic!(
            "docs/editor-defaults.md rule table row at line {line}: rule cell `{cell}` is not \
             `CODE (name)`; the table holds only rule rows"
        )
    });
    let name = rest
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or_else(|| {
            panic!(
                "docs/editor-defaults.md rule table row at line {line}: rule cell `{cell}` does \
                 not wrap the rule name in parentheses"
            )
        });
    (code.to_string(), name.to_string())
}

#[test]
fn editor_defaults_rule_table_matches_rule_registry() {
    let rows = editor_defaults_rule_rows();
    let mut problems: Vec<String> = Vec::new();

    // The table is a curated subset, so each row must be checked on its own:
    // an unknown rule, a wrong name or severity, or a wrong enabled-by-default
    // status would otherwise pass unnoticed, and a pasted-duplicate row must
    // be caught too.
    let mut seen: HashSet<&str> = HashSet::new();
    for row in &rows {
        let Some(rule) = RULES.iter().find(|r| r.code == row.code) else {
            problems.push(format!(
                "unknown row: docs/editor-defaults.md line {} documents {} (`{}`) but no rule \
                 with that code is registered; remove the row or fix the code",
                row.line, row.code, row.name
            ));
            continue;
        };
        if row.name != rule.name {
            problems.push(format!(
                "name mismatch: docs/editor-defaults.md line {} calls {} `{}` but the registry \
                 calls it `{}`",
                row.line, row.code, row.name, rule.name
            ));
        }
        let severity = rule.default_severity.as_str();
        if row.severity != severity {
            problems.push(format!(
                "severity mismatch: docs/editor-defaults.md line {} lists {} as `{}` but its \
                 registry default severity is `{}`",
                row.line, row.code, row.severity, severity
            ));
        }
        let default = if enabled_by_default(&row.code) {
            "Enabled"
        } else {
            "Disabled"
        };
        if row.default_enabled != default {
            problems.push(format!(
                "default mismatch: docs/editor-defaults.md line {} lists {} as `{}` but the \
                 registry default is `{}`",
                row.line, row.code, row.default_enabled, default
            ));
        }
        if !seen.insert(row.code.as_str()) {
            problems.push(format!(
                "duplicate row: docs/editor-defaults.md line {} repeats {} ({})",
                row.line, row.code, row.name
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "docs/editor-defaults.md rule table (## Default profile policy) is out of sync with \
         ry_checker::rules::RULES ({} problem(s)):\n  - {}",
        problems.len(),
        problems.join("\n  - ")
    );
}
