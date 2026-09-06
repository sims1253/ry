use std::fs;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn invoke(directory: &std::path::Path, command: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ry"))
        .current_dir(directory)
        .arg(command)
        .args(args)
        .output()
        .unwrap()
}

fn facts(directory: &std::path::Path, args: &[&str]) -> Value {
    let output = invoke(directory, "dump-facts", args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn binding<'a>(scope: &'a Value, name: &str) -> &'a Value {
    scope["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap()
}

#[test]
fn scope_exit_and_first_assignment_are_not_reference_facts() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("scope.R"),
        "x <- 1L\ny <- x\nx <- \"later\"\nf <- function(x) x\n",
    )
    .unwrap();
    let output = facts(dir.path(), &["scope.R"]);
    assert_eq!(output["schema_version"], 1);
    assert_eq!(output["snapshot_kind"], "scope_exit");
    let scopes = output["files"][0]["scopes"].as_array().unwrap();
    let top = scopes.iter().find(|scope| scope["kind"] == "top").unwrap();
    let x = binding(top, "x");
    assert_eq!(x["type"]["mode"], "character");
    assert_eq!(x["declaration"]["kind"], "first_assignment");
    assert_eq!(x["declaration"]["span"]["bytes"], json!([0, 1]));
    assert_eq!(x["declaration"]["defines_final_value"], "not_established");
    assert_eq!(binding(top, "y")["type"]["mode"], "integer");
    let inner = scopes.iter().find(|scope| scope["name"] == "f").unwrap();
    assert_eq!(binding(inner, "x")["kind"], "param");
    assert_ne!(
        binding(inner, "x")["declaration"]["span"],
        x["declaration"]["span"]
    );
    for scope in scopes {
        assert_eq!(scope["snapshot_kind"], "scope_exit");
        for entry in scope["bindings"].as_array().unwrap() {
            assert_eq!(entry["snapshot_kind"], "scope_exit");
            assert!(entry.get("type_at_reference").is_none());
        }
    }
    let rejected = invoke(dir.path(), "dump-facts", &["scope.R", "--position", "2:6"]);
    assert!(!rejected.status.success());
    let old = invoke(dir.path(), "dump-types", &["scope.R", "--position", "2:6"]);
    assert!(old.status.success());
    let old: Value = serde_json::from_slice(&old.stdout).unwrap();
    assert!(old.get("schema_version").is_none());
    assert_eq!(
        binding(&old["files"][0]["scopes"][0], "x")["type"],
        "character<len=1>"
    );
}

#[test]
fn byte_spans_round_trip_unicode_tabs_and_multiline_functions() {
    let dir = tempfile::tempdir().unwrap();
    let source = "é <- 1L\r\nf <- function(λ) {\r\n\t結果 <- λ\r\n\t結果\r\n}\r\n`空 白` <- 1L\r\n";
    fs::write(dir.path().join("unicode.R"), source).unwrap();
    let output = facts(dir.path(), &["unicode.R"]);
    assert_eq!(output["coordinates"]["bytes"], "zero_based_half_open");
    let scopes = output["files"][0]["scopes"].as_array().unwrap();
    for scope in scopes {
        let range = scope["span"]["bytes"].as_array().unwrap();
        let slice =
            &source[range[0].as_u64().unwrap() as usize..range[1].as_u64().unwrap() as usize];
        if scope["kind"] == "function" {
            assert!(slice.starts_with("function(λ)"));
            assert!(slice.ends_with('}'));
        }
        for entry in scope["bindings"].as_array().unwrap() {
            if let Some(range) = entry["declaration"]["span"]["bytes"].as_array() {
                let actual = &source
                    [range[0].as_u64().unwrap() as usize..range[1].as_u64().unwrap() as usize];
                let name = entry["name"].as_str().unwrap();
                assert_eq!(actual, if name == "空 白" { "`空 白`" } else { name });
            }
        }
    }
    let inner = scopes.iter().find(|scope| scope["name"] == "f").unwrap();
    assert_eq!(
        binding(inner, "結果")["declaration"]["span"]["start"],
        json!([3, 2])
    );
    assert_eq!(
        binding(inner, "結果")["declaration"]["span"]["end"],
        json!([3, 4])
    );
    assert_eq!(binding(inner, "é")["kind"], "unclassified");
}

#[test]
fn aliases_and_unknown_search_paths_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("alias.R"),
        "f <- base::identity\nlibrary(package_which_does_not_exist_ry_facts)\nx <- unavailable_value_ry_facts\n",
    )
    .unwrap();
    let output = facts(dir.path(), &["alias.R"]);
    let scope = &output["files"][0]["scopes"][0];
    assert_eq!(scope["search_path_unknown"], true);
    assert_eq!(scope["data_mask_unknown"], false);
    assert_eq!(
        binding(scope, "f")["origin"]["callee_alias"]["target"],
        "base::identity"
    );
    assert_eq!(binding(scope, "x")["type"]["mode"], "opaque");
    assert_eq!(binding(scope, "x")["type"]["columns"]["kind"], "unknown");
}

#[test]
fn bounded_discovery_is_not_reported_as_complete() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("ry.toml"), "[index]\nmax-file-bytes = 9\n").unwrap();
    fs::write(dir.path().join("a.R"), "a <- 1L\n").unwrap();
    fs::write(dir.path().join("b.R"), "b <- 2L # oversized\n").unwrap();
    let output = facts(dir.path(), &["."]);
    assert_eq!(output["discovery"]["complete"], false);
    assert_eq!(output["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["discovery"]["truncations"][0]["oversized_files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn parse_failures_cannot_silently_omit_requested_facts() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("bad.R"), "function(\n").unwrap();
    let output = invoke(dir.path(), "dump-facts", &["bad.R"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn file_count_caps_reject_unstable_discovery_subsets() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("ry.toml"), "[index]\nmax-files = 1\n").unwrap();
    fs::write(dir.path().join("a.R"), "a <- 1L\n").unwrap();
    fs::write(dir.path().join("b.R"), "b <- 2L\n").unwrap();
    let output = invoke(dir.path(), "dump-facts", &["."]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("max-files"));
}

#[test]
fn captured_data_masks_keep_uncertainty_in_nested_scopes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("mask.R"),
        "f <- function(df) with(df, { g <- function() column; g() })\n",
    )
    .unwrap();
    let output = facts(dir.path(), &["mask.R"]);
    let scopes = output["files"][0]["scopes"].as_array().unwrap();
    let outer = scopes.iter().find(|scope| scope["name"] == "f").unwrap();
    let masked = scopes.iter().find(|scope| scope["name"] == "g").unwrap();
    assert_eq!(outer["data_mask_unknown"], false);
    assert_eq!(masked["data_mask_unknown"], true);
    assert_eq!(masked["snapshot_kind"], "scope_exit");
    assert_eq!(binding(masked, "df")["kind"], "unclassified");
}

#[test]
fn relative_package_input_records_import_origin() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("R")).unwrap();
    fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: factsfixture\nVersion: 1.0\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("NAMESPACE"),
        "importFrom(facts_dependency, imported_function)\n",
    )
    .unwrap();
    fs::write(dir.path().join("R/main.R"), "x <- imported_function()\n").unwrap();
    let output = facts(dir.path(), &["R/main.R"]);
    assert_eq!(
        output["files"][0]["imports"]["imported_function"],
        "facts_dependency"
    );
    // Workspace imports are resolver metadata, not captured lexical bindings.
    assert!(
        output["files"][0]["scopes"][0]["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["name"] != "imported_function")
    );
    let legacy = invoke(dir.path(), "dump-types", &["R/main.R"]);
    assert!(
        legacy.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy.stderr)
    );
}

#[cfg(unix)]
#[test]
fn duplicate_canonical_file_ids_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("source.R"), "x <- 1L\n").unwrap();
    std::os::unix::fs::symlink(dir.path().join("source.R"), dir.path().join("alias.R")).unwrap();
    let output = invoke(dir.path(), "dump-facts", &["source.R", "alias.R"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("same physical file"));
}

#[test]
fn unnamed_list_elements_export_schema_keys_without_named_field_claims() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("list.R"), "x <- list(1L, label = \"a\")\n").unwrap();
    let output = facts(dir.path(), &["list.R"]);
    let columns = &binding(&output["files"][0]["scopes"][0], "x")["type"]["columns"];
    assert_eq!(columns["kind"], "complete");
    assert_eq!(columns["entries"][0]["key"], "[[1]]");
    assert_eq!(columns["entries"][1]["key"], "label");
    assert!(columns["entries"][0].get("name").is_none());
}

#[test]
fn dynamic_assignments_do_not_invent_imported_or_captured_provenance() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("R")).unwrap();
    fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: factsfixture\nVersion: 1.0\n",
    )
    .unwrap();
    fs::write(dir.path().join("NAMESPACE"), "importFrom(stats, median)\n").unwrap();
    fs::write(
        dir.path().join("R/main.R"),
        concat!(
            "assign(\"median\", 1L)\ny <- median\n",
            "x <- \"outer\"\nf <- function() { assign(\"x\", 1L); x }\n",
        ),
    )
    .unwrap();
    let output = facts(dir.path(), &["R/main.R"]);
    let scopes = output["files"][0]["scopes"].as_array().unwrap();
    let top = scopes.iter().find(|scope| scope["kind"] == "top").unwrap();
    let nested = scopes.iter().find(|scope| scope["name"] == "f").unwrap();
    assert_eq!(output["files"][0]["imports"]["median"], "stats");
    for entry in [binding(top, "median"), binding(nested, "x")] {
        assert_eq!(entry["type"]["mode"], "integer");
        assert_eq!(entry["kind"], "unclassified");
        assert_eq!(entry["declaration"]["kind"], "unavailable");
        assert!(entry["declaration"]["span"].is_null());
        assert!(entry["origin"].get("imported_from").is_none());
    }
    let y = binding(top, "y");
    assert_eq!(y["type"]["mode"], "integer");
    assert_eq!(y["origin"]["callee_alias"]["target"], "median");
    assert_eq!(y["origin"]["callee_alias"]["resolution"], "not_established");
}
