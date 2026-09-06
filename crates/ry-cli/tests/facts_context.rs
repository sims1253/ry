//! Cache identities and input safety for the structured facts exporter.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn run(root: &Path, files: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ry"))
        .current_dir(root)
        .args(["dump-facts", "--format", "json"])
        .args(files)
        .output()
        .expect("invoke ry dump-facts")
}

fn dump(root: &Path, files: &[&str]) -> Value {
    let output = run(root, files);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("facts are JSON")
}

fn file<'a>(dump: &'a Value, name: &str) -> &'a Value {
    dump["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| Path::new(file["path"].as_str().unwrap()).ends_with(name))
        .unwrap_or_else(|| panic!("missing {name}: {dump}"))
}

fn context<'a>(dump: &'a Value, source: &Value) -> &'a Value {
    dump["contexts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|context| context["id"] == source["context_id"])
        .expect("source refers to an exported context")
}

fn assert_context_changed(before: &Value, after: &Value, name: &str) {
    let old = file(before, name);
    let new = file(after, name);
    assert_eq!(
        old["source_hash"], new["source_hash"],
        "the target source is unchanged"
    );
    assert_ne!(
        old["context_id"], new["context_id"],
        "context changes must invalidate cached facts"
    );
}

fn stub(mode: &str) -> String {
    serde_json::json!({
        "schema_version": "2", "package": "factspkg", "version": "test",
        "functions": {"value": {"params": [], "return": {"mode": mode, "length": "1"}}}
    })
    .to_string()
}

#[test]
fn rejects_latin1_instead_of_exporting_transcoded_byte_spans() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("latin1.R"), b"caf\xe9 <- 1L\n").unwrap();
    let output = run(temp.path(), &["latin1.R"]);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "an invalid input must not yield usable facts"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("UTF-8"));
}

#[test]
fn repeated_runs_and_reversed_file_arguments_produce_identical_json() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("ry.toml"),
        "globals = [\"ambient_z\", \"ambient_a\"]\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("a.R"),
        "z <- 1L\na <- list(last = z, first = 2L)\n",
    )
    .unwrap();
    fs::write(temp.path().join("b.R"), "f <- function(x) { y <- x; y }\n").unwrap();
    let first = run(temp.path(), &["b.R", "a.R"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    for files in [["b.R", "a.R"], ["a.R", "b.R"]] {
        let repeated = run(temp.path(), &files);
        assert!(
            repeated.status.success(),
            "{}",
            String::from_utf8_lossy(&repeated.stderr)
        );
        if first.stdout != repeated.stdout {
            let original = String::from_utf8_lossy(&first.stdout);
            let changed = String::from_utf8_lossy(&repeated.stdout);
            let difference = original
                .lines()
                .zip(changed.lines())
                .enumerate()
                .find(|(_, (old, new))| old != new);
            panic!("facts changed for {files:?}; first differing line: {difference:?}");
        }
    }
    let facts: Value = serde_json::from_slice(&first.stdout).unwrap();
    let paths: Vec<_> = facts["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect();
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    for path in paths {
        assert!(Path::new(path).is_absolute());
        assert_eq!(Path::new(path).canonicalize().unwrap(), Path::new(path));
    }
}

#[test]
fn namespace_import_changes_invalidate_unchanged_source() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("R")).unwrap();
    fs::write(
        temp.path().join("DESCRIPTION"),
        "Package: factsfixture\nVersion: 0.0.1\nImports: stats\n",
    )
    .unwrap();
    fs::write(temp.path().join("NAMESPACE"), "importFrom(stats, median)\n").unwrap();
    fs::write(temp.path().join("R/main.R"), "x <- 1L\n").unwrap();
    let before = dump(temp.path(), &["R/main.R"]);
    fs::write(temp.path().join("NAMESPACE"), "importFrom(stats, sd)\n").unwrap();
    let after = dump(temp.path(), &["R/main.R"]);
    assert_context_changed(&before, &after, "R/main.R");
    assert_ne!(
        context(&before, file(&before, "R/main.R"))["inputs"]["workspace_hash"],
        context(&after, file(&after, "R/main.R"))["inputs"]["workspace_hash"]
    );
}

#[test]
fn custom_stub_content_invalidates_context_without_a_version_change() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("stubs")).unwrap();
    fs::write(temp.path().join("ry.toml"), "typeshed = [\"stubs\"]\n").unwrap();
    fs::write(temp.path().join("main.R"), "x <- factspkg::value()\n").unwrap();
    fs::write(temp.path().join("stubs/factspkg.json"), stub("integer")).unwrap();
    let before = dump(temp.path(), &["main.R"]);
    fs::write(temp.path().join("stubs/factspkg.json"), stub("character")).unwrap();
    let after = dump(temp.path(), &["main.R"]);
    assert_context_changed(&before, &after, "main.R");
    assert_ne!(
        context(&before, file(&before, "main.R"))["inputs"]["typeshed"]["custom_hash"],
        context(&after, file(&after, "main.R"))["inputs"]["typeshed"]["custom_hash"]
    );
}

#[test]
fn another_analyzed_source_invalidates_the_shared_group_context() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("main.R"), "answer <- helper()\n").unwrap();
    fs::write(temp.path().join("helper.R"), "helper <- function() 1L\n").unwrap();
    let before = dump(temp.path(), &["main.R", "helper.R"]);
    fs::write(
        temp.path().join("helper.R"),
        "helper <- function() \"changed\"\n",
    )
    .unwrap();
    let after = dump(temp.path(), &["main.R", "helper.R"]);
    assert_context_changed(&before, &after, "main.R");
    assert_ne!(
        file(&before, "helper.R")["source_hash"],
        file(&after, "helper.R")["source_hash"]
    );
    assert_eq!(
        file(&after, "main.R")["context_id"],
        file(&after, "helper.R")["context_id"]
    );
}

#[test]
fn effective_config_changes_invalidate_unchanged_source() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("main.R"), "x <- ambient\n").unwrap();
    fs::write(temp.path().join("ry.toml"), "globals = []\n").unwrap();
    let before = dump(temp.path(), &["main.R"]);
    fs::write(temp.path().join("ry.toml"), "globals = [\"ambient\"]\n").unwrap();
    let after = dump(temp.path(), &["main.R"]);
    assert_context_changed(&before, &after, "main.R");
    assert_ne!(
        context(&before, file(&before, "main.R"))["inputs"]["config_hash"],
        context(&after, file(&after, "main.R"))["inputs"]["config_hash"]
    );
}

#[test]
fn exporting_facts_does_not_execute_the_analyzed_source() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("main.R"),
        concat!(
            "writeLines(\"source was executed\", \"executed.txt\")\n",
            "stop(\"analysis must not execute this code\")\n",
        ),
    )
    .unwrap();
    let facts = dump(temp.path(), &["main.R"]);
    assert_eq!(facts["files"].as_array().unwrap().len(), 1);
    assert!(!temp.path().join("executed.txt").exists());
}

#[test]
fn unselected_shiny_marker_invalidates_ambient_binding_facts() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("fragment.R"), "x <- 1L\n").unwrap();
    let before = dump(temp.path(), &["fragment.R"]);
    let binding_names = |facts: &Value| {
        file(facts, "fragment.R")["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|scope| scope["kind"] == "top")
            .unwrap()["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|binding| binding["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(binding_names(&before), ["x"]);
    // The marker is deliberately outside the selected analysis sources.
    let marker = temp.path().join("app.R");
    fs::write(&marker, "# Shiny application marker\n").unwrap();
    let with_marker = dump(temp.path(), &["fragment.R"]);
    assert_context_changed(&before, &with_marker, "fragment.R");
    assert_eq!(
        binding_names(&with_marker),
        ["input", "output", "session", "x"]
    );
    assert_eq!(with_marker["files"].as_array().unwrap().len(), 1);
    fs::remove_file(marker).unwrap();
    let removed = dump(temp.path(), &["fragment.R"]);
    assert_context_changed(&with_marker, &removed, "fragment.R");
    assert_eq!(
        file(&before, "fragment.R")["context_id"],
        file(&removed, "fragment.R")["context_id"]
    );
    assert_eq!(binding_names(&before), binding_names(&removed));
}

#[cfg(unix)]
#[test]
fn oversized_non_utf8_path_returns_an_error_without_panicking() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("ry.toml"), "[index]\nmax-file-bytes = 1\n").unwrap();
    let filename = OsString::from_vec(b"oversized\xff.R".to_vec());
    fs::write(temp.path().join(filename), b"x <- 1L\n").unwrap();
    let output = run(temp.path(), &["."]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert_ne!(
        output.status.code(),
        Some(101),
        "must report an input error: {stderr}"
    );
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(stderr.contains("UTF-8"), "{stderr}");
    assert!(
        output.stdout.is_empty(),
        "must not emit unusable truncation paths"
    );
}
