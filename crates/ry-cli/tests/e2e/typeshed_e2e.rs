use std::fs;

use ry_testkit::{CliOutput, ry};

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

fn validate(dir: &std::path::Path) -> CliOutput {
    ry!().args(["typeshed", "validate"]).arg(dir).run()
}

fn output_text(output: &CliOutput) -> String {
    output.stdout() + &output.stderr()
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
    assert!(output.success(), "{}", output_text(&output));
    assert!(output.stdout().contains("Validated 2 stub files"));
}

/// Each invalid stub fails validation with a message that names the
/// problem.
#[test]
fn rejects_invalid_stubs() {
    // (file stem and package, functions, expected message)
    let cases = [
        (
            "bad",
            r#""f": {"params": [], "return": {"mode": "imaginary", "length": "1"}}"#,
            "invalid mode `imaginary`",
        ),
        (
            "aliases",
            r#""first": {"params": [], "return": "arg0", "aliases": ["shared"]},
               "second": {"params": [], "return": "arg0", "aliases": ["shared"]}"#,
            "duplicate function name `shared`",
        ),
        (
            "canonical",
            r#""first": {"params": [], "return": "arg0", "aliases": ["second"]},
               "second": {"params": [], "return": "arg0"}"#,
            "duplicate function name `second`",
        ),
        (
            "unknown",
            r#""f": {"params": [], "return": "arg0", "checker_hint": true}"#,
            "unknown field `checker_hint`",
        ),
    ];
    for (package, functions, message) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(format!("{package}.json"));
        fs::write(&path, stub(package, functions)).unwrap();
        let output = validate(tmp.path());
        let text = output_text(&output);
        assert!(!output.success(), "{text}");
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains(message), "{text}");
    }
}

#[test]
fn rejects_package_name_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("actual.json"),
        stub("different", r#""f": {"params": [], "return": "arg0"}"#),
    )
    .unwrap();

    let output = validate(tmp.path());
    let text = output_text(&output);
    assert!(!output.success(), "{text}");
    assert!(
        text.contains("package `different` does not match"),
        "{text}"
    );
}

#[test]
fn quiet_prints_summary_only() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("bad.json"),
        stub("wrong", r#""f": {"params": [], "return": "arg0"}"#),
    )
    .unwrap();
    let output = ry!()
        .args(["typeshed", "validate", "--quiet"])
        .arg(tmp.path())
        .run();
    assert!(!output.success());
    assert!(output.stderr().is_empty(), "{}", output_text(&output));
    assert!(output.stdout().contains("1 errors"));
}

#[test]
fn function_key_order_does_not_affect_validation() {
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
    assert!(output.success(), "{text}");
    assert!(text.contains("0 errors."), "{text}");
}
