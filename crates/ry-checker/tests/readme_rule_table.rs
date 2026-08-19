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

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ry_checker::rules::RULES;

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

    let table: Vec<(usize, &str)> = lines[heading + 1..section_end]
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('|'))
        .map(|(i, l)| (heading + 2 + i, *l))
        .collect();

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
    // Only checked when membership matches, so a missing/unknown row does
    // not also produce a noisy order diff on top of its own message.
    let readme_order: Vec<&str> = rows.iter().map(|r| r.code.as_str()).collect();
    let registry_order: Vec<&str> = RULES.iter().map(|r| r.code).collect();
    let same_membership =
        rows.len() == RULES.len() && rows.iter().all(|r| RULES.iter().any(|g| g.code == r.code));
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
