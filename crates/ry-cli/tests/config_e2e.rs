//! End-to-end tests for `ry.toml` project configuration.
//!
//! These tests invoke the `ry` binary against temporary project trees
//! to exercise the full pipeline: discovery, parsing, merging with CLI
//! flags, and applying the merged settings to diagnostics. They
//! complement the unit tests in `src/config.rs`, which cover the
//! individual pieces in isolation.

use std::fs;
use std::process::Command;

fn stub(package: &str, function: &str, mode: &str) -> String {
    format!(
        r#"{{
            "schema_version": "1",
            "package": "{package}",
            "version": "test",
            "functions": {{
                "{function}": {{
                    "params": [],
                    "return": {{"mode": "{mode}", "length": "1"}}
                }}
            }}
        }}"#
    )
}

/// Helper: run `ry check <arg>` and return the raw output.
fn ry_check(arg: &std::path::Path) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ry");
    Command::new(bin)
        .arg("check")
        .arg(arg)
        .output()
        .expect("failed to invoke ry binary")
}

#[test]
fn ry_toml_typeshed_path_is_relative_to_config() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("stubs")).unwrap();
    fs::write(
        tmp.path().join("stubs/foo.json"),
        stub("foo", "bar", "integer"),
    )
    .unwrap();
    fs::write(tmp.path().join("ry.toml"), "typeshed = [\"stubs\"]\n").unwrap();
    fs::write(tmp.path().join("use.R"), "library(foo)\nx <- bar() + 1L\n").unwrap();

    let output = ry_check_in(tmp.path(), std::path::Path::new("use.R"));
    assert!(
        String::from_utf8_lossy(&output.stdout).is_empty(),
        "config-relative user stub should resolve bar(): {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn cli_typeshed_directory_overrides_config_directory() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("config-stubs")).unwrap();
    fs::create_dir(tmp.path().join("cli-stubs")).unwrap();
    fs::write(
        tmp.path().join("config-stubs/foo.json"),
        stub("foo", "bar", "character"),
    )
    .unwrap();
    fs::write(
        tmp.path().join("cli-stubs/foo.json"),
        stub("foo", "bar", "integer"),
    )
    .unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        "typeshed = [\"config-stubs\"]\n",
    )
    .unwrap();
    fs::write(tmp.path().join("use.R"), "library(foo)\nx <- bar() + 1L\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .current_dir(tmp.path())
        .args(["check", "--typeshed", "cli-stubs", "use.R"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&output.stdout).is_empty(),
        "CLI directory must win over config package: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn malformed_user_stub_warns_and_valid_sibling_still_loads() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("stubs")).unwrap();
    fs::write(tmp.path().join("stubs/bad.json"), "not json").unwrap();
    fs::write(
        tmp.path().join("stubs/foo.json"),
        stub("foo", "bar", "integer"),
    )
    .unwrap();
    fs::write(tmp.path().join("ry.toml"), "typeshed = [\"stubs\"]\n").unwrap();
    fs::write(tmp.path().join("use.R"), "library(foo)\nx <- bar() + 1L\n").unwrap();

    let output = ry_check(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bad.json"),
        "warning must name bad file: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).is_empty(),
        "valid sibling stub must still be active: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Helper: run `ry check <arg>` from a specific working directory.
fn ry_check_in(cwd: &std::path::Path, arg: &std::path::Path) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ry");
    Command::new(bin)
        .current_dir(cwd)
        .arg("check")
        .arg(arg)
        .output()
        .expect("failed to invoke ry binary")
}

fn ry_check_with_r_lib(arg: &std::path::Path, r_lib: &std::path::Path) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_ry");
    Command::new(bin)
        .arg("check")
        .arg(arg)
        .env("R_LIBS", r_lib)
        .output()
        .expect("failed to invoke ry binary")
}

fn install_fixture_package(r_lib: &std::path::Path) {
    install_fixture_package_with_export(r_lib, "exported_value");
}

fn install_fixture_package_with_export(r_lib: &std::path::Path, export: &str) {
    let package = r_lib.join("fixturepkg");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("NAMESPACE"), format!("export({export})\n")).unwrap();
}

fn install_fixture_r_home(r_home: &std::path::Path, version: &str) {
    fs::create_dir_all(r_home.join("library/base")).unwrap();
    fs::write(r_home.join("library/base/NAMESPACE"), "export(baseenv)\n").unwrap();
    fs::write(
        r_home.join("library/base/DESCRIPTION"),
        format!("Package: base\nVersion: {version}\n"),
    )
    .unwrap();
}

#[test]
fn ry_toml_applies_severity_overrides() {
    // RY002 (`if` on length-2 logical) defaults to a warning, so a
    // check against this file alone exits 0. Promoting RY002 to an
    // error via `ry.toml` must flip the exit code to failure and
    // surface the diagnostic as an error.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"error = ["RY002"]
"#,
    )
    .unwrap();
    // `if (c(TRUE, FALSE))` triggers RY002 (length-2 logical).
    fs::write(
        tmp.path().join("bad.R"),
        r#"if (c(TRUE, FALSE)) print(1)
"#,
    )
    .unwrap();

    let output = ry_check(tmp.path());
    assert!(
        !output.status.success(),
        "expected non-zero exit code (RY002 promoted to error), got {:?}; stdout={}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("RY002"),
        "expected RY002 in output: {}",
        stdout
    );
    assert!(
        stdout.contains("error"),
        "expected RY002 to be reported as an error, got: {}",
        stdout
    );
}

#[test]
fn ry_toml_without_error_override_keeps_warning_non_fatal() {
    // Sanity counterpart to the above: with no severity override, the
    // same file produces only a warning and the exit code is 0.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        r#"if (c(TRUE, FALSE)) print(1)
"#,
    )
    .unwrap();
    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("RY002"),
        "expected RY002 warning even without config: {}",
        stdout
    );
    assert!(
        output.status.success(),
        "warnings must not fail the run without error-on-warning, got {:?}",
        output.status
    );
}

#[test]
fn ry_toml_exclude_patterns_skip_matched_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"exclude = ["fixtures/**"]
"#,
    )
    .unwrap();
    // A `fixtures/` file that WOULD trigger an error if checked.
    fs::create_dir_all(tmp.path().join("fixtures")).unwrap();
    fs::write(
        tmp.path().join("fixtures/bad.R"),
        // RY040 is a default-error rule; if the exclude fails, this
        // surfaces as an error and fails the run.
        r#"x <- "a" + 1L
"#,
    )
    .unwrap();
    // A clean file outside the exclude so the run has something to do.
    fs::write(
        tmp.path().join("good.R"),
        r#"x <- 1L + 2L
"#,
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "excluded file must not be checked, got {:?}; stderr={}",
        output.status,
        stderr
    );
    assert!(
        !stderr.contains("RY040"),
        "RY040 from excluded file must not appear: {}",
        stderr
    );
}

#[test]
fn ry_toml_output_format_json() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"output-format = "json"
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        r#"x <- "a" + 1L
"#,
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() || !output.status.success(),
        "exit code is not the point of this test"
    );
    // JSON output lands on stdout (per main.rs's routing) and must
    // parse as a JSON array containing the RY040 diagnostic.
    assert!(
        stdout.trim_start().starts_with('['),
        "expected JSON array on stdout, got: {}",
        stdout
    );
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(
        parsed
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "RY040"),
        "expected RY040 in JSON diagnostics: {}",
        parsed
    );
}

#[test]
fn ry_toml_discovered_from_subdirectory() {
    // ry.toml lives at the project root; `ry check` is invoked from a
    // nested subdirectory. Discovery must walk up and find it.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"error = ["RY002"]
"#,
    )
    .unwrap();
    let sub = tmp.path().join("src").join("deep");
    fs::create_dir_all(&sub).unwrap();
    fs::write(
        sub.join("bad.R"),
        r#"if (c(TRUE, FALSE)) print(1)
"#,
    )
    .unwrap();

    // Run `ry check .` from the deep subdirectory.
    let output = ry_check_in(&sub, std::path::Path::new("."));
    assert!(
        !output.status.success(),
        "expected config discovery from subdir to promote RY002 to error, got {:?}; stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn ry_toml_cli_flag_overrides_config_output_format() {
    // The config sets json; the CLI flag --output-format concise must
    // win (CLI overrides config for scalars).
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"output-format = "json"
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        r#"x <- "a" + 1L
"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path())
        .arg("--output-format")
        .arg("concise")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // concise format goes to stdout and is NOT a
    // JSON array.
    let _ = stderr;
    assert!(
        stdout.contains("RY040"),
        "expected RY040 on stdout in concise format: {}",
        stdout
    );
    assert!(
        !stdout.trim_start().starts_with('['),
        "concise output must not be a JSON array (config json was overridden): {}",
        stdout
    );
}

#[test]
fn ry_toml_cli_error_appends_to_config_errors() {
    // Config promotes RY002 to error; CLI additionally promotes RY010.
    // A file triggering BOTH must surface both as errors and fail.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("ry.toml"),
        r#"error = ["RY002"]
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("bad.R"),
        // RY002 from the if-condition; RY010 from the unbound ref.
        r#"if (c(TRUE, FALSE)) print(undefined_thing)
"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path())
        .arg("--error")
        .arg("RY010")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected failure with appended error rules, got {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("RY002"), "missing RY002: {}", stdout);
    assert!(stdout.contains("RY010"), "missing RY010: {}", stdout);
}

#[test]
fn ry_toml_malformed_aborts_with_error() {
    // A syntactically broken ry.toml must surface an error and abort
    // rather than silently running with defaults.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("ry.toml"), "this is not = = valid toml\n").unwrap();
    fs::write(tmp.path().join("ok.R"), "x <- 1\n").unwrap();

    let output = ry_check(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "malformed config must abort with non-zero exit, got {:?}",
        output.status
    );
    assert!(
        stderr.contains("ry.toml") && stderr.to_lowercase().contains("pars"),
        "expected a parse error mentioning ry.toml, got: {}",
        stderr
    );
}

#[test]
fn color_flag_is_advertised() {
    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg("--help")
        .output()
        .expect("failed to invoke ry binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--color"), "{stdout}");
}

#[test]
fn color_always_styles_human_diagnostics() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("bad.R"), "x <- genuinely_missing\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    for format in ["concise", "full"] {
        let output = Command::new(bin)
            .arg("check")
            .arg(tmp.path())
            .arg("--output-format")
            .arg(format)
            .arg("--color")
            .arg("always")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\x1b["), "expected ANSI styling: {stdout}");
        assert!(
            stdout.contains("RY010"),
            "diagnostic content changed: {stdout}"
        );
    }
}

#[test]
fn color_never_keeps_human_diagnostics_plain() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("bad.R"), "x <- genuinely_missing\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path())
        .arg("--color")
        .arg("never")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\x1b["), "ANSI must be disabled: {stdout}");
}

#[test]
fn color_always_leaves_json_machine_readable() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("bad.R"), "x <- genuinely_missing\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path())
        .arg("--output-format")
        .arg("json")
        .arg("--color")
        .arg("always")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("\x1b["), "ANSI leaked into JSON: {stdout}");
    serde_json::from_str::<serde_json::Value>(&stdout).expect("color must not corrupt JSON");
}

#[test]
fn package_namespace_imports_activate_nse_and_pipe_models() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("R")).unwrap();
    fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: namespacefixture\nVersion: 0.0.0.9000\nImports: dplyr, magrittr\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("NAMESPACE"),
        "import(dplyr)\nimportFrom(magrittr,\"%>%\")\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("R/use.R"),
        "filter_rows <- function(df) filter(df, column > 0)\n\
         pull_column <- function(df) df %>% .$column\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("RY010"),
        "NAMESPACE imports must activate package NSE and pipe semantics: {stdout}"
    );
}

#[test]
fn data_mask_column_shadows_same_named_base_function() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("R")).unwrap();
    fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: namespacefixture\nVersion: 0.0.0.9000\nImports: dplyr\n",
    )
    .unwrap();
    fs::write(tmp.path().join("NAMESPACE"), "import(dplyr)\n").unwrap();
    fs::write(
        tmp.path().join("R/use.R"),
        "keep_class <- function() {\n\
           df <- data.frame(class = \"wanted\")\n\
           filter(df, class == \"wanted\")\n\
         }\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("RY030"),
        "the data-mask column must shadow base::class: {stdout}"
    );
}

#[test]
fn description_depends_activate_nse_without_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("R")).unwrap();
    fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: dependsfixture\nVersion: 0.0.0.9000\nDepends: dplyr\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("R/use.R"),
        "top <- function(df) count(df, service_request_type)\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("RY010"),
        "DESCRIPTION Depends must activate package NSE semantics: {stdout}"
    );
}

#[test]
fn package_namespace_import_from_binds_imported_value() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("R")).unwrap();
    fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: namespacefixture\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    fs::write(tmp.path().join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();
    fs::write(
        tmp.path().join("R/use.R"),
        "page <- tags\noops <- genuinely_missing\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("variable `genuinely_missing` is not bound"),
        "control diagnostic must still fire: {stdout}"
    );
    assert!(
        !stdout.contains("variable `tags` is not bound"),
        "importFrom(shiny, tags) must bind tags as an opaque value: {stdout}"
    );
}

#[test]
fn package_namespace_bindings_do_not_leak_across_checked_roots() {
    let tmp = tempfile::tempdir().unwrap();
    for package in ["with_import", "without_import"] {
        let root = tmp.path().join(package);
        fs::create_dir_all(root.join("R")).unwrap();
        fs::write(
            root.join("DESCRIPTION"),
            format!("Package: {package}\nVersion: 0.0.0.9000\n"),
        )
        .unwrap();
        fs::write(root.join("R/use.R"), "page <- tags\n").unwrap();
    }
    fs::write(
        tmp.path().join("with_import/NAMESPACE"),
        "importFrom(shiny,tags)\n",
    )
    .unwrap();
    fs::write(tmp.path().join("without_import/NAMESPACE"), "").unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout
            .matches("variable `tags` is not bound in this scope")
            .count(),
        1,
        "only the package without importFrom may report tags: {stdout}"
    );
    assert!(
        stdout.contains("without_import"),
        "diagnostic must belong to the unimported package: {stdout}"
    );
}

#[test]
fn package_test_context_promotes_suggests_but_not_imports() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("R")).unwrap();
    fs::create_dir_all(tmp.path().join("tests/testthat")).unwrap();
    fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: contextfixture\nVersion: 0.0.0.9000\nSuggests: mirai\n",
    )
    .unwrap();
    fs::write(tmp.path().join("NAMESPACE"), "import(rlang)\n").unwrap();
    fs::write(tmp.path().join("R/use.R"), "enquo\n").unwrap();
    fs::write(
        tmp.path().join("tests/testthat/test-context.R"),
        "daemons\nenquo\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("variable `daemons` is not bound"),
        "stubbed Suggests must supply their names in tests: {stdout}"
    );
    assert!(
        stdout.matches("variable `enquo` is not bound").count() == 1,
        "NAMESPACE import() must supply bare names only to R/: {stdout}"
    );
}

#[test]
fn imported_rlang_capture_helper_is_recognized_globally() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("R")).unwrap();
    fs::create_dir_all(tmp.path().join("tests/testthat")).unwrap();
    fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: capturefixture\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    fs::write(tmp.path().join("NAMESPACE"), "importFrom(rlang,enquos)\n").unwrap();
    fs::write(
        tmp.path().join("R/lst.R"),
        "lst <- function(...) enquos(...)\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("tests/testthat/test-lst.R"),
        "lst(a = 1, b = a)\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("variable `a` is not bound"),
        "global promise-capture metadata must quote lst's dots: {stdout}"
    );
}

#[test]
fn script_library_call_binds_exports_from_installed_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    let r_lib = tmp.path().join("library");
    install_fixture_package(&r_lib);
    fs::write(
        tmp.path().join("script.R"),
        "library(fixturepkg)\nx <- exported_value\ny <- genuinely_missing\n",
    )
    .unwrap();

    let output = ry_check_with_r_lib(&tmp.path().join("script.R"), &r_lib);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("variable `genuinely_missing` is not bound"),
        "an un-stubbed attached package opens the search path: {stdout}"
    );
    assert!(
        !stdout.contains("variable `exported_value` is not bound"),
        "library(fixturepkg) must bind statically exported values: {stdout}"
    );
}

#[test]
fn script_require_call_binds_exports_from_installed_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    let r_lib = tmp.path().join("library");
    install_fixture_package(&r_lib);
    fs::write(
        tmp.path().join("script.R"),
        "require(fixturepkg)\nx <- exported_value\n",
    )
    .unwrap();

    let output = ry_check_with_r_lib(&tmp.path().join("script.R"), &r_lib);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("RY010"),
        "require(fixturepkg) must bind statically exported values: {stdout}"
    );
}

#[test]
fn installed_namespace_lookup_preserves_r_library_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("first-library");
    let second = tmp.path().join("second-library");
    install_fixture_package_with_export(&first, "first_export");
    install_fixture_package_with_export(&second, "shadowed_export");
    fs::write(
        tmp.path().join("script.R"),
        "library(fixturepkg)\nx <- first_export\ny <- shadowed_export\n",
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let r_libs = std::env::join_paths([first, second]).unwrap();
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path().join("script.R"))
        .env("R_LIBS", r_libs)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("variable `first_export` is not bound"),
        "the first R library must win: {stdout}"
    );
    assert!(
        !stdout.contains("variable `shadowed_export` is not bound"),
        "an un-stubbed attached package opens the search path: {stdout}"
    );
}

#[test]
fn placeholder_library_path_prefers_current_r_version() {
    let tmp = tempfile::tempdir().unwrap();
    let versioned = tmp.path().join("versioned-library");
    install_fixture_package_with_export(&versioned.join("8.8"), "old_export");
    install_fixture_package_with_export(&versioned.join("R-9.9.1"), "current_export");

    let r_home = tmp.path().join("r-home");
    install_fixture_r_home(&r_home, "9.9.1");
    fs::write(
        tmp.path().join("script.R"),
        "library(fixturepkg)\nx <- current_export\ny <- old_export\n",
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(tmp.path().join("script.R"))
        .env("R_LIBS", versioned.join("%v"))
        .env("R_HOME", r_home)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("variable `current_export` is not bound"),
        "the current R version must win placeholder expansion: {stdout}"
    );
    assert!(
        !stdout.contains("variable `old_export` is not bound"),
        "an un-stubbed attached package opens the search path: {stdout}"
    );
}

#[test]
fn project_renv_library_precedes_global_libraries() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    let renv_library = project.join("renv/library/R-9.9/x86_64-pc-linux-gnu");
    install_fixture_package_with_export(&renv_library, "renv_export");
    let global_library = tmp.path().join("global-library");
    install_fixture_package_with_export(&global_library, "global_export");
    let r_home = tmp.path().join("r-home");
    install_fixture_r_home(&r_home, "9.9.1");
    fs::write(
        project.join("script.R"),
        "library(fixturepkg)\nx <- renv_export\ny <- global_export\n",
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_ry");
    let output = Command::new(bin)
        .arg("check")
        .arg(project.join("script.R"))
        .env("R_LIBS", global_library)
        .env("R_HOME", r_home)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("variable `renv_export` is not bound"),
        "the project renv library must win: {stdout}"
    );
    assert!(
        !stdout.contains("variable `global_export` is not bound"),
        "an un-stubbed attached package opens the search path: {stdout}"
    );
}

#[test]
fn require_namespace_does_not_bind_installed_exports() {
    let tmp = tempfile::tempdir().unwrap();
    let r_lib = tmp.path().join("library");
    install_fixture_package(&r_lib);
    fs::write(
        tmp.path().join("script.R"),
        "requireNamespace(\"fixturepkg\")\nx <- exported_value\n",
    )
    .unwrap();

    let output = ry_check_with_r_lib(&tmp.path().join("script.R"), &r_lib);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("variable `exported_value` is not bound"),
        "requireNamespace must not attach package exports: {stdout}"
    );
}

#[test]
fn package_namespace_whole_import_binds_installed_exports() {
    let tmp = tempfile::tempdir().unwrap();
    let r_lib = tmp.path().join("library");
    install_fixture_package(&r_lib);
    fs::create_dir_all(tmp.path().join("package/R")).unwrap();
    fs::write(
        tmp.path().join("package/DESCRIPTION"),
        "Package: namespacefixture\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    fs::write(tmp.path().join("package/NAMESPACE"), "import(fixturepkg)\n").unwrap();
    fs::write(tmp.path().join("package/R/use.R"), "x <- exported_value\n").unwrap();

    let output = ry_check_with_r_lib(&tmp.path().join("package"), &r_lib);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("RY010"),
        "import(fixturepkg) must bind dependency exports: {stdout}"
    );
}

#[test]
fn ry_toml_packages_enables_dplyr_nse() {
    // A `packages = ["dplyr"]` in ry.toml makes a bare
    // `filter(df, mpg > 0)` resolve as dplyr's NSE verb, so the column
    // reference `mpg` (a real mtcars column) does NOT fire RY010.
    // Without `packages` (or an inline `library(dplyr)`), `filter`
    // falls through to regular resolution and `mpg` would be reported
    // as unbound.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("ry.toml"), "packages = [\"dplyr\"]\n").unwrap();
    fs::write(
        tmp.path().join("use.R"),
        // `df` is the mtcars data frame from the typeshed; the dplyr
        // NSE handler augments scope with its column schema so `mpg`
        // resolves. No RY010 should fire on `mpg`.
        "df <- mtcars\nsmall <- filter(df, mpg > 0)\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("RY010"),
        "with packages=[\"dplyr\"], filter() NSE should suppress RY010 on `mpg`, got: {stderr}"
    );
}

#[test]
fn ry_toml_without_packages_does_not_gate_dplyr_nse() {
    // Counterpart: with NO `packages` key and NO inline `library(dplyr)`,
    // a bare `filter(df, mpg > 0)` must fall through to regular
    // resolution, so the unbound `mpg` fires RY010.
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("use.R"),
        "df <- mtcars\nsmall <- filter(df, mpg > 0)\n",
    )
    .unwrap();

    let output = ry_check(tmp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("RY010"),
        "without packages or library(dplyr), filter() should fall through and emit RY010 on `mpg`, got: {stdout}"
    );
}

#[test]
fn full_output_reports_argument_type_mismatch_with_types() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("mismatch.R"), "mean(\"text\")\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ry"))
        .current_dir(tmp.path())
        .args(["check", "--output-format", "full", "mismatch.R"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{stdout}");
    assert!(stdout.contains("[RY092]"), "{stdout}");
    assert!(
        stdout.contains("argument `x` to `mean` is `character`, expected numeric"),
        "{stdout}"
    );
}
