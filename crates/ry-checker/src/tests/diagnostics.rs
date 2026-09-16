use super::*;

// ---- inline suppression comment tests ----

fn scan_comments(src: &str) -> Vec<ry_core::ast::Comment> {
    parse_file("test.R", src).comments
}

#[test]
fn parse_trailing_ignore_comment() {
    let src = "x <- bad  # ry: ignore\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 0);
    assert!(supps[0].rules.is_empty()); // suppress all
}

#[test]
fn parse_specific_rule_ignore() {
    let src = "x <- \"a\" * 3  # ry: ignore[RY040]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].rules, vec!["RY040"]);
}

#[test]
fn parse_multiple_rules() {
    let src = "x <- bad  # ry: ignore[RY040, RY010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY040".to_string()));
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_standalone_comment_applies_to_next_line() {
    let src = "# ry: ignore\nx <- bad\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 1);
}

#[test]
fn parse_standalone_comment_skips_blank_lines() {
    let src = "# ry: ignore\n\nx <- bad\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].line, 2);
}

#[test]
fn parse_noqa_alias() {
    let src = "x <- bad  # noqa: RY010\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_bare_noqa_suppresses_all() {
    let src = "x <- bad  # noqa\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.is_empty());
}

#[test]
fn parse_noqa_bracket_form() {
    let src = "x <- bad  # noqa[RY010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_compact_ry_ignore_no_space() {
    let src = "x <- bad  # ry:ignore[RY010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_case_insensitive_marker() {
    let src = "x <- bad  # RY: IGNORE[ry010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].rules.contains(&"RY010".to_string()));
}

#[test]
fn parse_non_suppression_comment_is_ignored() {
    let src = "# just a regular comment\nx <- bad\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert!(supps.is_empty());
}

#[test]
fn parse_file_level_suppression() {
    assert!(has_file_suppression_from_comments(&scan_comments(
        "# ry: ignore-file\nx <- bad\n"
    )));
    assert!(has_file_suppression_from_comments(&scan_comments(
        "# ry:ignore-file\nx <- bad\n"
    )));
    assert!(!has_file_suppression_from_comments(&scan_comments(
        "# ry: ignore\nx <- bad\n"
    )));
}

#[test]
fn file_level_marker_not_treated_as_line_level() {
    // `# ry: ignore-file` must NOT also register as a line-level
    // "ignore all" (it's handled by has_file_suppression_from_comments
    // instead).
    let src = "# ry: ignore-file\nx <- bad\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert!(
        supps.is_empty(),
        "ignore-file should not produce line-level suppressions, got {:?}",
        supps
    );
}

#[test]
fn is_suppressed_matches_line_and_code() {
    let supps = vec![Suppression {
        line: 2,
        rules: vec!["RY010".to_string()],
    }];
    let diag_matching = Diagnostic {
        severity: Severity::Warning,
        span: Span {
            start: 0,
            end: 1,
            line: 2,
            col: 0,
        },
        path: "x.R".into(),
        code: "RY010",
        message: "test".into(),
        confidence: Confidence::Medium,
    };
    let diag_wrong_line = Diagnostic {
        span: Span {
            line: 0,
            ..diag_matching.span
        },
        ..diag_matching.clone()
    };
    let diag_wrong_code = Diagnostic {
        code: "RY040",
        ..diag_matching.clone()
    };
    assert!(is_suppressed(&diag_matching, &supps));
    assert!(!is_suppressed(&diag_wrong_line, &supps));
    assert!(!is_suppressed(&diag_wrong_code, &supps));
}

#[test]
fn is_suppressed_empty_rules_matches_any_code() {
    let supps = vec![Suppression {
        line: 0,
        rules: vec![],
    }];
    let diag = Diagnostic {
        severity: Severity::Warning,
        span: Span {
            start: 0,
            end: 1,
            line: 0,
            col: 0,
        },
        path: "x.R".into(),
        code: "RY999",
        message: "test".into(),
        confidence: Confidence::Medium,
    };
    assert!(is_suppressed(&diag, &supps));
}

#[test]
fn filter_suppressed_end_to_end() {
    // Trailing `# ry: ignore[RY010]` on the offending line drops RY010.
    let src = "x <- undefined_var  # ry: ignore[RY010]\n";
    let diags = check(src);
    let filtered = filter_suppressed_with_comments(diags, &scan_comments(src), src);
    assert!(
        filtered.iter().all(|d| d.code != "RY010"),
        "RY010 should be suppressed, got {:?}",
        filtered
    );
}

#[test]
fn filter_suppressed_file_level_drops_everything() {
    let src = "# ry: ignore-file\nx <- undefined_var\n";
    let diags = check(src);
    let filtered = filter_suppressed_with_comments(diags, &scan_comments(src), src);
    assert!(
        filtered.is_empty(),
        "file-level suppression should drop all diagnostics, got {:?}",
        filtered
    );
}

#[test]
fn filter_suppressed_other_rules_still_fire() {
    // Suppressing RY010 on line 0 should NOT affect RY040 on line 1.
    let src = "x <- undefined_var  # ry: ignore[RY010]\ny <- \"a\" * 3L\n";
    let diags = check(src);
    let filtered = filter_suppressed_with_comments(diags, &scan_comments(src), src);
    assert!(
        filtered.iter().any(|d| d.code == "RY040"),
        "RY040 should still fire (it's on a different line), got {:?}",
        filtered
    );
    assert!(
        filtered.iter().all(|d| d.code != "RY010"),
        "RY010 should be suppressed"
    );
}

/// Idempotence: running the checker twice on the same
/// input must yield identical diagnostics. The fixpoint/refinement
/// machinery walks function tables whose iteration order is not
/// semantically meaningful, so any order-leak that bleeds into
/// observed types would show up here.
#[test]
fn diagnostics_are_deterministic_across_runs() {
    let sources = [
        // recursion (cycle detection in the fixpoint)
        "f <- function(n) { if (n > 0) f(n - 1) else 0L }\nx <- f(3) + 1\n",
        // mutual / cross-referencing function bodies
        "f <- function() { g() }\ng <- function() { 1L }\nx <- f() + 1\n",
        // a body with an arithmetic error + unbound var (exercises the
        // function-body walk in both passes)
        "h <- function() { a <- \"x\" + 1; b <- missing_thing }\n",
        // higher-order callback inference
        "v <- sapply(c(1.0, 2.0), function(x) x * 2)\ny <- v + 1\n",
        // a clean file (no diagnostics) with a closure factory
        "make_adder <- function(x) function(y) x + y\nadd5 <- make_adder(5)\nz <- add5(3)\n",
    ];
    for src in sources {
        let d1 = check(src);
        let d2 = check(src);
        // Compare on the semantically meaningful fields; `Diagnostic`
        // also carries `path` (constant here) and `message` (stable).
        let key = |d: &Diagnostic| (d.code, d.severity, d.span.start, d.span.end);
        let k1: Vec<_> = d1.iter().map(key).collect();
        let k2: Vec<_> = d2.iter().map(key).collect();
        assert_eq!(
            k1, k2,
            "non-deterministic diagnostics for src={src:?}\n  run1={d1:?}\n  run2={d2:?}"
        );
    }
}

#[test]
fn public_check_with_scope_surfaces_ry000_on_broken_file() {
    // Regression: `check_with_scope` used to clear diagnostics
    // AFTER emitting parse errors, wiping the RY000s. It must now
    // surface them.
    let (diags, _scope) = check_with_scope("f <- function( { 1 }\n");
    assert!(
        diags.iter().any(|d| d.code == "RY000"),
        "check_with_scope must surface RY000 on a broken file, got {:?}",
        diags
    );
}

#[test]
fn public_check_emits_each_parse_error_once() {
    let file = parse_file("test.R", "f <- function( { 1 }\n");
    let expected = file.parse_errors.len();
    assert!(expected > 0, "fixture must contain a parse error");
    let mut checker = Checker::new("test.R");
    let actual = checker
        .check(&file)
        .iter()
        .filter(|diagnostic| diagnostic.code == "RY000")
        .count();

    assert_eq!(
        actual, expected,
        "each parser error must produce exactly one RY000"
    );
}

// ---- recovered-tree semantic suppression (issue #380) ----

#[test]
fn recovered_tree_suppresses_semantic_diagnostics() {
    // The incomplete `function(` header makes tree-sitter invent a call
    // expression over the rest of the file; before the suppression the
    // unbound-variable rule reported `x` (and, on corpus files, names as
    // empty as ``) on top of the parse error. RY000 must be the only
    // survivor: it is the actionable signal for a recovered tree.
    let diags = check("x <- function(\n");
    assert!(
        diags.iter().any(|d| d.code == "RY000"),
        "broken file must still report RY000, got {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code == "RY000"),
        "recovered tree must not carry semantic diagnostics, got {diags:?}"
    );
}

#[test]
fn recovered_tree_suppression_leaves_clean_files_alone() {
    // The same shape with the syntax repaired keeps its semantic finding:
    // suppression keys on the presence of parse errors, not on the
    // construct that happened to be broken.
    let clean = check("print(undefined_thing)\n");
    assert!(
        clean.iter().any(|d| d.code == "RY010"),
        "clean file must keep semantic diagnostics, got {clean:?}"
    );
}

// ---- dynamic formals/body construction (issue #380) ----

#[test]
fn formals_replacement_makes_placeholder_literal_opaque() {
    // distr6's genExp shape: the placeholder literal references `x`,
    // which only the alist() installed by formals<- binds at runtime.
    // The placeholder walk must drop that RY010 (the whole point of
    // #380); the construction statements themselves are clean R.
    let diags = check(
        "gen_exp <- function(trafo = NULL) {\n\
         \x20 if (is.null(trafo)) {\n\
         \x20   trafo <- function() {\n\
         \x20     return(x)\n\
         \x20   }\n\
         \x20   formals(trafo) <- alist(x = )\n\
         \x20 }\n\
         \x20 trafo\n\
         }\n",
    );
    assert!(
        diags.is_empty(),
        "formals<- placeholder must stay quiet, got {diags:?}"
    );
}

#[test]
fn body_replacement_makes_placeholder_literal_opaque() {
    // The makeChecks shape: an empty placeholder whose body is supplied
    // by body(value) <- substitute(...) and whose formals come from an
    // alist() parameter. Neither the placeholder internals nor the
    // construction site may report.
    let diags = check(
        "make_check <- function(cond, args = alist(object = )) {\n\
         \x20 value <- function() {}\n\
         \x20 formals(value) <- args\n\
         \x20 body(value) <- substitute(assertThat(object, arg1), list(arg1 = cond))\n\
         \x20 value\n\
         }\n",
    );
    assert!(
        diags.is_empty(),
        "body<- placeholder must stay quiet, got {diags:?}"
    );
}

#[test]
fn formals_opacity_is_lexically_scoped() {
    // A formals<- inside one function body must not silence the
    // same-named closure in a different scope, nor a body that contains
    // no replacement at all.
    let diags = check(
        "outer <- function() {\n\
         \x20 trafo <- function() x\n\
         \x20 formals(trafo) <- alist(x = )\n\
         \x20 trafo\n\
         }\n\
         top_trafo <- function() x\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY010").count(),
        1,
        "only the placeholder's internals may go quiet, got {diags:?}"
    );
}

#[test]
fn environment_replacement_grants_no_opacity() {
    // environment(f) <- ... changes the closure's enclosure, not its
    // formals or body: the placeholder's unbound names stay reportable.
    let diags = check(
        "f <- function() {\n\
         \x20 still_unbound\n\
         }\n\
         environment(f) <- globalenv()\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "environment<- must not make the literal opaque, got {diags:?}"
    );
}

#[test]
fn formals_opacity_keeps_nested_closure_findings() {
    // formals(f) <- replaces the formals list only: closures defined in
    // the placeholder body survive verbatim, so their unbound names are
    // still genuine runtime errors and stay reportable...
    let diags = check(
        "f <- function() {\n\
         \x20 g <- function() undefined_name\n\
         \x20 g\n\
         }\n\
         formals(f) <- alist(x = )\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY010").count(),
        1,
        "a formals-only placeholder keeps nested-closure findings, got {diags:?}"
    );
    // ...while body(f) <- discards the walked body wholesale, nested
    // closures included, and their diagnostics go with it.
    let diags = check(
        "f <- function() {\n\
         \x20 g <- function() undefined_name\n\
         \x20 g\n\
         }\n\
         body(f) <- substitute(x + 1)\n",
    );
    assert!(
        diags.is_empty(),
        "a body<- placeholder is fully opaque, nested closures included, got {diags:?}"
    );
}

#[test]
fn formals_only_placeholder_keeps_typed_map_ry080() {
    // A typed-map call in a formals-only placeholder body is genuine:
    // the formals swap leaves the body (and the inline callback, closed
    // over nothing the alist provides) verbatim, and R errors "Can't
    // coerce from a string to a double". RY080 anchors at the map call,
    // never inside the nested callback the containment check exempts,
    // so it must survive the placeholder filter (review P1).
    let diags = check(
        "library(purrr)\n\
         f <- function() {\n\
         \x20 map_dbl(1:3, function(z) \"nope\")\n\
         }\n\
         formals(f) <- alist(x = )\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY080").count(),
        1,
        "a formals-only placeholder keeps its typed-map RY080, got {diags:?}"
    );
    // Only body<- invalidates the callback result: the replacement
    // discards the walked body, the map call included.
    let diags = check(
        "library(purrr)\n\
         f <- function() {\n\
         \x20 map_dbl(1:3, function(z) \"nope\")\n\
         }\n\
         body(f) <- substitute(x + 1)\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == "RY080"),
        "a body<- placeholder silences its typed-map RY080, got {diags:?}"
    );
}

#[test]
fn placeholder_association_is_source_ordered() {
    // A replacement marks only the literal bound to its name at that
    // point in the statement list. A rebind ends the association --
    // formals<- modified the first closure, and the rebound one is a
    // fresh object whose unbound names are genuine errors (review P2).
    let diags = check(
        "f <- function() x\n\
         formals(f) <- alist(x = )\n\
         f <- function() other_unbound\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY010").count(),
        1,
        "the rebound literal keeps its RY010, got {diags:?}"
    );
    // A replacement with no preceding literal binding marks nothing:
    // at runtime it errors (or touches some other closure), and the
    // later literal is an ordinary definition.
    let diags = check(
        "formals(g) <- alist(h = )\n\
         g <- function() later_unbound\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY010").count(),
        1,
        "a replacement before the literal grants no opacity, got {diags:?}"
    );
    // Sibling branches sharing a name: the else-branch literal is a
    // rebind in source order and keeps its finding.
    let diags = check(
        "pick <- function(cond) {\n\
         \x20 if (cond) {\n\
         \x20   f <- function() x\n\
         \x20   formals(f) <- alist(x = )\n\
         \x20 } else {\n\
         \x20   f <- function() else_unbound\n\
         \x20 }\n\
         \x20 f\n\
         }\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY010").count(),
        1,
        "the else-branch rebind keeps its RY010, got {diags:?}"
    );
}

#[test]
fn local_block_replacement_stays_inside_local() {
    // local({...}) evaluates its block in a fresh environment: the
    // formals<- inside assigns the modified closure to that environment
    // only, and the outer binding is untouched -- its unbound name is a
    // genuine runtime error (review P2). The boundary holds in both
    // directions: a literal inside the block pairs only with the
    // block's own replacements.
    let diags = check(
        "f <- function() undefined_name\n\
         local({ formals(f) <- alist(x = ) })\n",
    );
    assert_eq!(
        diags.iter().filter(|d| d.code == "RY010").count(),
        1,
        "a replacement inside local() must not reach an outer literal, got {diags:?}"
    );
    let diags = check(
        "local({\n\
         \x20 f <- function() x\n\
         \x20 formals(f) <- alist(x = )\n\
         })\n",
    );
    assert!(
        diags.is_empty(),
        "literal and replacement inside one local() block stay one scope, got {diags:?}"
    );
}

#[test]
fn placeholder_kind_ordering_is_upgrade_only() {
    // body<- is the stronger replacement in either order: formals<-
    // after body<- must not downgrade the kind back to FormalsOnly,
    // which would resurrect the nested closure's RY010 and (per the
    // P1 fix) keep RY080. Both orders with a nested closure go fully
    // opaque (review P3: previously verified only by hand).
    let diags = check(
        "f <- function() {\n\
         \x20 g <- function() nested_unbound\n\
         \x20 g\n\
         }\n\
         formals(f) <- alist(a = )\n\
         body(f) <- substitute(x + 1)\n",
    );
    assert!(
        !diags.iter().any(|d| matches!(d.code, "RY010" | "RY080")),
        "formals<- then body<- stays BodyReplaced (upgrade), got {diags:?}"
    );
    let diags = check(
        "f <- function() {\n\
         \x20 g <- function() nested_unbound\n\
         \x20 g\n\
         }\n\
         body(f) <- substitute(x + 1)\n\
         formals(f) <- alist(a = )\n",
    );
    assert!(
        !diags.iter().any(|d| matches!(d.code, "RY010" | "RY080")),
        "body<- then formals<- stays BodyReplaced (no downgrade), got {diags:?}"
    );
}

// ---- invalid UTF-8 (encoding) diagnostics, #376 ----

/// Parse `src` (already decoded, as the frontends hand it over) and mark
/// it with an invalid-UTF-8 span, exactly like the CLI/LSP read
/// boundary does for a transcoded file. The span points into the
/// decoded text at the byte where the original file was not UTF-8.
fn parse_transcoded(src: &str, invalid: Span) -> SourceFile {
    let mut file = parse_file("test.R", src);
    file.invalid_utf8 = vec![invalid];
    file
}

fn invalid_utf8_ry000(src: &str, invalid: Span) -> Vec<Diagnostic> {
    let mut checker = Checker::new("test.R");
    let file = parse_transcoded(src, invalid);
    checker.check(&file);
    checker
        .take_diagnostics()
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "RY000")
        .collect()
}

#[test]
fn invalid_utf8_in_a_string_is_flagged_as_ry000() {
    // Decoded form of `label <- "caf\xe9"`; the invalid byte became the
    // `é` at decoded bytes 9..11.
    let diags = invalid_utf8_ry000("label <- \"café\"\n", Span::new(9, 11, 0, 9));
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(
        diags[0].message.contains("not valid UTF-8"),
        "message should name the encoding problem: {}",
        diags[0].message
    );
    assert_eq!(diags[0].span.start, 9);
}

/// R's parser tolerates invalid bytes inside comments; a legacy file
/// whose only non-ASCII bytes sit in comments parses fine in R and must
/// stay clean in ry too.
#[test]
fn invalid_utf8_inside_a_comment_stays_clean_like_r() {
    // Decoded form of `# K\xf6lner Kommentar`: the whole high byte is
    // inside the comment starting at column 0.
    assert!(invalid_utf8_ry000("# Kölner Kommentar\nx <- 1\n", Span::new(2, 4, 0, 2)).is_empty());
    // Trailing comment after code: everything from `#` on is comment.
    assert!(invalid_utf8_ry000("x <- 1  # café\n", Span::new(11, 13, 0, 11)).is_empty());
}

/// A comment later on the same line does not shield an invalid byte
/// that precedes it: `s <- "café"  # ok` is rejected by R.
#[test]
fn invalid_utf8_before_a_trailing_comment_is_flagged() {
    let diags = invalid_utf8_ry000("s <- \"café\"  # ok\n", Span::new(7, 9, 0, 7));
    assert_eq!(diags.len(), 1, "{diags:?}");
}

/// R's lexer scans `%...%` special-operator tokens as raw bytes with no
/// multibyte validation (gram.y `SpecialValue`): `parse()` ACCEPTS
/// `10 %café% 5` and only `source()`/`Rscript` fail later, at
/// evaluation, with "could not find function". Invalid bytes inside the
/// operator token therefore must not flag.
#[test]
fn invalid_utf8_inside_a_special_operator_stays_clean_like_r() {
    // Decoded form of `x <- 10 %caf\xe9% 5`; the `é` sits at decoded
    // bytes 12..14, inside the `%café%` token.
    assert!(invalid_utf8_ry000("x <- 10 %café% 5\n", Span::new(12, 14, 0, 12)).is_empty());
    // A built-in special operator shields the same way.
    assert!(invalid_utf8_ry000("z <- a %ïn% b\n", Span::new(10, 12, 0, 10)).is_empty());
}

/// The operator tolerance only shields bytes inside the token: an
/// invalid byte in a string on the same line still flags, and the
/// operator bytes themselves do not shield later lines.
#[test]
fn invalid_utf8_beside_a_special_operator_still_flags_the_string() {
    // Decoded `ok <- 1 %café% 2` + `bad <- "café"`: only the string's
    // invalid byte is outside every tolerated token.
    let diags = invalid_utf8_ry000(
        "ok <- 1 %café% 2\nbad <- \"café\"\n",
        Span::new(28, 30, 1, 12),
    );
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].span.line, 1);
}

/// Like R ("invalid multibyte character in parser" reports the first
/// bad character and stops), only the first invalid sequence outside
/// every tolerated token (comment, `%...%` operator) is diagnosed,
/// even when the decode step recorded many.
#[test]
fn only_the_first_untolerated_invalid_sequence_is_reported() {
    let mut file = parse_file("test.R", "a <- \"café\"; b <- \"café\"\n");
    file.invalid_utf8 = vec![Span::new(5, 7, 0, 5), Span::new(17, 19, 0, 17)];
    let mut checker = Checker::new("test.R");
    checker.check(&file);
    let diagnostics = checker.take_diagnostics();
    let encoding: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| d.code == "RY000" && d.message.contains("not valid UTF-8"))
        .collect();
    assert_eq!(encoding.len(), 1, "{encoding:?}");
    assert_eq!(encoding[0].span.start, 5);
}

/// Valid UTF-8 files carry no invalid spans, so nothing fires.
#[test]
fn valid_utf8_source_emits_no_encoding_diagnostic() {
    let diags = check("label <- \"café\"\n");
    assert!(
        diags.iter().all(|d| d.code != "RY000"),
        "valid UTF-8 must not be flagged: {diags:?}"
    );
}

/// Like a recovered tree (#380), a file flagged for non-UTF-8 source
/// reports only its RY000: semantic findings over a lossy transcoding
/// are noise on top of the encoding failure R already reports.
#[test]
fn encoding_ry000_suppresses_semantic_diagnostics() {
    let mut file = parse_file("test.R", "missing_name\nlabel <- \"café\"\n");
    // The `é` in line 1's string, at decoded bytes 26..28.
    file.invalid_utf8 = vec![Span::new(26, 28, 1, 13)];
    let mut checker = Checker::new("test.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, "RY000");
    assert!(diags[0].message.contains("not valid UTF-8"), "{diags:?}");
}

// ---- comparison-in-call & format arity (moved from packages_typeshed) ----
#[test]
fn comparison_directly_inside_length_is_diagnosed() {
    let diags = check("if (length(x == y)) print(\"bad\")\nok <- length(x) == y\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        1,
        "only the comparison nested directly under length should fire: {diags:?}"
    );
}

#[test]
fn comparison_inside_selected_scalar_calls_is_diagnosed() {
    let diags = check("length(x > 0)\nnchar(x == y)\nabs(x != y)\nsum(x > 0)\nlength(x) > 0\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        2,
        "length and nchar should fire under RY093: {diags:?}"
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY100")
            .count(),
        1,
        "abs should fire under RY100, but sum and an outer comparison should not: {diags:?}"
    );
}

#[test]
fn comparison_directly_inside_numeric_math_is_diagnosed() {
    let diags = check(
        "abs(x > y)\nabs(x) > y\nsqrt(a == b)\nsum(x > 0)\nlog(x, base = 2)\nabs(x %in% y)\nabs((x > y))\n",
    );
    let math: Vec<_> = diags
        .iter()
        .filter(|diagnostic| diagnostic.code == "RY100")
        .collect();
    assert_eq!(
        math.len(),
        3,
        "only direct ordinary comparisons, including extra parentheses, should fire: {diags:?}"
    );
    assert!(
        math.iter()
            .all(|diagnostic| diagnostic.severity == Severity::Warning),
        "RY100 must be a warning: {diags:?}"
    );
}

// ---- any()/all() scalar comparison (RY107) ----

#[test]
fn any_all_scalar_comparison_negating_literal_is_diagnosed() {
    // glue R/utils.R:32. `any(lengths) == 0` computes `!any(lengths)`
    // (FALSE == 0 is TRUE), so the emptiness guard never runs.
    let diags = check(
        "u <- function(x) {\n  lengths <- vapply(x, NROW, integer(1))\n  if (any(lengths) == 0) return(character())\n}\n",
    );
    let hits: Vec<_> = diags
        .iter()
        .filter(|diagnostic| diagnostic.code == "RY107")
        .collect();
    assert_eq!(hits.len(), 1, "expected one RY107: {diags:?}");
    assert!(hits[0].message.contains("any(lengths == 0)"));
    assert!(hits[0].severity == Severity::Warning);
}

#[test]
fn any_all_scalar_comparison_constant_outcome_is_diagnosed() {
    // TRUE > 1 and FALSE > 1 are both FALSE, so the guard is dead.
    let diags = check("f <- function(x) if (any(x > 1) || all(x) > 1) 1\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY107")
            .count(),
        1,
        "only the constant all(x) > 1 comparison should fire: {diags:?}"
    );
}

#[test]
fn any_all_scalar_comparison_preserving_idioms_stay_silent() {
    // `== 1`, `> 0`, `>= 1`, and `!= 0` compute exactly the bare call's
    // value; diffobj's `!all(diff(x)) == 1L` (pinned in
    // ry095_ry096_real_shapes.R) is this family written on purpose.
    let diags = check(
        "a <- function(x) !all(diff(x)) == 1L\nb <- function(x) any(x) == 1\nc <- function(x) any(x) > 0\nd <- function(x) all(x) >= 1\ne <- function(x) any(x) != 0\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == "RY107"),
        "value-preserving comparisons are the deliberate idiom: {diags:?}"
    );
}

#[test]
fn any_all_scalar_comparison_correct_spellings_stay_silent() {
    let diags = check(
        "a <- function(x) any(x == 0)\nb <- function(x) all(x != 1)\nc <- function(x) any(x > 1)\nd <- function(x) any(x, na.rm = TRUE) == 0\n",
    );
    assert!(
        !diags.iter().any(|d| d.code == "RY107"),
        "comparison inside any()/all() is the intended spelling: {diags:?}"
    );
}

#[test]
fn any_all_scalar_comparison_shadowed_callee_stays_silent() {
    // A local `any` need not return a length-1 logical, so the scalar
    // premise does not hold.
    let diags =
        check("f <- function(v) {\n  any <- function(x) c(TRUE, FALSE)\n  if (any(v) == 0) 1\n}\n");
    assert!(
        !diags.iter().any(|d| d.code == "RY107"),
        "shadowed any() must not fire: {diags:?}"
    );
}

#[test]
fn any_all_scalar_comparison_yields_to_same_span_neighbors() {
    // RY093 and RY100 already report a comparison nested directly inside
    // length()/nchar()/math calls on the same span; RY107 must not
    // double-report it. RY105 owns the `length(any(...))` wrapper shape.
    let diags =
        check("a <- length(any(x) == 0)\nb <- abs(any(x) == 0)\nc <- if (length(any(1L)) > 0) 1\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY107")
            .count(),
        0,
        "RY093/RY100 own the inside-aggregate spans and RY105 owns the wrapped length: {diags:?}"
    );
    assert!(diags.iter().any(|d| d.code == "RY093"));
    assert!(diags.iter().any(|d| d.code == "RY100"));
    assert!(diags.iter().any(|d| d.code == "RY105"));
}

#[test]
fn sign_comparison_is_an_allowed_indicator_idiom() {
    let diags = check("sign(x <= y)\nabs(x <= y)\n");
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY100")
            .count(),
        1,
        "sign() must be allowed, while abs() remains diagnosed: {diags:?}"
    );
}

#[test]
fn comparison_inside_call_is_diagnosed_through_short_circuit_operators() {
    let diags = check(
        "q <- TRUE\nx <- 1L\ny <- 2L\nz <- TRUE\nif (length(x == y) || q) x\nstopifnot(length(x == y) && z)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY093")
            .count(),
        2,
        "both short-circuit operands must retain call diagnostics: {diags:?}"
    );
}

#[test]
fn negated_comparison_binds_loosely_and_stays_silent() {
    // R parses `!x == y` as `!(x == y)` (unary `!` binds looser than
    // comparison), so the idiomatic `!length(x) == 1` guard is correct
    // code. RY095 wrongly assumed C precedence and is retired.
    let diags =
        check("x <- c(1, 2)\nif (!length(x) == 1) x <- 1\nflag <- !\"a\" == \"b\"\n!(1L == 2L)\n");
    assert!(
        diags.is_empty(),
        "negated comparisons are valid R and must stay silent: {diags:?}"
    );
}

#[test]
fn hasarg_requires_a_formal_of_the_lexically_enclosing_function() {
    let diags = check(
        "good <- function(value) hasArg(value)\ndots_ok <- function(actual, ...) hasArg(missing)\nidiom_ok <- function(object, ...) if (hasArg(thresh)) list(...)$thresh else 0\nstring_bad <- function(actual) hasArg(\"missing\")\nbad <- function(actual) hasArg(missing)\nhasArg(top_level)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY096")
            .count(),
        2,
        "non-formals in dots-less functions should fire; formals, dots functions, and top-level calls stay silent: {diags:?}"
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY010"),
        "hasArg captures names and must not create unbound-name diagnostics: {diags:?}"
    );
}

#[test]
fn printf_family_literal_arity_is_checked() {
    let diags = check(
        "gettextf(\"select %s then %s\", \"first\")\nsprintf(\"value=%s %%\", \"ok\")\nsprintf(dynamic_format, value)\n",
    );
    assert_eq!(
        diags
            .iter()
            .filter(|diagnostic| diagnostic.code == "RY094")
            .count(),
        1,
        "only a proven literal format shortage should fire: {diags:?}"
    );
}
