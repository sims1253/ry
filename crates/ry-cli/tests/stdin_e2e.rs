//! End-to-end checks for one explicit stdin source overlay.

use std::fs;
use std::path::Path;

use ry_testkit::{CliOutput, ry};
use serde_json::json;

/// `ry check - --stdin-filename <logical> --output-format json <extra>`.
fn stdin(root: &Path, logical: &str, extra: &[&str], input: &[u8]) -> CliOutput {
    ry!()
        .cwd(root)
        .args(["check", "-", "--stdin-filename", logical])
        .args(["--output-format", "json"])
        .args(extra)
        .stdin(input)
        .run()
}

/// `ry check <operands> --output-format json`.
fn disk(root: &Path, operands: &[&str]) -> CliOutput {
    ry!()
        .cwd(root)
        .arg("check")
        .args(operands)
        .args(["--output-format", "json"])
        .run()
}

fn write(root: &Path, files: &[(&str, &[u8])]) {
    for (path, contents) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

#[test]
fn identical_buffer_matches_disk_with_package_and_neighbor() {
    let temp = tempfile::tempdir().unwrap();
    let source = b"result <- \"a\" + 1L\n";
    write(
        temp.path(),
        &[
            ("DESCRIPTION", b"Package: demo\nVersion: 0.1\n"),
            ("R/helper.R", b"unknown_symbol\n"),
            ("R/main.R", source),
        ],
    );
    let disk = disk(temp.path(), &["R/main.R", "R/helper.R"]);
    let buffer = stdin(temp.path(), "R/main.R", &["R/helper.R"], source);
    assert_eq!(buffer, disk);
    assert_eq!(
        buffer.findings(),
        [
            ["R/helper.R", "RY010", "warning"],
            ["R/main.R", "RY040", "error"],
        ]
    );
}

#[test]
fn logical_filename_selects_the_path_rule_policy() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        &[
            (
                "ry.toml",
                "warn = ['RY040']\n[[rule-overrides]]\npaths = ['R/**', 'mémoire.R']\nerror = ['RY040']\n"
                    .as_bytes(),
            ),
            ("R/sub/.keep", b""),
            ("R/saved.R", b"\"a\" + 1L\n"),
        ],
    );
    assert_eq!(
        stdin(temp.path(), "R/saved.R", &[], b"\"a\" + 1L\n"),
        disk(temp.path(), &["R/saved.R"])
    );
    for logical in [
        "R/saved.R",
        "R/new.R",
        "R/sub/../new.R",
        "mémoire.R",
        "other.R",
    ] {
        let expected = if logical == "other.R" {
            "warning"
        } else {
            "error"
        };
        let output = stdin(temp.path(), logical, &[], b"\"a\" + 1L\n");
        assert_eq!(
            output.json()[0]["severity"],
            expected,
            "{logical}: {output:?}"
        );
    }
    assert!(!temp.path().join("R/new.R").exists());
    assert!(!temp.path().join("mémoire.R").exists());
}

#[test]
fn overlay_replaces_stale_disk_copy_in_directory_scan() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        &[
            ("R/main.R", b"\"stale\" + 1L\n"),
            ("R/helper.R", b"unknown_symbol\n"),
        ],
    );
    let buffer = stdin(
        temp.path(),
        "R/./main.R",
        &[".", "--explain-files"],
        b"updated <- 1L\n",
    );
    assert!(buffer.success(), "{buffer:?}");
    assert_eq!(buffer.codes(), ["RY010"]);
    let stderr = buffer.stderr();
    assert_eq!(
        stderr.matches("include ./R/main.R (stdin overlay)").count(),
        1,
        "{stderr}"
    );
    assert_eq!(
        fs::read(temp.path().join("R/main.R")).unwrap(),
        b"\"stale\" + 1L\n"
    );
}

#[test]
fn typehint_adoption_uses_the_stdin_overlay_at_its_native_path() {
    let temp = tempfile::tempdir().unwrap();
    let disk_source = "f <- function(x) {\n #| x character\n x\n}\nf('ok')\n";
    write(
        temp.path(),
        &[
            (
                "ry.toml",
                b"[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
            ),
            ("R/main.R", disk_source.as_bytes()),
        ],
    );
    for path in ["R/main.R", "R/unsaved.R"] {
        for (class, mismatches) in [("integer", 1), ("character", 0)] {
            let source = format!("f <- function(x) {{\n #| x {class}\n x\n}}\nf('text')\n");
            let output = stdin(temp.path(), path, &[], source.as_bytes());
            assert!(output.success(), "{output:?}");
            assert_eq!(
                output
                    .codes()
                    .iter()
                    .filter(|code| *code == "RY114")
                    .count(),
                mismatches,
                "path: {path}, class: {class}"
            );
        }
    }
    assert_eq!(
        fs::read_to_string(temp.path().join("R/main.R")).unwrap(),
        disk_source
    );
    assert!(!temp.path().join("R/unsaved.R").exists());
}

#[test]
fn unsaved_file_is_checked_without_writing_it() {
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), &[("R/helper.R", b"unknown_symbol\n")]);
    let buffer = stdin(temp.path(), "R/new.R", &["."], b"\"buffer\" + 1L\n");
    assert_eq!(buffer.code(), Some(1), "{buffer:?}");
    assert_eq!(buffer.codes(), ["RY010", "RY040"]);
    // Spelled below the directory root so it groups with its neighbours.
    assert_eq!(buffer.json()[1]["path"], "./R/new.R");
    assert!(!temp.path().join("R/new.R").exists());

    // stdin is the only source of an unsaved file; naming it again as a
    // disk operand is a missing input like any other.
    let duplicate = stdin(temp.path(), "R/new.R", &["R/new.R"], b"\"buffer\" + 1L\n");
    assert_eq!(duplicate.code(), Some(1), "{duplicate:?}");
    assert_eq!(duplicate.json(), json!([]));
    assert!(duplicate.stderr().contains("R/new.R"), "{duplicate:?}");
}

#[test]
fn missing_roots_keep_disk_output_and_exit_policy_with_stdin() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("blocker"), "not a directory").unwrap();
    for missing in ["blocker/missing.R", "missing/child.R"] {
        for exit_zero in [&[][..], &["--exit-zero"][..]] {
            let operands = [&[missing][..], exit_zero].concat();
            let disk = disk(temp.path(), &operands);
            let buffer = stdin(temp.path(), "source.R", &operands, b"x <- 1L\n");
            assert_eq!(disk.success(), !exit_zero.is_empty());
            assert_eq!(disk.json(), json!([]));
            assert!(disk.stderr().contains(missing), "{disk:?}");
            assert_eq!(buffer, disk);
        }
    }
}

/// A buffer replaces its stale disk copy everywhere the package context
/// reads sources: dynamic bindings, `data/` scripts, and test helpers.
#[test]
fn package_context_uses_stdin_instead_of_stale_disk() {
    struct Case {
        files: &'static [(&'static str, &'static [u8])],
        target: &'static str,
        spellings: &'static [&'static str],
        neighbors: &'static [&'static str],
        /// (disk content before the run, buffer, expected codes)
        steps: &'static [(Option<&'static str>, &'static str, &'static [&'static str])],
    }
    const DATA: &[(&str, &[u8])] = &[
        ("DESCRIPTION", b"Package: demo\nLazyData: true\n"),
        ("R/neighbor.R", b"gold\nsilver\ncopper\nbronze\nplatinum\n"),
        ("data/other.R", b"silver <- 1L\n"),
        ("data/copper.rds", b""),
        (
            "data/bronze.rda",
            include_bytes!("../../../testdata/serialized/empty.rda"),
        ),
        (
            "data/platinum.RData",
            include_bytes!("../../../testdata/serialized/empty-ascii.rda"),
        ),
    ];
    let cases = [
        Case {
            files: &[("DESCRIPTION", b"Package: demo\n")],
            target: "R/main.R",
            spellings: &["R/main.R"],
            neighbors: &[],
            steps: &[
                (Some("assign(\"ghost\", 1L)\n"), "ghost\n", &["RY010"]),
                (None, "assign(\"ghost\", 1L)\nghost\n", &[]),
            ],
        },
        Case {
            files: &[
                ("DESCRIPTION", b"Package: demo\n"),
                ("R/neighbor.R", b"ghost\n"),
            ],
            target: "R/main.R",
            spellings: &["R/main.R", "./R/main.R"],
            neighbors: &["R/neighbor.R"],
            steps: &[(Some("assign(\"ghost\", 1L)\n"), "1L\n", &["RY010"])],
        },
        Case {
            files: DATA,
            target: "data/preprocess.r",
            spellings: &["data/preprocess.r", "./data/preprocess.r"],
            neighbors: &["R/neighbor.R"],
            steps: &[
                (Some("gold <- 1L\n"), "1L\n", &["RY010"]),
                (Some("1L\n"), "gold <- 1L\n", &[]),
                (None, "gold <- 1L\n", &[]),
            ],
        },
        Case {
            files: &[
                ("DESCRIPTION", b"Package: demo\n"),
                ("tests/testthat/test-main.R", b"ghost\n"),
            ],
            target: "tests/testthat/helper-main.R",
            spellings: &[
                "tests/testthat/helper-main.R",
                "./tests/testthat/helper-main.R",
            ],
            neighbors: &["tests/testthat/test-main.R"],
            steps: &[
                (Some("ghost <- 1L\n"), "1L\n", &["RY010"]),
                (None, "ghost <- 1L\n", &[]),
            ],
        },
    ];
    for case in cases {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), case.files);
        let path = temp.path().join(case.target);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for &(old, source, expected) in case.steps {
            match old {
                Some(old) => fs::write(&path, old).unwrap(),
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
            let buffers: Vec<_> = case
                .spellings
                .iter()
                .map(|logical| stdin(temp.path(), logical, case.neighbors, source.as_bytes()))
                .collect();
            assert_eq!(fs::read_to_string(&path).ok().as_deref(), old);
            fs::write(&path, source).unwrap();
            let disk = disk(temp.path(), &[case.neighbors, &[case.target]].concat());
            assert_eq!(disk.codes(), expected, "{}: {disk:?}", case.target);
            for buffer in buffers {
                assert_eq!(buffer, disk, "{}", case.target);
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn symlink_alias_replaces_its_target() {
    let cases: [(&[(&str, &[u8])], &str, &str, &[&str], &[u8], &str); 2] = [
        (
            &[
                ("DESCRIPTION", b"Package: demo\nLazyData: true\n"),
                ("R/neighbor.R", b"gold\n"),
                ("data/preprocess.r", b"gold <- 1L\n"),
            ],
            "data/preprocess.r",
            "data/alias.R",
            &["R/neighbor.R"],
            b"1L\n",
            "R/neighbor.R",
        ),
        (
            &[
                ("DESCRIPTION", b"Package: demo\n"),
                ("R/main.R", b"assign(\"ghost\", 1L)\n"),
            ],
            "R/main.R",
            "R/alias.R",
            &[],
            b"ghost\n",
            "R/alias.R",
        ),
    ];
    for (files, target, alias, neighbors, input, unbound_at) in cases {
        let temp = tempfile::tempdir().unwrap();
        write(temp.path(), files);
        let link_target = Path::new(target).file_name().unwrap();
        std::os::unix::fs::symlink(link_target, temp.path().join(alias)).unwrap();
        let before = fs::read(temp.path().join(target)).unwrap();
        let buffer = stdin(temp.path(), alias, neighbors, input);
        assert_eq!(
            buffer.findings(),
            [[unbound_at, "RY010", "warning"]],
            "{alias}"
        );
        assert_eq!(fs::read(temp.path().join(target)).unwrap(), before);
    }
}

#[test]
fn explicit_stdin_errors_and_watch_rejection_are_clear() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("directory")).unwrap();
    for (args, expected) in [
        (&["-"][..], "requires --stdin-filename"),
        (&["-", "-", "--stdin-filename", "a.R"], "only once"),
        (&["--stdin-filename", "a.R"], "requires a `-` input"),
        (
            &["-", "--stdin-filename", "a.R", "--watch"],
            "--watch cannot be used with stdin",
        ),
        (
            &["-", "--stdin-filename", "directory"],
            "must name a source file, not a directory",
        ),
    ] {
        let output = ry!()
            .cwd(temp.path())
            .arg("check")
            .args(args)
            .stdin("x <- 1L\n")
            .run();
        assert_eq!(output.code(), Some(1), "{output:?}");
        assert!(output.stderr().contains(expected), "{output:?}");
        assert!(output.stdout().is_empty(), "{output:?}");
    }
}

#[cfg(unix)]
#[test]
fn symlinked_parent_dotdot_replaces_the_actual_source() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        &[
            ("a/b/.keep", b""),
            ("a/foo.R", b"\"stale\" + 1L\n"),
            ("foo.R", b"\"neighbor\" + 2L\n"),
        ],
    );
    std::os::unix::fs::symlink("a/b", temp.path().join("link")).unwrap();
    let run = |logical| {
        stdin(
            temp.path(),
            logical,
            &[".", "--explain-files"],
            b"updated <- 1L\n",
        )
    };
    let through_link = run("link/../foo.R");
    let direct = run("a/foo.R");
    assert_eq!(through_link.status(), direct.status());
    assert_eq!(through_link.json(), direct.json());
    assert_eq!(through_link.findings(), [["./foo.R", "RY040", "error"]]);
    let explained = through_link.stderr();
    assert!(
        explained.contains("include ./a/foo.R (stdin overlay)"),
        "{explained}"
    );

    // The final file may be unsaved while its symlinked parent already
    // exists; it must still use the parent's filesystem meaning.
    assert_eq!(run("link/../new.R").json(), run("a/new.R").json());
    assert!(!temp.path().join("a/new.R").exists());
}

#[test]
fn empty_parse_error_unicode_crlf_and_invalid_bytes_use_file_pipeline() {
    let temp = tempfile::tempdir().unwrap();
    let empty = stdin(temp.path(), "source.R", &[], b"");
    assert!(empty.success(), "{empty:?}");
    assert_eq!(empty.json(), json!([]));
    assert!(!empty.stderr().contains("no .R"));

    for source in [
        "café <- 1L\r\n\"x\" + 1L\r\n".as_bytes(),
        b"x <- (\n".as_slice(),
        b"caf\xe9 <- 1L\n".as_slice(),
        b"\xef\xbb\xbfx <- 1L\n".as_slice(),
    ] {
        fs::write(temp.path().join("source.R"), source).unwrap();
        let disk = disk(temp.path(), &["source.R"]);
        let buffer = stdin(temp.path(), "source.R", &[], source);
        assert_eq!(buffer.status(), disk.status(), "{source:?}");
        assert_eq!(buffer.json(), disk.json(), "{source:?}");
    }
    assert_eq!(
        stdin(temp.path(), "source.R", &[], b"caf\xe9 <- 1L\n").codes(),
        ["RY000"]
    );
}

#[test]
fn stdin_size_cap_and_read_error_fail_with_empty_machine_report() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("ry.toml"), "[index]\nmax-file-bytes = 8\n").unwrap();
    for (input, extra, success) in [
        (&b"x <- 1L\n"[..], &[][..], true),
        (b"x <- 1L\nX", &[], false),
        (b"x <- 1L\nX", &["--exit-zero"], true),
    ] {
        let output = stdin(temp.path(), "source.R", extra, input);
        assert_eq!(output.success(), success, "{output:?}");
        assert_eq!(output.json(), json!([]));
        if input.len() > 8 {
            assert!(output.stderr().contains("index.max-file-bytes"));
        }
    }

    // A directory file descriptor is readable to open on Unix, but its
    // first read fails. This exercises the actual stdin I/O error branch.
    #[cfg(unix)]
    {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_ry"))
            .current_dir(temp.path())
            .args(["check", "-", "--stdin-filename", "source.R"])
            .args(["--output-format", "json"])
            .stdin(fs::File::open(temp.path()).unwrap())
            .output()
            .unwrap();
        let output = CliOutput(output);
        assert_eq!(output.code(), Some(1), "{output:?}");
        assert_eq!(output.json(), json!([]));
        assert!(output.stderr().contains("stdin for source.R"));
    }
}

#[test]
fn excluded_file_is_explicitly_checked_and_baseline_applies() {
    let temp = tempfile::tempdir().unwrap();
    let source = b"\"a\" + 1L\n";
    write(
        temp.path(),
        &[
            ("ry.toml", b"exclude = [\"R/ignored.R\"]\n"),
            ("R/ignored.R", source),
        ],
    );
    // An absolute root: relative roots do not apply `exclude` today.
    let directory = disk(temp.path(), &[temp.path().to_str().unwrap()]);
    assert!(directory.success());
    assert_eq!(directory.json(), json!([]));

    let stdin_run = stdin(temp.path(), "R/ignored.R", &[], source);
    assert_eq!(stdin_run.json(), disk(temp.path(), &["R/ignored.R"]).json());
    assert_eq!(stdin_run.codes(), ["RY040"]);

    let written = stdin(
        temp.path(),
        "R/ignored.R",
        &["--write-baseline", "accepted.json"],
        source,
    );
    assert_eq!(written.code(), Some(1));
    assert!(temp.path().join("accepted.json").is_file());
    let baseline = ["--baseline", "accepted.json"];
    let suppressed = stdin(temp.path(), "R/ignored.R", &baseline, source);
    assert!(suppressed.success(), "{suppressed:?}");
    assert_eq!(suppressed.json(), json!([]));
    let changed = stdin(temp.path(), "R/ignored.R", &baseline, b"unknown_symbol\n");
    assert_eq!(changed.codes(), ["RY010"]);
}

#[test]
fn warning_and_exit_zero_flags_apply_to_stdin() {
    let temp = tempfile::tempdir().unwrap();
    for (extra, success, severity) in [
        (&["--warn", "RY040"][..], true, "warning"),
        (&["--warn", "RY040", "--error-on-warning"], false, "warning"),
        (&["--exit-zero"], true, "error"),
    ] {
        let output = stdin(temp.path(), "source.R", extra, b"\"a\" + 1L\n");
        assert_eq!(output.success(), success, "{extra:?}: {output:?}");
        assert_eq!(output.findings(), [["source.R", "RY040", severity]]);
    }
}

#[test]
fn stdin_uses_logical_path_config_and_buffer_ast_for_unused_ignore() {
    let temp = tempfile::tempdir().unwrap();
    write(
        temp.path(),
        &[
            ("ry.toml", b"warn = [\"RY113\"]\n"),
            ("R/example.R", b"1L == NA # ry: ignore[RY034]\n"),
        ],
    );
    assert!(
        !disk(temp.path(), &["R/example.R"])
            .codes()
            .contains(&"RY113".to_owned())
    );

    let buffer = stdin(
        temp.path(),
        "R/example.R",
        &[],
        b"1L == 1L # ry: ignore[RY034]\n",
    );
    let findings = buffer.findings();
    assert!(
        findings.iter().any(|[_, code, _]| code == "RY113"),
        "{buffer:?}"
    );
    assert!(findings.iter().all(|[path, _, _]| path == "R/example.R"));
}
