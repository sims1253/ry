//! End-to-end tests for `ry dump-types`: run the real binary against a
//! fixture package and assert on the JSON contract (shape, binding
//! kinds, type strings, --position filtering, exit-code semantics).

use std::fs;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(args)
        .output()
        .expect("failed to invoke ry")
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout is valid JSON")
}

/// A small package exercising every dump shape:
///
/// ```r
/// 1  threshold <- 10L
/// 3  moving_average <- function(x, n = 10L, w = NULL) {
/// 4    if (is.null(w)) {
/// 5      w <- rep(1, n)
/// 7    weighted <- x * w          # omitted below for line-6 brace
/// 8    window <- function(values, size) {
/// 9      head(values, size)
/// 11   window(weighted, n)
/// 13 result <- moving_average(c(1, 2, 3))
/// 14 missing_binding               # diagnostics never fail the dump
/// ```
fn fixture_package() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: dumpfix\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("R/analysis.R"),
        concat!(
            "threshold <- 10L\n",
            "\n",
            "moving_average <- function(x, n = 10L, w = NULL) {\n",
            "  if (is.null(w)) {\n",
            "    w <- rep(1, n)\n",
            "  }\n",
            "  weighted <- x * w\n",
            "  window <- function(values, size) {\n",
            "    head(values, size)\n",
            "  }\n",
            "  window(weighted, n)\n",
            "}\n",
            "\n",
            "result <- moving_average(c(1, 2, 3))\n",
            "missing_binding\n",
        ),
    )
    .unwrap();
    temp
}

fn bindings_of(scope: &serde_json::Value) -> Vec<(&str, &str, &str)> {
    scope["bindings"]
        .as_array()
        .expect("bindings array")
        .iter()
        .map(|binding| {
            (
                binding["name"].as_str().expect("name"),
                binding["kind"].as_str().expect("kind"),
                binding["type"].as_str().expect("type"),
            )
        })
        .collect()
}

#[test]
fn dumps_json_shape_with_param_concrete_and_unknown_types() {
    let temp = fixture_package();
    let file = temp.path().join("R/analysis.R");
    let output = run(&["dump-types", file.to_str().unwrap()]);

    // Diagnostics exist (missing_binding -> RY010) but never fail the dump.
    assert!(output.status.success(), "{}", output.status);
    let dump = stdout_json(&output);

    let files = dump["files"].as_array().expect("files array");
    assert_eq!(files.len(), 1);
    assert!(
        files[0]["path"]
            .as_str()
            .expect("path")
            .ends_with("R/analysis.R")
    );

    let scopes = files[0]["scopes"].as_array().expect("scopes array");
    // Top level, moving_average, and the nested window function.
    assert_eq!(scopes.len(), 3, "{scopes:?}");
    assert_eq!(scopes[0]["kind"], "top");
    assert_eq!(scopes[0]["name"], serde_json::Value::Null);
    assert_eq!(scopes[1]["kind"], "function");
    assert_eq!(scopes[1]["name"], "moving_average");
    assert_eq!(scopes[2]["kind"], "function");
    assert_eq!(scopes[2]["name"], "window");

    // Scopes are ordered by start position; positions are 1-based
    // [row, col] pairs.
    assert_eq!(scopes[0]["start"], serde_json::json!([1, 1]));
    assert_eq!(scopes[1]["start"], serde_json::json!([3, 19]));
    assert_eq!(scopes[2]["start"], serde_json::json!([8, 13]));

    let outer = &scopes[1];
    let bindings = bindings_of(outer);
    // Bindings are sorted by name.
    let names: Vec<_> = bindings.iter().map(|(name, _, _)| *name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);

    // A defaulted formal whose argument a call site omits carries its
    // literal type: the acceptance "param with a concrete type".
    let n = bindings
        .iter()
        .find(|(name, _, _)| *name == "n")
        .expect("n bound");
    assert_eq!(n.1, "param");
    assert_eq!(n.2, "integer<len=1>");

    // A default-less formal is honest about missing inference.
    let x = bindings
        .iter()
        .find(|(name, _, _)| *name == "x")
        .expect("x bound");
    assert_eq!(x.1, "param");
    assert_eq!(x.2, "unknown");

    // Locals assigned in the body.
    let weighted = bindings
        .iter()
        .find(|(name, _, _)| *name == "weighted")
        .expect("weighted bound");
    assert_eq!(weighted.1, "local");

    // Top-level bindings: the function object and the scalar.
    let top_bindings = bindings_of(&scopes[0]);
    let threshold = top_bindings
        .iter()
        .find(|(name, _, _)| *name == "threshold")
        .expect("threshold bound");
    assert_eq!(threshold.1, "local");
    assert_eq!(threshold.2, "integer<len=1>");
    assert!(
        top_bindings
            .iter()
            .any(|(name, kind, _)| *name == "moving_average" && *kind == "local")
    );

    // The nested function closes over names its own body never assigns.
    let inner_bindings = bindings_of(&scopes[2]);
    let n_capture = inner_bindings
        .iter()
        .find(|(name, _, _)| *name == "n")
        .expect("n captured");
    assert_eq!(n_capture.1, "closed-over");
    assert_eq!(n_capture.2, "integer<len=1>");
    let values = inner_bindings
        .iter()
        .find(|(name, _, _)| *name == "values")
        .expect("values formal");
    assert_eq!(values.1, "param");
}

#[test]
fn position_flag_selects_innermost_scope_and_filters_late_bindings() {
    let temp = fixture_package();
    let file = temp
        .path()
        .join("R/analysis.R")
        .to_string_lossy()
        .into_owned();

    // Line 9 is inside window's body: only the innermost scope is dumped.
    let output = run(&["dump-types", &file, "--position", "9:5"]);
    assert!(output.status.success());
    let dump = stdout_json(&output);
    let scopes = dump["files"][0]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(scopes[0]["name"], "window");

    // Line 4 is inside moving_average (an `if` body creates no scope in
    // R) but before `weighted` is assigned, so that local is not yet in
    // scope there.
    let output = run(&["dump-types", &file, "--position", "4:3"]);
    let dump = stdout_json(&output);
    let scopes = dump["files"][0]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0]["name"], "moving_average");
    let names: Vec<_> = bindings_of(&scopes[0])
        .iter()
        .map(|(name, _, _)| *name)
        .collect();
    assert!(names.contains(&"x"), "formals always visible: {names:?}");
    assert!(
        !names.contains(&"weighted"),
        "assigned after the position: {names:?}"
    );

    // Line 1 is top-level only.
    let output = run(&["dump-types", &file, "--position", "1:1"]);
    let dump = stdout_json(&output);
    let scopes = dump["files"][0]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0]["kind"], "top");

    // Repeated positions select the union of their innermost scopes.
    let output = run(&[
        "dump-types",
        &file,
        "--position",
        "9:5",
        "--position",
        "1:1",
    ]);
    let dump = stdout_json(&output);
    let scopes = dump["files"][0]["scopes"].as_array().unwrap();
    let selected: Vec<&str> = scopes
        .iter()
        .map(|scope| scope["name"].as_str().unwrap_or("top"))
        .collect();
    assert_eq!(selected, vec!["top", "window"]);
}

#[test]
fn directory_input_dumps_every_discovered_file() {
    let temp = fixture_package();
    let root = temp.path().to_str().unwrap();
    let output = run(&["dump-types", root]);
    assert!(output.status.success());
    let dump = stdout_json(&output);
    let files = dump["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0]["path"].as_str().unwrap().ends_with("R/analysis.R"));
}

/// A directory dump must apply the discovered `ry.toml`'s `exclude`
/// patterns with the same anchoring as `ry check`, so the two commands
/// never disagree on the file set.
#[test]
fn directory_dump_applies_config_exclude_patterns_like_check() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("keep.R"), "a <- 1L\n").unwrap();
    fs::create_dir_all(temp.path().join("vendor")).unwrap();
    fs::write(temp.path().join("vendor/skip.R"), "b <- 2L\n").unwrap();
    // Gitignore-style patterns are matched against paths relative to the
    // directory owning the ry.toml; `**` on both sides tolerates the
    // relative `./` discovery prefix.
    fs::write(
        temp.path().join("ry.toml"),
        "exclude = [\"**/vendor/**\"]\n",
    )
    .unwrap();
    let root = temp.path().to_str().unwrap();

    let dump = stdout_json(&run(&["dump-types", root]));
    let dumped: Vec<&str> = dump["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect();
    assert_eq!(dumped.len(), 1, "{dumped:?}");
    assert!(dumped[0].ends_with("keep.R"), "{dumped:?}");
    assert!(
        !dumped.iter().any(|path| path.contains("vendor")),
        "excluded file must not be dumped: {dumped:?}"
    );

    // The same directory through `ry check` analyzes exactly one file:
    // the dumped set and the checked set agree.
    let check = run(&["check", root]);
    let stderr = String::from_utf8_lossy(&check.stderr);
    assert!(
        stderr.contains("checked 1 file(s)"),
        "check and dump disagree on the file set: {stderr}"
    );
}

/// A file whose first statement is a bare `function` literal produces a
/// function record whose span starts at the same byte 0 as the whole-file
/// top scope; the dump dedup must key on the kind too and keep both.
#[test]
fn leading_function_literal_keeps_top_scope_and_its_bindings() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("f.R");
    fs::write(&file, "function(x) x\na <- 1L\n").unwrap();
    let file = file.to_str().unwrap();

    let dump = stdout_json(&run(&["dump-types", file]));
    let scopes = dump["files"][0]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 2, "{scopes:?}");
    assert_eq!(scopes[0]["kind"], "function");
    assert_eq!(scopes[0]["name"], serde_json::Value::Null);
    assert_eq!(scopes[1]["kind"], "top");
    let top = bindings_of(&scopes[1]);
    assert!(
        top.iter()
            .any(|(name, kind, _)| *name == "a" && *kind == "local"),
        "top-level binding vanished: {top:?}"
    );

    // A position on a later line resolves through the surviving top
    // scope instead of selecting nothing.
    let dump = stdout_json(&run(&["dump-types", file, "--position", "2:1"]));
    let scopes = dump["files"][0]["scopes"].as_array().unwrap();
    assert_eq!(scopes.len(), 1, "{scopes:?}");
    assert_eq!(scopes[0]["kind"], "top");
    assert!(
        bindings_of(&scopes[0])
            .iter()
            .any(|(name, _, _)| *name == "a")
    );
}

/// The README records this asymmetry: an anonymous function literal used
/// as a call argument is inferred in discarding mode and is not dumped,
/// but a named function defined inside it completes and is.
#[test]
fn named_function_inside_anonymous_callback_is_recorded() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("g.R");
    fs::write(
        &file,
        concat!(
            "x <- lapply(1:3, function(item) {\n",
            "  helper <- function(v) v + 1L\n",
            "  helper(item)\n",
            "})\n",
        ),
    )
    .unwrap();
    let dump = stdout_json(&run(&["dump-types", file.to_str().unwrap()]));
    let scopes = dump["files"][0]["scopes"].as_array().unwrap();
    let names: Vec<&str> = scopes
        .iter()
        .map(|scope| scope["name"].as_str().unwrap_or("top"))
        .collect();
    // The anonymous callback scope is absent; `helper` inside it is not.
    assert_eq!(names, vec!["top", "helper"], "{scopes:?}");
}

/// Degraded scopes (serialized data over the byte cap) print a stderr
/// note like `ry check`'s, without polluting the JSON on stdout.
#[test]
fn degraded_scopes_are_noted_on_stderr_only() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("R")).unwrap();
    fs::create_dir_all(temp.path().join("data")).unwrap();
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: degr\nVersion: 0.0.0.9000\nLazyData: true\n",
    )
    .unwrap();
    fs::write(temp.path().join("R/x.R"), "a <- 1L\n").unwrap();
    fs::write(temp.path().join("data/big.rda"), vec![0u8; 4096]).unwrap();
    fs::write(temp.path().join("ry.toml"), "max-serialized-bytes = 16\n").unwrap();
    let root = temp.path().to_str().unwrap();

    let output = run(&["dump-types", root]);
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("degraded scope"),
        "missing parity note on stderr: {stderr}"
    );
    // The note never leaks into the machine-facing dump.
    let dump = stdout_json(&output);
    let files = dump["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0]["path"].as_str().unwrap().ends_with("R/x.R"));
}

#[test]
fn exit_code_nonzero_only_for_usage_and_io_failures() {
    let temp = fixture_package();
    let file = temp
        .path()
        .join("R/analysis.R")
        .to_string_lossy()
        .into_owned();

    // Unknown format is a usage failure.
    let output = run(&["dump-types", &file, "--format", "yaml"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected one of: json"));

    // Missing file is an IO failure.
    let output = run(&["dump-types", &temp.path().join("nope.R").to_string_lossy()]);
    assert!(!output.status.success());

    // Malformed position is a usage failure (clap rejects the value).
    let output = run(&["dump-types", &file, "--position", "line:col"]);
    assert!(!output.status.success());

    // Files are required.
    let output = run(&["dump-types"]);
    assert!(!output.status.success());
}
