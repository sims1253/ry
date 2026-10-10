//! Shared diagnostic post-processing for `ry check` and the LSP server.
//!
//! `subtract_baseline` consumes counts per `(path, code, message)` key in
//! vector order, so the order of the post-processing stages is
//! observable: a diagnostic that a later stage would drop — an
//! inline-suppressed one, or one below the confidence threshold — must
//! not consume a baseline count that an identical unsuppressed twin
//! still needs. The pipeline below is the single order both frontends
//! apply (the order `ry check` has always used):
//!
//!   1. inline suppression comments (`# ry: ignore`, `# noqa`, ...)
//!   2. severity filter (select/ignore/error/warn)
//!   3. path-based confidence demotion — the seam between the two
//!      phases; both frontends demote non-source paths here through
//!      [`PostProcess::demote_non_source_paths`] (#492)
//!   4. baseline subtraction
//!   5. min-confidence threshold
//!
//! The pipeline is split at the demotion seam on purpose: the CLI runs
//! [`PostProcess::pre_demotion`] per file, demotes the aggregated
//! result, then finishes with [`PostProcess::post_demotion`], while the
//! LSP runs the same three stages per published file. Both frontends
//! call the demotion stage itself, so a finding from a package's
//! `tests/` tree lands below the threshold in the editor exactly when
//! it does on the command line. This crate is the natural home because
//! it already owns the suppression and severity filters and already
//! depends on ry-config (the reverse dependency would be a cycle).

use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::Path;

use ry_config::Baseline;
use ry_core::ast::{Expr, Stmt};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use ry_core::{SourceFile, Span};

use crate::diagnostics::{
    Confidence, Diagnostic, SeverityFilter, SuppressionKind, SuppressionOrigin,
};

/// The initial audit covers only rules whose premise is recognized from
/// local syntax. For inference-dependent rules, absence of a diagnostic can
/// mean that analysis lacked a type, signature, or package fact; that is not
/// evidence that an ignore is unused.
const UNUSED_IGNORE_AUDITABLE_CODES: &[&str] = &["RY034", "RY102"];

fn unchecked_audit_regions(file: &SourceFile) -> Vec<Span> {
    let mut assigned_functions = HashSet::new();
    let _ = walk_stmts(
        &file.stmts,
        Walk::ALL,
        |node, _| -> ControlFlow<(), Descend> {
            if let AstNode::Stmt(Stmt::Assign {
                value: Expr::Function { span, .. },
                ..
            }) = node
            {
                assigned_functions.insert(*span);
            }
            ControlFlow::Continue(Descend::Into)
        },
    );

    let mut unchecked = Vec::new();
    let _ = walk_stmts(
        &file.stmts,
        Walk::ALL,
        |node, _| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Expr(Expr::Function { span, .. })
                    if !assigned_functions.contains(span) =>
                {
                    unchecked.push(*span);
                    ControlFlow::Continue(Descend::Skip)
                }
                AstNode::Expr(Expr::Unknown(span)) => {
                    unchecked.push(*span);
                    ControlFlow::Continue(Descend::Skip)
                }
                _ => ControlFlow::Continue(Descend::Into),
            }
        },
    );
    unchecked
}

/// The post-processing configuration shared by the CLI and the LSP.
pub struct PostProcess<'a> {
    pub filter: &'a SeverityFilter,
    pub baseline: Option<&'a Baseline>,
    /// Diagnostics below this confidence are dropped in step 5.
    /// `Confidence::Low` disables the threshold.
    pub min_confidence: Confidence,
    /// Root the baseline's relative paths are resolved against.
    pub repo_root: Option<&'a Path>,
}

impl PostProcess<'_> {
    /// Steps 1-2: drop inline-suppressed diagnostics, then apply the
    /// severity filter. Suppression runs FIRST so a suppressed
    /// occurrence never reaches baseline subtraction (step 4) to
    /// consume a count its unsuppressed twin — the same
    /// `(path, code, message)` key — needs. Diagnostics with no source
    /// `file` simply skip the suppression step.
    pub fn pre_demotion(
        &self,
        diagnostics: Vec<Diagnostic>,
        path: &str,
        file: Option<&SourceFile>,
    ) -> Vec<Diagnostic> {
        let (comments, src) = file.map_or((&[][..], ""), |file| {
            (file.comments.as_slice(), file.source.as_str())
        });
        let audit = self.unused_ignore_diagnostics(&diagnostics, comments, src, path, file);
        let mut diagnostics = crate::filter_suppressed_with_comments(diagnostics, comments, src);
        diagnostics.extend(audit);
        crate::apply_filter_to_diagnostics(&mut diagnostics, self.filter);
        diagnostics
    }

    fn unused_ignore_diagnostics(
        &self,
        raw: &[Diagnostic],
        comments: &[ry_core::ast::Comment],
        src: &str,
        path: &str,
        file: Option<&SourceFile>,
    ) -> Vec<Diagnostic> {
        if self
            .filter
            .effective("RY113", crate::Severity::Warning)
            .is_none()
            || raw.iter().any(|d| d.code == "RY000")
            || crate::has_file_suppression_from_comments(comments)
        {
            return Vec::new();
        }
        let directives = crate::parse_suppressions_from_comments(comments, src);
        if !directives.iter().any(|directive| {
            directive.origin == SuppressionOrigin::Ry
                && matches!(directive.kind, SuppressionKind::Selective(_))
        }) {
            return Vec::new();
        }
        // A missing raw finding proves nothing where pass 3 did not visit
        // the source in diagnostic mode. Anonymous function values are
        // traversed for their return signatures with emissions discarded;
        // direct assigned functions and bare function definitions receive
        // a separate diagnostic walk. Unknown AST nodes are also opaque.
        let Some(file) = file else {
            return Vec::new();
        };
        let uncovered = unchecked_audit_regions(file);
        let line_starts = crate::diagnostics::line_starts(src);
        let mut unchecked_lines = vec![false; line_starts.len()];
        for region in uncovered {
            let first = line_starts
                .partition_point(|start| *start <= region.start)
                .saturating_sub(1);
            let last = line_starts
                .partition_point(|start| *start < region.end.max(region.start + 1))
                .saturating_sub(1);
            unchecked_lines[first..=last].fill(true);
        }

        let mut audit = Vec::new();
        for directive in directives {
            if directive.origin != SuppressionOrigin::Ry {
                continue;
            }
            let SuppressionKind::Selective(codes) = directive.kind else {
                continue;
            };
            if unchecked_lines.get(directive.line).copied().unwrap_or(true)
                || unchecked_lines
                    .get(directive.span.line)
                    .copied()
                    .unwrap_or(true)
            {
                continue;
            }
            for code in codes {
                if !UNUSED_IGNORE_AUDITABLE_CODES.contains(&code.as_str()) {
                    continue;
                }
                let Some(rule) = crate::rules::find(&code) else {
                    continue;
                };
                if self
                    .filter
                    .effective(&code, rule.default_severity)
                    .is_none()
                {
                    continue;
                }
                if raw
                    .iter()
                    .any(|d| d.code == code && d.span.line == directive.line)
                {
                    continue;
                }
                audit.push(Diagnostic::new(
                    crate::Severity::Warning,
                    directive.span,
                    path,
                    "RY113",
                    format!("unused ry ignore for `{code}`"),
                ));
            }
        }
        audit
    }

    /// Step 3: demote diagnostics from a package's non-source trees
    /// (`tests/`, `data-raw/`, `demo/`, `vignettes/`, `inst/`) one
    /// confidence tier, between the severity filter and the baseline.
    /// Both frontends run this stage at the seam so the editor and
    /// `ry check` demote identically before the threshold (and before
    /// baseline subtraction, which never reads confidence) sees the
    /// finding.
    pub fn demote_non_source_paths(&self, diagnostics: &mut [Diagnostic]) {
        demote_non_source_paths(diagnostics, self.repo_root);
    }

    /// Steps 4-5: subtract the baseline, then apply the min-confidence
    /// threshold. The threshold runs AFTER subtraction so a
    /// below-threshold diagnostic still consumes its baseline count,
    /// exactly like `ry check`.
    pub fn post_demotion(&self, diagnostics: &mut Vec<Diagnostic>) {
        if let Some(baseline) = self.baseline {
            ry_config::subtract_baseline(diagnostics, baseline, self.repo_root);
        }
        diagnostics.retain(|diagnostic| diagnostic.confidence >= self.min_confidence);
    }
}

/// Demote diagnostics from a package's non-source trees (`tests/`,
/// `data-raw/`, `demo/`, `vignettes/`, `inst/`) one confidence tier:
/// code there is not what CRAN ships or checks first.
///
/// The nearest ancestor holding a `DESCRIPTION` is the package root,
/// so a nested package demotes against its own root rather than the
/// enclosing one, and a path with no package root above it — a bare
/// directory merely named `tests` — is left alone. Relative diagnostic
/// paths resolve against `repo_root` (or the working directory when
/// it is `None`). Only confidence moves; severity is a separate axis.
fn demote_non_source_paths(diagnostics: &mut [Diagnostic], repo_root: Option<&Path>) {
    const DEMOTED: [&str; 5] = ["tests", "data-raw", "demo", "vignettes", "inst"];
    for diagnostic in diagnostics {
        let path = Path::new(&diagnostic.path);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            repo_root.unwrap_or_else(|| Path::new(".")).join(path)
        };
        let mut package_root = absolute.parent();
        while let Some(root) = package_root {
            if root.join("DESCRIPTION").is_file() {
                if let Ok(relative) = absolute.strip_prefix(root)
                    && relative.components().any(|component| {
                        component
                            .as_os_str()
                            .to_str()
                            .is_some_and(|name| DEMOTED.contains(&name))
                    })
                {
                    diagnostic.confidence = diagnostic.confidence.demote();
                }
                break;
            }
            package_root = root.parent();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_config::{Baseline, BaselineEntry};

    fn entry(path: &str, code: &str, message: &str, count: usize) -> BaselineEntry {
        BaselineEntry {
            path: path.to_string(),
            code: code.to_string(),
            message: message.to_string(),
            count,
        }
    }

    fn baseline(count: usize) -> Baseline {
        Baseline {
            version: 1,
            entries: vec![entry("a.R", "RY010", "same message", count)],
        }
    }

    /// A diagnostic with every field set explicitly, so tests can vary
    /// the confidence within one `(path, code, message)` key — something
    /// the checker's own output cannot do (confidence is derived from
    /// the code) but the ordering contract must still cover.
    fn diag(path: &str, confidence: Confidence, line: usize) -> Diagnostic {
        Diagnostic {
            severity: crate::Severity::Warning,
            span: ry_core::Span::new(line * 10, line * 10 + 1, line, 0),
            path: path.to_string(),
            code: "RY010",
            message: "same message".to_string(),
            confidence,
        }
    }

    fn pipeline<'a>(
        filter: &'a SeverityFilter,
        baseline: Option<&'a Baseline>,
        min_confidence: Confidence,
    ) -> PostProcess<'a> {
        PostProcess {
            filter,
            baseline,
            min_confidence,
            repo_root: None,
        }
    }

    fn run(diagnostics: Vec<Diagnostic>, src: &str, post: &PostProcess<'_>) -> Vec<Diagnostic> {
        let path = diagnostics
            .first()
            .map_or("a.R", |d| d.path.as_str())
            .to_string();
        let file = ry_core::RParser::new().unwrap().parse(&path, src).unwrap();
        let mut diagnostics = post.pre_demotion(diagnostics, &path, Some(&file));
        post.post_demotion(&mut diagnostics);
        diagnostics
    }

    fn checked(src: &str) -> Vec<Diagnostic> {
        let file = ry_core::RParser::new().unwrap().parse("a.R", src).unwrap();
        let mut checker = crate::Checker::new("a.R");
        checker.check(&file);
        checker.take_diagnostics()
    }

    fn audit_filter() -> SeverityFilter {
        let mut filter = SeverityFilter::default();
        filter.add_warn("RY113");
        filter
    }

    #[test]
    fn unused_ignore_is_opt_in_and_points_to_the_comment() {
        let src = "1L == 1L # ry: ignore[RY034]\n";
        let raw = checked(src);
        assert!(
            run(
                raw.clone(),
                src,
                &pipeline(&SeverityFilter::default(), None, Confidence::Low)
            )
            .is_empty()
        );

        let filter = audit_filter();
        let findings = run(raw, src, &pipeline(&filter, None, Confidence::Low));
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].code, "RY113");
        assert!(findings[0].message.contains("RY034"));
        assert_eq!(findings[0].span.start, src.find('#').unwrap());
    }

    #[test]
    fn mixed_codes_and_duplicate_standalone_directives_use_raw_facts_independently() {
        let filter = audit_filter();
        let src = "1L == NA # ry: ignore[RY034, RY102]\n";
        let raw = checked(src);
        assert!(raw.iter().any(|d| d.code == "RY034"));
        let findings = run(raw, src, &pipeline(&filter, None, Confidence::Low));
        assert_eq!(
            findings.iter().filter(|d| d.code == "RY113").count(),
            1,
            "{findings:?}"
        );
        assert!(findings[0].message.contains("RY102"), "{findings:?}");

        for (expression, expected_unused) in [("1L == NA", 0), ("1L == 1L", 2)] {
            let src = format!("# ry: ignore[RY034]\n# ry: ignore[RY034]\n\n{expression}\n");
            let raw = checked(&src);
            let findings = run(raw, &src, &pipeline(&filter, None, Confidence::Low));
            assert_eq!(
                findings.iter().filter(|d| d.code == "RY113").count(),
                expected_unused,
                "{expression}: {findings:?}"
            );
        }
    }

    #[test]
    fn anonymous_function_bodies_without_diagnostic_walk_are_not_audited() {
        let filter = audit_filter();
        for (code, used, corrected) in [
            ("RY034", "1L == NA", "1L == 1L"),
            ("RY102", "list(\"a\" <- 1L)", "list(\"a\" = 1L)"),
        ] {
            for expression in [used, corrected] {
                let src =
                    format!("identity(function() {{\n  {expression} # ry: ignore[{code}]\n}})\n");
                let raw = checked(&src);
                assert!(
                    !raw.iter().any(|d| d.code == code),
                    "anonymous body is visited only in discarding mode: {raw:?}"
                );
                let findings = run(raw, &src, &pipeline(&filter, None, Confidence::Low));
                assert!(
                    !findings.iter().any(|d| d.code == "RY113"),
                    "{src}: {findings:?}"
                );
            }

            let named_used = format!("f <- function() {{\n  {used} # ry: ignore[{code}]\n}}\n");
            let raw = checked(&named_used);
            assert!(raw.iter().any(|d| d.code == code), "{named_used}: {raw:?}");
            let findings = run(raw, &named_used, &pipeline(&filter, None, Confidence::Low));
            assert!(!findings.iter().any(|d| d.code == "RY113"));

            let named_corrected =
                format!("f <- function() {{\n  {corrected} # ry: ignore[{code}]\n}}\n");
            let raw = checked(&named_corrected);
            let findings = run(
                raw,
                &named_corrected,
                &pipeline(&filter, None, Confidence::Low),
            );
            assert!(
                findings.iter().any(|d| d.code == "RY113"),
                "{named_corrected}: {findings:?}"
            );
        }
    }

    #[test]
    fn baseline_confidence_and_disabled_rules_do_not_invent_unused_facts() {
        let src = "1L == NA # ry: ignore[RY034]\n";
        let raw = checked(src);
        let original = raw.iter().find(|d| d.code == "RY034").unwrap();
        let baseline = Baseline {
            version: 1,
            entries: vec![entry("a.R", "RY034", &original.message, 1)],
        };
        let filter = audit_filter();
        let post = pipeline(&filter, Some(&baseline), Confidence::High);
        let file = ry_core::RParser::new().unwrap().parse("a.R", src).unwrap();
        let pre = post.pre_demotion(raw, "a.R", Some(&file));
        assert!(!pre.iter().any(|d| d.code == "RY113"), "{pre:?}");
        let mut kept = pre;
        post.post_demotion(&mut kept);
        assert!(kept.is_empty(), "{kept:?}");

        let mut disabled = audit_filter();
        disabled.add_ignore("RY034");
        let corrected = "1L == 1L # ry: ignore[RY034]\n";
        let raw = checked(corrected);
        assert!(run(raw, corrected, &pipeline(&disabled, None, Confidence::Low)).is_empty());
    }

    #[test]
    fn incomplete_and_unsupported_analysis_is_left_unaudited() {
        let filter = audit_filter();
        for src in [
            "1L == 1L # ry: ignore[RY034]\nif (\n",
            "1L + 1L # ry: ignore[RY040]\n",
            "1L == 1L # noqa: RY034\n",
            "1L == 1L # ry: ignore\n",
            "# ry: ignore-file\n1L == 1L # ry: ignore[RY034]\n",
        ] {
            let raw = checked(src);
            let findings = run(raw, src, &pipeline(&filter, None, Confidence::Low));
            assert!(
                !findings.iter().any(|d| d.code == "RY113"),
                "{src}: {findings:?}"
            );
        }
    }

    #[test]
    fn bare_ignore_cannot_hide_audit_and_severity_override_can_disable_it() {
        let src = "# ry: ignore\n1L == 1L # ry: ignore[RY034, RY113]\n";
        let raw = checked(src);
        let filter = audit_filter();
        let findings = run(raw, src, &pipeline(&filter, None, Confidence::Low));
        assert!(findings.iter().any(|d| d.code == "RY113"), "{findings:?}");

        let mut disabled = filter;
        disabled.add_ignore("RY113");
        let raw = checked(src);
        assert!(run(raw, src, &pipeline(&disabled, None, Confidence::Low)).is_empty());
    }

    /// The #491 scenario: two identical `(path, code, message)`
    /// diagnostics where the first carries `# ry: ignore[...]`, with a
    /// baseline count of 1. Suppression must drop the first occurrence
    /// BEFORE the baseline subtracts, so the count absorbs the
    /// unsuppressed twin and the pipeline goes quiet. Filtering the
    /// other way around would consume the count on the suppressed
    /// occurrence and leave the twin visible.
    #[test]
    fn suppressed_occurrence_does_not_consume_the_baseline_budget() {
        let src = "bad_one  # ry: ignore[RY010]\nbad_two\n";
        let filter = SeverityFilter::default();
        let base = baseline(1);
        let post = pipeline(&filter, Some(&base), Confidence::Low);
        let diagnostics = vec![
            diag("a.R", Confidence::Medium, 0),
            diag("a.R", Confidence::Medium, 1),
        ];
        let kept = run(diagnostics, src, &post);
        assert!(
            kept.is_empty(),
            "the baseline count must absorb the unsuppressed twin, got {kept:?}"
        );
    }

    /// The second parity break from #491: a below-threshold occurrence
    /// still consumes baseline budget (threshold AFTER subtraction).
    /// With a Medium and a High diagnostic on one key and a count of 1,
    /// the Medium occurrence eats the count, so the High one survives.
    #[test]
    fn below_threshold_occurrence_still_consumes_the_baseline_budget() {
        let filter = SeverityFilter::default();
        let base = baseline(1);
        let post = pipeline(&filter, Some(&base), Confidence::High);
        let diagnostics = vec![
            diag("a.R", Confidence::Medium, 0),
            diag("a.R", Confidence::High, 1),
        ];
        let kept = run(diagnostics, "", &post);
        // Threshold-first would drop the Medium before subtraction, the
        // High would eat the count, and nothing would survive.
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].confidence, Confidence::High);
    }

    /// The demotion seam: confidence demoted between the two phases
    /// lands before the min-confidence threshold — the CLI's documented
    /// position. No baseline here: subtraction never reads confidence,
    /// so demotion-versus-subtraction order is unobservable by
    /// construction, and only the threshold can tell where demotion
    /// happened. Demoting the `High` diagnostic to `Medium` between the
    /// phases makes the `High` threshold drop it; demotion after
    /// `post_demotion` (or none at all) leaves the survivor visible.
    #[test]
    fn demotion_between_the_phases_happens_before_the_threshold() {
        let filter = SeverityFilter::default();
        let post = pipeline(&filter, None, Confidence::High);
        let before = vec![diag("a.R", Confidence::High, 0)];
        let mut diagnostics = post.pre_demotion(before, "a.R", None);
        for diagnostic in &mut diagnostics {
            diagnostic.confidence = diagnostic.confidence.demote();
        }
        post.post_demotion(&mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "the demoted confidence must fall below the threshold, got {diagnostics:?}"
        );
    }

    /// Straight-line sanity: each stage filtering on its own —
    /// file-level suppression, an ignored rule, baseline subtraction,
    /// and the confidence threshold.
    #[test]
    fn each_stage_filters_independently() {
        let src = "# ry: ignore-file\nbad\n";
        let filter = SeverityFilter::default();
        let post = pipeline(&filter, None, Confidence::Low);
        let kept = run(vec![diag("a.R", Confidence::Medium, 0)], src, &post);
        assert!(
            kept.is_empty(),
            "file-level suppression must drop everything"
        );

        let mut ignoring = SeverityFilter::default();
        ignoring.add_ignore("RY010");
        let post = pipeline(&ignoring, None, Confidence::Low);
        let kept = run(vec![diag("a.R", Confidence::Medium, 0)], "", &post);
        assert!(kept.is_empty(), "ignored rule must not survive the filter");

        let filter = SeverityFilter::default();
        let base = baseline(1);
        let post = pipeline(&filter, Some(&base), Confidence::Medium);
        let kept = run(vec![diag("a.R", Confidence::Medium, 0)], "", &post);
        assert!(kept.is_empty(), "baseline must absorb the occurrence");

        let filter = SeverityFilter::default();
        let post = pipeline(&filter, None, Confidence::High);
        let kept = run(vec![diag("a.R", Confidence::Medium, 0)], "", &post);
        assert!(kept.is_empty(), "below-threshold must be dropped");
    }

    /// Baseline paths are repo-root relative: with a root set, a
    /// diagnostic on an absolute path matches a relative entry.
    #[test]
    fn baseline_paths_resolve_against_the_repo_root() {
        let temp = tempfile::tempdir().unwrap();
        let filter = SeverityFilter::default();
        let relative_entry = Baseline {
            version: 1,
            entries: vec![entry("R/a.R", "RY010", "same message", 1)],
        };
        let post = PostProcess {
            filter: &filter,
            baseline: Some(&relative_entry),
            min_confidence: Confidence::Low,
            repo_root: Some(temp.path()),
        };
        let absolute = temp.path().join("R/a.R");
        let kept = run(
            vec![diag(absolute.to_str().unwrap(), Confidence::Medium, 0)],
            "",
            &post,
        );
        assert!(
            kept.is_empty(),
            "absolute path must match the relative entry"
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Path-based confidence demotion (#492)
    // ─────────────────────────────────────────────────────────────────

    /// A diagnostic whose confidence comes from its rule code, the way
    /// the checker produces them — demotion must move the code-derived
    /// tier, not just a hand-set field.
    fn coded(path: &std::path::Path, code: &'static str) -> Diagnostic {
        Diagnostic::new(
            crate::Severity::Warning,
            ry_core::Span::new(0, 1, 0, 0),
            &path.to_string_lossy(),
            code,
            "same message",
        )
    }

    /// An empty package root: a tempdir holding a `DESCRIPTION`, with
    /// `tree` created beneath it. Returns the path of a (fictional)
    /// file inside that tree — demotion only stats `DESCRIPTION`, it
    /// never reads the diagnostic's file.
    fn package_tree(temp: &tempfile::TempDir, tree: &str) -> std::path::PathBuf {
        std::fs::write(temp.path().join("DESCRIPTION"), "Package: example\n").unwrap();
        let dir = temp.path().join(tree);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("code.R")
    }

    /// The demoted confidence of a diagnostic on `path`, run through
    /// the seam method with `root` as the pipeline's repo root.
    fn demoted_at(path: &std::path::Path, root: &std::path::Path) -> Confidence {
        let filter = SeverityFilter::default();
        let post = PostProcess {
            filter: &filter,
            baseline: None,
            min_confidence: Confidence::Low,
            repo_root: Some(root),
        };
        let mut diagnostics = vec![coded(path, "RY030")];
        assert_eq!(diagnostics[0].confidence, Confidence::High);
        post.demote_non_source_paths(&mut diagnostics);
        diagnostics[0].confidence
    }

    /// The CLI's original pin, moved with the function: a package
    /// `tests/` path demotes one tier.
    #[test]
    fn package_tests_path_demotes_confidence_one_tier() {
        let temp = tempfile::tempdir().unwrap();
        let path = package_tree(&temp, "tests");
        assert_eq!(demoted_at(&path, temp.path()), Confidence::Medium);
    }

    /// All five support trees demote; `R/` — the tree CRAN ships and
    /// checks first — does not.
    #[test]
    fn every_support_tree_demotes_and_r_source_does_not() {
        for tree in ["tests", "data-raw", "demo", "vignettes", "inst"] {
            let temp = tempfile::tempdir().unwrap();
            assert_eq!(
                demoted_at(&package_tree(&temp, tree), temp.path()),
                Confidence::Medium,
                "`{tree}` must demote one tier"
            );
        }
        let temp = tempfile::tempdir().unwrap();
        let source = package_tree(&temp, "R");
        assert_eq!(
            demoted_at(&source, temp.path()),
            Confidence::High,
            "`R/` is source: no demotion"
        );
    }

    /// Nested packages demote against the NEAREST `DESCRIPTION` root:
    /// a nested package's `tests/` demotes, while its `R/` tree stays
    /// put even when the file also sits under the outer package's
    /// `tests/` directory.
    #[test]
    fn nested_packages_demote_against_their_own_root() {
        let temp = tempfile::tempdir().unwrap();
        package_tree(&temp, "R"); // writes the outer package's DESCRIPTION
        let nested = temp.path().join("tests/subpkg");
        std::fs::create_dir_all(nested.join("R")).unwrap();
        std::fs::create_dir_all(nested.join("tests")).unwrap();
        std::fs::write(nested.join("DESCRIPTION"), "Package: subpkg\n").unwrap();

        let nested_tests = nested.join("tests/code.R");
        assert_eq!(demoted_at(&nested_tests, temp.path()), Confidence::Medium);
        // Under the outer package's `tests/`, but inside the nested
        // package's `R/`: the nearest root wins, no demotion.
        let nested_r = nested.join("R/code.R");
        assert_eq!(demoted_at(&nested_r, temp.path()), Confidence::High);
    }

    /// A directory merely named `tests` with no `DESCRIPTION` anywhere
    /// above it is not a package support tree: no demotion.
    #[test]
    fn plain_tests_directory_without_a_package_root_stays_undemoted() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("code.R");
        assert_eq!(demoted_at(&path, temp.path()), Confidence::High);
    }

    /// Demotion moves only confidence. Severity — the error/warning
    /// axis users filter separately — must survive the stage untouched.
    #[test]
    fn demotion_leaves_severity_alone() {
        let temp = tempfile::tempdir().unwrap();
        let filter = SeverityFilter::default();
        let post = PostProcess {
            filter: &filter,
            baseline: None,
            min_confidence: Confidence::Low,
            repo_root: Some(temp.path()),
        };
        let path = package_tree(&temp, "tests");
        let mut diagnostic = coded(&path, "RY030");
        diagnostic.severity = crate::Severity::Error;
        post.demote_non_source_paths(std::slice::from_mut(&mut diagnostic));
        assert_eq!(diagnostic.severity, crate::Severity::Error);
        assert_eq!(diagnostic.confidence, Confidence::Medium);
    }

    /// Relative diagnostic paths resolve against the pipeline's repo
    /// root, matching how the CLI feeds discovery output through the
    /// seam: the same `tests/` finding demotes with a root set and
    /// would not resolve to a package without one.
    #[test]
    fn relative_paths_demote_through_the_repo_root() {
        let temp = tempfile::tempdir().unwrap();
        let path = package_tree(&temp, "tests");
        let relative = path.strip_prefix(temp.path()).unwrap().to_path_buf();
        assert_eq!(demoted_at(&relative, temp.path()), Confidence::Medium);
    }
}
