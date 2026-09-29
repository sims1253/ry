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

fn dump(root: &Path, flags: &[&str]) -> Value {
    let mut args = vec!["dump-facts", "R/main.R"];
    args.extend(flags);
    let output = invoke(root, &args);
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn adopted_contract_checks_and_exports_the_same_source_record() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("R/main.R"),
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(1L)\nf(\"bad\")\n",
    )
    .unwrap();
    assert!(!check_codes(temp.path()).iter().any(|code| code == "RY114"));

    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
    )
    .unwrap();
    assert_eq!(
        check_codes(temp.path())
            .into_iter()
            .filter(|code| code == "RY114")
            .count(),
        1
    );

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
fn nested_headers_and_inline_comments_cannot_create_an_outer_contract() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
    )
    .unwrap();
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

    let temp = tempfile::tempdir().unwrap();
    let r = temp.path().join("R");
    fs::create_dir(&r).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/bad�.R']\n",
    )
    .unwrap();
    let raw = r.join(OsString::from_vec(b"bad\xff.R".to_vec()));
    let unicode = r.join("bad�.R");
    let source = "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"bad\")\n";
    fs::write(&raw, source).unwrap();
    assert!(!check_codes(temp.path()).iter().any(|code| code == "RY114"));

    fs::write(&unicode, source).unwrap();
    // Both parser paths display as bad�.R with identical definition spans.
    // Neither is safe to attach while the source identities collide.
    let codes = check_codes(temp.path());
    assert!(!codes.iter().any(|code| code == "RY114"));
    assert_eq!(codes.iter().filter(|code| *code == "RY117").count(), 1);

    fs::remove_file(&raw).unwrap();
    assert_eq!(
        check_codes(temp.path())
            .into_iter()
            .filter(|code| code == "RY114")
            .count(),
        1
    );
    assert!(!check_codes(temp.path()).iter().any(|code| code == "RY117"));
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
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("R")).unwrap();
        fs::write(
            temp.path().join("ry.toml"),
            "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("R/main.R"),
            format!("f <- function(x) {{\n #| x integer\n x\n}}\nf({actual})\n"),
        )
        .unwrap();
        let count = check_codes(temp.path())
            .iter()
            .filter(|code| *code == "RY114")
            .count();
        assert_eq!(count, expected, "actual {actual}");
    }
}

#[test]
fn omitted_default_is_not_checked_but_explicit_actual_is() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
    )
    .unwrap();
    let file = temp.path().join("R/main.R");
    fs::write(
        &file,
        "f <- function(x = \"wrong\") {\n #| x integer\n x\n}\nf()\n",
    )
    .unwrap();
    assert!(!check_codes(temp.path()).iter().any(|code| code == "RY114"));
    fs::write(
        &file,
        "f <- function(x = \"wrong\") {\n #| x integer\n x\n}\nf()\nf(x = \"wrong\")\n",
    )
    .unwrap();
    assert_eq!(
        check_codes(temp.path())
            .iter()
            .filter(|code| *code == "RY114")
            .count(),
        1
    );
    let annotation = &dump(temp.path(), &["--annotations"])["files"][0]["annotations"][0];
    assert_eq!(
        annotation["translation"]["supported"]["parameters"][0]["supplied"],
        "defaulted_supplied_only"
    );
}

#[test]
fn partial_unsupported_and_invalid_source_records_keep_their_status() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
    )
    .unwrap();
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
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
    )
    .unwrap();
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
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: typedemo\nTitle: Typehint Adoption Fixture\nVersion: 0.0.1\nDescription: Static declarations across package files.\nLicense: MIT\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/contracts.R']\n",
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
    assert_eq!(
        check_codes(temp.path())
            .iter()
            .filter(|code| *code == "RY114")
            .count(),
        1
    );
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
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
    )
    .unwrap();
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
