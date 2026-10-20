//! ry flags a source file exactly when R's parser rejects its bytes. With
//! Rscript on `PATH`, each expectation is also checked against real R.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::support::check;

/// Whether R's parser rejects the file, with its stderr, or `None` without
/// Rscript. `parse()` reads like `Rscript file.R` without evaluating.
/// `LC_ALL` is pinned to UTF-8 because under `LC_ALL=C`, R 4.6.1 accepts
/// invalid bytes even in string tokens.
fn r_rejects(file: &Path) -> Option<(bool, String)> {
    let rscript = rscript_on_path()?;
    let escaped = file
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let output = Command::new(rscript)
        .env("LC_ALL", "C.UTF-8")
        .args(["--vanilla", "-e"])
        .arg(format!("invisible(parse(\"{escaped}\"))"))
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Some((!output.status.success(), stderr))
}

fn rscript_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("Rscript"))
        .find(|candidate| candidate.is_file())
}

/// What ry reports for one file.
enum Expect {
    /// One RY000 error whose message contains the fragment.
    Flag(&'static str),
    /// No RY000. Other diagnostics may appear.
    NoRy000,
    /// No diagnostics at all.
    Clean,
}

#[test]
fn encoding_errors_follow_rs_parser() {
    // (file, bytes, ry's report, R's parse error if R rejects). The
    // fixtures hold raw bytes an editor would rewrite.
    type Case<'a> = (&'a str, &'a [u8], Expect, Option<&'a str>);
    let cases: [Case; 8] = [
        // CP1252 bytes in a string token.
        (
            "cp1252_string.R",
            include_bytes!("fixtures/encoding/cp1252_string.R"),
            Expect::Flag("not valid UTF-8"),
            Some("invalid multibyte character in parser"),
        ),
        // R's lexer tolerates invalid bytes inside comments.
        (
            "latin1_comment.R",
            include_bytes!("fixtures/encoding/latin1_comment.R"),
            Expect::NoRy000,
            None,
        ),
        (
            "utf8.R",
            "title <- \"café au lait\"\n# Köln: „kölsch“ commentary\ny <- 2\n".as_bytes(),
            Expect::Clean,
            None,
        ),
        // R scans `%...%` operator tokens as raw bytes (gram.y `SpecialValue`).
        (
            "special_operator.R",
            include_bytes!("fixtures/encoding/special_operator.R"),
            Expect::NoRy000,
            None,
        ),
        // Known gap: R drops a lone undecodable byte and parses an empty
        // `expression()`. The same byte anywhere else is an R error, so ry
        // keeps flagging it.
        (
            "lone_byte.R",
            b"\xfc\n",
            Expect::Flag("not valid UTF-8"),
            None,
        ),
        // A leading BOM is "unexpected input" at 1:1 unless
        // `parse(keep.source = TRUE)` strips it.
        (
            "bom.R",
            include_bytes!("fixtures/encoding/bom.R"),
            Expect::Flag("byte order mark"),
            Some("unexpected input"),
        ),
        // Unlike invalid bytes, a BOM is rejected even before a comment.
        (
            "bom_comment_only.R",
            b"\xef\xbb\xbf# just a comment\n",
            Expect::Flag("byte order mark"),
            Some("unexpected input"),
        ),
        // U+FEFF after the first byte is an ordinary character.
        (
            "bom_midfile.R",
            "s <- \"\u{feff}\"\nx <- 1 # \u{feff} comment\n".as_bytes(),
            Expect::NoRy000,
            None,
        ),
    ];
    let temp = tempfile::tempdir().unwrap();
    for (name, bytes, expect, r_error) in cases {
        let path = temp.path().join(name);
        fs::write(&path, bytes).unwrap();
        let output = check().args(["--output-format", "json"]).arg(&path).run();
        let diagnostics = output.json();
        let diagnostics = diagnostics.as_array().unwrap();
        let errors = diagnostics.iter().any(|d| d["severity"] == "error");
        assert_eq!(output.code(), Some(i32::from(errors)), "{name}: {output:?}");
        let encoding: Vec<_> = diagnostics
            .iter()
            .filter(|d| d["code"] == "RY000")
            .collect();
        match expect {
            Expect::Flag(fragment) => {
                assert_eq!(encoding.len(), 1, "{name}: {output:?}");
                assert_eq!(encoding[0]["severity"], "error", "{name}");
                assert!(
                    encoding[0]["message"].as_str().unwrap().contains(fragment),
                    "{name}: {output:?}"
                );
            }
            Expect::NoRy000 => assert!(encoding.is_empty(), "{name}: {output:?}"),
            Expect::Clean => assert!(diagnostics.is_empty(), "{name}: {output:?}"),
        }
        if let Some((rejected, stderr)) = r_rejects(&path) {
            assert_eq!(rejected, r_error.is_some(), "{name}: R said {stderr}");
            if let Some(error) = r_error {
                assert!(stderr.contains(error), "{name}: R said {stderr}");
            }
        }
    }
}
