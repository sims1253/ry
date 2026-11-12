//! Every rule's worked example, as `ry explain rule` prints it, runs
//! through `ry check`: the flagged file reports only that rule, and the
//! fixed file reports nothing.

use ry_checker::rules::{RULES, Rule};
use ry_testkit::{FixtureProject, ry};
use serde_json::Value;

/// `code line:column message` for each diagnostic `ry check` reports on
/// the example project.
fn diagnostics(rule: &Rule, example_file: &str) -> Vec<String> {
    let example = rule.example();
    let project = FixtureProject::empty().unwrap();
    for (path, contents) in &example.support {
        project.write_file(path, contents).unwrap();
    }
    project
        .write_file(format!("example.{}", example.extension), example_file)
        .unwrap();
    let output = ry!()
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .check(&project, ".", ["--output-format", "json", "--exit-zero"])
        .unwrap();
    assert!(output.status.success(), "{}: {output:?}", rule.code);
    let json: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{}: {error}: {output:?}", rule.code));
    json.as_array()
        .unwrap()
        .iter()
        .map(|d| {
            format!(
                "{} {}:{} {}",
                d["code"].as_str().unwrap(),
                d["line"],
                d["column"],
                d["message"].as_str().unwrap()
            )
        })
        .collect()
}

#[test]
fn rule_examples_flag_only_their_rule_and_fixes_are_silent() {
    let mut failures = Vec::new();
    for rule in RULES {
        let example = rule.example();
        let flagged = diagnostics(rule, example.flagged);
        if flagged.is_empty() || flagged.iter().any(|d| !d.starts_with(rule.code)) {
            failures.push(format!("{} flagged: {flagged:#?}", rule.code));
        }
        let fixed = diagnostics(rule, example.fixed);
        if !fixed.is_empty() {
            failures.push(format!("{} fixed: {fixed:#?}", rule.code));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
