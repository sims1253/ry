//! End-to-end checks for one explicit stdin source overlay (#582).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ry"))
        .current_dir(root)
        .arg("check")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        // Argument-validation failures may exit before the writer reaches
        // the pipe. The status/stderr assertions below verify that path.
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): stdout={:?}, stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn codes(output: &Output) -> Vec<String> {
    json(output)
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn identical_buffer_matches_disk_with_package_and_neighbor() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: demo\nVersion: 0.1\n",
    )
    .unwrap();
    fs::write(temp.path().join("R/helper.R"), "unknown_symbol\n").unwrap();
    let source = b"result <- \"a\" + 1L\n";
    fs::write(temp.path().join("R/main.R"), source).unwrap();

    let disk = run(
        temp.path(),
        &["R/main.R", "R/helper.R", "--output-format", "json"],
        b"ignored redirected stdin",
    );
    let buffer = run(
        temp.path(),
        &[
            "-",
            "R/helper.R",
            "--stdin-filename",
            "R/main.R",
            "--output-format",
            "json",
        ],
        source,
    );
    assert_eq!(disk.status, buffer.status);
    assert_eq!(json(&disk), json(&buffer));
    assert_eq!(codes(&buffer), ["RY010", "RY040"]);
    assert_eq!(json(&buffer)[1]["path"], "R/main.R");
}

#[test]
fn stdin_logical_filename_uses_the_same_path_rule_policy_as_disk() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("R/sub")).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[[rule-overrides]]\npaths = [\"R/**\"]\nwarn = [\"RY040\"]\n",
    )
    .unwrap();
    let source = b"\"a\" + 1L\n";
    fs::write(temp.path().join("R/new.R"), source).unwrap();
    let disk = run(
        temp.path(),
        &["R/new.R", "--output-format", "json"],
        b"unused",
    );
    let overlay = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/new.R",
            "--output-format",
            "json",
        ],
        source,
    );
    assert_eq!(json(&disk), json(&overlay));
    assert_eq!(json(&overlay)[0]["severity"], "warning");
    fs::remove_file(temp.path().join("R/new.R")).unwrap();
    let unsaved = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/new.R",
            "--output-format",
            "json",
        ],
        source,
    );
    assert_eq!(json(&unsaved)[0]["severity"], "warning");
    let dotted_unsaved = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/sub/../new.R",
            "--output-format",
            "json",
        ],
        source,
    );
    assert_eq!(json(&dotted_unsaved)[0]["severity"], "warning");
}

#[test]
fn unicode_unsaved_filename_keeps_its_path_scope() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "warn = ['RY040']\n[[rule-overrides]]\npaths = ['mémoire.R']\nerror = ['RY040']\n",
    )
    .unwrap();
    let output = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "mémoire.R",
            "--output-format",
            "json",
        ],
        b"\"text\" + 1L\n",
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let findings = json(&output);
    assert_eq!(findings[0]["severity"], "error", "{findings}");
    assert!(!temp.path().join("mémoire.R").exists());
}

#[test]
fn overlay_replaces_stale_disk_copy_in_directory_scan() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(temp.path().join("R/main.R"), b"\"stale\" + 1L\n").unwrap();
    fs::write(temp.path().join("R/helper.R"), b"unknown_symbol\n").unwrap();
    let buffer = run(
        temp.path(),
        &[
            ".",
            "-",
            "--stdin-filename",
            "R/./main.R",
            "--output-format",
            "json",
            "--explain-files",
        ],
        b"updated <- 1L\n",
    );
    assert!(buffer.status.success(), "{buffer:?}");
    assert_eq!(codes(&buffer), ["RY010"]);
    let stderr = String::from_utf8_lossy(&buffer.stderr);
    assert_eq!(stderr.matches("include ./R/main.R").count(), 1, "{stderr}");
    assert!(stderr.contains("stdin overlay"), "{stderr}");
    assert_eq!(
        fs::read(temp.path().join("R/main.R")).unwrap(),
        b"\"stale\" + 1L\n"
    );
}

#[test]
fn typehint_adoption_uses_the_stdin_overlay_at_its_native_path() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['R/**']\n",
    )
    .unwrap();
    let disk = "f <- function(x) {\n #| x character\n x\n}\nf('ok')\n";
    fs::write(temp.path().join("R/main.R"), disk).unwrap();
    for path in ["R/main.R", "R/unsaved.R"] {
        for (class, mismatches) in [("integer", 1), ("character", 0)] {
            let source = format!("f <- function(x) {{\n #| x {class}\n x\n}}\nf('text')\n");
            let output = run(
                temp.path(),
                &["-", "--stdin-filename", path, "--output-format", "json"],
                source.as_bytes(),
            );
            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                codes(&output)
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
        disk
    );
    assert!(!temp.path().join("R/unsaved.R").exists());
}

#[test]
fn unsaved_file_is_checked_without_writing_it() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(temp.path().join("R/helper.R"), b"unknown_symbol\n").unwrap();
    let path = temp.path().join("R/new.R");
    let buffer = run(
        temp.path(),
        &[
            ".",
            "-",
            "--stdin-filename",
            "R/new.R",
            "--output-format",
            "json",
        ],
        b"\"buffer\" + 1L\n",
    );
    assert_eq!(buffer.status.code(), Some(1), "{buffer:?}");
    assert_eq!(codes(&buffer), ["RY010", "RY040"]);
    assert_eq!(json(&buffer)[1]["path"], "./R/new.R");
    assert!(!path.exists());

    // Naming the same unsaved file as another explicit operand still
    // replaces that operand rather than reporting a missing disk root.
    let duplicate_operand = run(
        temp.path(),
        &[
            "R/new.R",
            "-",
            "--stdin-filename",
            "R/new.R",
            "--output-format",
            "json",
        ],
        b"\"buffer\" + 1L\n",
    );
    assert_eq!(
        duplicate_operand.status.code(),
        Some(1),
        "{duplicate_operand:?}"
    );
    assert_eq!(codes(&duplicate_operand), ["RY040"]);
    assert!(!String::from_utf8_lossy(&duplicate_operand.stderr).contains("no such file"));
    assert!(!path.exists());

    let bare_alias = run(
        temp.path(),
        &[
            "new.R",
            "-",
            "--stdin-filename",
            "./new.R",
            "--output-format",
            "json",
        ],
        b"\"buffer\" + 1L\n",
    );
    assert_eq!(codes(&bare_alias), ["RY040"]);
    assert!(!String::from_utf8_lossy(&bare_alias.stderr).contains("no such file"));
    assert!(!temp.path().join("new.R").exists());
}

#[test]
fn missing_roots_keep_disk_output_and_exit_policy_with_stdin() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("blocker"), "not a directory").unwrap();
    for missing in ["blocker/missing.R", "missing/child.R"] {
        for exit_zero in [false, true] {
            let mut disk_args = vec![missing, "--output-format", "json"];
            let mut buffer_args = vec![
                missing,
                "-",
                "--stdin-filename",
                "source.R",
                "--output-format",
                "json",
            ];
            if exit_zero {
                disk_args.push("--exit-zero");
                buffer_args.push("--exit-zero");
            }
            let disk = run(temp.path(), &disk_args, b"");
            let buffer = run(temp.path(), &buffer_args, b"x <- 1L\n");
            assert_eq!(disk.status.code(), Some(i32::from(!exit_zero)));
            assert_eq!(json(&disk), serde_json::json!([]));
            assert!(String::from_utf8_lossy(&disk.stderr).contains("no such file or directory"));
            assert_eq!(buffer.status, disk.status, "{buffer:?}");
            assert_eq!(buffer.stdout, disk.stdout, "{buffer:?}");
            assert_eq!(buffer.stderr, disk.stderr, "{buffer:?}");
        }
    }
}

#[test]
fn package_dynamic_bindings_use_stdin_instead_of_stale_disk() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(temp.path().join("DESCRIPTION"), "Package: demo\n").unwrap();
    let path = temp.path().join("R/main.R");
    for (old, source, expected) in [
        (Some("assign(\"ghost\", 1L)\n"), "ghost\n", vec!["RY010"]),
        (None, "assign(\"ghost\", 1L)\nghost\n", vec![]),
    ] {
        if let Some(old) = old {
            fs::write(&path, old).unwrap();
        } else if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        let buffer = run(
            temp.path(),
            &[
                "-",
                "--stdin-filename",
                "R/main.R",
                "--output-format",
                "json",
            ],
            source.as_bytes(),
        );
        assert_eq!(fs::read_to_string(&path).ok().as_deref(), old);
        fs::write(&path, source).unwrap();
        let disk = run(temp.path(), &["R/main.R", "--output-format", "json"], b"");
        assert_eq!(codes(&disk), expected, "{disk:?}");
        assert_eq!(json(&buffer), json(&disk), "{buffer:?}");
        assert_eq!(buffer.status, disk.status);
        assert_eq!(buffer.stderr, disk.stderr);
    }
}

#[test]
fn package_dataset_bindings_use_stdin_instead_of_stale_disk() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::create_dir(temp.path().join("data")).unwrap();
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: demo\nLazyData: true\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("R/neighbor.R"),
        "gold\nsilver\ncopper\nbronze\nplatinum\n",
    )
    .unwrap();
    fs::write(temp.path().join("data/other.R"), "silver <- 1L\n").unwrap();
    fs::write(temp.path().join("data/copper.rds"), b"").unwrap();
    fs::write(
        temp.path().join("data/bronze.rda"),
        include_bytes!("../../../testdata/serialized/empty.rda"),
    )
    .unwrap();
    fs::write(
        temp.path().join("data/platinum.RData"),
        include_bytes!("../../../testdata/serialized/empty-ascii.rda"),
    )
    .unwrap();
    let path = temp.path().join("data/preprocess.r");
    for (old, source, expected) in [
        (Some("gold <- 1L\n"), "1L\n", vec!["RY010"]),
        (Some("1L\n"), "gold <- 1L\n", vec![]),
        (None, "gold <- 1L\n", vec![]),
    ] {
        if let Some(old) = old {
            fs::write(&path, old).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        let buffers: Vec<_> = ["data/preprocess.r", "./data/preprocess.r"]
            .into_iter()
            .map(|logical| {
                run(
                    temp.path(),
                    &[
                        "R/neighbor.R",
                        "-",
                        "--stdin-filename",
                        logical,
                        "--output-format",
                        "json",
                    ],
                    source.as_bytes(),
                )
            })
            .collect();
        assert_eq!(fs::read_to_string(&path).ok().as_deref(), old);
        fs::write(&path, source).unwrap();
        let disk = run(
            temp.path(),
            &[
                "R/neighbor.R",
                "data/preprocess.r",
                "--output-format",
                "json",
            ],
            b"",
        );
        assert_eq!(codes(&disk), expected, "{disk:?}");
        if !expected.is_empty() {
            assert_eq!(json(&disk)[0]["path"], "R/neighbor.R");
            assert!(json(&disk)[0]["message"].as_str().unwrap().contains("gold"));
        }
        assert!(disk.stderr.is_empty(), "{disk:?}");
        for buffer in buffers {
            assert_eq!(json(&buffer), json(&disk), "{buffer:?}");
            assert_eq!(buffer.status, disk.status);
            assert_eq!(buffer.stderr, disk.stderr);
        }
    }
}

#[cfg(unix)]
#[test]
fn package_dataset_bindings_replace_a_symlink_alias() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::create_dir(temp.path().join("data")).unwrap();
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: demo\nLazyData: true\n",
    )
    .unwrap();
    fs::write(temp.path().join("R/neighbor.R"), "gold\n").unwrap();
    fs::write(temp.path().join("data/preprocess.r"), "gold <- 1L\n").unwrap();
    symlink("preprocess.r", temp.path().join("data/alias.R")).unwrap();
    let buffer = run(
        temp.path(),
        &[
            "R/neighbor.R",
            "-",
            "--stdin-filename",
            "data/alias.R",
            "--output-format",
            "json",
        ],
        b"1L\n",
    );
    assert_eq!(codes(&buffer), ["RY010"], "{buffer:?}");
    assert_eq!(json(&buffer)[0]["path"], "R/neighbor.R");
    assert_eq!(
        fs::read_to_string(temp.path().join("data/preprocess.r")).unwrap(),
        "gold <- 1L\n"
    );
}

#[test]
fn test_helpers_use_stdin_for_neighboring_test_context() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("tests/testthat")).unwrap();
    fs::write(temp.path().join("DESCRIPTION"), "Package: demo\n").unwrap();
    fs::write(temp.path().join("tests/testthat/test-main.R"), "ghost\n").unwrap();
    let path = temp.path().join("tests/testthat/helper-main.R");
    for (old, source, expected) in [
        (Some("ghost <- 1L\n"), "1L\n", vec!["RY010"]),
        (None, "ghost <- 1L\n", vec![]),
    ] {
        if let Some(old) = old {
            fs::write(&path, old).unwrap();
        } else if path.exists() {
            fs::remove_file(&path).unwrap();
        }
        let buffers: Vec<_> = [
            "tests/testthat/helper-main.R",
            "./tests/testthat/helper-main.R",
        ]
        .into_iter()
        .map(|logical| {
            run(
                temp.path(),
                &[
                    "tests/testthat/test-main.R",
                    "-",
                    "--stdin-filename",
                    logical,
                    "--output-format",
                    "json",
                ],
                source.as_bytes(),
            )
        })
        .collect();
        assert_eq!(fs::read_to_string(&path).ok().as_deref(), old);
        fs::write(&path, source).unwrap();
        let disk = run(
            temp.path(),
            &[
                "tests/testthat/test-main.R",
                "tests/testthat/helper-main.R",
                "--output-format",
                "json",
            ],
            b"",
        );
        assert_eq!(codes(&disk), expected, "{disk:?}");
        for buffer in buffers {
            assert_eq!(json(&buffer), json(&disk), "{buffer:?}");
            assert_eq!(buffer.status, disk.status);
            assert_eq!(buffer.stderr, disk.stderr);
        }
    }
}

#[test]
fn package_dynamic_bindings_share_context_across_root_spellings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(temp.path().join("DESCRIPTION"), "Package: demo\n").unwrap();
    fs::write(temp.path().join("R/main.R"), "assign(\"ghost\", 1L)\n").unwrap();
    fs::write(temp.path().join("R/neighbor.R"), "ghost\n").unwrap();
    let buffers: Vec<_> = ["R/main.R", "./R/main.R"]
        .into_iter()
        .map(|logical| {
            run(
                temp.path(),
                &[
                    "R/neighbor.R",
                    "-",
                    "--stdin-filename",
                    logical,
                    "--output-format",
                    "json",
                ],
                b"1L\n",
            )
        })
        .collect();
    fs::write(temp.path().join("R/main.R"), "1L\n").unwrap();
    let disk = run(
        temp.path(),
        &["R/neighbor.R", "R/main.R", "--output-format", "json"],
        b"",
    );
    assert_eq!(codes(&disk), ["RY010"], "{disk:?}");
    assert_eq!(json(&disk)[0]["path"], "R/neighbor.R");
    for buffer in buffers {
        assert_eq!(json(&buffer), json(&disk), "{buffer:?}");
        assert_eq!(buffer.status, disk.status);
        assert_eq!(buffer.stderr, disk.stderr);
    }
}

#[cfg(unix)]
#[test]
fn package_dynamic_bindings_replace_a_symlink_alias() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(temp.path().join("DESCRIPTION"), "Package: demo\n").unwrap();
    fs::write(temp.path().join("R/main.R"), "assign(\"ghost\", 1L)\n").unwrap();
    symlink("main.R", temp.path().join("R/alias.R")).unwrap();
    let buffer = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/alias.R",
            "--output-format",
            "json",
        ],
        b"ghost\n",
    );
    assert_eq!(codes(&buffer), ["RY010"], "{buffer:?}");
    assert_eq!(json(&buffer)[0]["path"], "R/alias.R");
    assert_eq!(
        fs::read_to_string(temp.path().join("R/main.R")).unwrap(),
        "assign(\"ghost\", 1L)\n"
    );
}

#[test]
fn explicit_stdin_errors_and_watch_rejection_are_clear() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("directory")).unwrap();
    for (args, expected) in [
        (vec!["-"], "requires --stdin-filename"),
        (vec!["-", "-", "--stdin-filename", "a.R"], "only once"),
        (vec!["--stdin-filename", "a.R"], "requires a `-` input"),
        (
            vec!["-", "--stdin-filename", "a.R", "--watch"],
            "--watch cannot be used with stdin",
        ),
        (
            vec!["-", "--stdin-filename", "directory"],
            "must name a source file, not a directory",
        ),
    ] {
        let output = run(temp.path(), &args, b"x <- 1L\n");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{output:?}"
        );
        assert!(output.stdout.is_empty(), "{output:?}");
    }
}

#[cfg(unix)]
#[test]
fn symlinked_parent_dotdot_replaces_the_actual_source() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("a/b")).unwrap();
    fs::write(temp.path().join("a/foo.R"), b"\"stale\" + 1L\n").unwrap();
    fs::write(temp.path().join("foo.R"), b"\"neighbor\" + 2L\n").unwrap();
    symlink("a/b", temp.path().join("link")).unwrap();
    let buffer = b"updated <- 1L\n";
    let args = |logical| {
        run(
            temp.path(),
            &[
                ".",
                "-",
                "--stdin-filename",
                logical,
                "--output-format",
                "json",
                "--explain-files",
            ],
            buffer,
        )
    };
    let through_link = args("link/../foo.R");
    let direct = args("a/foo.R");
    assert_eq!(through_link.status, direct.status);
    assert_eq!(json(&through_link), json(&direct));
    assert_eq!(codes(&through_link), ["RY040"]);
    assert_eq!(json(&through_link)[0]["path"], "./foo.R");
    let explained = String::from_utf8_lossy(&through_link.stderr);
    assert!(
        explained.contains("include ./a/foo.R (stdin overlay)"),
        "{explained}"
    );

    // The final file may be unsaved while its symlinked parent already
    // exists; it must still use the parent's filesystem meaning.
    let unsaved_link = args("link/../new.R");
    let unsaved_direct = args("a/new.R");
    assert_eq!(json(&unsaved_link), json(&unsaved_direct));
    assert!(!temp.path().join("a/new.R").exists());
}

#[test]
fn empty_parse_error_unicode_crlf_and_invalid_bytes_use_file_pipeline() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.R");
    let args = [
        "-",
        "--stdin-filename",
        "source.R",
        "--output-format",
        "json",
    ];
    let empty = run(temp.path(), &args, b"");
    assert!(empty.status.success(), "{empty:?}");
    assert_eq!(json(&empty), serde_json::json!([]));
    assert!(!String::from_utf8_lossy(&empty.stderr).contains("no .R"));

    for source in [
        "café <- 1L\r\n\"x\" + 1L\r\n".as_bytes(),
        b"x <- (\n".as_slice(),
        b"caf\xe9 <- 1L\n".as_slice(),
        b"\xef\xbb\xbfx <- 1L\n".as_slice(),
    ] {
        fs::write(&path, source).unwrap();
        let disk = run(temp.path(), &["source.R", "--output-format", "json"], b"");
        let buffer = run(temp.path(), &args, source);
        assert_eq!(disk.status, buffer.status, "{source:?}");
        assert_eq!(json(&disk), json(&buffer), "{source:?}");
    }
    assert_eq!(
        codes(&run(temp.path(), &args, b"caf\xe9 <- 1L\n")),
        ["RY000"]
    );
}

#[test]
fn stdin_size_cap_and_read_error_fail_with_empty_machine_report() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("ry.toml"), "[index]\nmax-file-bytes = 8\n").unwrap();
    let args = [
        "-",
        "--stdin-filename",
        "source.R",
        "--output-format",
        "json",
    ];
    let at_limit = run(temp.path(), &args, b"x <- 1L\n");
    assert!(at_limit.status.success(), "{at_limit:?}");
    assert_eq!(json(&at_limit), serde_json::json!([]));
    let over_limit = run(temp.path(), &args, b"x <- 1L\nX");
    assert_eq!(over_limit.status.code(), Some(1));
    assert_eq!(json(&over_limit), serde_json::json!([]));
    assert!(String::from_utf8_lossy(&over_limit.stderr).contains("index.max-file-bytes"));
    let exit_zero = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "source.R",
            "--output-format",
            "json",
            "--exit-zero",
        ],
        b"x <- 1L\nX",
    );
    assert!(exit_zero.status.success(), "{exit_zero:?}");
    assert_eq!(json(&exit_zero), serde_json::json!([]));

    // A directory file descriptor is readable to open on Unix, but its
    // first read fails. This exercises the actual stdin I/O error branch.
    #[cfg(unix)]
    {
        let output = Command::new(env!("CARGO_BIN_EXE_ry"))
            .current_dir(temp.path())
            .args([
                "check",
                "-",
                "--stdin-filename",
                "source.R",
                "--output-format",
                "json",
            ])
            .stdin(Stdio::from(fs::File::open(temp.path()).unwrap()))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(json(&output), serde_json::json!([]));
        assert!(String::from_utf8_lossy(&output.stderr).contains("stdin for source.R"));
    }
}

#[test]
fn excluded_file_is_explicitly_checked_and_baseline_applies() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(temp.path().join("ry.toml"), "exclude = [\"R/ignored.R\"]\n").unwrap();
    fs::write(temp.path().join("R/ignored.R"), b"\"a\" + 1L\n").unwrap();
    let directory = run(
        temp.path(),
        &[temp.path().to_str().unwrap(), "--output-format", "json"],
        b"",
    );
    assert!(directory.status.success());
    assert_eq!(json(&directory), serde_json::json!([]));

    let args = [
        "-",
        "--stdin-filename",
        "R/ignored.R",
        "--output-format",
        "json",
    ];
    let direct = run(
        temp.path(),
        &["R/ignored.R", "--output-format", "json"],
        b"",
    );
    let stdin = run(temp.path(), &args, b"\"a\" + 1L\n");
    assert_eq!(json(&direct), json(&stdin));
    assert_eq!(codes(&stdin), ["RY040"]);

    let baseline = temp.path().join("accepted.json");
    let written = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/ignored.R",
            "--output-format",
            "json",
            "--write-baseline",
            "accepted.json",
        ],
        b"\"a\" + 1L\n",
    );
    assert_eq!(written.status.code(), Some(1));
    assert!(baseline.is_file());
    let suppressed = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/ignored.R",
            "--output-format",
            "json",
            "--baseline",
            "accepted.json",
        ],
        b"\"a\" + 1L\n",
    );
    assert!(suppressed.status.success(), "{suppressed:?}");
    assert_eq!(json(&suppressed), serde_json::json!([]));
    let changed = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/ignored.R",
            "--output-format",
            "json",
            "--baseline",
            "accepted.json",
        ],
        b"unknown_symbol\n",
    );
    assert_eq!(codes(&changed), ["RY010"]);
}

#[test]
fn warning_and_exit_zero_flags_apply_to_stdin() {
    let temp = tempfile::tempdir().unwrap();
    let args = [
        "-",
        "--stdin-filename",
        "source.R",
        "--output-format",
        "json",
        "--warn",
        "RY040",
    ];
    let warning = run(temp.path(), &args, b"\"a\" + 1L\n");
    assert!(warning.status.success(), "{warning:?}");
    assert_eq!(json(&warning)[0]["severity"], "warning");
    let error_on_warning = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "source.R",
            "--output-format",
            "json",
            "--warn",
            "RY040",
            "--error-on-warning",
        ],
        b"\"a\" + 1L\n",
    );
    assert_eq!(error_on_warning.status.code(), Some(1));
    let exit_zero = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "source.R",
            "--output-format",
            "json",
            "--exit-zero",
        ],
        b"\"a\" + 1L\n",
    );
    assert!(exit_zero.status.success());
    assert_eq!(codes(&exit_zero), ["RY040"]);
}

#[test]
fn stdin_uses_logical_path_config_and_buffer_ast_for_unused_ignore() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(temp.path().join("ry.toml"), "warn = [\"RY113\"]\n").unwrap();
    fs::write(
        temp.path().join("R/example.R"),
        "1L == NA # ry: ignore[RY034]\n",
    )
    .unwrap();
    let disk = run(
        temp.path(),
        &["R/example.R", "--output-format", "json"],
        b"",
    );
    assert!(!codes(&disk).contains(&"RY113".to_owned()));

    let buffer = run(
        temp.path(),
        &[
            "-",
            "--stdin-filename",
            "R/example.R",
            "--output-format",
            "json",
        ],
        b"1L == 1L # ry: ignore[RY034]\n",
    );
    assert!(codes(&buffer).contains(&"RY113".to_owned()), "{buffer:?}");
    assert!(
        json(&buffer)
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["path"] == "R/example.R")
    );
}
