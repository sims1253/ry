//! Shared diagnostic post-processing for `ry check` and the LSP server.
//!
//! Both frontends apply the stages in this order:
//!
//!   1. inline suppression comments (`# ry: ignore`, `# noqa`, ...)
//!   2. severity filter (select/ignore/error/warn)
//!   3. confidence demotion of package non-source paths
//!   4. baseline subtraction
//!   5. min-confidence threshold
//!
//! The order is observable because `subtract_baseline` consumes counts
//! per `(path, code, message)` key in vector order. Suppression and the
//! severity filter run first so a dropped diagnostic never consumes a
//! count its identical twin needs. The threshold runs after, so a
//! below-threshold diagnostic does consume one.
//! [`PostProcess::pre_demotion`] runs steps 1-2 per file, and
//! [`PostProcess::post_demotion`] runs steps 3-5 on the collected result.

use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::Path;

use ry_config::Baseline;
use ry_core::ast::{Expr, Stmt};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use ry_core::{SourceFile, Span};

use crate::diagnostics::{
    Confidence, Diagnostic, SeverityFilter, Suppression, SuppressionKind, SuppressionOrigin,
};

/// The unused-ignore audit covers only rules whose premise is local syntax.
/// For an inference-dependent rule, a missing diagnostic can mean the
/// analysis lacked a fact, which does not prove the ignore unused.
const UNUSED_IGNORE_AUDITABLE_CODES: &[&str] = &["RY034", "RY102"];

fn unchecked_audit_regions(file: &SourceFile) -> Vec<Span> {
    // The walk visits an assignment before its value, so a directly
    // assigned function is known by the time its `Expr::Function` is reached.
    let mut assigned_functions = HashSet::new();
    let mut unchecked = Vec::new();
    let _ = walk_stmts(
        &file.stmts,
        Walk::ALL,
        |node, _| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Stmt(Stmt::Assign {
                    value: Expr::Function { span, .. },
                    ..
                }) => {
                    assigned_functions.insert(*span);
                }
                AstNode::Expr(Expr::Function { span, .. })
                    if !assigned_functions.contains(span) =>
                {
                    unchecked.push(*span);
                    return ControlFlow::Continue(Descend::Skip);
                }
                AstNode::Expr(Expr::Unknown(span)) => {
                    unchecked.push(*span);
                    return ControlFlow::Continue(Descend::Skip);
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
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
    /// severity filter. Without a source `file`, suppression is skipped.
    pub fn pre_demotion(
        &self,
        diagnostics: Vec<Diagnostic>,
        path: &str,
        file: Option<&SourceFile>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = match file {
            Some(file) if crate::has_file_suppression_from_comments(&file.comments) => Vec::new(),
            Some(file) => {
                let directives =
                    crate::parse_suppressions_from_comments(&file.comments, &file.source);
                let audit = self.unused_ignore_diagnostics(&diagnostics, path, file, &directives);
                let mut kept: Vec<_> = diagnostics
                    .into_iter()
                    .filter(|diagnostic| !crate::is_suppressed(diagnostic, &directives))
                    .collect();
                kept.extend(audit);
                kept
            }
            None => diagnostics,
        };
        crate::apply_filter_to_diagnostics(&mut diagnostics, self.filter);
        diagnostics
    }

    fn unused_ignore_diagnostics(
        &self,
        raw: &[Diagnostic],
        path: &str,
        file: &SourceFile,
        directives: &[Suppression],
    ) -> Vec<Diagnostic> {
        if self
            .filter
            .effective(crate::rules::UNUSED_IGNORE, crate::Severity::Warning)
            .is_none()
            || raw.iter().any(|d| d.code == "RY000")
        {
            return Vec::new();
        }
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
        let uncovered = unchecked_audit_regions(file);
        let line_starts = crate::diagnostics::line_starts(&file.source);
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
            let SuppressionKind::Selective(codes) = &directive.kind else {
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
                let Some(rule) = crate::rules::find(code) else {
                    continue;
                };
                if self.filter.effective(code, rule.default_severity).is_none() {
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
                    crate::rules::UNUSED_IGNORE,
                    format!("unused ry ignore for `{code}`"),
                ));
            }
        }
        audit
    }

    /// Steps 3-5: demote non-source paths, subtract the baseline, then
    /// apply the min-confidence threshold. The threshold runs after
    /// subtraction so a below-threshold diagnostic still consumes its
    /// baseline count.
    pub fn post_demotion(&self, diagnostics: &mut Vec<Diagnostic>) {
        demote_non_source_paths(diagnostics, self.repo_root);
        if let Some(baseline) = self.baseline {
            ry_config::subtract_baseline(diagnostics, baseline, self.repo_root);
        }
        diagnostics.retain(|diagnostic| diagnostic.confidence >= self.min_confidence);
    }
}

/// Demote diagnostics from a package's non-source trees (`tests/`,
/// `data-raw/`, `demo/`, `vignettes/`, `inst/`) one confidence tier,
/// because CRAN does not ship or check that code first. The package root
/// is the nearest ancestor holding a `DESCRIPTION`. A path with no package
/// root above it is left alone. Relative paths resolve against
/// `repo_root`, or the working directory when it is `None`.
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

    /// A diagnostic with an explicit confidence, so tests can vary it
    /// within one `(path, code, message)` key.
    fn diag(path: &str, confidence: Confidence, line: usize) -> Diagnostic {
        Diagnostic {
            severity: crate::Severity::Warning,
            span: ry_core::Span::new(line * 10, line * 10 + 1, line, 0),
            path: path.to_string(),
            code: "RY010",
            message: "same message".to_string(),
            confidence,
            related: Vec::new(),
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

    /// Two identical diagnostics, the first suppressed inline, and a
    /// baseline count of 1. Suppression runs before subtraction, so the
    /// count absorbs the unsuppressed twin.
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

    /// A below-threshold occurrence still consumes a baseline count. With
    /// a Medium and a High diagnostic on one key and a count of 1, the
    /// Medium one takes the count and the High one survives.
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

    /// Confidence lowered between the two phases reaches the threshold.
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

    /// Each stage filters on its own: file-level suppression, an ignored
    /// rule, baseline subtraction, and the confidence threshold.
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

    /// A diagnostic whose confidence comes from its rule code.
    fn coded(path: &std::path::Path, code: &'static str) -> Diagnostic {
        Diagnostic::new(
            crate::Severity::Warning,
            ry_core::Span::new(0, 1, 0, 0),
            &path.to_string_lossy(),
            code,
            "same message",
        )
    }

    /// A package root with `tree` beneath it. Returns a path inside `tree`
    /// that need not exist, because demotion only stats `DESCRIPTION`.
    fn package_tree(temp: &tempfile::TempDir, tree: &str) -> std::path::PathBuf {
        std::fs::write(temp.path().join("DESCRIPTION"), "Package: example\n").unwrap();
        let dir = temp.path().join(tree);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("code.R")
    }

    /// The demoted confidence of a diagnostic on `path`, run through
    /// `post_demotion` with `root` as the pipeline's repo root.
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
        post.post_demotion(&mut diagnostics);
        diagnostics[0].confidence
    }

    #[test]
    fn package_tests_path_demotes_confidence_one_tier() {
        let temp = tempfile::tempdir().unwrap();
        let path = package_tree(&temp, "tests");
        assert_eq!(demoted_at(&path, temp.path()), Confidence::Medium);
    }

    /// All five support trees demote. `R/` does not.
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

    /// Nested packages demote against the nearest `DESCRIPTION` root.
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
        // Under the outer `tests/` but inside the nested `R/`.
        let nested_r = nested.join("R/code.R");
        assert_eq!(demoted_at(&nested_r, temp.path()), Confidence::High);
    }

    /// A `tests` directory with no `DESCRIPTION` above it does not demote.
    #[test]
    fn plain_tests_directory_without_a_package_root_stays_undemoted() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("code.R");
        assert_eq!(demoted_at(&path, temp.path()), Confidence::High);
    }

    /// Demotion moves only confidence, never severity.
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
        let mut diagnostics = vec![coded(&path, "RY030")];
        diagnostics[0].severity = crate::Severity::Error;
        post.post_demotion(&mut diagnostics);
        assert_eq!(diagnostics[0].severity, crate::Severity::Error);
        assert_eq!(diagnostics[0].confidence, Confidence::Medium);
    }

    /// Relative diagnostic paths resolve against the repo root.
    #[test]
    fn relative_paths_demote_through_the_repo_root() {
        let temp = tempfile::tempdir().unwrap();
        let path = package_tree(&temp, "tests");
        let relative = path.strip_prefix(temp.path()).unwrap().to_path_buf();
        assert_eq!(demoted_at(&relative, temp.path()), Confidence::Medium);
    }
}
