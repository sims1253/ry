//! Keep the editor playground's documented diagnostic locations current.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use crate::support::check;

#[test]
fn playground_diagnostics_match_readme() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../editors/example-project")
        .canonicalize()
        .unwrap();
    let output = check()
        .cwd(&project)
        .args([".", "--output-format", "json"])
        .run();
    assert_eq!(
        output.code(),
        Some(1),
        "the playground intentionally contains errors: {output:?}"
    );
    let diagnostics = output.json().as_array().unwrap().clone();
    let mut by_file: BTreeMap<String, Vec<&serde_json::Value>> =
        std::fs::read_dir(project.join("R"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "R"))
            .map(|path| {
                (
                    path.file_name().unwrap().to_str().unwrap().to_owned(),
                    vec![],
                )
            })
            .collect();
    for diagnostic in &diagnostics {
        let filename = Path::new(diagnostic["path"].as_str().unwrap())
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        by_file
            .get_mut(filename)
            .expect("diagnostic belongs to a playground R file")
            .push(diagnostic);
    }

    let mut table = String::from(
        "| File | Errors | Warnings | Code at line:column |\n| :--- | ---: | ---: | :--- |\n",
    );
    for (filename, mut diagnostics) in by_file {
        diagnostics.sort_by_key(|diagnostic| {
            (
                diagnostic["line"].as_u64().unwrap(),
                diagnostic["column"].as_u64().unwrap(),
                diagnostic["code"].as_str().unwrap(),
            )
        });
        let errors = diagnostics
            .iter()
            .filter(|d| d["severity"] == "error")
            .count();
        let warnings = diagnostics
            .iter()
            .filter(|d| d["severity"] == "warning")
            .count();
        assert_eq!(
            errors + warnings,
            diagnostics.len(),
            "document any new severity"
        );
        let locations = diagnostics
            .iter()
            .map(|d| {
                format!(
                    "{}@{}:{}",
                    d["code"].as_str().unwrap(),
                    d["line"],
                    d["column"]
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        writeln!(
            table,
            "| R/{filename} | {errors} | {warnings} | {} |",
            if locations.is_empty() {
                "none"
            } else {
                &locations
            }
        )
        .unwrap();
    }
    let readme = std::fs::read_to_string(project.join("README.md")).unwrap();
    let documented = readme
        .split_once("<!-- playground-diagnostics:start -->")
        .unwrap()
        .1
        .split_once("<!-- playground-diagnostics:end -->")
        .unwrap()
        .0
        .trim();
    assert_eq!(
        documented,
        table.trim(),
        "refresh the README table after reviewing the changed findings"
    );
}
