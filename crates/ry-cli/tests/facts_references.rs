use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn invoke(directory: &Path, references: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
    command
        .current_dir(directory)
        .args(["dump-facts", "case.R"]);
    if references {
        command.arg("--references");
    }
    command.output().unwrap()
}

fn parse(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn analyze(source: &str) -> Value {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("case.R"), source).unwrap();
    parse(&invoke(directory.path(), true))
}

fn reference_at(output: &Value, source: &str, fragment: &str, token: &str) -> Value {
    let fragment_start = source.find(fragment).unwrap();
    let token_start = fragment_start + fragment.find(token).unwrap();
    let expected = json!([token_start, token_start + token.len()]);
    let matching: Vec<_> = output["files"][0]["references"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|reference| reference["span"]["bytes"] == expected)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected one reference to {token} in {fragment}: {output}"
    );
    matching[0].clone()
}

fn definition<'a>(output: &'a Value, reference: &Value) -> &'a Value {
    let id = reference["definition_id"]
        .as_u64()
        .expect("resolved definition ID");
    output["files"][0]["definitions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|definition| definition["id"] == id)
        .unwrap()
}

fn assert_uncertain(reference: &Value) {
    assert!(
        matches!(
            reference["resolution_status"].as_str(),
            Some("unsupported" | "unresolved" | "ambiguous")
        ),
        "{reference}"
    );
    assert!(reference["definition_id"].is_null(), "{reference}");
    assert!(reference["type_at_reference"].is_null(), "{reference}");
    assert!(
        reference["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty()),
        "{reference}"
    );
}

#[test]
fn literal_copy_resolves_to_source_definition_with_reference_type() {
    let source = "x <- 1L\ny <- x\nz <- y\n";
    let output = analyze(source);
    assert_eq!(output["schema_version"], 2);
    assert_eq!(output["scope_snapshot_kind"], "scope_exit");
    assert!(output.get("snapshot_kind").is_none());
    assert_eq!(output["capabilities"]["reference_coverage"], "partial");
    assert_eq!(
        output["capabilities"]["reference_facts"],
        "same_file_ordered_prefix"
    );
    for (fragment, name, definition_start) in [("y <- x", "x", 0), ("z <- y", "y", 8)] {
        let reference = reference_at(&output, source, fragment, name);
        assert_eq!(reference["snapshot_kind"], "reference");
        assert_eq!(reference["resolution_status"], "resolved");
        assert_eq!(reference["type_at_reference"]["mode"], "integer");
        let definition = definition(&output, &reference);
        assert_eq!(definition["name"], name);
        assert_eq!(definition["kind"], "assignment");
        assert_eq!(
            definition["span"]["bytes"],
            json!([definition_start, definition_start + 1])
        );
    }
}

#[test]
fn later_reassignment_never_lends_its_final_type_to_an_earlier_read() {
    let source = "x <- 1L\ny <- x\nx <- \"later\"\n";
    let output = analyze(source);
    let early = reference_at(&output, source, "y <- x", "x");
    assert_eq!(early["resolution_status"], "resolved");
    assert_eq!(early["type_at_reference"]["mode"], "integer");
    let top = output["files"][0]["scopes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scope| scope["kind"] == "top")
        .unwrap();
    let final_x = top["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["name"] == "x")
        .unwrap();
    assert_eq!(final_x["type"]["mode"], "character");
    assert_eq!(top["snapshot_kind"], "scope_exit");
}

#[test]
fn own_formals_resolve_unknown_but_default_derived_copies_fail_closed() {
    let source = "x <- 1L\nf <- function(x = 1L) {\n  copy <- x\n  copy\n}\n";
    let output = analyze(source);
    let formal_read = reference_at(&output, source, "copy <- x", "x");
    assert_eq!(formal_read["resolution_status"], "resolved");
    assert_eq!(formal_read["type_at_reference"]["mode"], "opaque");
    let formal = definition(&output, &formal_read);
    assert_eq!(formal["kind"], "formal");
    let formal_start = source.find("x = 1L").unwrap();
    assert_eq!(
        formal["span"]["bytes"],
        json!([formal_start, formal_start + 1])
    );
    let copied_read = reference_at(&output, source, "  copy\n}", "copy");
    assert_uncertain(&copied_read);
}

#[test]
fn function_locals_are_supported_but_outer_captures_and_early_reads_are_not() {
    let source = "outer <- 1L\nf <- function() {\n  local <- 1L\n  copy <- local\n  outer\n}\n";
    let output = analyze(source);
    let local = reference_at(&output, source, "copy <- local", "local");
    assert_eq!(local["resolution_status"], "resolved");
    assert_eq!(local["type_at_reference"]["mode"], "integer");
    assert_uncertain(&reference_at(&output, source, "  outer\n}", "outer"));

    // R may resolve the first x to the outer value; the later local x does not
    // identify that read even though it is the only assignment in this body.
    let source = "x <- \"outer\"\nf <- function() {\n  copy <- x\n  x <- 1L\n  copy\n}\n";
    let output = analyze(source);
    assert_uncertain(&reference_at(&output, source, "copy <- x", "x"));
    let source = "copy <- missing\nmissing <- 1L\n";
    let output = analyze(source);
    assert_uncertain(&reference_at(&output, source, "copy <- missing", "missing"));
}

#[test]
fn dynamic_import_control_flow_and_ordinary_calls_fail_closed() {
    for source in [
        "x <- 1L\nassign(\"x\", \"later\")\ny <- x\n",
        "x <- 1L\nlibrary(stats)\ny <- x\n",
        "x <- 1L\nif (flag) x <- \"later\"\ny <- x\n",
        "x <- 1L\nbase::identity(1L)\ny <- x\n",
        "x <- 1L\ny <- x + 1L\n",
    ] {
        let output = analyze(source);
        assert_uncertain(&reference_at(&output, source, "y <- x", "x"));
    }
}

#[test]
fn unsafe_wrappers_do_not_make_nested_function_reads_look_lexical() {
    for source in [
        "quoted <- quote(function(x) x)\n",
        "masked <- with(data, function(x) x)\n",
        "piped <- data |> identity(function(x) x)\n",
        "wrapped <- identity(function(x) x)\n",
    ] {
        let output = analyze(source);
        assert_uncertain(&reference_at(&output, source, " x)", "x"));
    }
}

#[test]
fn utf8_backticks_tabs_and_crlf_have_exact_nonduplicated_reference_spans() {
    let source =
        "`空 白` <- 1L\r\n結果 <- `空 白`\r\nf <- function(λ) {\r\n\tcopy <- λ\r\n\tcopy\r\n}\r\n";
    let output = analyze(source);
    let quoted = reference_at(&output, source, "結果 <- `空 白`", "`空 白`");
    assert_eq!(quoted["name"], "`空 白`");
    assert_eq!(quoted["resolution_status"], "resolved");
    let formal = reference_at(&output, source, "copy <- λ", "λ");
    assert_eq!(formal["resolution_status"], "resolved");
    assert_eq!(formal["span"]["start"], json!([4, 10]));
    let references = output["files"][0]["references"].as_array().unwrap();
    let mut seen = BTreeSet::new();
    for reference in references {
        let range = reference["span"]["bytes"].as_array().unwrap();
        let start = range[0].as_u64().unwrap() as usize;
        let end = range[1].as_u64().unwrap() as usize;
        assert!(
            seen.insert((start, end)),
            "duplicate reference: {reference}"
        );
        let token = &source[start..end];
        let name = reference["name"].as_str().unwrap();
        assert!(token == name || token == format!("`{name}`"));
    }
}

#[test]
fn reference_capture_is_deterministic_opt_in_and_does_not_execute_source() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("case.R"),
        "x <- 1L\ny <- x\nx <- 'later'\ny\nf <- function(arg = 1L) { copy <- arg; copy }\nmutate()\ny\n",
    )
    .unwrap();
    let legacy_before = invoke(directory.path(), false);
    let first = invoke(directory.path(), true);
    let second = invoke(directory.path(), true);
    let legacy_after = invoke(directory.path(), false);
    let legacy = parse(&legacy_before);
    let current = parse(&first);
    parse(&second);
    parse(&legacy_after);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(legacy_before.stdout, legacy_after.stdout);
    assert_eq!(legacy["schema_version"], 1);
    assert!(legacy["files"][0].get("references").is_none());
    assert!(legacy["files"][0].get("definitions").is_none());
    assert_eq!(legacy["files"][0]["scopes"], current["files"][0]["scopes"]);
    let references = current["files"][0]["references"].as_array().unwrap();
    let spans: BTreeSet<_> = references
        .iter()
        .map(|reference| reference["span"]["bytes"].to_string())
        .collect();
    assert_eq!(
        spans.len(),
        references.len(),
        "inference revisits duplicated reference records"
    );

    fs::write(
        directory.path().join("case.R"),
        "writeLines(\"executed\", \"execution-marker\")\nx <- 1L\ny <- x\n",
    )
    .unwrap();
    parse(&invoke(directory.path(), true));
    assert!(!directory.path().join("execution-marker").exists());
}

#[test]
fn forcing_a_default_can_change_a_previous_local_binding() {
    let source =
        "f <- function(p = { x <- \"changed\"; 1L }) {\n  x <- 1L\n  y <- p\n  z <- x\n}\n";
    let output = analyze(source);
    assert_uncertain(&reference_at(&output, source, "z <- x", "x"));

    let source = "f <- function(p) { x <- 1L; y <- p; z <- x }\n";
    let output = analyze(source);
    let formal = reference_at(&output, source, "y <- p", "p");
    assert_eq!(formal["resolution_status"], "resolved");
    assert_eq!(formal["type_at_reference"]["mode"], "opaque");
    assert_uncertain(&reference_at(&output, source, "z <- x", "x"));

    // The promise can install an active binding before the literal assignment,
    // so a fresh assignment cannot restore ordinary-binding certainty.
    let source = "f <- function(p) { y <- p; x <- 1L; z <- x }\n";
    let output = analyze(source);
    assert_uncertain(&reference_at(&output, source, "z <- x", "x"));
}

#[test]
fn nonlocal_and_special_assignment_operators_do_not_define_ordinary_locals() {
    for source in [
        "f <- function() { 1L ->> x; y <- x }\n",
        "f <- function() { x <<- 1L; y <- x }\n",
        "x := 1L\ny <- x\n",
    ] {
        let output = analyze(source);
        assert_uncertain(&reference_at(&output, source, "y <- x", "x"));
    }
}

#[test]
fn unsupported_outer_reads_cannot_preserve_later_local_type_certainty() {
    // An outer active-binding getter can mutate the current frame while read.
    let source = "f <- function() { local <- 1L; copy <- outer; result <- local }\n";
    let output = analyze(source);
    assert_uncertain(&reference_at(&output, source, "copy <- outer", "outer"));
    assert_uncertain(&reference_at(&output, source, "result <- local", "local"));
}

#[test]
fn equivalent_identifier_spellings_and_syntax_rebinding_fail_closed() {
    for source in [
        "x <- 1L\n`x` <- \"later\"\ny <- x\n",
        "x <- 1L\n`\\x78` <- \"later\"\ny <- x\n",
        "x <- 1L\nbase::x <- 2L\ny <- x\n",
        "`<-` <- function(...) NULL\nx <- 1L\ny <- x\n",
    ] {
        let output = analyze(source);
        assert_uncertain(&reference_at(&output, source, "y <- x", "x"));
    }
}

#[test]
fn eager_length_argument_exports_point_evidence_and_keeps_suffix_unsupported() {
    for (source, name, mode, kind) in [
        (
            "é <- 1L; base::length(é); é <- 'later'; é",
            "é",
            "integer",
            "assignment",
        ),
        (
            "f <- function(p = { x <- 'changed'; 1L }) { x <- 1L; base::length(p); x }",
            "p",
            "opaque",
            "formal",
        ),
    ] {
        let output = analyze(source);
        let fragment = format!("base::length({name})");
        let read = reference_at(&output, source, &fragment, name);
        assert_eq!(read["resolution_status"], "resolved");
        assert!(read["blocker"].is_null());
        assert_eq!(read["type_at_reference"]["mode"], mode);
        assert_eq!(definition(&output, &read)["kind"], kind);
        assert_uncertain(&reference_at(&output, source, &fragment, "base::length"));
        let references = output["files"][0]["references"].as_array().unwrap();
        assert_uncertain(references.last().unwrap());
        let repeated = analyze(source);
        assert_eq!(
            output["files"][0]["references"],
            repeated["files"][0]["references"]
        );
        assert_eq!(
            output["files"][0]["definitions"],
            repeated["files"][0]["definitions"]
        );
    }
}

#[test]
fn blocker_provenance_distinguishes_statement_prefix_and_ancestor() {
    for (source, fragment, kind, barrier) in [
        (
            "x <- 1L; base::identity(x)",
            "identity(x)",
            "containing_statement",
            "base::identity(x)",
        ),
        ("x <- 1L; mutate(); x", "; x", "prior_statement", "mutate()"),
        (
            "f <- function(x) { x }; mutate()",
            "x }",
            "inherited_scope",
            "mutate()",
        ),
    ] {
        let output = analyze(source);
        let reference = reference_at(&output, source, fragment, "x");
        assert_eq!(reference["resolution_status"], "unsupported");
        assert_eq!(reference["reason"], "unsupported_scope");
        assert!(reference["definition_id"].is_null());
        assert!(reference["type_at_reference"].is_null());
        assert_eq!(reference["blocker"]["kind"], kind, "{output}");
        assert_eq!(reference["blocker"]["cause"], "unsupported_statement");
        let start = source.find(barrier).unwrap();
        assert_eq!(
            reference["blocker"]["span"]["bytes"],
            json!([start, start + barrier.len()])
        );
        assert_eq!(
            reference["blocker"]["scope_span"]["bytes"],
            json!([0, source.len()])
        );
    }
}

#[test]
fn semantic_blocker_retains_first_unsafe_read_with_utf8_positions() {
    let source = "f <- function(π) {\r\n\tπ; π; π\r\n}";
    let output = analyze(source);
    let reads: Vec<_> = output["files"][0]["references"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|reference| reference["name"] == "π")
        .collect();
    assert_eq!(reads.len(), 3);
    assert_eq!(reads[0]["resolution_status"], "resolved");
    assert!(reads[0]["blocker"].is_null());
    let first = source.find("\tπ").unwrap() + 1;
    for reference in &reads[1..] {
        assert_eq!(reference["reason"], "after_unsafe_read");
        assert_eq!(reference["blocker"]["kind"], "prior_read");
        assert_eq!(reference["blocker"]["cause"], "unsafe_read");
        assert_eq!(
            reference["blocker"]["span"]["bytes"],
            json!([first, first + "π".len()])
        );
        assert_eq!(reference["blocker"]["span"]["start"], json!([2, 2]));
        assert_eq!(reference["blocker"]["span"]["end"], json!([2, 3]));
        assert_eq!(
            reference["blocker"]["scope_span"]["bytes"],
            json!([source.find("function").unwrap(), source.len()])
        );
        assert!(reference["definition_id"].is_null());
        assert!(reference["type_at_reference"].is_null());
    }
}

#[test]
fn whole_scope_and_ancestor_blockers_have_documented_precedence() {
    for (source, fragment, kind, cause, blocker) in [
        (
            "f <- function(x) { x; mutate(); x <- 1L; x }",
            "x; mutate",
            "whole_scope",
            "formal_write",
            "x <-",
        ),
        (
            "`x` <- 1L; mutate(); x <- 2L; x",
            "2L; x",
            "whole_scope",
            "mixed_spelling",
            "x <-",
        ),
        (
            "mutate(); f <- function(x) { x; x <- 1L }",
            "x; x",
            "inherited_scope",
            "unsupported_statement",
            "mutate()",
        ),
        (
            "x <- 1L; first(); second(x)",
            "second(x)",
            "prior_statement",
            "unsupported_statement",
            "first()",
        ),
    ] {
        let output = analyze(source);
        let reference = reference_at(&output, source, fragment, "x");
        assert_eq!(reference["reason"], "unsupported_scope", "{output}");
        assert_eq!(reference["blocker"]["kind"], kind, "{output}");
        assert_eq!(reference["blocker"]["cause"], cause, "{output}");
        let start = source.find(blocker).unwrap();
        let length = if kind == "whole_scope" {
            1
        } else {
            blocker.len()
        };
        assert_eq!(
            reference["blocker"]["span"]["bytes"],
            json!([start, start + length])
        );
        assert!(reference["definition_id"].is_null());
        assert!(reference["type_at_reference"].is_null());
    }
}

#[test]
fn inherited_blocker_keeps_the_original_nested_owner() {
    let source = "outer <- function() { inner <- function(x) { x }; mutate() }";
    let output = analyze(source);
    let reference = reference_at(&output, source, "x }", "x");
    assert_eq!(reference["blocker"]["kind"], "inherited_scope");
    let barrier = source.find("mutate()").unwrap();
    assert_eq!(
        reference["blocker"]["span"]["bytes"],
        json!([barrier, barrier + "mutate()".len()])
    );
    assert_eq!(
        reference["blocker"]["scope_span"]["bytes"],
        json!([source.find("function").unwrap(), source.len()])
    );
}

#[test]
fn inherited_semantic_blocker_keeps_the_first_read_and_outer_owner() {
    let source = "outer <- function(p) { p; inner <- function(q) q }";
    let output = analyze(source);
    let reference = reference_at(&output, source, "q }", "q");
    assert_eq!(reference["reason"], "after_unsafe_read");
    assert_eq!(reference["blocker"]["kind"], "prior_read");
    let first = source.find("p; inner").unwrap();
    assert_eq!(
        reference["blocker"]["span"]["bytes"],
        json!([first, first + 1])
    );
    assert_eq!(
        reference["blocker"]["scope_span"]["bytes"],
        json!([source.find("function").unwrap(), source.len()])
    );
    assert!(reference["definition_id"].is_null());
    assert!(reference["type_at_reference"].is_null());
}

#[test]
fn lazy_defaults_retain_their_separate_expression_spans() {
    let source = "f <- function(a = first, b = second) 1L";
    let output = analyze(source);
    for name in ["first", "second"] {
        let reference = reference_at(&output, source, name, name);
        assert_eq!(reference["reason"], "unsupported_scope");
        assert_eq!(reference["blocker"]["kind"], "default_expression");
        assert_eq!(reference["blocker"]["cause"], "lazy_default");
        let start = source.find(name).unwrap();
        assert_eq!(
            reference["blocker"]["span"]["bytes"],
            json!([start, start + name.len()])
        );
        assert_eq!(
            reference["blocker"]["scope_span"]["bytes"],
            json!([source.find("function").unwrap(), source.len()])
        );
    }
}
