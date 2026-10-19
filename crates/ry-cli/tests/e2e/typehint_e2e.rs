//! End-to-end coverage of opt-in static typehint adoption and schema-3 export.

use std::fs;
use std::path::Path;

use ry_testkit::{CliOutput, ry};
use serde_json::{Value, json};

fn invoke(root: &Path, args: &[&str]) -> CliOutput {
    ry!()
        .cwd(root)
        .args(args)
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .run()
}

fn check_codes(root: &Path) -> Vec<String> {
    let output = invoke(root, &["check", "--output-format", "json", "."]);
    assert!(output.success(), "{output:?}");
    output.codes()
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

/// `ry dump-facts <path> --annotations`, which must succeed.
fn dump_path(root: &Path, path: &str) -> Value {
    let output = invoke(root, &["dump-facts", path, "--annotations"]);
    assert!(output.success(), "{output:?}");
    output.json()
}

fn dump(root: &Path, flags: &[&str]) -> Value {
    let mut args = vec!["dump-facts", "R/main.R"];
    args.extend(flags);
    let output = invoke(root, &args);
    assert!(output.success(), "{output:?}");
    output.json()
}

#[test]
fn adopted_contract_checks_and_exports_the_same_source_record() {
    let temp = project(None);
    // A checked call runs user code, which ends later contract checks in
    // its frame (docs/declarations.md), so the bad actual comes first.
    fs::write(
        temp.path().join("R/main.R"),
        "f <- function(x) {\n  #| x integer\n  x\n}\nf(\"bad\")\nf(1L)\n",
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 0);

    fs::write(temp.path().join("ry.toml"), ADOPT).unwrap();
    assert_eq!(count(temp.path(), "RY114"), 1);

    assert_eq!(dump(temp.path(), &[])["schema_version"], 1);
    assert_eq!(dump(temp.path(), &["--references"])["schema_version"], 2);
    let facts = dump(temp.path(), &["--annotations"]);
    assert_eq!(facts["schema_version"], 3);
    // Schema 3 keeps the schema 2 root shape even without `--references`.
    assert!(facts.get("snapshot_kind").is_none(), "{facts}");
    assert_eq!(facts["scope_snapshot_kind"], "scope_exit");
    assert_eq!(
        facts["capabilities"],
        json!({
            "scope_snapshots": true,
            "reference_facts": null,
            "reference_coverage": null,
            "annotation_records": "typehint_0.1.0_static_subset",
        })
    );
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
        include_str!("../../../ry-checker/testdata/oracle/typehint_class_guards.R"),
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

    // A rejecting `||` continuation keeps the known class vector too.
    fs::write(
        temp.path().join("R/main.R"),
        "g <- function() {\n x <- structure(1L, class = c('foo', 'bar'))\n if (!inherits(x, 'foo') || length(x) != 1) stop('bad')\n f <- function(y) {\n  #| y foo\n  y\n }\n f(x)\n}\ng()\n",
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 1);
}

#[test]
fn methods_selected_from_class_tests_do_not_return_exact_classes() {
    let temp = project(Some(ADOPT));
    let fixture = include_str!("../../../ry-checker/testdata/oracle/typehint_guarded_dispatch.R");
    fs::write(temp.path().join("R/main.R"), fixture).unwrap();
    assert_eq!(count(temp.path(), "RY114"), 0);

    // An exact receiver class still selects a method with a known result.
    for call in ["test(x)", "x + 1L"] {
        fs::write(
            temp.path().join("R/main.R"),
            format!(
                "{fixture}known <- function() {{\n x <- structure(1L, class = 'foo')\n if (TRUE) {{\n  f <- function(y) {{\n   #| y foo\n   y\n  }}\n  f({call})\n }}\n}}\nknown()\n"
            ),
        )
        .unwrap();
        assert_eq!(count(temp.path(), "RY114"), 1, "{call}");
    }
}

const GUARD_STUBS: &str = r#"{
  "schema_version": "2",
  "package": "guards",
  "version": "test",
  "functions": {
    "is_bar": {
      "params": [{"name": "x", "required": true}],
      "return": {"mode": "logical", "length": "1", "na": false},
      "predicate": {"subject_param": "x", "target": {"mode": "union", "members": ["integer", "double"], "class": ["bar"], "length": "1"}}
    },
    "has_a": {
      "params": [{"name": "x", "required": true}],
      "return": {"mode": "logical", "length": "1", "na": false},
      "predicate": {"subject_param": "x", "target": {"mode": "list", "length": "unknown", "columns": {"a": {"mode": "integer", "length": "1", "class": ["bar"]}}}}
    },
    "check_bar": {
      "params": ["x", "arg", "call"],
      "return": {"mode": "null", "length": "0", "na": false},
      "assertion": {
        "subject_param": "x",
        "target": {"mode": "union", "members": ["integer", "double"], "class": ["bar"], "length": "1"},
        "provenance": {"kind": "standalone_types_check", "fingerprint_params": ["arg", "call"]}
      }
    }
  }
}"#;

#[test]
fn stub_class_tests_mark_union_member_and_column_classes() {
    let temp = project(Some(&format!("typeshed = ['stubs']\n{ADOPT}")));
    fs::create_dir(temp.path().join("stubs")).unwrap();
    fs::write(temp.path().join("stubs/guards.json"), GUARD_STUBS).unwrap();
    for (setup, guard, actual, mismatches) in [
        ("", "if (guards::is_bar(x))", "x", 0),
        (
            "x <- structure(1L, class = 'bar')",
            "if (guards::is_bar(x))",
            "x",
            1,
        ),
        ("", "if (guards::has_a(x))", "x$a", 0),
        (
            "x <- list(a = structure(1L, class = 'bar'))",
            "if (guards::has_a(x))",
            "x$a",
            1,
        ),
        ("guards::check_bar(x)", "if (TRUE)", "x", 0),
    ] {
        fs::write(
            temp.path().join("R/main.R"),
            format!(
                "g <- function(x) {{\n {setup}\n {guard} {{\n  f <- function(y) {{\n   #| y baz\n   y\n  }}\n  f({actual})\n }}\n}}\ng()\n"
            ),
        )
        .unwrap();
        assert_eq!(
            count(temp.path(), "RY114"),
            mismatches,
            "setup: {setup}, guard: {guard}"
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
    assert!(checked.success(), "{checked:?}");
    assert!(checked.codes().iter().any(|code| code == "RY117"));

    for flags in [
        &["--annotations"][..],
        &["--references", "--annotations"][..],
    ] {
        let mut args = vec!["dump-facts", "R/bad�.R"];
        args.extend_from_slice(flags);
        let output = invoke(temp.path(), &args);
        assert!(!output.success(), "{output:?}");
        assert!(output.stdout().is_empty(), "{output:?}");
        let stderr = output.stderr();
        assert!(stderr.contains("RY117"), "{stderr}");
        assert!(stderr.contains("bad�.R"), "{stderr}");
    }

    // Exports without annotations do not attach records, so they succeed.
    for flags in [&[][..], &["--references"][..]] {
        let output = invoke(
            temp.path(),
            &[&["dump-facts", "R/bad�.R"][..], flags].concat(),
        );
        assert!(output.success(), "{flags:?}: {output:?}");
    }

    fs::write(
        temp.path().join("ry.toml"),
        ADOPT.replace("adopt = true", "adopt = false"),
    )
    .unwrap();
    let disabled_facts = dump_path(temp.path(), "R/bad�.R");
    assert!(
        disabled_facts["files"][0]["annotations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::write(temp.path().join("ry.toml"), ADOPT).unwrap();

    // A directory export reaches the raw filename itself and refuses it.
    let directory = invoke(temp.path(), &["dump-facts", "R", "--annotations"]);
    assert!(!directory.success(), "{directory:?}");
    assert!(directory.stdout().is_empty());
    assert!(directory.stderr().contains("bad"), "{directory:?}");

    fs::remove_file(&raw).unwrap();
    let facts = dump_path(temp.path(), "R/bad�.R");
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
        "f <- function(x = \"wrong\") {\n #| x integer\n x\n}\nf(x = \"wrong\")\nf()\n",
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
fn non_exact_and_conflicting_source_records_keep_their_status() {
    let temp = project(Some(ADOPT));
    for (source, expected_codes, expected_kinds) in [
        (
            concat!(
                "partial <- function(x) {\n #| x integer dim(1L)\n x\n}\n",
                "unsupported <- function(x) {\n #| x custom-class\n x\n}\n",
                "invalid <- function(x) {\n #| x\n x\n}\n",
            ),
            &["RY115", "RY115", "RY117"][..],
            &["invalid_syntax", "partial", "unsupported"][..],
        ),
        (
            "f <- function(x) {\n #| x integer\n #| x character\n x\n}\nf(\"bad\")\n",
            &["RY116"],
            &["exact", "exact"],
        ),
    ] {
        fs::write(temp.path().join("R/main.R"), source).unwrap();
        let mut codes = check_codes(temp.path());
        codes.retain(|code| ["RY114", "RY115", "RY116", "RY117"].contains(&code.as_str()));
        codes.sort();
        assert_eq!(codes, expected_codes, "{source}");

        let facts = dump(temp.path(), &["--annotations"]);
        let mut kinds: Vec<_> = facts["files"][0]["annotations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| record["translation"]["kind"].as_str().unwrap())
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, expected_kinds, "{source}");
    }
}

#[test]
fn schema_three_residuals_use_structural_token_offsets_and_unicode_columns() {
    let temp = project(Some(ADOPT));
    let source = "f <- function(x) {\n #| x integer dim(\"λ\")\n x\n}\n";
    let raw = "dim(\"λ\")";
    fs::write(temp.path().join("R/main.R"), source).unwrap();
    let facts = dump(temp.path(), &["--annotations"]);
    let residual = &facts["files"][0]["annotations"][0]["translation"]["residuals"][0];
    assert_eq!(residual["raw"], raw);
    let start = residual["span"]["bytes"][0].as_u64().unwrap() as usize;
    let end = residual["span"]["bytes"][1].as_u64().unwrap() as usize;
    assert_eq!(&source[start..end], raw);
    // Line and column count Unicode scalars, not bytes.
    assert_eq!(residual["span"]["start"], json!([2, 15]));
    assert_eq!(
        residual["span"]["end"],
        json!([2, 15 + raw.chars().count()])
    );
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
        "accepted(\"wrong\")\naccepted(1L)\n",
    )
    .unwrap();
    assert_eq!(count(temp.path(), "RY114"), 1);
    let facts = dump_path(temp.path(), "R");
    let files = facts["files"].as_array().unwrap();
    let annotations = files
        .iter()
        .flat_map(|file| file["annotations"].as_array().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0]["source"]["raw"], "#| x integer");
    assert_eq!(annotations[0]["evidence_use"], "adopted_contract");
}
