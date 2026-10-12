//! Report input runs through the same discovery and checker path as R files.

use std::fs;
use std::path::Path;

use ry_testkit::{CliProcess, ry};

fn ry_check() -> CliProcess {
    ry!().arg("check").env("RY_NO_INSTALLED_LIBRARIES", "1")
}

fn check(root: &Path) -> Vec<serde_json::Value> {
    match ry_check()
        .args(["--output-format", "json"])
        .arg(root)
        .run()
        .json()
    {
        serde_json::Value::Array(diagnostics) => diagnostics,
        other => panic!("{other}"),
    }
}

/// Check `files` in a fresh project that opts in to reports.
fn check_reports(files: &[(&str, &str)]) -> Vec<serde_json::Value> {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    for (name, source) in files {
        fs::write(root.path().join(name), source).unwrap();
    }
    check(root.path())
}

fn code<'a>(diags: &'a [serde_json::Value], code: &str) -> Vec<&'a serde_json::Value> {
    diags.iter().filter(|diag| diag["code"] == code).collect()
}

#[test]
fn opt_in_reports_keep_chunk_order_and_report_environments_separate() {
    let diagnostics = check_reports(&[
        (
            "first.qmd",
            "é prose\r\n```{r}\r\nx <- \"a\"\r\n```\r\n```{r}\r\nx + 1L\r\n```\r\n",
        ),
        ("second.Rmd", "```{r}\nx + 1L\n```\n"),
    ]);
    assert_eq!(code(&diagnostics, "RY040").len(), 1, "{diagnostics:?}");
    assert_eq!(code(&diagnostics, "RY010").len(), 1, "{diagnostics:?}");
    let bad = code(&diagnostics, "RY040")[0];
    assert!(bad.to_string().contains("first.qmd"), "{bad}");
    assert_eq!(bad["line"], 6, "{bad}");
}

#[test]
fn disabled_and_uncertain_chunks_have_visible_boundaries() {
    let diagnostics = check_reports(&[(
        "a.qmd",
        "```{r}\nknown <- 1L\n```\n```{r}\n#| eval: false\nknown <- \"hidden\"\n```\n```{r, eval=choose()}\nuncertain <- 1L\n```\n```{r}\nuncertain + \"x\"\n```\n",
    )]);
    assert_eq!(code(&diagnostics, "RY121").len(), 1, "{diagnostics:?}");
    assert!(code(&diagnostics, "RY040").is_empty(), "{diagnostics:?}");
}

/// Each case either runs the later `'a' + 1L` chunk (one RY040) or stops
/// before it at a visible RY121 boundary; a disabled chunk yields neither.
#[test]
fn chunk_execution_options_are_truthful_end_to_end() {
    let later = "```{r}\n'a' + 1L\n```\n";
    let body = |body: &str| format!("```{{r}}\n{body}\n```\n{later}");
    let header = |header: &str| format!("```{header}\nNULL\n```\n{later}");
    let yaml = |yaml: &str| format!("---\n{yaml}\n---\n{later}");
    let single = |header: &str| format!("```{header}\n'a' + 1L\n```\n");
    // Representative rows; the option grammar is unit-tested in ry-workspace.
    let cases = [
        (body("opts_chunkish <- 1L"), 1, 0),
        (body("knitr:::opts_chunk$set(eval=FALSE)"), 0, 1),
        (header("{r, fig.cap=\"caption, eval=FALSE\"}"), 1, 0),
        (
            header("{r, fig.cap={knitr::opts_chunk$set(eval=FALSE); \"caption\"}}"),
            0,
            1,
        ),
        (single("{r}\n#| eval: false"), 0, 0),
        (yaml("metadata:\n  eval: false"), 1, 0),
        (yaml("{execute: {eval: false}}"), 0, 1),
        (format!("~~~{{r}}\nx <- 'a'\n~~~\n{later}"), 1, 0),
        (format!("````md\n```{{r}}\nNULL\n```\n````\n{later}"), 0, 1),
    ];
    for (source, ry040, ry121) in cases {
        let diagnostics = check_reports(&[("report.Rmd", &source)]);
        assert_eq!(
            (
                code(&diagnostics, "RY040").len(),
                code(&diagnostics, "RY121").len()
            ),
            (ry040, ry121),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn quarto_cell_options_do_not_replace_function_local_typehint_comments() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("ry.toml"),
        "[reports]\nenabled = true\n[annotations.typehint]\nadopt = true\nversion = '0.1.0'\npaths = ['**/*.qmd']\n",
    )
    .unwrap();
    let report = root.path().join("typed.qmd");
    let active =
        "```{r}\n#| echo: false\nf <- function(x) {\n #| x integer\n x\n}\nf(\"bad\")\n```\n";
    fs::write(&report, active).unwrap();
    let diagnostics = check(root.path());
    assert_eq!(code(&diagnostics, "RY114").len(), 1, "{diagnostics:?}");

    fs::write(&report, active.replace("#| echo: false", "#| eval: false")).unwrap();
    let disabled = check(root.path());
    assert!(code(&disabled, "RY114").is_empty(), "{disabled:?}");
}

#[test]
fn report_input_is_disabled_by_default_and_r_files_remain_checked() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.qmd"), "```{r}\n\"a\" + 1L\n```\n").unwrap();
    fs::write(root.path().join("a.R"), "\"a\" + 1L\n").unwrap();
    let diagnostics = check(root.path());
    assert_eq!(code(&diagnostics, "RY040").len(), 1, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|d| !d.to_string().contains("a.qmd")));

    // An explicitly named report explains why it was not checked.
    let explained = ry_check()
        .arg("--explain-files")
        .arg(root.path().join("a.qmd"))
        .run();
    let stderr = explained.stderr();
    assert!(
        stderr.contains("a.qmd (reports.enabled = false)"),
        "{stderr}"
    );
}

#[test]
fn report_extensions_ignore_case_for_direct_and_directory_inputs() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    let path = root.path().join("memo.QMD");
    fs::write(&path, "Some prose.\n```{r}\n\"a\" + 1L\n```\n").unwrap();
    for input in [root.path(), path.as_path()] {
        let diagnostics = check(input);
        assert_eq!(code(&diagnostics, "RY040").len(), 1, "{diagnostics:?}");
        assert!(code(&diagnostics, "RY000").is_empty(), "{diagnostics:?}");
    }
}

#[test]
fn findings_keep_original_report_coordinates() {
    for (source, rule, expected) in [
        // Each chunk parses on its own, at its CRLF line after multibyte prose.
        (
            "😀 intro\r\n```{r}\r\nx <- (\r\n```\r\n```{r}\r\n1L)\r\n```\r\n",
            "RY000",
            &[(3, 3), (6, 3)][..],
        ),
        // Cell options do not shift a parse error into metadata.
        (
            "😀 prose\r\n```{r}\r\n#| eval: true\r\n#| echo: false\r\nx <- (\r\n```\r\n",
            "RY000",
            &[(5, 3)],
        ),
        (
            "plain text\n  ```{r}\n  \"a\" + 1L\n  ```\n",
            "RY040",
            &[(3, 3)],
        ),
    ] {
        let diagnostics = check_reports(&[("report.qmd", source)]);
        let positions: Vec<_> = code(&diagnostics, rule)
            .iter()
            .map(|diag| {
                (
                    diag["line"].as_u64().unwrap(),
                    diag["column"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(positions, expected, "{source}: {diagnostics:?}");
    }
}

#[test]
fn fact_source_hash_tracks_original_prose_and_uncertain_reports_refuse_export() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("ry.toml"), "[reports]\nenabled = true\n").unwrap();
    let path = root.path().join("memo.qmd");
    let dump = || {
        ry!()
            .args(["dump-facts", "--format", "json"])
            .arg(&path)
            .run()
    };
    fs::write(&path, "😀 prose\n```{r}\nx <- 1L\n```\n").unwrap();
    let first = dump().json();
    fs::write(&path, "界 prose\n```{r}\nx <- 1L\n```\n").unwrap();
    let second = dump().json();
    assert_ne!(
        first["files"][0]["source_hash"],
        second["files"][0]["source_hash"]
    );
    for source in [
        "```{r, eval=choose()}\nx <- 1L\n```\n",
        "---\nengine: markdown\n---\n```{r}\nx <- 1L\n```\n",
        "\n---\nexecute:\n  eval: false\n---\n```{r}\nx <- 1L\n```\n",
    ] {
        fs::write(&path, source).unwrap();
        for refused in [dump(), ry!().arg("dump-types").arg(&path).run()] {
            assert!(!refused.success(), "{source}");
            assert!(refused.stdout().is_empty(), "{source}: {refused:?}");
            assert!(refused.stderr().contains("memo.qmd"), "{refused:?}");
        }
    }
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
    let diagnostics = check_reports(&[
        ("unclosed.qmd", "```{r}\nx <- 1L\n"),
        ("malformed.Rmd", "```{r\nx <- 1L\n```\n"),
    ]);
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

    let diagnostics = check(&path);
    assert_eq!(code(&diagnostics, "RY120").len(), 1, "{diagnostics:?}");

    let discovered = ry_check()
        .args(["--output-format", "json"])
        .arg(root.path())
        .run();
    let stderr = discovered.stderr();
    assert!(
        stderr.contains("large.qmd") && stderr.contains("index.max-file-bytes"),
        "{discovered:?}"
    );

    // The default formatter slices the original source line using the issue
    // span; a one-byte RY120 span may end inside a multibyte character.
    fs::write(
        &path,
        format!("é\n{}", " ".repeat(ry_workspace::reports::MAX_REPORT_BYTES)),
    )
    .unwrap();
    let human = ry_check().arg(&path).run();
    assert!(human.success(), "{human:?}");
    let output = human.stdout() + &human.stderr();
    assert!(output.contains("RY120"), "{output}");
    assert!(output.contains("é"), "{output}");
    assert!(!output.contains("panicked"), "{output}");
}
