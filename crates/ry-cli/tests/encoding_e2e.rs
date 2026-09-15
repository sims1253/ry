//! End-to-end coverage for non-UTF-8 source files (#376).
//!
//! R's parser rejects a file whose bytes are not valid UTF-8 with
//! "invalid multibyte character in parser" — except that invalid bytes
//! inside comments and `%...%` special-operator tokens are tolerated.
//! ry must make the same call: flag where R errors, stay silent where
//! R parses, so "ry clean" keeps meaning "R can load this file". The
//! fixtures under `fixtures/encoding/` hold real CP1252/Latin-1 bytes
//! and are read as bytes (`include_bytes!`), never edited as text. When
//! Rscript is installed each expectation is arbitrated against real R,
//! mirroring the oracle harness.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The fixtures' raw bytes. A fixture whose bytes are valid UTF-8 would
/// defeat the purpose, so the names are fixed here instead of globbed.
fn fixture_bytes(name: &str) -> &'static [u8] {
    match name {
        // CP1252 in tokens: `caf\xe9` in a string, `\x93`\x94`` smart
        // quotes in a comment-plus-token mix. R's parser rejects it.
        "cp1252_string.R" => include_bytes!("fixtures/encoding/cp1252_string.R"),
        // Latin-1 0xf6 confined to a comment. R's parser ACCEPTS this;
        // ry must not flag it.
        "latin1_comment.R" => include_bytes!("fixtures/encoding/latin1_comment.R"),
        // Latin-1 0xe9 inside a `%...%` operator token. R's parse()
        // ACCEPTS this (SpecialValue scans raw bytes); only evaluation
        // of the undefined operator would fail. ry must not flag it.
        "special_operator.R" => include_bytes!("fixtures/encoding/special_operator.R"),
        _ => panic!("unknown encoding fixture: {name}"),
    }
}

fn write_fixture(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, fixture_bytes(name)).unwrap();
    path
}

/// `ry check --output-format json <file>` as parsed diagnostics.
fn check_json(file: &Path) -> Vec<serde_json::Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["check", "--output-format", "json"])
        .arg(file)
        .output()
        .expect("failed to invoke ry binary");
    let diagnostics: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("ry check json output");
    // A flagged file must fail the run (error severity), a clean one
    // must pass; both matter for the ry-clean-iff-R-parses invariant.
    let expected_status = u8::from(
        diagnostics
            .iter()
            .any(|d| d["severity"].as_str() == Some("error")),
    );
    assert_eq!(
        output.status.code(),
        Some(expected_status as i32),
        "exit code must follow error diagnostics"
    );
    diagnostics
}

/// Whether R's parser rejects the file, or `None` without Rscript.
/// `parse()` (not execution) is the encoding oracle: it reads and
/// parses exactly like `Rscript file.R` would, without evaluating.
/// `LC_ALL` is pinned to a UTF-8 locale because R's byte tolerance is
/// locale-dependent: under `LC_ALL=C`, R 4.6.1 accepts invalid bytes
/// even in string tokens, and the arbitration here would flip on such
/// a machine.
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

#[test]
fn non_utf8_bytes_in_tokens_are_flagged_like_rs_parser() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "cp1252_string.R");

    let diagnostics = check_json(&path);
    let encoding: Vec<_> = diagnostics
        .iter()
        .filter(|d| d["code"].as_str() == Some("RY000"))
        .collect();
    assert_eq!(encoding.len(), 1, "{diagnostics:?}");
    assert_eq!(encoding[0]["severity"].as_str(), Some("error"));
    assert!(
        encoding[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("not valid UTF-8")),
        "{diagnostics:?}"
    );

    if let Some((rejected, stderr)) = r_rejects(&path) {
        assert!(rejected, "R must reject the fixture");
        assert!(
            stderr.contains("invalid multibyte character in parser"),
            "R should reject it as an encoding error, got: {stderr}"
        );
    }
}

#[test]
fn latin1_bytes_confined_to_comments_stay_clean_like_rs_parser() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "latin1_comment.R");

    let diagnostics = check_json(&path);
    assert!(
        diagnostics
            .iter()
            .all(|d| d["code"].as_str() != Some("RY000")),
        "comment-only legacy bytes must stay clean like R: {diagnostics:?}"
    );

    if let Some((rejected, _)) = r_rejects(&path) {
        assert!(!rejected, "R must parse the comment-only fixture");
    }
}

/// A valid UTF-8 file with plenty of non-ASCII text is the adjacent
/// idiom that must stay quiet: only genuinely undecodable bytes flag.
#[test]
fn valid_utf8_multibyte_source_stays_clean() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("utf8.R");
    fs::write(
        &path,
        "title <- \"café au lait\"\n# Köln: „kölsch“ commentary\ny <- 2\n",
    )
    .unwrap();

    assert!(check_json(&path).is_empty());

    if let Some((rejected, _)) = r_rejects(&path) {
        assert!(!rejected, "R must parse the valid UTF-8 fixture");
    }
}

/// R's lexer scans `%...%` operator tokens as raw bytes (gram.y
/// `SpecialValue`), so `parse()` accepts accented user-defined
/// operators; only evaluating the undefined operator fails. ry must
/// stay clean on the same shape.
#[test]
fn latin1_bytes_inside_a_special_operator_stay_clean_like_rs_parser() {
    let temp = tempfile::tempdir().unwrap();
    let path = write_fixture(temp.path(), "special_operator.R");

    let diagnostics = check_json(&path);
    assert!(
        diagnostics
            .iter()
            .all(|d| d["code"].as_str() != Some("RY000")),
        "operator-confined legacy bytes must stay clean like R: {diagnostics:?}"
    );

    if let Some((rejected, _)) = r_rejects(&path) {
        assert!(
            !rejected,
            "R's parse() must accept the special-operator fixture"
        );
    }
}

/// Known gap, pinned: a file whose entire content is one invalid byte
/// parses in R (all four contexts) to an EMPTY `expression()` — the
/// scanner silently drops the lone undecodable byte — while ry flags
/// it. The byte is genuinely invalid UTF-8 and the same byte in any
/// other token position is rejected by R, so ry keeps flagging rather
/// than model the drop quirk; this test pins that decision.
#[test]
fn lone_invalid_byte_file_is_flagged_known_gap_vs_rs_empty_parse() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("lone_byte.R");
    fs::write(&path, b"\xfc\n").unwrap();

    let diagnostics = check_json(&path);
    let encoding: Vec<_> = diagnostics
        .iter()
        .filter(|d| d["code"].as_str() == Some("RY000"))
        .collect();
    assert_eq!(encoding.len(), 1, "{diagnostics:?}");
    assert!(
        encoding[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("not valid UTF-8")),
        "{diagnostics:?}"
    );
}
