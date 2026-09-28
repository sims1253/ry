//! Compiled per-file rule policy shared by CLI, watch, and LSP publication.
//! Checking still sees every file; this policy changes only the diagnostic
//! filter passed to the common post-processing pipeline.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use ry_config::{Config, ScopedPaths};

use crate::{Severity, SeverityFilter};

#[derive(Debug, Clone)]
struct Table {
    paths: ScopedPaths,
    choices: Vec<(&'static str, Option<Severity>)>,
}

#[derive(Debug, Clone, Default)]
pub struct ScopedRulePolicy {
    tables: Vec<Table>,
    protected: Vec<String>,
    global_ignores: Vec<&'static str>,
}

impl ScopedRulePolicy {
    /// `protected` contains explicit CLI or editor rule tokens. Their
    /// already-computed global result takes precedence over path tables.
    /// Config loading validates the globs. A programmatically constructed
    /// config with a bad glob is skipped rather than crashing a checker.
    pub fn new(config: &Config, config_root: Option<&Path>, protected: &[String]) -> Self {
        let mut tables = Vec::new();
        if let Some(root) = config_root {
            for table in &config.rule_overrides {
                let paths = match ScopedPaths::new(root, &table.paths) {
                    Ok(paths) => paths,
                    Err(error) => {
                        tracing::warn!(%error, "skipping invalid rule-override paths");
                        continue;
                    }
                };
                let mut choices = Vec::new();
                // Within one table, ignore wins over error and error over warn.
                // Repeated codes in this vector are harmless: last insertion
                // into the filter wins, so put strongest choice last.
                for (tokens, severity) in [
                    (&table.warn, Some(Severity::Warning)),
                    (&table.error, Some(Severity::Error)),
                    (&table.ignore, None),
                ] {
                    for token in tokens {
                        for code in SeverityFilter::expand(token) {
                            choices.push((code, severity));
                        }
                    }
                }
                tables.push(Table { paths, choices });
            }
        }
        Self {
            tables,
            protected: protected.to_vec(),
            global_ignores: config
                .ignore
                .iter()
                .flat_map(|token| SeverityFilter::expand(token))
                .collect(),
        }
    }

    pub fn filter_for<'a>(
        &self,
        source: &Path,
        base: &'a SeverityFilter,
    ) -> Cow<'a, SeverityFilter> {
        let mut scoped = None;
        for table in &self.tables {
            if table.paths.matches(source) {
                let filter = scoped.get_or_insert_with(|| {
                    let mut filter = base.clone();
                    for token in &self.protected {
                        filter.protect(token);
                    }
                    filter
                });
                for &(code, severity) in &table.choices {
                    if !self.global_ignores.contains(&code) {
                        filter.set_scoped(code, severity);
                    }
                }
            }
        }
        let Some(mut filter) = scoped else {
            return Cow::Borrowed(base);
        };
        for code in &self.global_ignores {
            filter.add_ignore(code);
        }
        Cow::Owned(filter)
    }

    pub fn filter_for_str<'a>(
        &self,
        source: &str,
        base: &'a SeverityFilter,
    ) -> Cow<'a, SeverityFilter> {
        self.filter_for(&PathBuf::from(source), base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_config::RuleOverrideConfig;

    fn table(paths: &[&str], error: &[&str], warn: &[&str], ignore: &[&str]) -> RuleOverrideConfig {
        RuleOverrideConfig {
            paths: paths.iter().map(ToString::to_string).collect(),
            error: error.iter().map(ToString::to_string).collect(),
            warn: warn.iter().map(ToString::to_string).collect(),
            ignore: ignore.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn ordered_scopes_change_only_mentioned_rules() {
        let root = Path::new("/tmp/ry-scoped-policy");
        let config = Config {
            warn: vec!["RY040".into()],
            rule_overrides: vec![
                table(&["R/**"], &["RY040"], &[], &[]),
                table(&["R/scratch/**"], &[], &["RY040"], &[]),
            ],
            ..Config::default()
        };
        let base = crate::filter_from_config(&config);
        let policy = ScopedRulePolicy::new(&config, Some(root), &[]);
        let severity = |path: &str, code: &str| {
            policy
                .filter_for(&root.join(path), &base)
                .effective(code, Severity::Warning)
        };
        assert_eq!(severity("R/main.R", "RY040"), Some(Severity::Error));
        assert_eq!(
            severity("R/scratch/try.R", "RY040"),
            Some(Severity::Warning)
        );
        assert_eq!(severity("scripts/try.R", "RY040"), Some(Severity::Warning));
        assert_eq!(
            severity("R/main.R", "RY010"),
            base.effective("RY010", Severity::Warning)
        );
        assert!(matches!(
            policy.filter_for(&root.join("scripts/try.R"), &base),
            Cow::Borrowed(_)
        ));
        assert_eq!(severity("R/deep/main.R", "RY040"), Some(Severity::Error));
        assert_eq!(severity("R2/main.R", "RY040"), Some(Severity::Warning));
    }

    #[test]
    fn global_ignore_and_explicit_cli_win_while_path_can_enable_selected_off_rule() {
        let root = Path::new("/tmp/ry-scoped-policy");
        let config = Config {
            ignore: vec!["RY040".into()],
            select: Some(Vec::new()),
            rule_overrides: vec![table(&["R/**"], &["RY040", "RY010"], &[], &[])],
            ..Config::default()
        };
        let base = crate::filter_from_config(&config);
        let policy = ScopedRulePolicy::new(&config, Some(root), &[]);
        let in_r = policy.filter_for(&root.join("R/main.R"), &base);
        assert_eq!(in_r.effective("RY040", Severity::Warning), None);
        assert_eq!(
            in_r.effective("RY010", Severity::Warning),
            Some(Severity::Error)
        );
        assert_eq!(base.effective("RY010", Severity::Warning), None);

        let protected = ScopedRulePolicy::new(&config, Some(root), &["RY010".into()]);
        let in_r = protected.filter_for(&root.join("R/main.R"), &base);
        assert_eq!(in_r.effective("RY010", Severity::Warning), None);
    }

    #[test]
    fn scoped_audit_opt_in_and_target_eligibility_share_filter() {
        let root = Path::new("/tmp/ry-scoped-policy");
        let config = Config {
            rule_overrides: vec![
                table(&["R/**"], &[], &["RY113"], &[]),
                table(&["R/quiet/**"], &[], &[], &["RY010"]),
            ],
            ..Config::default()
        };
        let base = crate::filter_from_config(&config);
        let policy = ScopedRulePolicy::new(&config, Some(root), &[]);
        assert_eq!(base.effective("RY113", Severity::Warning), None);
        let active = policy.filter_for(&root.join("R/main.R"), &base);
        assert_eq!(
            active.effective("RY113", Severity::Warning),
            Some(Severity::Warning)
        );
        assert!(active.effective("RY010", Severity::Warning).is_some());
        let quiet = policy.filter_for(&root.join("R/quiet/one.R"), &base);
        assert_eq!(
            quiet.effective("RY113", Severity::Warning),
            Some(Severity::Warning)
        );
        assert_eq!(quiet.effective("RY010", Severity::Warning), None);
    }

    #[test]
    fn scoped_filter_controls_real_unused_ignore_audit_before_publication() {
        let root = Path::new("/tmp/ry-scoped-policy");
        let config = Config {
            rule_overrides: vec![
                table(&["R/**"], &[], &["RY113"], &[]),
                table(&["R/quiet/**"], &[], &[], &["RY034"]),
            ],
            ..Config::default()
        };
        let base = crate::filter_from_config(&config);
        let policy = ScopedRulePolicy::new(&config, Some(root), &[]);
        let src = "1L == 1L # ry: ignore[RY034]\n";
        for (name, audit_expected) in [
            ("R/main.R", true),
            ("R/quiet/one.R", false),
            ("scripts/one.R", false),
        ] {
            let path = root.join(name);
            let path_str = path.to_str().unwrap();
            let file = ry_core::RParser::new()
                .unwrap()
                .parse(path_str, src)
                .unwrap();
            let mut checker = crate::Checker::new(path_str);
            checker.check(&file);
            let filter = policy.filter_for(&path, &base);
            let post = crate::PostProcess {
                filter: &filter,
                baseline: None,
                min_confidence: crate::Confidence::Low,
                repo_root: Some(root),
            };
            let result = post.pre_demotion(
                checker.take_diagnostics(),
                &file.comments,
                src,
                path_str,
                Some(&file),
            );
            assert_eq!(
                result.iter().any(|finding| finding.code == "RY113"),
                audit_expected,
                "{name}: {result:?}"
            );
        }
    }
}
