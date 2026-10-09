//! End-to-end coverage of opt-in static typehint adoption and schema-3 export.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn invoke(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ry"))
        .current_dir(root)
        .args(args)
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .output()
        .unwrap()
}

fn check_codes(root: &Path) -> Vec<String> {
    let output = invoke(root, &["check", "--output-format", "json", "."]);
    assert!(output.status.success(), "{output:?}");
    let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic["code"].as_str().map(str::to_owned))
        .collect()
}

fn count(root: &Path, code: &str) -> usize {
    check_codes(root)
        .iter()
        .filter(|found| *found == code)
        .count()
}

const ADOPT: &str = "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n";

/// A temporary project with an `R/` directory and an optional `ry.toml`.
fn project(config: Option<&str>) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    if let Some(config) = config {
        fs::write(temp.path().join("ry.toml"), config).unwrap();
    }
    temp
}

fn dump(root: &Path, flags: &[&str]) -> Value {
    let mut args = vec!["dump-facts", "R/main.R"];
    args.extend(flags);
    let output = invoke(root, &args);
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn adopted_contract_checks_and_exports_the_same_source_record() {
    let temp = project(None);
    fs::write(
        temp.path().join("R/main.R"),
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(1L)\nf(\"bad\")\n",
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 0);

    fs::write(temp.path().join("ry.toml"), ADOPT).unwrap();
    assert_eq!(count(temp.path(), "RY114"), 1);

    assert_eq!(dump(temp.path(), &[])["schema_version"], 1);
    assert_eq!(dump(temp.path(), &["--references"])["schema_version"], 2);
    let facts = dump(temp.path(), &["--annotations"]);
    assert_eq!(facts["schema_version"], 3);
    let annotation = &facts["files"][0]["annotations"][0];
    assert_eq!(annotation["source"]["provider"], "typehint");
    assert_eq!(annotation["source"]["provider_version"], "0.1.0");
    assert_eq!(annotation["source"]["raw"], "#| x integer");
    assert_eq!(annotation["translation"]["kind"], "exact");
    assert_eq!(
        annotation["translation"]["supported"]["parameters"][0]["constraint"],
        "class[\"integer\"]"
    );
    assert_eq!(annotation["target"]["kind"], "local_function");
    assert_eq!(annotation["evidence_use"], "adopted_contract");
}

#[test]
fn class_guards_do_not_establish_or_erase_an_explicit_class() {
    let temp = project(Some(ADOPT));
    fs::write(
        temp.path().join("R/main.R"),
        include_str!("../../ry-checker/testdata/oracle/typehint_class_guards.R"),
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 0);

    for (value, guard, class, mismatches) in [
        ("new.env()", "TRUE", "'foo'", 0),
        ("new.env()", "TRUE", "'bar'", 1),
        ("new.env()", "is.environment(x)", "'foo'", 0),
        ("new.env()", "is.environment(x)", "'bar'", 1),
        ("1L", "is.integer(x)", "'foo'", 0),
        ("1L", "is.integer(x)", "'bar'", 1),
        ("1L", "is.object(x)", "'foo'", 0),
        ("1L", "is.object(x)", "'bar'", 1),
        ("matrix(1L)", "is.matrix(x)", "'foo'", 0),
        ("matrix(1L)", "is.matrix(x)", "'bar'", 1),
        ("list()", "inherits(x, 'foo')", "c('foo', 'bar')", 1),
    ] {
        fs::write(
            temp.path().join("R/main.R"),
            format!(
                "g <- function() {{\n x <- structure({value}, class = {class})\n if ({guard}) {{\n  f <- function(y) {{\n   #| y foo\n   y\n  }}\n  f(x)\n }}\n}}\ng()\n"
            ),
        )
        .unwrap();
        assert_eq!(
            count(temp.path(), "RY114"),
            mismatches,
            "value: {value}, guard: {guard}, class: {class}"
        );
    }
}

#[test]
fn nested_headers_and_inline_comments_cannot_create_an_outer_contract() {
    let temp = project(Some(ADOPT));
    for body in [
        "g <- function(\n #| x integer\n y) { y }\n x",
        "g <- function(y = {\n #| x integer\n 1L\n }) { y }\n x",
        "g <- function(y)\n #| x integer\n y\n x",
        "NULL #| x integer\n x",
        "x; #| x integer\n x",
    ] {
        fs::write(
            temp.path().join("R/main.R"),
            format!("f <- function(x) {{\n {body}\n}}\nf(\"bad\")\n"),
        )
        .unwrap();
        let codes = check_codes(temp.path());
        assert!(!codes.iter().any(|code| code == "RY114"), "{body}");
        assert!(
            dump(temp.path(), &["--annotations"])["files"][0]["annotations"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{body}"
        );
    }
}

#[cfg(unix)]
#[test]
fn native_filename_collision_cannot_attach_another_files_contract() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = project(Some(&ADOPT.replace("R/**", "R/bad�.R")));
    let r = temp.path().join("R");
    let raw = r.join(OsString::from_vec(b"bad\xff.R".to_vec()));
    let unicode = r.join("bad�.R");
    let source = "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"bad\")\n";
    fs::write(&raw, source).unwrap();
    assert_eq!(count(temp.path(), "RY114"), 0);

    fs::write(&unicode, source).unwrap();
    // Both parser paths display as bad�.R with identical definition spans.
    // Neither is safe to attach while the source identities collide.
    assert_eq!(count(temp.path(), "RY114"), 0);
    assert_eq!(count(temp.path(), "RY117"), 1);

    fs::remove_file(&raw).unwrap();
    assert_eq!(count(temp.path(), "RY114"), 1);
    assert_eq!(count(temp.path(), "RY117"), 0);
}

#[cfg(unix)]
#[test]
fn annotation_export_refuses_a_selected_file_with_ambiguous_native_identity() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = project(Some(ADOPT));
    let r = temp.path().join("R");
    let unicode = r.join("bad�.R");
    let raw = r.join(OsString::from_vec(b"bad\xff.R".to_vec()));
    fs::write(
        &unicode,
        "f <- function(x) {\n #| x integer\n x\n}\nf(\"bad\")\n",
    )
    .unwrap();
    fs::write(&raw, "other <- 1L\n").unwrap();

    let checked = invoke(
        temp.path(),
        &["check", "--output-format", "json", "R/bad�.R"],
    );
    assert!(checked.status.success(), "{checked:?}");
    let diagnostics: Vec<Value> = serde_json::from_slice(&checked.stdout).unwrap();
    assert!(diagnostics.iter().any(|entry| entry["code"] == "RY117"));

    for flags in [
        &["--annotations"][..],
        &["--references", "--annotations"][..],
    ] {
        let mut args = vec!["dump-facts", "R/bad�.R"];
        args.extend_from_slice(flags);
        let output = invoke(temp.path(), &args);
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("RY117"), "{stderr}");
        assert!(stderr.contains("bad�.R"), "{stderr}");
        assert!(stderr.contains("ambiguous"), "{stderr}");
    }

    // Schemas without annotations do not claim an adopted-record snapshot.
    for (flags, schema) in [(&[][..], 1), (&["--references"][..], 2)] {
        let mut args = vec!["dump-facts", "R/bad�.R"];
        args.extend_from_slice(flags);
        let output = invoke(temp.path(), &args);
        assert!(output.status.success(), "{output:?}");
        let facts: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(facts["schema_version"], schema);
    }

    fs::write(
        temp.path().join("ry.toml"),
        ADOPT.replace("adopt = true", "adopt = false"),
    )
    .unwrap();
    let disabled = invoke(temp.path(), &["dump-facts", "R/bad�.R", "--annotations"]);
    assert!(disabled.status.success(), "{disabled:?}");
    let disabled_facts: Value = serde_json::from_slice(&disabled.stdout).unwrap();
    assert!(
        disabled_facts["files"][0]["annotations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::write(temp.path().join("ry.toml"), ADOPT).unwrap();

    let directory = invoke(temp.path(), &["dump-facts", "R", "--annotations"]);
    assert!(!directory.status.success(), "{directory:?}");
    assert!(directory.stdout.is_empty());
    assert!(String::from_utf8_lossy(&directory.stderr).contains("UTF-8 paths"));

    fs::remove_file(&raw).unwrap();
    let restored = invoke(temp.path(), &["dump-facts", "R/bad�.R", "--annotations"]);
    assert!(restored.status.success(), "{restored:?}");
    let facts: Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert_eq!(facts["schema_version"], 3);
    assert_eq!(
        facts["files"][0]["annotations"][0]["source"]["raw"],
        "#| x integer"
    );
}

#[test]
fn effective_class_checks_use_class_facts_and_keep_uncertain_values_quiet() {
    for (actual, expected) in [
        ("1L", 0),
        ("1", 1),
        ("\"x\"", 1),
        ("structure(1.0, class = \"integer\")", 0),
        ("structure(\"x\", class = \"integer\")", 0),
        ("matrix(1L, nrow = 1L)", 0),
        ("structure(1L, class = c(\"a\", \"b\"))", 1),
        ("unknown_value", 0),
    ] {
        let temp = project(Some(ADOPT));
        fs::write(
            temp.path().join("R/main.R"),
            format!("f <- function(x) {{\n #| x integer\n x\n}}\nf({actual})\n"),
        )
        .unwrap();
        let count = count(temp.path(), "RY114");
        assert_eq!(count, expected, "actual {actual}");
    }
}

#[test]
fn omitted_default_is_not_checked_but_explicit_actual_is() {
    let temp = project(Some(ADOPT));
    let file = temp.path().join("R/main.R");
    fs::write(
        &file,
        "f <- function(x = \"wrong\") {\n #| x integer\n x\n}\nf()\n",
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 0);
    fs::write(
        &file,
        "f <- function(x = \"wrong\") {\n #| x integer\n x\n}\nf()\nf(x = \"wrong\")\n",
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 1);
    let annotation = &dump(temp.path(), &["--annotations"])["files"][0]["annotations"][0];
    assert_eq!(
        annotation["translation"]["supported"]["parameters"][0]["supplied"],
        "defaulted_supplied_only"
    );
}

#[test]
fn partial_unsupported_and_invalid_source_records_keep_their_status() {
    let temp = project(Some(ADOPT));
    fs::write(
        temp.path().join("R/main.R"),
        concat!(
            "partial <- function(x) {\n #| x integer dim(1L)\n x\n}\n",
            "unsupported <- function(x) {\n #| x custom-class\n x\n}\n",
            "invalid <- function(x) {\n #| x\n x\n}\n",
        ),
    )
    .unwrap();

    let codes = check_codes(temp.path());
    assert_eq!(codes.iter().filter(|code| *code == "RY115").count(), 2);
    assert_eq!(codes.iter().filter(|code| *code == "RY117").count(), 1);
    assert!(!codes.iter().any(|code| code == "RY114"));

    let facts = dump(temp.path(), &["--annotations"]);
    let annotations = facts["files"][0]["annotations"].as_array().unwrap();
    assert_eq!(annotations.len(), 3);
    let mut kinds: Vec<_> = annotations
        .iter()
        .map(|record| record["translation"]["kind"].as_str().unwrap())
        .collect();
    kinds.sort_unstable();
    assert_eq!(kinds, ["invalid_syntax", "partial", "unsupported"]);
    let residuals: Vec<_> = annotations
        .iter()
        .filter_map(|record| record["translation"]["residuals"][0]["raw"].as_str())
        .collect();
    assert!(residuals.contains(&"dim(1L)"));
    assert!(residuals.contains(&"custom-class"));
}

#[test]
fn schema_three_residuals_use_structural_token_offsets_and_unicode_columns() {
    let temp = project(Some(ADOPT));
    for (source, raw, expected_start) in [
        (
            "f <- function(x) {\n #| x integer x\n x\n}\n",
            "x",
            [2_u64, 15],
        ),
        (
            "f <- function(x) {\n #| x integer dim(\"λ\")\n x\n}\n",
            "dim(\"λ\")",
            [2_u64, 15],
        ),
    ] {
        fs::write(temp.path().join("R/main.R"), source).unwrap();
        let facts = dump(temp.path(), &["--annotations"]);
        let residual = &facts["files"][0]["annotations"][0]["translation"]["residuals"][0];
        assert_eq!(residual["raw"], raw);
        let start = residual["span"]["bytes"][0].as_u64().unwrap() as usize;
        let end = residual["span"]["bytes"][1].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], raw);
        assert_eq!(residual["span"]["start"], serde_json::json!(expected_start));
        assert_eq!(
            residual["span"]["end"][1].as_u64().unwrap(),
            expected_start[1] + raw.chars().count() as u64
        );
    }
}

#[test]
fn multi_file_package_uses_one_adopted_source_for_checks_and_export() {
    let temp = project(Some(&ADOPT.replace("R/**", "R/contracts.R")));
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: typedemo\nTitle: Typehint Adoption Fixture\nVersion: 0.0.1\nDescription: Static declarations across package files.\nLicense: MIT\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("R/contracts.R"),
        "accepted <- function(x) {\n #| x integer\n x\n}\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("R/calls.R"),
        "accepted(1L)\naccepted(\"wrong\")\n",
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 1);
    let output = invoke(temp.path(), &["dump-facts", "R", "--annotations"]);
    assert!(output.status.success(), "{output:?}");
    let facts: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(facts["schema_version"], 3);
    let files = facts["files"].as_array().unwrap();
    let annotations = files
        .iter()
        .flat_map(|file| file["annotations"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["source"]["raw"], "#| x integer");
    assert_eq!(annotations[0]["evidence_use"], "adopted_contract");
}

#[test]
fn conflicting_source_comments_emit_ry116_and_export_each_claim() {
    let temp = project(Some(ADOPT));
    fs::write(
        temp.path().join("R/main.R"),
        "f <- function(x) {\n #| x integer\n #| x character\n x\n}\nf(\"bad\")\n",
    )
    .unwrap();
    let codes = check_codes(temp.path());
    assert!(codes.iter().any(|code| code == "RY116"));
    assert!(!codes.iter().any(|code| code == "RY114"));
    let facts = dump(temp.path(), &["--annotations"]);
    let annotations = facts["files"][0]["annotations"].as_array().unwrap();
    assert_eq!(annotations.len(), 2);
    assert!(
        annotations
            .iter()
            .all(|record| record["translation"]["kind"] == "exact")
    );
}
