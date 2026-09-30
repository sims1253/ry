//! Report input runs through the same discovery and checker path as R files.

use std::fs;
use std::process::Command;

fn check(root: &std::path::Path) -> Vec<serde_json::Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["check", "--output-format", "json"])
        .arg(root)
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"))
}

fn code<'a>(diags: &'a [serde_json::Value], code: &str) -> Vec<&'a serde_json::Value> {
    diags.iter().filter(|diag| diag["code"] == code).collect()
}

#[test]
fn opt_in_reports_keep_chunk_order_and_report_environments_separate() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    fs::write(
        root.path().join("first.qmd"),
        "é prose\r\n```{r}\r\nx <- \"a\"\r\n```\r\n```{r}\r\nx + 1L\r\n```\r\n",
    )
    .unwrap();
    fs::write(root.path().join("second.Rmd"), "```{r}\nx + 1L\n```\n").unwrap();
    let diagnostics = check(root.path());
    assert_eq!(code(&diagnostics, "RY040").len(), 1, "{diagnostics:?}");
    assert_eq!(code(&diagnostics, "RY010").len(), 1, "{diagnostics:?}");
    let bad = code(&diagnostics, "RY040")[0];
    assert!(bad.to_string().contains("first.qmd"), "{bad}");
    assert_eq!(bad["line"], 6, "{bad}");
}

#[test]
fn disabled_and_uncertain_chunks_have_visible_boundaries() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    fs::write(root.path().join("a.qmd"), "```{r}\nknown <- 1L\n```\n```{r}\n#| eval: false\nknown <- \"hidden\"\n```\n```{r, eval=choose()}\nuncertain <- 1L\n```\n```{r}\nuncertain + \"x\"\n```\n").unwrap();
    let diagnostics = check(root.path());
    assert_eq!(code(&diagnostics, "RY121").len(), 1, "{diagnostics:?}");
    assert!(code(&diagnostics, "RY040").is_empty(), "{diagnostics:?}");
}

#[test]
fn report_input_is_disabled_by_default_and_r_files_remain_checked() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.qmd"), "```{r}\n\"a\" + 1L\n```\n").unwrap();
    fs::write(root.path().join("a.R"), "\"a\" + 1L\n").unwrap();
    let diagnostics = check(root.path());
    assert_eq!(code(&diagnostics, "RY040").len(), 1, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|d| !d.to_string().contains("a.qmd")));
}

#[test]
fn split_r_syntax_reports_each_chunk_at_original_crlf_line() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    fs::write(
        root.path().join("broken.qmd"),
        "😀 intro\r\n```{r}\r\nx <- (\r\n```\r\n```{r}\r\n1L)\r\n```\r\n",
    )
    .unwrap();
    let diagnostics = check(root.path());
    let errors = code(&diagnostics, "RY000");
    assert_eq!(errors.len(), 2, "{diagnostics:?}");
    assert_eq!(errors[0]["line"], 3);
    assert_eq!(errors[0]["column"], 3);
    assert_eq!(errors[1]["line"], 6);
}

#[test]
fn chunk_options_do_not_shift_a_parse_error_into_metadata() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    fs::write(
        root.path().join("broken.qmd"),
        "😀 prose\r\n```{r}\r\n#| eval: true\r\n#| echo: false\r\nx <- (\r\n```\r\n",
    )
    .unwrap();
    let diagnostics = check(root.path());
    let errors = code(&diagnostics, "RY000");
    assert_eq!(errors.len(), 1, "{diagnostics:?}");
    assert_eq!(errors[0]["line"], 5, "{diagnostics:?}");
}

#[test]
fn indented_fences_keep_the_original_type_error_column() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    fs::write(
        root.path().join("indented.Rmd"),
        "plain text\n  ```{r}\n  \"a\" + 1L\n  ```\n",
    )
    .unwrap();
    let diagnostics = check(root.path());
    let errors = code(&diagnostics, "RY040");
    assert_eq!(errors.len(), 1, "{diagnostics:?}");
    assert_eq!(errors[0]["line"], 3);
    assert_eq!(errors[0]["column"], 3);
}

#[test]
fn fact_source_hash_tracks_original_prose_and_uncertain_reports_refuse_export() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    let path = root.path().join("memo.qmd");
    let dump = || {
        Command::new(env!("CARGO_BIN_EXE_ry"))
            .args(["dump-facts", "--format", "json"])
            .arg(&path)
            .output()
            .unwrap()
    };
    fs::write(&path, "😀 prose\n```{r}\nx <- 1L\n```\n").unwrap();
    let first = dump();
    assert!(first.status.success(), "{first:?}");
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    fs::write(&path, "界 prose\n```{r}\nx <- 1L\n```\n").unwrap();
    let second = dump();
    assert!(second.status.success(), "{second:?}");
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_ne!(
        first["files"][0]["source_hash"],
        second["files"][0]["source_hash"]
    );
    fs::write(&path, "```{r, eval=choose()}\nx <- 1L\n```\n").unwrap();
    let uncertain = dump();
    assert!(!uncertain.status.success());
    assert!(String::from_utf8_lossy(&uncertain.stderr).contains("uncertain input boundary"));
    let types = Command::new(env!("CARGO_BIN_EXE_ry"))
        .arg("dump-types")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!types.status.success());
    assert!(String::from_utf8_lossy(&types.stderr).contains("uncertain input"));
}

#[test]
fn source_call_keeps_report_origin_without_executing_helper_file() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    let marker = root.path().join("ran-marker");
    fs::write(
        root.path().join("helper.R"),
        format!(
            "writeLines('ran', '{}')\nfrom_helper <- function() 1L\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::write(
        root.path().join("memo.qmd"),
        "```{r}\nsource('helper.R')\nfrom_helper()\n```\n",
    )
    .unwrap();
    let diagnostics = check(root.path());
    assert!(code(&diagnostics, "RY000").is_empty(), "{diagnostics:?}");
    assert!(!marker.exists());
}

#[test]
fn malformed_and_unclosed_r_fences_have_visible_status() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    fs::write(root.path().join("unclosed.qmd"), "```{r}\nx <- 1L\n").unwrap();
    fs::write(root.path().join("malformed.Rmd"), "```{r\nx <- 1L\n```\n").unwrap();
    let diagnostics = check(root.path());
    assert_eq!(code(&diagnostics, "RY120").len(), 2, "{diagnostics:?}");
    assert!(code(&diagnostics, "RY000").is_empty(), "{diagnostics:?}");
}

#[test]
fn oversized_report_is_visible_for_direct_and_directory_inputs() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    let path = root.path().join("large.qmd");
    fs::write(
        &path,
        "x".repeat(ry_workspace::reports::MAX_REPORT_BYTES + 1),
    )
    .unwrap();

    let direct = Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["check", "--output-format", "json"])
        .arg(&path)
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .output()
        .unwrap();
    let diagnostics: Vec<serde_json::Value> = serde_json::from_slice(&direct.stdout).unwrap();
    assert_eq!(code(&diagnostics, "RY120").len(), 1, "{diagnostics:?}");

    let discovered = Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["check", "--output-format", "json"])
        .arg(root.path())
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&discovered.stderr).contains("per-file size cap"),
        "{discovered:?}"
    );
}
