//! End-to-end tests for `ry.toml` discovery, merging with CLI flags, and
//! its effect on diagnostics.

use std::fs;
use std::path::Path;

use ry_testkit::{CliOutput, ry};
use serde_json::json;

use crate::support::{Files, check, install_namespace, unbound_names, write};

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

/// `ry check <args>` from `cwd`.
fn check_in(cwd: &Path, args: &[&str]) -> CliOutput {
    check().cwd(cwd).args(args).run()
}

/// `ry check <path>` with the path spelled absolutely.
fn ry_check(path: &Path) -> CliOutput {
    check().arg(path).run()
}

/// User stubs load from config-relative `typeshed` dirs, a `--typeshed` dir
/// wins over the config's, and a malformed stub warns without hiding a
/// valid sibling. Each case only resolves `bar()` through the integer stub.
#[test]
fn user_stub_dirs_resolve_relative_to_config_and_cli_wins() {
    let integer = stub("foo", "bar", "integer");
    let character = stub("foo", "bar", "character");
    // (files besides use.R, extra args, expected stderr fragment)
    let cases: [(Files, &[&str], &str); 3] = [
        (
            &[
                ("ry.toml", b"typeshed = [\"stubs\"]\n"),
                ("stubs/foo.json", integer.as_bytes()),
            ],
            &[],
            "",
        ),
        (
            &[
                ("ry.toml", b"typeshed = [\"config-stubs\"]\n"),
                ("config-stubs/foo.json", character.as_bytes()),
                ("cli-stubs/foo.json", integer.as_bytes()),
            ],
            &["--typeshed", "cli-stubs"],
            "",
        ),
        (
            &[
                ("ry.toml", b"typeshed = [\"stubs\"]\n"),
                ("stubs/bad.json", b"not json"),
                ("stubs/foo.json", integer.as_bytes()),
            ],
            &[],
            "bad.json",
        ),
    ];
    for (files, args, warning) in cases {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), files);
        write(tmp.path(), &[("use.R", b"library(foo)\nx <- bar() + 1L\n")]);
        let output = check_in(tmp.path(), &[args, &["use.R"]].concat());
        assert!(output.stdout().is_empty(), "{args:?}: {output:?}");
        assert!(output.stderr().contains(warning), "{args:?}: {output:?}");
    }
}

/// A scoped rule override selects files by their native path: a Unix
/// backslash is a filename byte, and a non-UTF-8 filename never matches the
/// lossy spelling of its display name.
#[cfg(unix)]
#[test]
fn scoped_severity_matches_native_filenames() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    // Each source yields one RY040 whose message names its operand order.
    let text_first = ("\"text\" + 1L\n", "`character` and `integer`");
    let integer_first = ("1L + \"text\"\n", "`integer` and `character`");
    let backslash = OsString::from("back\\slash.R");
    let mut cases = vec![
        ("back\\slash.R", vec![(backslash, text_first, "error")]),
        (
            "back\\slash.R",
            vec![(OsString::from("back/slash.R"), text_first, "warning")],
        ),
    ];
    // APFS requires valid UTF-8 filenames.
    if cfg!(not(target_os = "macos")) {
        let unicode = OsString::from("bad\u{fffd}.R");
        let raw = OsString::from_vec(b"bad\xff.R".to_vec());
        cases.push((
            "bad\u{fffd}.R",
            vec![(unicode.clone(), text_first, "error")],
        ));
        cases.push(("bad\u{fffd}.R", vec![(raw.clone(), text_first, "warning")]));
        // Both display as `bad\u{fffd}.R` in one run yet keep their own policy.
        cases.push((
            "bad\u{fffd}.R",
            vec![
                (raw, text_first, "warning"),
                (unicode, integer_first, "error"),
            ],
        ));
    }
    for (pattern, files) in cases {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("ry.toml"),
            format!(
                "warn = ['RY040']\n[[rule-overrides]]\npaths = ['{pattern}']\nerror = ['RY040']\n"
            ),
        )
        .unwrap();
        let mut process = check().args(["--output-format", "json"]);
        for (file, (source, _), _) in &files {
            let path = temp.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, source).unwrap();
            process = process.arg(path);
        }
        let output = process.run();
        let findings = output.json();
        let findings = findings.as_array().unwrap();
        assert_eq!(findings.len(), files.len(), "{output:?}");
        for (file, (_, operands), severity) in &files {
            let finding = findings
                .iter()
                .find(|finding| finding["message"].as_str().unwrap().contains(operands))
                .unwrap_or_else(|| panic!("{file:?}: {output:?}"));
            assert_eq!(finding["code"], "RY040");
            assert_eq!(finding["severity"], *severity, "{file:?}");
        }
    }
}

#[test]
fn path_rule_overrides_keep_all_files_in_inference_and_respect_cli_choice() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        &[
            ("DESCRIPTION", b"Package: scoped\nVersion: 0.1\n"),
            ("ry.toml", b"warn = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/**\"]\nerror = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/scratch/**\"]\nwarn = [\"RY040\"]\n"),
            ("R/defs.R", b"shared <- 1L\n\"x\" + 1L\n"),
            ("R/scratch/try.R", b"\"x\" + 1L\n"),
            ("scripts/use.R", b"shared\n\"x\" + 1L\n"),
        ],
    );

    let run = |args: &[&str]| {
        let output = check()
            .cwd(temp.path())
            .args(args)
            .args([".", "--output-format", "json"])
            .run();
        assert!(matches!(output.code(), Some(0 | 1)), "{output:?}");
        output.findings()
    };
    let ry040 = |findings: &[[String; 3]]| {
        findings
            .iter()
            .filter(|[_, code, _]| code == "RY040")
            .map(|[path, _, severity]| [path.clone(), severity.clone()])
            .collect::<Vec<_>>()
    };
    let findings = run(&[]);
    assert_eq!(
        ry040(&findings),
        [
            ["./R/defs.R", "error"],
            ["./R/scratch/try.R", "warning"],
            ["./scripts/use.R", "warning"],
        ]
    );
    assert!(
        findings.iter().all(|[_, code, _]| code != "RY010"),
        "scripts/use.R must still resolve bindings from R/defs.R: {findings:?}"
    );

    // A direct file input from a different working directory must resolve
    // the same owning ry.toml and report the same scoped severity.
    let direct = check_in(
        &temp.path().join("scripts"),
        &["../R/defs.R", "--output-format", "json"],
    );
    assert_eq!(ry040(&direct.findings()), [["../R/defs.R", "error"]]);

    let cli = run(&["--error", "RY040"]);
    assert!(
        ry040(&cli).iter().all(|[_, severity]| severity == "error"),
        "{cli:?}"
    );

    let high = run(&["--min-confidence", "high"]);
    assert!(
        ry040(&high).is_empty(),
        "scoped errors must retain their original medium confidence: {high:?}"
    );

    let snapshot = check_in(temp.path(), &[".", "--write-baseline", "baseline.json"]);
    assert!(matches!(snapshot.code(), Some(0 | 1)), "{snapshot:?}");
    fs::write(
        temp.path().join("ry.toml"),
        "warn = [\"RY040\"]\nbaseline = \"baseline.json\"\n[[rule-overrides]]\npaths = [\"R/**\"]\nerror = [\"RY040\"]\n[[rule-overrides]]\npaths = [\"R/scratch/**\"]\nwarn = [\"RY040\"]\n",
    )
    .unwrap();
    let accepted = run(&[]);
    assert!(
        accepted.is_empty(),
        "baseline subtraction must see scoped diagnostics: {accepted:?}"
    );

    fs::write(
        temp.path().join("ry.toml"),
        "[[rule-overrides]]\npaths = [\"R/defs.R\"]\nignore = [\"RY040\"]\n",
    )
    .unwrap();
    let suppressed = run(&[]);
    assert!(
        ry040(&suppressed)
            .iter()
            .all(|[path, _]| path != "./R/defs.R"),
        "source-local suppression must hide its own diagnostic: {suppressed:?}"
    );
    assert!(
        suppressed.iter().all(|[_, code, _]| code != "RY010"),
        "the suppressed defining file must still supply its binding: {suppressed:?}"
    );
}

#[test]
fn path_rule_overrides_normalize_equivalent_in_root_operands() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("R/sub")).unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    write(
        temp.path(),
        &[
            ("anchor.R", b"x <- 1L\n"),
            ("R/a.R", b"\"bad\" + 1L\n"),
            ("ry.toml", b"warn = [\"RY040\"]\noutput-format = \"json\"\n[[rule-overrides]]\npaths = [\"R/**\"]\nerror = [\"RY040\"]\n"),
        ],
    );

    let root = temp.path().to_str().unwrap();
    let scripts = temp.path().join("scripts");
    let absolute_anchor = temp.path().join("anchor.R");
    let absolute_source = temp.path().join("R/a.R");
    let cases = [
        (
            temp.path(),
            vec!["anchor.R".to_string(), "R/a.R".to_string()],
        ),
        (
            temp.path(),
            vec!["anchor.R".to_string(), "R/sub/../a.R".to_string()],
        ),
        (temp.path(), vec!["anchor.R".to_string(), "R".to_string()]),
        (
            scripts.as_path(),
            vec![
                absolute_anchor.to_string_lossy().into_owned(),
                "../R/a.R".to_string(),
            ],
        ),
        (
            scripts.as_path(),
            vec![
                "../anchor.R".to_string(),
                absolute_source.to_string_lossy().into_owned(),
            ],
        ),
    ];
    for (cwd, args) in cases {
        let output = check().cwd(cwd).args(&args).run();
        assert_eq!(output.code(), Some(1), "{root} {args:?}: {output:?}");
        let findings = output.json();
        assert!(
            findings
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| { finding["code"] == "RY040" && finding["severity"] == "error" }),
            "{args:?}: {findings}"
        );
    }
}

fn ry_check_with_r_lib(arg: &Path, r_lib: &Path) -> CliOutput {
    check()
        .arg(arg)
        .env_remove("RY_NO_INSTALLED_LIBRARIES")
        .env("R_LIBS", r_lib)
        .run()
}

fn install_fixture_package(r_lib: &Path) {
    install_namespace(r_lib, "fixturepkg", "exported_value");
}

fn install_fixture_r_home(r_home: &Path, version: &str) {
    write(
        r_home,
        &[
            ("library/base/NAMESPACE", b"export(baseenv)\n"),
            (
                "library/base/DESCRIPTION",
                format!("Package: base\nVersion: {version}\n").as_bytes(),
            ),
        ],
    );
}

/// `ry.toml` severities apply to files below the config, found by walking
/// up from a subdirectory, and `--error` adds to them. RY002 (`if` on a
/// length-2 logical) and RY010 default to warnings.
#[test]
fn severity_overrides_from_config_and_cli() {
    // (ry.toml, extra args, exit code, [code, severity] findings)
    type Case<'a> = (&'a str, &'a [&'a str], i32, [[&'a str; 2]; 2]);
    let cases: [Case; 3] = [
        ("", &[], 0, [["RY002", "warning"], ["RY010", "warning"]]),
        (
            "error = [\"RY002\"]\n",
            &[],
            1,
            [["RY002", "error"], ["RY010", "warning"]],
        ),
        (
            "error = [\"RY002\"]\n",
            &["--error", "RY010"],
            1,
            [["RY002", "error"], ["RY010", "error"]],
        ),
    ];
    for (config, args, code, expected) in cases {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            &[
                ("ry.toml", config.as_bytes()),
                (
                    "src/deep/bad.R",
                    b"if (c(TRUE, FALSE)) print(undefined_thing)\n",
                ),
            ],
        );
        let output = check()
            .cwd(tmp.path().join("src/deep"))
            .args([".", "--output-format", "json"])
            .args(args)
            .run();
        assert_eq!(output.code(), Some(code), "{config:?} {args:?}: {output:?}");
        let mut findings: Vec<_> = output
            .findings()
            .into_iter()
            .map(|[_, code, severity]| [code, severity])
            .collect();
        findings.sort();
        assert_eq!(findings, expected, "{config:?} {args:?}");
    }
}

#[test]
fn unused_ignore_audit_is_opt_in_and_updates_after_a_fix() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("example.R");
    fs::write(&path, "1L == 1L # ry: ignore[RY034]\n").unwrap();
    let default = ry_check(&path);
    assert!(!default.stdout().contains("RY113"));

    fs::write(tmp.path().join("ry.toml"), "warn = [\"RY113\"]\n").unwrap();
    let audited = ry_check(&path);
    let output = audited.stdout();
    assert!(
        output.contains("RY113") && output.contains("RY034"),
        "{output}"
    );

    fs::write(
        tmp.path().join("ry.toml"),
        "warn = [\"RY113\"]\nexclude = [\"example.R\"]\n",
    )
    .unwrap();
    let excluded = ry_check(tmp.path());
    assert!(!excluded.stdout().contains("RY113"));

    fs::write(tmp.path().join("ry.toml"), "warn = [\"RY113\"]\n").unwrap();
    fs::write(&path, "1L == 1L\n").unwrap();
    let fixed = check().args(["--output-format", "json"]).arg(&path).run();
    assert_eq!(fixed.json(), json!([]), "{fixed:?}");
}

#[test]
fn package_test_fixtures_are_skipped_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            ("DESCRIPTION", b"Package: fixturepkg\nVersion: 0.0.0.9000\n"),
            ("R/good.R", b"value <- 1L\n"),
            ("tests/testthat/test-live.R", b"live_missing_name\n"),
            ("tests/testthat/fixtures/bad.R", b"fixture_missing_name\n"),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    assert!(
        stdout.contains("live_missing_name"),
        "test code was skipped: {stdout}"
    );
    assert!(
        !stdout.contains("fixture_missing_name"),
        "fixture data must be skipped by default: {stdout}"
    );
}

#[test]
fn check_test_fixtures_config_opts_back_in() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            ("DESCRIPTION", b"Package: fixturepkg\nVersion: 0.0.0.9000\n"),
            ("ry.toml", b"check-test-fixtures = true\n"),
            ("R/good.R", b"value <- 1L\n"),
            ("tests/testthat/fixtures/bad.R", b"fixture_missing_name\n"),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    assert!(
        stdout.contains("fixture_missing_name"),
        "configured fixture data must be checked: {stdout}"
    );
}

#[test]
fn ry_toml_output_format_json() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "ry.toml",
                br#"output-format = "json"
"#,
            ),
            (
                "bad.R",
                br#"x <- "a" + 1L
"#,
            ),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    // JSON output lands on stdout (per check.rs's routing) and must
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
fn ry_toml_cli_flag_overrides_config_output_format() {
    // The config sets json; the CLI flag --output-format concise must
    // win (CLI overrides config for scalars).
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "ry.toml",
                br#"output-format = "json"
"#,
            ),
            (
                "bad.R",
                br#"x <- "a" + 1L
"#,
            ),
        ],
    );
    let output = check()
        .arg(tmp.path())
        .arg("--output-format")
        .arg("concise")
        .run();
    let stdout = output.stdout();
    // concise format goes to stdout and is NOT a
    // JSON array.
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
fn ry_toml_malformed_aborts_with_error() {
    // A syntactically broken ry.toml must surface an error and abort
    // rather than silently running with defaults.
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            ("ry.toml", b"this is not = = valid toml\n"),
            ("ok.R", b"x <- 1\n"),
        ],
    );

    let output = ry_check(tmp.path());
    let stderr = output.stderr();
    assert!(
        !output.success(),
        "malformed config must abort with non-zero exit: {output:?}"
    );
    assert!(
        stderr.contains("ry.toml") && stderr.to_lowercase().contains("pars"),
        "expected a parse error mentioning ry.toml, got: {}",
        stderr
    );
}

/// A reader that closes stdout early (`ry check | head`) ends the command
/// without a panic.
#[cfg(unix)]
#[test]
fn closed_stdout_does_not_panic() {
    use std::process::Stdio;

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("bad.R"), "x <- 1 + 'a'\n").unwrap();
    for format in ["full", "concise", "json"] {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_ry"))
            .args(["check", "--exit-zero", "--output-format", format])
            .arg(temp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = CliOutput(child.wait_with_output().unwrap());
        let stderr = output.stderr();
        assert!(!stderr.contains("panicked"), "{format}: {stderr}");
        assert_ne!(output.code(), Some(101), "{format}: {stderr}");
    }
}

#[test]
fn color_flag_styles_human_output_only() {
    assert!(check().arg("--help").run().stdout().contains("--color"));
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), &[("bad.R", b"x <- genuinely_missing\n")]);
    // (output format, --color, whether stdout carries ANSI escapes)
    for (format, color, styled) in [
        ("concise", "always", true),
        ("full", "always", true),
        ("full", "never", false),
        ("json", "always", false),
    ] {
        let output = check()
            .arg(tmp.path())
            .args(["--output-format", format, "--color", color])
            .run();
        let stdout = output.stdout();
        assert_eq!(
            stdout.contains("\x1b["),
            styled,
            "{format} {color}: {stdout}"
        );
        assert!(stdout.contains("RY010"), "{format} {color}: {stdout}");
        if format == "json" {
            output.json();
        }
    }
}

#[test]
fn package_namespace_imports_activate_nse_and_pipe_models() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "DESCRIPTION",
                b"Package: namespacefixture\nVersion: 0.0.0.9000\nImports: dplyr, magrittr\n",
            ),
            (
                "NAMESPACE",
                b"import(dplyr)\nimportFrom(magrittr,\"%>%\")\n",
            ),
            (
                "R/use.R",
                b"filter_rows <- function(df) filter(df, column > 0)\n\
         pull_column <- function(df) df %>% .$column\n",
            ),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    assert!(
        !stdout.contains("RY010"),
        "NAMESPACE imports must activate package NSE and pipe semantics: {stdout}"
    );
}

#[test]
fn data_mask_column_shadows_same_named_base_function() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "DESCRIPTION",
                b"Package: namespacefixture\nVersion: 0.0.0.9000\nImports: dplyr\n",
            ),
            ("NAMESPACE", b"import(dplyr)\n"),
            (
                "R/use.R",
                b"keep_class <- function() {\n\
           df <- data.frame(class = \"wanted\")\n\
           filter(df, class == \"wanted\")\n\
         }\n",
            ),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    assert!(
        !stdout.contains("RY030"),
        "the data-mask column must shadow base::class: {stdout}"
    );
}

#[test]
fn description_depends_activate_nse_without_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "DESCRIPTION",
                b"Package: dependsfixture\nVersion: 0.0.0.9000\nDepends: dplyr\n",
            ),
            (
                "R/use.R",
                b"top <- function(df) count(df, service_request_type)\n",
            ),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    assert!(
        !stdout.contains("RY010"),
        "DESCRIPTION Depends must activate package NSE semantics: {stdout}"
    );
}

#[test]
fn package_namespace_import_from_binds_imported_value() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "DESCRIPTION",
                b"Package: namespacefixture\nVersion: 0.0.0.9000\n",
            ),
            ("NAMESPACE", b"importFrom(shiny,tags)\n"),
            ("R/use.R", b"page <- tags\noops <- genuinely_missing\n"),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
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
        write(
            &root,
            &[
                (
                    "DESCRIPTION",
                    format!("Package: {package}\nVersion: 0.0.0.9000\n").as_bytes(),
                ),
                ("R/use.R", b"page <- tags\n"),
            ],
        );
    }
    write(
        tmp.path(),
        &[
            ("with_import/NAMESPACE", b"importFrom(shiny,tags)\n"),
            ("without_import/NAMESPACE", b""),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
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
fn package_test_context_supplies_suggests_and_namespace_imports() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "DESCRIPTION",
                b"Package: contextfixture\nVersion: 0.0.0.9000\nSuggests: mirai\n",
            ),
            ("NAMESPACE", b"import(rlang)\n"),
            ("R/use.R", b"enquo\n"),
            ("tests/testthat/test-context.R", b"daemons\nenquo\n"),
        ],
    );
    // R CMD check executes `tests/` root scripts in the global environment
    // after `library(package)`: the namespace's wholesale imports stay
    // internal there, so the same bare name is genuinely unbound.
    fs::write(tmp.path().join("tests/spellcheck.R"), "enquo\n").unwrap();

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    assert!(
        !stdout.contains("variable `daemons` is not bound"),
        "stubbed Suggests must supply their names in tests: {stdout}"
    );
    assert!(
        !stdout.contains("test-context.R:1") && !stdout.contains("test-context.R:2"),
        "testthat runs tests/ in a clone of the package namespace, so import(rlang) must supply bare names there too: {stdout}"
    );
    assert!(
        stdout.contains("variable `enquo` is not bound") && stdout.contains("spellcheck.R"),
        "a tests/ root script runs outside the namespace: enquo must stay unbound there: {stdout}"
    );
}

#[test]
fn imported_rlang_capture_helper_is_recognized_globally() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "DESCRIPTION",
                b"Package: capturefixture\nVersion: 0.0.0.9000\n",
            ),
            ("NAMESPACE", b"importFrom(rlang,enquos)\n"),
            ("R/lst.R", b"lst <- function(...) enquos(...)\n"),
            ("tests/testthat/test-lst.R", b"lst(a = 1, b = a)\n"),
        ],
    );

    let output = ry_check(tmp.path());
    let stdout = output.stdout();
    assert!(
        !stdout.contains("variable `a` is not bound"),
        "global promise-capture metadata must quote lst's dots: {stdout}"
    );
}

/// Library selection seen through a package import: `R_LIBS` roots keep
/// their order, a project renv library precedes them, a missing root falls
/// back to the user and then the site library, and a `%v` placeholder picks
/// the current R version below its prefix. The consumer reports every name
/// its import does not supply.
#[test]
fn library_environment_selects_the_installed_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    let path = |name: &str| tmp.path().join(name);
    let package = path("consumer");
    let libraries = [
        ("user library", "user_export"),
        ("site library", "site_export"),
        ("unversioned", "direct_export"),
        ("versioned/4.3", "old_export"),
        ("versioned/4.4", "new_export"),
        ("consumer/renv/library", "renv_export"),
    ];
    for (library, export) in libraries {
        install_namespace(&path(library), "fixturepkg", export);
    }
    for version in ["4.3", "4.4"] {
        install_fixture_r_home(&path(&format!("r-home-{version}")), &format!("{version}.1"));
    }
    fs::create_dir_all(package.join("R")).unwrap();
    write(
        &package,
        &[
            ("DESCRIPTION", b"Package: consumer\nVersion: 0.1\n"),
            ("NAMESPACE", b"import(fixturepkg)\n"),
        ],
    );
    let source: String = libraries
        .iter()
        .map(|(_, name)| format!("x <- {name}\n"))
        .collect();
    fs::write(package.join("R/use.R"), source).unwrap();

    let missing = path("missing library").into_os_string();
    let roots = |names: &[&str]| std::env::join_paths(names.iter().map(|name| path(name))).unwrap();
    let placeholder = path("versioned/%v").into_os_string();
    // (environment, whether the project renv library exists, supplied export)
    let cases = [
        (
            vec![("R_LIBS", roots(&["user library", "site library"]))],
            false,
            "user_export",
        ),
        (
            vec![("R_LIBS", roots(&["site library", "user library"]))],
            false,
            "site_export",
        ),
        (
            vec![("R_LIBS", roots(&["user library"]))],
            true,
            "renv_export",
        ),
        (
            vec![
                ("R_LIBS", missing.clone()),
                ("R_LIBS_USER", roots(&["user library"])),
            ],
            false,
            "user_export",
        ),
        (
            vec![
                ("R_LIBS", missing.clone()),
                ("R_LIBS_USER", missing.clone()),
                ("R_LIBS_SITE", roots(&["site library"])),
            ],
            false,
            "site_export",
        ),
        (
            vec![("R_LIBS", path("unversioned/%v").into_os_string())],
            false,
            "direct_export",
        ),
        (
            vec![
                ("R_LIBS", placeholder.clone()),
                ("R_HOME", roots(&["r-home-4.3"])),
            ],
            false,
            "old_export",
        ),
        (
            vec![
                ("R_LIBS", placeholder.clone()),
                ("R_HOME", roots(&["r-home-4.4"])),
            ],
            false,
            "new_export",
        ),
    ];
    let renv = package.join("renv");
    for (env, with_renv, supplied) in cases {
        let renv_backup = path("renv-backup");
        if with_renv != renv.exists() {
            let (from, to) = if with_renv {
                (&renv_backup, &renv)
            } else {
                (&renv, &renv_backup)
            };
            fs::rename(from, to).unwrap();
        }
        // Only the child's environment is isolated, and only to these roots.
        let mut process = check()
            .args(["--output-format", "json"])
            .arg(&package)
            .env("HOME", &missing)
            .env("USERPROFILE", &missing);
        for key in [
            "R_LIBS",
            "R_LIBS_USER",
            "R_LIBS_SITE",
            "R_HOME",
            "LOCALAPPDATA",
            "APPDATA",
            "RY_NO_INSTALLED_LIBRARIES",
        ] {
            process = process.env_remove(key);
        }
        for (key, value) in &env {
            process = process.env(key, value);
        }
        let output = process.run();
        assert!(output.success(), "{output:?}");
        let expected: Vec<_> = libraries
            .iter()
            .map(|(_, name)| *name)
            .filter(|name| *name != supplied)
            .collect();
        assert_eq!(
            unbound_names(output.json().as_array().unwrap()),
            expected,
            "{env:?}"
        );
    }
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
    let stdout = output.stdout();
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
    write(
        tmp.path(),
        &[
            (
                "package/DESCRIPTION",
                b"Package: namespacefixture\nVersion: 0.0.0.9000\n",
            ),
            ("package/NAMESPACE", b"import(fixturepkg)\n"),
            ("package/R/use.R", b"x <- exported_value\n"),
        ],
    );

    let output = ry_check_with_r_lib(&tmp.path().join("package"), &r_lib);
    let stdout = output.stdout();
    assert!(
        !stdout.contains("RY010"),
        "import(fixturepkg) must bind dependency exports: {stdout}"
    );
}

#[test]
fn bundled_set6_imports_bind_exports_without_hiding_misspellings() {
    for (namespace, unbound) in [
        ("import(set6)\nimport(dictionar6)\n", vec!["Reaals"]),
        (
            "importFrom(set6, Set)\nimportFrom(dictionar6, Dictionary)\n",
            vec!["Reals", "Interval", "as.Set", "dct", "Reaals"],
        ),
        (
            "",
            vec![
                "Set",
                "Reals",
                "Interval",
                "Dictionary",
                "as.Set",
                "dct",
                "Reaals",
            ],
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            &[
                (
                    "DESCRIPTION",
                    b"Package: inventoryfixture\nVersion: 0.0.1\nImports: set6, dictionar6\n",
                ),
                ("NAMESPACE", namespace.as_bytes()),
                (
                    "R/use.R",
                    b"values <- list(Set, Reals, Interval, Dictionary, as.Set, dct, Reaals)\n",
                ),
            ],
        );

        let output = check()
            .args(["--output-format", "json"])
            .arg(tmp.path())
            .run();
        assert!(output.success(), "{output:?}");
        let diagnostics = output.json();
        let messages: Vec<_> = diagnostics
            .as_array()
            .unwrap()
            .iter()
            .map(|diagnostic| {
                assert_eq!(diagnostic["code"], "RY010", "{diagnostic}");
                diagnostic["message"].as_str().unwrap()
            })
            .collect();
        let expected: Vec<_> = unbound
            .iter()
            .map(|name| format!("variable `{name}` is not bound in this scope"))
            .collect();
        assert_eq!(messages, expected, "NAMESPACE: {namespace:?}");
    }
}

/// `packages = ["dplyr"]` in ry.toml makes a bare `filter()` resolve as
/// dplyr's NSE verb, so the `mtcars` column `mpg` is bound. Without the key
/// or a `library(dplyr)`, `filter()` takes regular resolution and `mpg` is
/// unbound.
#[test]
fn ry_toml_packages_enables_dplyr_nse() {
    for (config, expected) in [
        (None, vec!["mpg"]),
        (Some("packages = [\"dplyr\"]\n"), vec![]),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("use.R"),
            "df <- mtcars\nsmall <- filter(df, mpg > 0)\n",
        )
        .unwrap();
        if let Some(config) = config {
            fs::write(tmp.path().join("ry.toml"), config).unwrap();
        }
        let output = check()
            .args(["--output-format", "json"])
            .arg(tmp.path())
            .run();
        assert_eq!(
            unbound_names(output.json().as_array().unwrap()),
            expected,
            "ry.toml: {config:?}"
        );
    }
}

#[test]
fn full_output_reports_argument_type_mismatch_with_types() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("mismatch.R"), "mean(\"text\")\n").unwrap();
    let output = check()
        .cwd(tmp.path())
        .args(["--output-format", "full", "mismatch.R"])
        .run();
    let stdout = output.stdout();
    assert!(!output.success(), "{stdout}");
    assert!(stdout.contains("[RY092]"), "{stdout}");
    assert!(
        stdout.contains("argument `x` to `mean` is `character`, expected numeric"),
        "{stdout}"
    );
}

#[test]
fn cyclic_serialized_data_keeps_bindings_and_unbound_diagnostics() {
    // Generated from two distinct self-referential R environments wrapped
    // in pairlists. The decoder must not compare their cyclic graphs in dedup.
    // Regenerate in tests/e2e/fixtures/ with R:
    // a <- new.env(parent = emptyenv()); a$self <- a
    // b <- new.env(parent = emptyenv()); b$self <- b
    // x <- pairlist(a); y <- pairlist(b)
    // save(x, y, file = "cyclic-pairlists.rda", version = 2, compress = FALSE)
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path();
    write(
        pkg,
        &[
            ("DESCRIPTION", b"Package: cycleprobe\nLazyData: true\n"),
            ("R/probe.R", b"x\ny\nnot_a_bound_cycle_symbol\n"),
            (
                "data/cyclic-pairlists.rda",
                include_bytes!("fixtures/cyclic-pairlists.rda"),
            ),
        ],
    );
    let output = check()
        .args([
            pkg.to_str().unwrap(),
            "--output-format",
            "json",
            "--exit-zero",
        ])
        .run();
    assert!(output.success(), "checker failed: {}", output.stderr());
    let diagnostics = output.json();
    assert_eq!(diagnostics.as_array().unwrap().len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["code"], "RY010");
    assert_eq!(diagnostics[0]["line"], 3);
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("not_a_bound_cycle_symbol")
    );
    assert!(!output.stderr().contains("degraded scope"));
}

/// Serialized data that cannot be enumerated degrades only its own scope:
/// the file-stem binding stands in and RY010 stays live elsewhere. The
/// notice names each offending file once, on stderr, and only for human
/// output or `--statistics`, so the JSON stream stays stable.
#[test]
fn serialized_inventory_failures_are_reported_and_repair_restores_bindings() {
    const USE: (&str, &[u8]) = ("R/use.R", b"ok <- sysdata\nbad <- genuinely_missing\n");
    const CAP: &str = "max-serialized-bytes = 1\n";
    // (ry.toml, files, degraded file and reason, names reported unbound)
    type Case<'a> = (
        &'a str,
        Files<'a>,
        Option<(&'a str, &'a str)>,
        &'a [&'a str],
    );
    let cases: [Case; 6] = [
        (
            CAP,
            &[USE, ("R/sysdata.rda", b"over-cap-bytes")],
            Some(("sysdata.rda", "decoded-byte limit exceeded")),
            &["genuinely_missing"],
        ),
        // Every file in the package reads the scope, one notice in total.
        (
            CAP,
            &[
                ("R/sysdata.rda", b"over-cap-bytes"),
                ("R/a.R", b"x <- sysdata\n"),
                ("R/b.R", b"x <- sysdata\n"),
                ("R/c.R", b"x <- sysdata\n"),
            ],
            Some(("sysdata.rda", "decoded-byte limit exceeded")),
            &[],
        ),
        (
            "",
            &[
                USE,
                (
                    "R/sysdata.rda",
                    include_bytes!("../../../../testdata/serialized/nested-limit.rda"),
                ),
            ],
            Some(("sysdata.rda", "serialized parser resource limit exceeded")),
            &["genuinely_missing"],
        ),
        // A repaired, known empty inventory binds nothing.
        (
            "",
            &[
                USE,
                (
                    "R/sysdata.rda",
                    include_bytes!("../../../../testdata/serialized/empty.rda"),
                ),
            ],
            None,
            &["sysdata", "genuinely_missing"],
        ),
        // A real R-produced empty workspace in ASCII serialization. Static
        // XDR inventory cannot read it, but its valid envelope must not be
        // mislabeled as corrupt.
        (
            "",
            &[
                USE,
                (
                    "R/sysdata.rda",
                    include_bytes!("../../../../testdata/serialized/empty-ascii.rda"),
                ),
            ],
            Some(("sysdata.rda", "unsupported serialized input")),
            &["sysdata", "genuinely_missing"],
        ),
        // A missing `load()` target is unreadable. An absent optional
        // `sysdata.rda` is normal.
        (
            "",
            &[
                ("R/use.R", b"load('missing.rda')\n"),
                ("R/other.R", b"genuinely_missing\n"),
            ],
            Some(("missing.rda", "serialized input could not be read")),
            &["genuinely_missing"],
        ),
    ];
    for (config, files, degraded, unbound) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path();
        write(
            pkg,
            &[
                ("DESCRIPTION", b"Package: fixture\n"),
                ("ry.toml", config.as_bytes()),
            ],
        );
        write(pkg, files);
        let assert_notice = |stderr: &str| match degraded {
            Some((file, reason)) => {
                assert!(stderr.contains("1 degraded scope(s)"), "{stderr}");
                let notes: Vec<_> = stderr
                    .lines()
                    .filter(|line| line.starts_with("  - "))
                    .collect();
                assert_eq!(notes.len(), 1, "{stderr}");
                assert!(
                    notes[0].ends_with(&format!("{file} ({reason})")),
                    "{stderr}"
                );
            }
            None => assert!(!stderr.contains("degraded"), "{stderr}"),
        };
        let json = check_in(pkg, &["R", "--output-format", "json", "--exit-zero"]);
        assert_eq!(
            unbound_names(json.json().as_array().unwrap()),
            unbound,
            "{degraded:?}"
        );
        assert!(json.stderr().is_empty(), "{json:?}");
        let statistics = check_in(pkg, &["R", "--output-format", "json", "--statistics"]);
        assert_notice(&statistics.stderr());
        let human = check_in(pkg, &["R"]);
        assert_notice(&human.stderr());
        assert!(!human.stdout().contains("degraded"), "{human:?}");
        let facts = ry!().cwd(pkg).args(["dump-facts", "R"]).run().json();
        let scopes = &facts["contexts"][0]["inputs"]["degraded_scopes"];
        match degraded {
            Some((_, reason)) => {
                assert_eq!(scopes.as_array().unwrap().len(), 1, "{scopes}");
                assert_eq!(scopes[0][1], reason);
            }
            None => assert_eq!(scopes, &json!([])),
        }
    }
}

/// `useDynLib(pkg, .registration = TRUE)` binds every routine in
/// the package's `R_registerRoutines` table into the namespace. ry does not
/// read `src/`, so a name the package itself passes as an FFI entry point is
/// the evidence that it is one of them — which is what lets rlang's
/// `capture_arg = ffi_enquo` resolve, eight `R/nse-defuse.R` findings that
/// the call-position rule alone cannot reach.
#[test]
fn registered_native_symbols_resolve_in_value_position() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path();
    write(
        pkg,
        &[
            ("DESCRIPTION", b"Package: ffi\n"),
            (
                "R/defuse.R",
                b"enquo <- function(arg) .Call(ffi_enquo, substitute(arg))\n\
         enquos <- function(...) endots(capture_arg = ffi_enquo)\n\
         bad <- function() genuinely_missing_xyz\n",
            ),
        ],
    );

    fs::write(
        pkg.join("NAMESPACE"),
        "useDynLib(ffi, .registration = TRUE)\n",
    )
    .unwrap();
    let declared = check().args([pkg.to_str().unwrap()]).run();
    let declared_stdout = declared.stdout();
    assert!(
        !declared_stdout.contains("ffi_enquo"),
        "a symbol proven native by .Call must resolve in value position too: {declared_stdout}"
    );
    assert!(
        declared_stdout.contains("genuinely_missing_xyz"),
        "the witness set must not disable RY010 generally: {declared_stdout}"
    );

    // Negative control: without the declaration the value-position use is an
    // ordinary unbound read again. (The `.Call` entry-point argument stays
    // suppressed either way — that is the long-standing call-position rule.)
    fs::write(pkg.join("NAMESPACE"), "export(enquo)\n").unwrap();
    let undeclared = check().args([pkg.to_str().unwrap(), "--exit-zero"]).run();
    let undeclared_stdout = undeclared.stdout();
    assert!(
        undeclared_stdout.contains("ffi_enquo"),
        "without .registration the symbol is not proven bound: {undeclared_stdout}"
    );
}

#[test]
fn empty_discovery_preserves_output_format_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: dataonly\nVersion: 1.0\n",
    )
    .unwrap();
    for format in ["json", "gitlab", "junit", "github", "full", "concise"] {
        let output = check()
            .cwd(tmp.path())
            .args([".", "--output-format", format])
            .run();
        assert!(output.success(), "{format}: {output:?}");
        assert!(output.stderr().contains("no .R / .r files found"));
        match format {
            "json" | "gitlab" => {
                assert_eq!(output.json(), json!([]), "{format}");
            }
            "junit" => {
                let stdout = output.stdout();
                assert!(stdout.contains("<?xml"), "{stdout}");
                assert!(stdout.contains("tests=\"0\""), "{stdout}");
                assert!(stdout.contains("failures=\"0\""), "{stdout}");
                assert!(stdout.contains("</testsuites>"), "{stdout}");
            }
            _ => assert!(output.stdout().is_empty(), "{format}: {output:?}"),
        }
    }
}

#[test]
fn excluded_sources_still_emit_json_document() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            (
                "ry.toml",
                b"exclude = [\"*.R\"]\noutput-format = \"json\"\n",
            ),
            ("excluded.R", b"1 + 'bad'\n"),
        ],
    );
    let output = check().cwd(tmp.path()).args(["."]).run();
    assert!(output.success());
    assert!(output.stderr().contains("no .R / .r files found"));
    let value = output.json();
    assert_eq!(value, json!([]));
}

#[test]
fn explain_files_keeps_json_valid_and_reports_deliberate_reinclusion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        &[
            ("DESCRIPTION", b"Package: example\n"),
            (".Rbuildignore", b"^vignettes$\n"),
            ("vignettes/broken.R", b"x <- )\n"),
        ],
    );
    let run = || {
        check()
            .cwd(root)
            .args([".", "--explain-files", "--output-format", "json"])
            .run()
    };
    let skipped = run();
    let json = skipped.json();
    assert_eq!(json, json!([]));
    assert!(skipped.stderr().contains("(.Rbuildignore)"));
    fs::write(
        root.join("ry.toml"),
        "include-build-ignored = ['vignettes/*.R']\n",
    )
    .unwrap();
    let included = run();
    let json = included.json();
    assert!(
        json.as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "RY000")
    );
    assert!(included.stderr().contains("include ./vignettes/broken.R"));
}

#[test]
fn explicitly_requested_build_ignored_directory_still_gets_checked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        &[
            ("DESCRIPTION", b"Package: example\n"),
            (".Rbuildignore", b"^vignettes$\n"),
            ("vignettes/doc.R", b"y <- )\n"),
        ],
    );
    let output = check()
        .cwd(root)
        .args(["vignettes", "--explain-files", "--output-format", "json"])
        .run();
    assert!(!output.success());
    let diagnostics = output.json();
    assert!(
        diagnostics
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "RY000"),
        "{diagnostics}"
    );
    assert!(output.stderr().contains("include vignettes/doc.R"));
}

/// Missing and unreadable inputs fail the run, named on stderr, and stdout
/// stays a well-formed report. A missing input aborts before anything is
/// checked. An unreadable directory fails the exit code while readable
/// inputs are still checked. `--exit-zero` defuses both.
#[test]
fn missing_and_unreadable_inputs_fail_the_run() {
    let tmp = tempfile::tempdir().unwrap();
    let path = |name: &str| tmp.path().join(name);
    write(
        tmp.path(),
        &[
            ("valid.R", b"genuinely_missing\n"),
            ("locked/inside.R", b"also_missing\n"),
        ],
    );
    let missing = |name: &str| format!("{}: no such file or directory", path(name).display());
    // (inputs, stderr fragments, names reported unbound)
    let mut cases = vec![
        (
            vec![path("nonexistent.R")],
            vec![missing("nonexistent.R")],
            vec![],
        ),
        (
            vec![path("missing_dir")],
            vec![missing("missing_dir")],
            vec![],
        ),
        (
            vec![path("valid.R"), path("nope1.R"), path("nope2.R")],
            vec![missing("nope1.R"), missing("nope2.R")],
            vec![],
        ),
    ];
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        // Root lists mode-0 directories anyway.
        if fs::read_dir(path("locked")).is_err() {
            let named = vec![format!("ry: {}", path("locked").display())];
            cases.push((vec![path("locked")], named.clone(), vec![]));
            cases.push((
                vec![path("valid.R"), path("locked")],
                named,
                vec!["genuinely_missing"],
            ));
        }
    }
    for (inputs, named, unbound) in cases {
        for (args, code) in [(&[][..], 1), (&["--exit-zero"][..], 0)] {
            let output = check()
                .args(["--output-format", "json"])
                .args(args)
                .args(&inputs)
                .run();
            assert_eq!(output.code(), Some(code), "{inputs:?} {args:?}: {output:?}");
            let stderr = output.stderr();
            for fragment in &named {
                assert!(stderr.contains(fragment), "{inputs:?}: {stderr}");
            }
            assert!(!stderr.contains("no .R / .r files found"), "{stderr}");
            assert_eq!(unbound_names(output.json().as_array().unwrap()), unbound);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn c_registration_inventory_resolves_wrapper_arguments_and_keeps_typos() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        &[
            ("DESCRIPTION", b"Package: example\n"),
            ("src/init.c", br#"
#define CALLDEF(name, n) { #name, (DL_FUNC)&name, n }
static const R_CallMethodDef calls[] = { CALLDEF(entry, 1), {"native_alias_xyz", (DL_FUNC)&implementation, 0}, {NULL, NULL, 0} };
void R_init_example(DllInfo *dll) { R_registerRoutines(dll, NULL, calls, NULL, NULL); }
"#),
            ("R/wrapper.R", b"wrapper <- function(ptr, ...) .Call(cleancall_call, pairlist(ptr, ...))\nrun <- function() { wrapper(C_entry); wrapper(C_native_alias_xyz); wrapper(C_typo); wrapper(other_missing) }\n"),
            ("NAMESPACE", b"useDynLib(example, .registration = TRUE, .fixes = 'C_')\n"),
        ],
    );
    let declared = ry_check(root);
    let stdout = declared.stdout();
    assert!(
        !stdout.contains("`C_entry`") && !stdout.contains("`C_native_alias_xyz`"),
        "{stdout}"
    );
    // Prefix handling has its own existing fallback; use an unrelated name
    // as the control that wrapper arguments remain ordinary value reads.
    assert!(stdout.contains("other_missing"), "{stdout}");
    write(
        root,
        &[
            ("NAMESPACE", b"useDynLib(example, .registration = TRUE)\n"),
            ("R/wrapper.R", b"wrapper <- function(ptr, ...) .Call(cleancall_call, pairlist(ptr, ...))\nrun <- function() { wrapper(entry); wrapper(native_alias_xyz); wrapper(typo); wrapper(other_missing) }\n"),
        ],
    );
    let declared = ry_check(root);
    let stdout = declared.stdout();
    assert!(
        !stdout.contains("`entry`") && !stdout.contains("`native_alias_xyz`"),
        "{stdout}"
    );
    assert!(
        stdout.contains("`typo`") && stdout.contains("other_missing"),
        "{stdout}"
    );
    fs::write(root.join("NAMESPACE"), "useDynLib(example)\n").unwrap();
    let undeclared = ry_check(root);
    let stdout = undeclared.stdout();
    assert!(
        stdout.contains("`entry`") && stdout.contains("`native_alias_xyz`"),
        "{stdout}"
    );
}

// --- Baselines (--write-baseline with a configured `baseline`) ---

/// Read and parse a baseline file written by `--write-baseline`.
fn read_baseline(path: &Path) -> serde_json::Value {
    let contents = fs::read_to_string(path).unwrap();
    serde_json::from_str(&contents).unwrap()
}

/// With `baseline` set in ry.toml, `--write-baseline` snapshots what the run
/// reports instead of subtracting the old file first. Regenerating an
/// unchanged project is a no-op, a plain check subtracts recorded counts,
/// and regeneration drops fixed findings and adds new ones.
#[test]
fn configured_baseline_regenerates_and_subtracts_counts() {
    let tmp = tempfile::tempdir().unwrap();
    // RY040's message names only the operand modes, so both lines share
    // one entry and its count is exercised.
    write(
        tmp.path(),
        &[
            ("ry.toml", b"baseline = \"ry-baseline.json\"\n"),
            ("bad.R", b"x <- \"a\" + 1L\ny <- \"a\" + 1L\n"),
        ],
    );
    let regenerate = || {
        let output = check_in(tmp.path(), &["--write-baseline", "ry-baseline.json", "."]);
        // The run reports, and fails on, the error findings it writes.
        assert_eq!(output.code(), Some(1), "{output:?}");
        read_baseline(&tmp.path().join("ry-baseline.json"))
    };
    let entries = |baseline: &serde_json::Value| {
        baseline["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry["path"].as_str().unwrap().to_owned(),
                    entry["code"].clone(),
                    entry["count"].clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let first = regenerate();
    assert_eq!(
        entries(&first),
        [("./bad.R".to_owned(), json!("RY040"), json!(2))],
        "{first}"
    );
    assert_eq!(
        regenerate(),
        first,
        "regenerating an unchanged project is a no-op"
    );

    let plain = check_in(tmp.path(), &["--output-format", "json", "."]);
    assert!(plain.success(), "{plain:?}");
    assert_eq!(plain.json(), json!([]));

    // A third occurrence exceeds the recorded count.
    write(
        tmp.path(),
        &[(
            "bad.R",
            b"x <- \"a\" + 1L\ny <- \"a\" + 1L\nz <- \"a\" + 1L\n",
        )],
    );
    let surplus = check_in(tmp.path(), &["--output-format", "json", "."]);
    assert_eq!(surplus.code(), Some(1), "{surplus:?}");
    assert_eq!(surplus.codes(), ["RY040"]);

    // Fix bad.R and introduce the finding in a new file.
    write(
        tmp.path(),
        &[
            ("bad.R", b"x <- 1L + 2L\n"),
            ("new.R", b"y <- \"a\" + 1L\n"),
        ],
    );
    let fixed = regenerate();
    assert_eq!(
        entries(&fixed),
        [("./new.R".to_owned(), json!("RY040"), json!(1))],
        "{fixed}"
    );
}

/// A bare `ry` runs `ry check` in the working directory, global flags
/// included.
#[test]
fn bare_ry_runs_check() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), &[("bad.R", b"x <- \"a\" + 1L\n")]);
    let run = |args: &[&str]| ry!().cwd(tmp.path()).args(args).run();
    let bare = run(&[]);
    assert_eq!(bare.code(), Some(1), "{bare:?}");
    assert!(bare.stdout().contains("RY040"), "{bare:?}");
    assert_eq!(bare, run(&["check"]));
    assert_eq!(run(&["-q"]), run(&["check", "-q"]));
}

/// `exclude` skips `fixtures/bad.R`, whose RY040 error would fail the run,
/// for absolute and relative roots.
#[test]
fn ry_toml_exclude_patterns_skip_matched_files() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        &[
            ("ry.toml", b"exclude = [\"fixtures/**\"]\n"),
            ("fixtures/bad.R", b"x <- \"a\" + 1L\n"),
            ("good.R", b"x <- 1L + 2L\n"),
        ],
    );
    let absolute = tmp.path().to_str().unwrap();
    for root in [absolute, ".", "./fixtures"] {
        let output = check()
            .cwd(tmp.path())
            .args(["--output-format", "json", root])
            .run();
        assert_eq!(output.json(), json!([]), "{root:?}: {output:?}");
        assert!(output.success(), "{root:?}: {output:?}");
    }
}
