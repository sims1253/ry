//! Semantic coherence tests for Plan 35 W7.
//!
//! Two deliverables are exercised here:
//!
//! 1. **SEMANTIC_LISTS registry**: every registered hardcoded list is
//!    validated by its declared check. An unregistered list fails.
//!    Adding a member to a list in only one representation fails.
//!
//! 2. **Canonical base-call resolution**: qualified base calls, unshadowed
//!    search-path calls, lexical shadowing, and cross-file binding.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use ry_checker::semantic_lists::{self, CheckKind};
use ry_checker::{Checker, Project};
use ry_core::RParser;

// ── Helpers ───────────────────────────────────────────────────────────────

fn rscript_available() -> bool {
    Command::new("Rscript")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// Run an R expression and capture stdout.
fn r_eval(expr: &str) -> String {
    let output = Command::new("Rscript")
        .args(["--vanilla", "-e", expr])
        .output()
        .expect("Rscript invocation");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Load the embedded base typeshed functions as a name set.
fn base_typeshed_names() -> HashSet<String> {
    let typeshed = ry_typeshed::load_base().expect("base typeshed loads");
    typeshed.functions.keys().cloned().collect()
}

/// Load the embedded rlang vendor typeshed functions.
fn rlang_typeshed_names() -> Option<HashSet<String>> {
    let typeshed = ry_typeshed::load_package("rlang")?;
    Some(typeshed.functions.keys().cloned().collect())
}

/// Parse and check R source, returning diagnostic codes.
fn check_source(src: &str) -> Vec<String> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("test.R", src).expect("parse");
    let mut checker = Checker::new("test.R");
    checker.check(&file);
    checker
        .take_diagnostics()
        .into_iter()
        .map(|d| d.code.to_string())
        .collect()
}

/// Parse and check R source, returning (code, has_fix) pairs.
fn check_source_with_fixs(src: &str) -> Vec<(String, bool)> {
    let mut parser = RParser::new().expect("parser init");
    let file = parser.parse("test.R", src).expect("parse");
    let mut checker = Checker::new("test.R");
    checker.check(&file);
    checker
        .take_diagnostics()
        .into_iter()
        .map(|d| (d.code.to_string(), d.fix.is_some()))
        .collect()
}

// ── Deliverable 1: SEMANTIC_LISTS registry coherence ─────────────────────

/// Every registered list has a non-empty name and non-empty items.
#[test]
fn registry_entries_are_well_formed() {
    let entries = semantic_lists::registry();
    assert!(!entries.is_empty(), "registry must not be empty");
    for entry in &entries {
        assert!(!entry.name.is_empty(), "registry entry has empty name");
        assert!(
            !entry.items.is_empty(),
            "registry entry {:?} has empty items",
            entry.name
        );
        assert!(
            !entry.claim.is_empty(),
            "registry entry {:?} has empty claim",
            entry.name
        );
    }
}

/// Registry names are unique.
#[test]
fn registry_names_are_unique() {
    let entries = semantic_lists::registry();
    let names: Vec<&str> = entries.iter().map(|e| e.name).collect();
    let unique: HashSet<&str> = names.iter().copied().collect();
    assert_eq!(names.len(), unique.len(), "duplicate registry names");
}

/// Typeshed-agreement lists: every item exists in the appropriate vendor
/// typeshed. Adding an item to the list without a matching stub fails.
#[test]
fn typeshed_agreement_lists_match_typeshed() {
    let base = base_typeshed_names();
    let rlang = rlang_typeshed_names().unwrap_or_default();

    for entry in semantic_lists::registry() {
        if entry.check != CheckKind::TypeshedAgreement {
            continue;
        }
        for item in entry.items {
            let found = base.contains(*item) || rlang.contains(*item);
            assert!(
                found,
                "list {:?} item {:?} not found in base or rlang typeshed",
                entry.name, item
            );
        }
    }
}

/// OPERATORS matches R's Arith + Compare group members.
#[test]
fn operators_match_r_oracle() {
    if !rscript_available() {
        eprintln!("Rscript not on PATH; skipping oracle check");
        return;
    }
    let output =
        r_eval("cat(getGroupMembers(\"Arith\"), getGroupMembers(\"Compare\"), sep=\"\\n\")");
    let r_ops: BTreeSet<&str> = output.trim().lines().collect();
    let list_ops: BTreeSet<&str> = semantic_lists::OPERATORS.iter().copied().collect();
    assert_eq!(
        list_ops, r_ops,
        "OPERATORS does not match R's Arith + Compare groups"
    );
}

/// METADATA_ARGS matches the non-`...` parameter names of `formals(data.frame)`.
#[test]
fn metadata_args_match_r_oracle() {
    if !rscript_available() {
        eprintln!("Rscript not on PATH; skipping oracle check");
        return;
    }
    let output = r_eval("cat(setdiff(names(formals(data.frame)), \"...\"), sep=\"\\n\")");
    let r_args: BTreeSet<&str> = output.trim().lines().collect();
    let list_args: BTreeSet<&str> = semantic_lists::METADATA_ARGS.iter().copied().collect();
    assert_eq!(
        list_args, r_args,
        "METADATA_ARGS does not match formals(data.frame)"
    );
}

/// NAME_CARRYING_CONTAINERS: each function exists in R and preserves names.
#[test]
fn name_carrying_containers_match_r_oracle() {
    if !rscript_available() {
        eprintln!("Rscript not on PATH; skipping oracle check");
        return;
    }
    for func in semantic_lists::NAME_CARRYING_CONTAINERS {
        // Each function must exist.
        let expr = format!("cat(exists(\"{func}\"), \"\\n\")");
        let output = r_eval(&expr);
        assert!(
            output.trim().starts_with("TRUE"),
            "R does not have function {:?}",
            func
        );
        // Named arguments must be preserved in the result.
        let check = match *func {
            "list" | "c" => {
                format!("cat(\"test_name\" %in% names({func}(test_name = 1)), \"\\n\")")
            }
            "data.frame" => {
                "cat(\"test_name\" %in% names(data.frame(test_name = 1)), \"\\n\")".to_string()
            }
            "structure" => {
                "cat(\"test_name\" %in% names(attributes(structure(1, test_name = 2))), \"\\n\")"
                    .to_string()
            }
            _ => continue,
        };
        let result = r_eval(&check);
        assert!(
            result.trim().starts_with("TRUE"),
            "{func} does not carry named arguments per R oracle"
        );
    }
}

/// BUILTIN_ENVIRONMENT_BINDINGS: these are Shiny server function parameters.
#[test]
fn builtin_environment_bindings_match_r_oracle() {
    if !rscript_available() {
        eprintln!("Rscript not on PATH; skipping oracle check");
        return;
    }
    let output = r_eval("cat(requireNamespace(\"shiny\", quietly=TRUE), \"\\n\")");
    if !output.trim().starts_with("TRUE") {
        eprintln!("shiny not installed; skipping oracle check");
        return;
    }
    let session_check =
        r_eval("cat(\"session\" %in% names(formals(shiny::moduleServer)), \"\\n\")");
    assert!(
        session_check.trim().starts_with("TRUE"),
        "session is not a Shiny moduleServer parameter"
    );
    for binding in semantic_lists::BUILTIN_ENVIRONMENT_BINDINGS {
        assert!(
            matches!(*binding, "input" | "output" | "session"),
            "unexpected BUILTIN_ENVIRONMENT_BINDINGS member: {binding}"
        );
    }
}

/// No hardcoded semantic list escapes the registry.
///
/// This test scans the checker source for `const ... : &[&str]` and
/// `const ... : [&str; N]` declarations and fails if any is not registered.
#[test]
fn no_unregistered_hardcoded_lists() {
    let registered: HashSet<&str> = semantic_lists::registry().iter().map(|e| e.name).collect();

    // Lists that are intentionally not semantic and do not belong in the
    // registry.
    let known_non_semantic: &[&str] = &[];

    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found_lists = Vec::new();

    fn scan_file(path: &Path, found: &mut Vec<(String, String)>) {
        let src = fs::read_to_string(path).unwrap_or_default();
        for line in src.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("const ")
                && let Some(colon_pos) = rest.find(':')
            {
                let name = rest[..colon_pos].trim();
                let type_part = rest[colon_pos + 1..].trim();
                if type_part.contains("&str")
                    && (type_part.contains("&[") || type_part.contains("[&str;"))
                {
                    found.push((name.to_string(), path.display().to_string()));
                }
            }
        }
    }

    fn walk_dir(dir: &Path, found: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).expect("read src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk_dir(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                scan_file(&path, found);
            }
        }
    }

    walk_dir(&src_dir, &mut found_lists);

    for (name, file) in &found_lists {
        let is_registry_const = file.ends_with("semantic_lists.rs");
        let is_reexport = file.ends_with("packages.rs");
        let is_registered = registered.contains(name.as_str());
        let is_known_non_semantic = known_non_semantic.contains(&name.as_str());

        if !is_registry_const && !is_reexport && !is_registered && !is_known_non_semantic {
            panic!(
                "Unregistered hardcoded list `{name}` found in {file}. \
                 Register it in semantic_lists::registry() or add it to the \
                 known_non_semantic exclusion with a justification."
            );
        }
    }
}

/// Every OPERATORS item is in R's Arith+Compare set.
#[test]
fn adding_bogus_operator_to_list_would_fail() {
    if !rscript_available() {
        return;
    }
    let output =
        r_eval("cat(getGroupMembers(\"Arith\"), getGroupMembers(\"Compare\"), sep=\"\\n\")");
    let r_ops: BTreeSet<&str> = output.trim().lines().collect();
    for op in semantic_lists::OPERATORS {
        assert!(
            r_ops.contains(*op),
            "OPERATORS contains {op:?} which R does not recognise"
        );
    }
}

// ── Deliverable 2: Canonical base-call resolution ────────────────────────

/// A qualified `base::class()` call resolves to base and gets a fix.
#[test]
fn base_qualified_call_resolves() {
    let diags = check_source_with_fixs("if (base::class(x) == \"foo\") TRUE\n");
    let ry103 = diags.iter().find(|(c, _)| c == "RY103");
    assert!(
        ry103.is_some(),
        "base::class comparison should fire RY103, got {diags:?}"
    );
    assert!(
        ry103.is_some_and(|(_, fix)| *fix),
        "base::class should offer a fix (resolves to base)"
    );
}

/// An unshadowed bare `class()` call resolves to base and gets a fix.
#[test]
fn unshadowed_bare_call_resolves() {
    let diags = check_source_with_fixs("if (class(x) == \"foo\") TRUE\n");
    let ry103 = diags.iter().find(|(c, _)| c == "RY103");
    assert!(
        ry103.is_some_and(|(_, fix)| *fix),
        "unshadowed class should offer a fix (resolves to base), got {diags:?}"
    );
}

/// A lexically shadowed `class` does not resolve to base.
/// RY103 fires on the comparison shape, but no fix is offered.
#[test]
fn lexical_shadow_does_not_resolve() {
    let diags =
        check_source_with_fixs("class <- function(x) \"myclass\"\nif (class(x) == \"foo\") TRUE\n");
    let ry103 = diags.iter().find(|(c, _)| c == "RY103");
    assert!(
        ry103.is_some(),
        "RY103 fires on class() comparison shape regardless of shadowing"
    );
    assert!(
        ry103.is_some_and(|(_, fix)| !*fix),
        "shadowed class should NOT offer a fix (does not resolve to base)"
    );
}

/// A fn_table shadowed `class` does not resolve to base (cross-file binding).
/// RY103 fires on the shape, but no fix is offered.
#[test]
fn fn_table_shadow_does_not_resolve() {
    let mut parser = RParser::new().expect("parser init");
    let mut project = Project::new();
    let def_file = parser
        .parse(
            "def.R",
            "class <- function(x) { structure(x, class = \"custom\") }\n",
        )
        .expect("parse def");
    let use_file = parser
        .parse("use.R", "f <- function(x) if (class(x) == \"foo\") TRUE\n")
        .expect("parse use");
    project.add_file("def.R".to_string(), def_file);
    project.add_file("use.R".to_string(), use_file);
    let results = project.check();
    let diags: Vec<(String, bool)> = results
        .into_iter()
        .filter(|(path, _)| path == "use.R")
        .flat_map(|(_, ds)| {
            ds.into_iter()
                .map(|d| (d.code.to_string(), d.fix.is_some()))
        })
        .collect();
    let ry103 = diags.iter().find(|(c, _)| c == "RY103");
    assert!(
        ry103.is_some(),
        "RY103 fires on the class() comparison shape, got {diags:?}"
    );
    assert!(
        ry103.is_some_and(|(_, fix)| !*fix),
        "fn_table shadowed class should NOT offer a fix"
    );
}

/// A non-base qualified call does not resolve to base.
/// RY103 fires on the shape, but no fix is offered.
#[test]
fn non_base_qualified_does_not_resolve() {
    let diags = check_source_with_fixs("if (other::class(x) == \"foo\") TRUE\n");
    let ry103 = diags.iter().find(|(c, _)| c == "RY103");
    assert!(
        ry103.is_some_and(|(_, fix)| !*fix),
        "non-base qualified class should NOT offer a fix, got {diags:?}"
    );
}

/// RY105: a base::sum call on a length-1 scalar triggers the dead-guard
/// diagnostic. A shadowed sum does not.
#[test]
fn scalar_reduction_respects_base_resolution() {
    let diags = check_source("f <- function(x) if (length(base::sum(x)) > 0) 1\n");
    assert!(
        diags.iter().any(|c| c == "RY105"),
        "base::sum length comparison should fire RY105, got {diags:?}"
    );

    let diags =
        check_source("sum <- function(x) c(x, x)\nf <- function(x) if (length(sum(x)) > 0) 1\n");
    assert!(
        !diags.iter().any(|c| c == "RY105"),
        "shadowed sum should NOT fire RY105, got {diags:?}"
    );
}

/// RY102: a base::c call with a `<-` argument fires the diagnostic.
/// A shadowed c does not.
#[test]
fn name_carrying_container_respects_base_resolution() {
    let diags = check_source("c(a = 1, b <- 2)\n");
    assert!(
        diags.iter().any(|c| c == "RY102"),
        "base c() arrow should fire RY102, got {diags:?}"
    );

    let diags = check_source("c <- function(...) list(...)\nc(a = 1, b <- 2)\n");
    assert!(
        !diags.iter().any(|c| c == "RY102"),
        "shadowed c() should NOT fire RY102, got {diags:?}"
    );
}

/// If a lexical binding shadows a name-carrying container, RY102 must not
/// fire. This test fails if the canonical resolution is replaced by the old
/// fn_table-only check that ignored lexical scope.
#[test]
fn lexical_shadow_of_container_suppresses_ry102() {
    let diags = check_source("f <- function() {\n  c <- function(...) 42\n  c(a = 1, b <- 2)\n}\n");
    assert!(
        !diags.iter().any(|c| c == "RY102"),
        "lexically shadowed c() should NOT fire RY102, got {diags:?}"
    );
}
