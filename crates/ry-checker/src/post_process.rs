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
//!      phases; the CLI demotes non-source paths here, the LSP does
//!      not demote (yet, see #492)
//!   4. baseline subtraction
//!   5. min-confidence threshold
//!
//! The pipeline is split at the demotion seam on purpose: the CLI runs
//! [`PostProcess::pre_demotion`] per file, demotes the aggregated
//! result, then finishes with [`PostProcess::post_demotion`], while the
//! LSP — which has no demotion stage today — runs the two phases back
//! to back. This crate is the natural home because it already owns the
//! suppression and severity filters and already depends on ry-config
//! (the reverse dependency would be a cycle).

use std::path::Path;

use ry_config::Baseline;

use crate::diagnostics::{Confidence, Diagnostic, SeverityFilter};

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
    /// `(path, code, message)` key — needs. `comments` and `src` come
    /// from the parsed/checked file; pass empty slices for diagnostics
    /// with no source (they simply skip the suppression step).
    pub fn pre_demotion(
        &self,
        diagnostics: Vec<Diagnostic>,
        comments: &[ry_core::ast::Comment],
        src: &str,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = crate::filter_suppressed_with_comments(diagnostics, comments, src);
        crate::apply_filter_to_diagnostics(&mut diagnostics, self.filter);
        diagnostics
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

    fn run(
        diagnostics: Vec<Diagnostic>,
        comments: &[ry_core::ast::Comment],
        src: &str,
        post: &PostProcess<'_>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = post.pre_demotion(diagnostics, comments, src);
        post.post_demotion(&mut diagnostics);
        diagnostics
    }

    fn scan_comments(src: &str) -> Vec<ry_core::ast::Comment> {
        ry_core::RParser::new()
            .unwrap()
            .parse("test.R", src)
            .unwrap()
            .comments
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
        let kept = run(diagnostics, &scan_comments(src), src, &post);
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
        let kept = run(diagnostics, &[], "", &post);
        // Threshold-first would drop the Medium before subtraction, the
        // High would eat the count, and nothing would survive.
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].confidence, Confidence::High);
    }

    /// The demotion seam: confidence demoted between the two phases
    /// lands before baseline subtraction and the threshold — the CLI's
    /// documented position.
    #[test]
    fn demotion_between_the_phases_happens_before_subtraction_and_threshold() {
        let filter = SeverityFilter::default();
        let base = baseline(1);
        let post = pipeline(&filter, Some(&base), Confidence::Medium);
        let before = vec![diag("a.R", Confidence::High, 0)];
        let mut diagnostics = post.pre_demotion(before, &[], "");
        for diagnostic in &mut diagnostics {
            diagnostic.confidence = diagnostic.confidence.demote();
        }
        post.post_demotion(&mut diagnostics);
        assert!(
            diagnostics.is_empty(),
            "demoted confidence must matter to the threshold, got {diagnostics:?}"
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
        let kept = run(
            vec![diag("a.R", Confidence::Medium, 0)],
            &scan_comments(src),
            src,
            &post,
        );
        assert!(
            kept.is_empty(),
            "file-level suppression must drop everything"
        );

        let mut ignoring = SeverityFilter::default();
        ignoring.add_ignore("RY010");
        let post = pipeline(&ignoring, None, Confidence::Low);
        let kept = run(vec![diag("a.R", Confidence::Medium, 0)], &[], "", &post);
        assert!(kept.is_empty(), "ignored rule must not survive the filter");

        let filter = SeverityFilter::default();
        let base = baseline(1);
        let post = pipeline(&filter, Some(&base), Confidence::Medium);
        let kept = run(vec![diag("a.R", Confidence::Medium, 0)], &[], "", &post);
        assert!(kept.is_empty(), "baseline must absorb the occurrence");

        let filter = SeverityFilter::default();
        let post = pipeline(&filter, None, Confidence::High);
        let kept = run(vec![diag("a.R", Confidence::Medium, 0)], &[], "", &post);
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
            &[],
            "",
            &post,
        );
        assert!(
            kept.is_empty(),
            "absolute path must match the relative entry"
        );
    }
}
