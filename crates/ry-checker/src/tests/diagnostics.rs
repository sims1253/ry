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
    assert_eq!(supps[0].kind, SuppressionKind::All);
}

#[test]
fn parse_specific_rule_ignore() {
    let src = "x <- \"a\" * 3  # ry: ignore[RY040]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].valid_rules().unwrap(), &["RY040"]);
}

#[test]
fn parse_multiple_rules() {
    let src = "x <- bad  # ry: ignore[RY040, RY010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].suppresses("RY040"));
    assert!(supps[0].suppresses("RY010"));
}

#[test]
fn unbracketed_code_like_list_validates_every_token() {
    for directive in [
        "ry: ignore RY040 RY010",
        "ry: ignore RY040, RY010",
        "ry: ignore RY040 , RY010",
        "ry: ignore RY040 ,RY010",
        "ry: ignore RY040,RY010",
    ] {
        let src = format!("x <- bad  # {directive}\n");
        let supps = parse_suppressions_from_comments(&scan_comments(&src), &src);
        assert_eq!(supps.len(), 1, "{directive}");
        assert_eq!(
            supps[0].valid_rules().unwrap(),
            &["RY040", "RY010"],
            "{directive}"
        );
    }

    let prose = "x <- bad  # ry: ignore RY040 reason documented under RY010\n";
    let supps = parse_suppressions_from_comments(&scan_comments(prose), prose);
    assert_eq!(supps[0].valid_rules().unwrap(), &["RY040"]);

    for directive in [
        "ry: ignore RY040 RX010",
        "ry: ignore RY040, RX010",
        "ry: ignore RY040 , RX010",
        "ry: ignore RY040 ,RX010",
        "ry: ignore RY040 ,",
        "ry: ignore RY040 ,, RY010",
    ] {
        let src = format!("\"a\" + 1L # {directive}\n");
        let filtered = filter_suppressed_with_comments(check(&src), &scan_comments(&src), &src);
        assert!(filtered.iter().any(|d| d.code == "RY040"), "{directive}");
        assert!(filtered.iter().any(|d| d.code == "RY112"), "{directive}");
    }
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
    assert!(supps[0].suppresses("RY010"));
}

#[test]
fn parse_bare_noqa_suppresses_all() {
    let src = "x <- bad  # noqa\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert_eq!(supps[0].kind, SuppressionKind::All);
}

#[test]
fn parse_noqa_bracket_form() {
    let src = "x <- bad  # noqa[RY010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].suppresses("RY010"));
}

#[test]
fn parse_compact_ry_ignore_no_space() {
    let src = "x <- bad  # ry:ignore[RY010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].suppresses("RY010"));
}

#[test]
fn parse_case_insensitive_marker() {
    let src = "x <- bad  # RY: IGNORE[ry010]\n";
    let supps = parse_suppressions_from_comments(&scan_comments(src), src);
    assert_eq!(supps.len(), 1);
    assert!(supps[0].suppresses("RY010"));
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
        span: Span {
            start: 0,
            end: 1,
            line: 2,
            col: 0,
        },
        origin: SuppressionOrigin::Ry,
        kind: SuppressionKind::Selective(vec!["RY010".to_string()]),
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
        related: Vec::new(),
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
        span: Span {
            start: 0,
            end: 1,
            line: 0,
            col: 0,
        },
        origin: SuppressionOrigin::Ry,
        kind: SuppressionKind::All,
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
        related: Vec::new(),
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

#[test]
fn invalid_native_ignores_report_and_leave_errors_visible() {
    for directive in [
        "ry: ignore[RX040]",
        "ry: ignore[RY999999]",
        "ry: ignore[RY040",
        "ry: ignore[RY040]]",
        "ry: ignore[RY040] ]",
        "ry: ignore[RY040][RX040]",
        "ry: ignore[RY040,]",
        "ry: ignore[RY040, RX040]",
    ] {
        let src = format!("\"a\" + 1L  # {directive}\n");
        let comments = scan_comments(&src);
        let raw = check(&src);
        let filtered = filter_suppressed_with_comments(raw, &comments, &src);
        assert!(
            filtered.iter().any(|d| d.code == "RY040"),
            "{directive}: {filtered:?}"
        );
        let invalid = filtered.iter().find(|d| d.code == "RY112").unwrap();
        assert_eq!(invalid.span.line, 0);
        assert_eq!(invalid.span.col, src.find('#').unwrap());
    }
}

#[test]
fn foreign_noqa_and_marker_prefixes_do_not_suppress() {
    for directive in [
        "noqa: E501",
        "noqa reason documented elsewhere",
        "noqa[RY040]]",
        "noqa[RY040] ]",
        "noqa[RY040][RX040]",
        "noqa-ish note",
        "noquality",
        "ry: ignored by upstream",
        "ry: ignore-file-extra",
    ] {
        let src = format!("\"a\" + 1L  # {directive}\n");
        let filtered = filter_suppressed_with_comments(check(&src), &scan_comments(&src), &src);
        assert!(
            filtered.iter().any(|d| d.code == "RY040"),
            "{directive}: {filtered:?}"
        );
        assert!(
            !filtered.iter().any(|d| d.code == "RY112"),
            "{directive}: {filtered:?}"
        );
    }
}

#[test]
fn selective_and_bare_ignores_remain_distinct() {
    for directive in [
        "ry: ignore[RY040]",
        "noqa: E501, RY040",
        "ry: ignore[ry040]",
    ] {
        let src = format!("\"a\" + 1L; missing_name  # {directive}\n");
        let filtered = filter_suppressed_with_comments(check(&src), &scan_comments(&src), &src);
        assert!(
            !filtered.iter().any(|d| d.code == "RY040"),
            "{directive}: {filtered:?}"
        );
        assert!(
            filtered.iter().any(|d| d.code == "RY010"),
            "{directive}: {filtered:?}"
        );
    }
    for directive in [
        "ry: ignore",
        "ry: ignore[]",
        "ry: ignore[ ]",
        "ry: ignore reason documented elsewhere",
        "noqa",
        "RY:IGNORE",
    ] {
        let src = format!("\"a\" + 1L  # {directive}\n");
        let filtered = filter_suppressed_with_comments(check(&src), &scan_comments(&src), &src);
        assert!(
            !filtered.iter().any(|d| d.code == "RY040"),
            "{directive}: {filtered:?}"
        );
    }
    let src = "\"a\" + 1L  # ry: ignore-file\n";
    assert!(filter_suppressed_with_comments(check(src), &scan_comments(src), src).is_empty());
}

#[test]
fn invalid_standalone_comment_reports_at_comment_and_does_not_suppress_next_line() {
    let src = "# ry: ignore[RX040]\n\"a\" + 1L\n";
    let filtered = filter_suppressed_with_comments(check(src), &scan_comments(src), src);
    assert!(
        filtered
            .iter()
            .any(|d| d.code == "RY040" && d.span.line == 1)
    );
    assert!(
        filtered
            .iter()
            .any(|d| d.code == "RY112" && d.span.line == 0)
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

// ---- leading UTF-8 BOM diagnostics, #474 ----

/// Parse `src` (already decoded, with the BOM surviving as a leading
/// U+FEFF, as the read boundary hands it over) and set the boundary's
/// leading-BOM flag, exactly like the CLI/LSP attach it for a
/// BOM-prefixed disk file.
fn parse_bom_flagged(src: &str) -> SourceFile {
    let mut file = parse_file("test.R", src);
    file.leading_bom = true;
    file
}

/// A leading BOM is flagged as one RY000 at 1:1 spanning the BOM's
/// three decoded bytes, with the wording of R's own parse error.
#[test]
fn leading_bom_is_flagged_as_ry000_at_1_1() {
    let mut checker = Checker::new("test.R");
    let file = parse_bom_flagged("\u{feff}x <- 1\n");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, "RY000");
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(
        diags[0].message.contains("byte order mark"),
        "message should name the BOM: {}",
        diags[0].message
    );
    assert_eq!(diags[0].span, Span::new(0, 3, 0, 0));
}

/// R rejects a BOM-prefixed file even when the rest is comment-only
/// (verified against R 4.6.1, unlike invalid bytes which comments
/// tolerate), so a comment-only BOM file still flags.
#[test]
fn leading_bom_before_only_comments_still_flags() {
    let mut checker = Checker::new("test.R");
    let file = parse_bom_flagged("\u{feff}# just a comment\n");
    checker.check(&file);
    assert_eq!(checker.take_diagnostics().len(), 1);
}

/// Like #376's encoding flag, a BOM-flagged file reports only its
/// RY000: R rejects the whole file at 1:1, so semantic findings over
/// the surviving tree are noise.
#[test]
fn leading_bom_suppresses_semantic_diagnostics() {
    let mut checker = Checker::new("test.R");
    let file = parse_bom_flagged("\u{feff}missing_name\n");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, "RY000");
    assert!(diags[0].message.contains("byte order mark"), "{diags:?}");
}

/// R reports the BOM (1:1) before any later invalid multibyte
/// character, so when a file carries both findings the BOM's RY000 is
/// the only one emitted.
#[test]
fn leading_bom_takes_precedence_over_invalid_utf8_spans() {
    let mut file = parse_file("test.R", "\u{feff}s <- \"café\"\n");
    file.leading_bom = true;
    // The `é` at decoded bytes 12..14 (after the 3-byte BOM).
    file.invalid_utf8 = vec![Span::new(12, 14, 0, 12)];
    let mut checker = Checker::new("test.R");
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert!(diags[0].message.contains("byte order mark"), "{diags:?}");
}

#[test]
fn related_spans_must_fit_the_source() {
    let related = |start, end| Related {
        path: "x.R".into(),
        span: Span::new(start, end, 0, start),
        message: String::new(),
    };
    let src = "a𝑥b";
    assert_eq!(related(0, 1).range_in(src), Some(0..1));
    assert_eq!(related(1, 5).range_in(src), Some(1..5));
    assert_eq!(related(0, 7).range_in(src), None, "past the end");
    assert_eq!(related(2, 5).range_in(src), None, "inside a scalar");
    assert_eq!(related(5, 1).range_in(src), None, "reversed");
}

#[test]
fn related_items_are_budgeted() {
    let item = Related {
        path: "x.R".into(),
        span: Span::default(),
        message: String::new(),
    };
    let d = Diagnostic::new(Severity::Warning, Span::default(), "x.R", "RY091", "m")
        .with_related(vec![item; MAX_RELATED + 2]);
    assert_eq!(d.related.len(), MAX_RELATED);
}
