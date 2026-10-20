//! RY090/RY091 point at the called formal only with call-site binding
//! evidence (`CallHead::bound_definition`). Every negative control below
//! keeps its arity finding and carries no related location.

use super::*;

/// Each arity finding with the byte ranges of its related items.
fn arity(diagnostics: &[Diagnostic]) -> Vec<(&'static str, Vec<std::ops::Range<usize>>)> {
    diagnostics
        .iter()
        .filter(|d| matches!(d.code, "RY090" | "RY091"))
        .map(|d| {
            let related = d.related.iter().map(|r| r.span.start..r.span.end).collect();
            (d.code, related)
        })
        .collect()
}

/// The byte range of the `nth` (0-based) occurrence of `needle`.
fn nth(src: &str, needle: &str, nth: usize) -> std::ops::Range<usize> {
    let start = src.match_indices(needle).nth(nth).unwrap().0;
    start..start + needle.len()
}

#[test]
fn arity_findings_point_at_the_declared_formal() {
    let src = "# Größe\nhelper <- function(lbl = \"𝑥\", scale) nchar(lbl) * scale\nuse <- function() {\n  é <- \"a\"; helper(é)\n}\ntypo <- function(é) helper(lbl = é, sclae = 2)\n";
    let diagnostics = check(src);
    let scale = nth(src, "scale", 0);
    assert_eq!(scale, 43..48);
    assert_eq!(
        arity(&diagnostics),
        [
            ("RY091", vec![scale.clone()]),
            ("RY090", vec![scale.clone()]),
            ("RY091", vec![scale.clone()]),
        ]
    );
    let messages: Vec<_> = diagnostics
        .iter()
        .flat_map(|d| &d.related)
        .map(|r| (r.path.as_str(), r.message.as_str()))
        .collect();
    assert_eq!(
        messages,
        [
            ("test.R", "`scale` is declared here without a default value"),
            ("test.R", "`scale` is declared here"),
            ("test.R", "`scale` is declared here without a default value"),
        ]
    );
}

#[test]
fn eager_calls_relate_to_the_current_binding() {
    let src = "helper <- function(x, scale) x * scale\nhelper(1)\n";
    assert_eq!(arity(&check(src)), [("RY091", vec![nth(src, "scale", 0)])]);
    // Duplicate names: the second literal is the binding, never the first.
    let src = "top <- function(k, l) k + l\ntop <- function(k, l) l - k\ntop(2)\n";
    assert_eq!(arity(&check(src)), [("RY091", vec![nth(src, "l", 2)])]);
}

#[test]
fn a_backticked_formal_relates_to_its_whole_spelling() {
    let src = "helper <- function(`my arg`, scale) `my arg` + scale\nhelper(scale = 1)\n";
    assert_eq!(
        arity(&check(src)),
        [("RY091", vec![nth(src, "`my arg`", 0)])]
    );
}

#[test]
fn a_nested_shadow_is_never_cited_for_a_global_call() {
    // `g`'s own `helper` is a lexical callable (unchecked); `f`'s call
    // reaches the global literal.
    let src = "helper <- function(x, scale) x * scale\ng <- function() {\n  helper <- function(x) x\n  helper(1)\n}\nf <- function() helper(1)\n";
    assert_eq!(arity(&check(src)), [("RY091", vec![nth(src, "scale", 0)])]);
}

#[test]
fn unproven_origins_keep_the_finding_without_a_location() {
    let helper = "helper <- function(x, scale) x * scale\n";
    for tail in [
        // Used before the definition.
        "helper(1)\nhelper <- function(x, scale) x * scale\n",
        // A deferred body before a conditional definition.
        "use <- function() identity(1)\nif (FALSE) identity <- function(x, scale) x * scale\nuse()\n",
        // A default may rebind the name in the calling frame.
        &format!(
            "{helper}use <- function(v = {{ helper <- function(x) x; 1 }}) {{ v; helper(1) }}\n"
        ),
        // Unproven calls, eager and deferred.
        &format!("{helper}do.call(\"<-\", list(\"helper\", function(x) x))\nhelper(1)\n"),
        &format!(
            "{helper}use <- function() helper(1)\ndo.call(\"<-\", list(\"helper\", function(x) x))\n"
        ),
        &format!(
            "{helper}paste <- assign\ndo.call(\"paste\", list(\"helper\", function(x) x))\nhelper(1)\n"
        ),
        &format!(
            "{helper}use <- function() helper(1)\npaste <- assign\ndo.call(\"paste\", list(\"helper\", function(x) x))\n"
        ),
        &format!("{helper}base::get(\"assign\")(\"helper\", function(x) x)\nhelper(1)\n"),
        &format!(
            "{helper}use <- function() helper(1)\nbase::get(\"assign\")(\"helper\", function(x) x)\n"
        ),
        &format!(
            "{helper}use <- function() {{\n  do.call(\"<-\", list(\"helper\", function(x) x))\n  helper(1)\n}}\n"
        ),
        // An alias retains the first literal; its name now binds another.
        "identity <- function(x, scale) x * scale\nf <- identity\nidentity <- function(x, scale) x + scale\nf(1)\n",
        // Code that may run later than its position.
        &format!(
            "{helper}f <- function() {{\n  on.exit(helper(1))\n  do.call(\"<-\", list(\"helper\", function(x) x))\n}}\n"
        ),
        &format!("{helper}f <- function(v = helper(1)) v\n"),
        &format!("{helper}lapply(1:2, function(i) helper(i))\n"),
        // Sol code review r1: an unproven default, an earlier loop iteration,
        // a project replacement function, and project dispatch from a safe call.
        &format!(
            "{helper}use <- function(v = do.call(\"<-\", list(\"helper\", function(x) x))) {{\n  v\n  helper(1)\n}}\n"
        ),
        &format!(
            "{helper}for (i in 1:2) {{\n  if (i == 2) helper(1)\n  do.call(\"<-\", list(\"helper\", function(x) x))\n}}\n"
        ),
        &format!(
            "{helper}`names<-` <- function(x, value) {{\n  do.call(\"<<-\", list(\"helper\", function(x) x))\n  x\n}}\nx <- 1\nnames(x) <- \"a\"\nhelper(1)\n"
        ),
        &format!(
            "{helper}length.foo <- function(x) {{\n  do.call(\"<<-\", list(\"helper\", function(x) x))\n  1L\n}}\nuse <- function(x) {{\n  length(x)\n  helper(1)\n}}\n"
        ),
        &format!("{helper}formals(helper) <- pairlist(x = 1)\nhelper(1)\n"),
        // pullfrog: a short-circuit operand's call reaches the continuation.
        &format!(
            "{helper}flag <- TRUE\nok <- flag && do.call(\"<-\", list(\"helper\", function(x) x))\nhelper(1)\n"
        ),
        // A project helper may rebind the global through `do.call` before
        // an outside caller runs `use()`.
        &format!(
            "{helper}swap <- function() do.call(\"<<-\", list(\"helper\", function(x) x))\nuse <- function() helper(1)\n"
        ),
        &format!(
            "{helper}swap <- function() eval.parent(quote(helper <- function(x) x))\nuse <- function() helper(1)\n"
        ),
        &format!(
            "{helper}swap <- function() list2env(list(helper = function(x) x), envir = parent.frame())\nuse <- function() helper(1)\n"
        ),
        // Captured from an intermediate frame.
        &format!("{helper}f <- function() {{\n  g <- function() helper(1)\n  g\n}}\n"),
        // Expected absence: `local()` runs code in the global frame's flow.
        &format!(
            "{helper}local({{ helper <- function(x) x; helper(1) }})\nf <- function() {{ helper(1) }}\n"
        ),
    ] {
        let found = arity(&check(tail));
        assert!(!found.is_empty(), "{tail}: the finding itself is unchanged");
        assert!(
            found.iter().all(|(_, related)| related.is_empty()),
            "{tail}: {found:?}"
        );
    }
}

#[test]
fn a_recognized_outward_write_drops_the_finding_like_superassignment() {
    // As for `helper <<- ...` in `swap`, the project call is no longer checked.
    for swap in [
        "helper <<- function(x) x",
        "local(helper <- function(x) x, envir = parent.frame())",
    ] {
        let src = format!(
            "helper <- function(x, scale) x * scale\nswap <- function() {swap}\nuse <- function() helper(1)\n"
        );
        assert_eq!(arity(&check(&src)), [], "{src}");
    }
}

#[test]
fn stub_calls_carry_no_location() {
    assert_eq!(
        arity(&check("length(xx = 1L)\n")),
        [("RY090", vec![]), ("RY091", vec![])]
    );
}

fn project_arity(
    files: &[(&str, &str)],
) -> Vec<(String, &'static str, Vec<std::ops::Range<usize>>)> {
    let mut project = Project::new();
    for (path, src) in files {
        project.add_file(path.to_string(), parse_file(path, src));
    }
    project
        .check()
        .into_iter()
        .flat_map(|(path, diagnostics)| {
            arity(&diagnostics)
                .into_iter()
                .map(move |(code, related)| (path.clone(), code, related))
        })
        .collect()
}

#[test]
fn project_evidence_stays_within_one_file() {
    let helper = "helper <- function(x, scale) x * scale\n";
    let same = format!("{helper}use <- function() helper(1)\n");
    assert_eq!(
        project_arity(&[("a.R", &same)]),
        [("a.R".into(), "RY091", vec![nth(&same, "scale", 0)])]
    );
    // Cross-file provenance is not established.
    assert_eq!(
        project_arity(&[
            ("helpers.R", helper),
            ("calls.R", "use <- function() helper(1)\n")
        ]),
        [("calls.R".into(), "RY091", vec![])]
    );
    // Another file's unproven top level may rebind the shared global.
    assert_eq!(
        project_arity(&[
            ("a.R", &same),
            ("b.R", "do.call(\"<-\", list(\"helper\", function(x) x))\n")
        ]),
        [("a.R".into(), "RY091", vec![])]
    );
    // Duplicate agreeing definitions in two files.
    let found = project_arity(&[("a.R", &same), ("b.R", helper)]);
    assert!(
        found.iter().all(|(_, _, related)| related.is_empty()),
        "{found:?}"
    );
}

#[test]
fn moved_definitions_relate_to_their_new_position() {
    let src = "helper <- function(x, scale) x * scale\nuse <- function() helper(1)\n";
    let mut project = Project::new();
    project.add_file("use.R".into(), parse_file("use.R", src));
    project.add_file(
        "other.R".into(),
        parse_file("other.R", "other <- function() 1\n"),
    );
    let _ = project.check();
    let moved = format!("\n\n{src}");
    project.update_file("use.R".into(), Arc::new(parse_file("use.R", &moved)));
    let warm = project.check_incremental();
    assert_eq!(warm, project.check(), "warm and cold results agree");
    let related: Vec<_> = warm
        .iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .flat_map(|d| &d.related)
        .map(|r| (r.span.start..r.span.end, r.span.line))
        .collect();
    assert_eq!(related, [(nth(&moved, "scale", 0), 2)]);
}
