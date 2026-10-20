//! RY090/RY091 related locations: an additive JSON field and one `note:`
//! line in full output. Other formats, baselines, and exit codes ignore them.
use ry_testkit::FixtureProject;
use serde_json::{Value, json};
use std::process::{Command, Output};

const SOURCE: &str = "# Größe
helper <- function(lbl = \"𝑥\", scale) nchar(lbl) * scale
use <- function() {
  é <- \"a\"; helper(é)
}
typo <- function(é) helper(lbl = é, sclae = 2)
";

fn ry(files: &FixtureProject, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ry"))
        .arg("check")
        .args(args)
        .arg("uni.R")
        .current_dir(files.root())
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .output()
        .unwrap()
}

fn fixture() -> FixtureProject {
    let files = FixtureProject::empty().unwrap();
    files.write_file("uni.R", SOURCE).unwrap();
    files
}

#[test]
fn json_and_full_output_carry_the_declared_formal() {
    let files = fixture();
    let output = ry(&files, &["--output-format", "json", "--exit-zero"]);
    let diagnostics: Value = serde_json::from_slice(&output.stdout).unwrap();
    let declared =
        |message: &str| json!([{"message": message, "path": "uni.R", "line": 2, "column": 31}]);
    let missing = declared("`scale` is declared here without a default value");
    let related: Vec<_> = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["code"].as_str().unwrap(), d["related"].clone()))
        .collect();
    assert_eq!(
        related,
        [
            ("RY091", missing.clone()),
            ("RY091", missing),
            ("RY090", declared("`scale` is declared here")),
        ]
    );

    let full = ry(
        &files,
        &["--output-format", "full", "--color", "never", "--exit-zero"],
    );
    let notes: Vec<_> = String::from_utf8(full.stdout)
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("  note: "))
        .map(str::to_string)
        .collect();
    assert_eq!(
        notes,
        [
            "  note: uni.R:2:31: `scale` is declared here without a default value",
            "  note: uni.R:2:31: `scale` is declared here without a default value",
            "  note: uni.R:2:31: `scale` is declared here",
        ]
    );
    let concise = ry(&files, &["--output-format", "concise", "--exit-zero"]);
    assert!(
        !String::from_utf8(concise.stdout)
            .unwrap()
            .contains("declared")
    );
}

#[test]
fn baselines_and_exit_codes_ignore_related_locations() {
    let files = fixture();
    assert_eq!(ry(&files, &["--error-on-warning"]).status.code(), Some(1));
    let written = ry(&files, &["--write-baseline", "baseline.json"]);
    assert!(written.status.success(), "{written:?}");
    let baseline = std::fs::read_to_string(files.root().join("baseline.json")).unwrap();
    assert!(!baseline.contains("declared"), "{baseline}");
    let suppressed = ry(
        &files,
        &["--error-on-warning", "--baseline", "baseline.json"],
    );
    assert!(suppressed.status.success(), "{suppressed:?}");
}
