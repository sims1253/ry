use std::fs;
use std::process::{Command, Output};

fn stub(package: &str, functions: &str) -> String {
    format!(
        r#"{{
  "schema_version": "1",
  "package": "{package}",
  "version": "test",
  "functions": {{{functions}}}
}}"#
    )
}

fn validate(dir: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["typeshed", "validate"])
        .arg(dir)
        .output()
        .expect("failed to invoke ry")
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn validates_flat_and_nested_stub_layouts() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("flat.json"),
        stub(
            "flat",
            r#""one": {"params": [], "return": {"mode": "integer", "length": "1"}}"#,
        ),
    )
    .unwrap();
    fs::create_dir(tmp.path().join("nested")).unwrap();
    fs::write(
        tmp.path().join("nested/nested.json"),
        stub("nested", r#""two": {"params": [], "return": "arg0"}"#),
    )
    .unwrap();

    let output = validate(tmp.path());
    assert!(output.status.success(), "{}", output_text(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validated 2 stub files"));
}

#[test]
fn rejects_bad_mode_and_names_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("bad.json");
    fs::write(
        &path,
        stub(
            "bad",
            r#""f": {"params": [], "return": {"mode": "imaginary", "length": "1"}}"#,
        ),
    )
    .unwrap();

    let output = validate(tmp.path());
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains(&path.display().to_string()), "{text}");
    assert!(text.contains("invalid mode `imaginary`"), "{text}");
}

#[test]
fn rejects_package_name_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("actual.json");
    fs::write(
        &path,
        stub("different", r#""f": {"params": [], "return": "arg0"}"#),
    )
    .unwrap();

    let output = validate(tmp.path());
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("package `different` does not match"),
        "{text}"
    );
}

#[test]
fn rejects_duplicate_name_after_alias_expansion() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("aliases.json");
    fs::write(
        &path,
        stub(
            "aliases",
            r#"
    "first": {"params": [], "return": "arg0", "aliases": ["shared"]},
    "second": {"params": [], "return": "arg0", "aliases": ["shared"]}
"#,
        ),
    )
    .unwrap();

    let output = validate(tmp.path());
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("duplicate function name `shared`"), "{text}");
}

#[test]
fn rejects_alias_colliding_with_canonical_function_name() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("canonical.json"),
        stub(
            "canonical",
            r#"
    "first": {"params": [], "return": "arg0", "aliases": ["second"]},
    "second": {"params": [], "return": "arg0"}
"#,
        ),
    )
    .unwrap();

    let output = validate(tmp.path());
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("duplicate function name `second`"), "{text}");
}

#[test]
fn quiet_prints_summary_only() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("bad.json"),
        stub("wrong", r#""f": {"params": [], "return": "arg0"}"#),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["typeshed", "validate", "--quiet"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty(), "{}", output_text(&output));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 errors"));
}

#[test]
fn warns_without_failing_on_unsorted_function_keys() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("order.json"),
        stub(
            "order",
            r#"
    "zebra": {"params": [], "return": "arg0"},
    "alpha": {"params": [], "return": "arg0"}
"#,
        ),
    )
    .unwrap();

    let output = validate(tmp.path());
    let text = output_text(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("warning: function keys are not sorted"),
        "{text}"
    );
}

#[test]
fn rejects_unknown_schema_fields_through_normative_parser() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("unknown.json"),
        stub(
            "unknown",
            r#""f": {"params": [], "return": "arg0", "checker_hint": true}"#,
        ),
    )
    .unwrap();

    let output = validate(tmp.path());
    let text = output_text(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("unknown field `checker_hint`"), "{text}");
}
