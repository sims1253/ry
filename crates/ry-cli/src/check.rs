//! Unified diagnostics query — one entry point for the CLI.
//!
//! The caller supplies parsed files and workspace context; the module
//! returns diagnostics.

use std::collections::BTreeMap;
use std::sync::Arc;

/// Input for a unified diagnostics check.
pub struct CheckInput {
    /// Parsed source files as (path, version, SourceFile) tuples.
    pub files: Vec<(String, i32, Arc<ry_core::SourceFile>)>,
    /// User typeshed stubs.
    pub user_stubs: Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
    /// Workspace context (package metadata, bindings).
    pub workspace: Option<ry_workspace::WorkspaceContext>,
}

/// Result of a unified diagnostics check.
pub struct CheckOutput {
    /// Per-file diagnostics: (path, Vec<Diagnostic>).
    pub diagnostics: Vec<(String, Vec<ry_checker::Diagnostic>)>,
}

/// Run a one-shot project check with workspace metadata.
///
/// This is the single entry point for diagnostics computation.
/// ry-cli calls this instead of coordinating Project setters.
pub fn check_project(input: CheckInput) -> CheckOutput {
    let mut project = apply_workspace(
        ry_checker::Project::new(),
        input.workspace.as_ref(),
        &input.user_stubs,
    );

    for (path, _, file) in &input.files {
        project.add_file(path.clone(), (**file).clone());
    }

    let diagnostics = project.check();

    CheckOutput { diagnostics }
}

/// Run the same one-shot check, additionally snapshotting every file's
/// lexical scopes (top level plus each walked function body).
///
/// The pipeline is identical to [`check_project`] -- same workspace
/// metadata, shared fixpoint, one pass -- so captured types match what a
/// `check` run infers. Returns one `(path, records)` entry per file, in
/// input order. Diagnostics are computed as usual but discarded: the
/// dump consumer (`ry dump-types`) treats diagnostics as irrelevant to
/// its exit code and output.
pub fn check_project_with_scope_capture(
    input: CheckInput,
) -> Vec<(String, Vec<ry_checker::ScopeRecord>)> {
    let mut project = apply_workspace(
        ry_checker::Project::new(),
        input.workspace.as_ref(),
        &input.user_stubs,
    );
    for (path, _, file) in &input.files {
        project.add_file(path.clone(), (**file).clone());
    }
    project.enable_scope_capture();
    project.check();
    project.take_scope_records()
}

/// Install the workspace metadata onto a fresh project. Shared by
/// [`check_project`] and [`check_project_with_scope_capture`] so the two
/// entry points can never drift in what environment they model.
fn apply_workspace(
    mut project: ry_checker::Project,
    workspace: Option<&ry_workspace::WorkspaceContext>,
    user_stubs: &Arc<BTreeMap<String, ry_typeshed::Typeshed>>,
) -> ry_checker::Project {
    let empty_workspace = ry_workspace::WorkspaceContext::default();
    let workspace = workspace.unwrap_or(&empty_workspace);
    project.set_loaded(workspace.attached_packages.clone());
    project.set_bare_loaded(workspace.bare_bindings.clone());
    project.set_user_stubs(Arc::clone(user_stubs));
    project.set_external_bindings(workspace.external_bindings.clone());
    project.set_imported_from(workspace.imported_bindings.clone());
    project.set_external_s3_methods(workspace.s3_methods.clone());
    project.set_load_bindings(workspace.load_bindings.clone());
    project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_project_basic() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("test.R", "x <- 1\n").unwrap();
        let input = CheckInput {
            files: vec![("test.R".to_string(), 0, Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: None,
        };
        let output = check_project(input);
        // A clean file should produce no diagnostics.
        let total: usize = output.diagnostics.iter().map(|(_, d)| d.len()).sum();
        assert_eq!(total, 0, "clean file should have no diagnostics");
    }

    #[test]
    fn check_project_finds_undefined_var() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("test.R", "undefined_var\n").unwrap();
        let input = CheckInput {
            files: vec![("test.R".to_string(), 0, Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: None,
        };
        let output = check_project(input);
        let total: usize = output.diagnostics.iter().map(|(_, d)| d.len()).sum();
        assert!(total > 0, "undefined variable should produce diagnostics");
    }

    #[test]
    fn check_project_with_workspace_context() {
        // `is_null(x)` narrows `x` away from NULL only when its defining
        // package is attached, so the trailing `x()` is an RY070
        // (calling a non-function) exactly when rlang is absent. Running
        // the same source with and without the workspace context proves
        // check_project actually feeds attached_packages into the checker
        // instead of ignoring it.
        let src = "x <- NULL\nif (is_null(x)) stop(\"missing\")\nx()\n";
        let run = |workspace: Option<ry_workspace::WorkspaceContext>| -> usize {
            let mut parser = ry_core::RParser::new().unwrap();
            let file = parser.parse("test.R", src).unwrap();
            let output = check_project(CheckInput {
                files: vec![("test.R".to_string(), 0, Arc::new(file))],
                user_stubs: Arc::new(BTreeMap::new()),
                workspace,
            });
            output
                .diagnostics
                .iter()
                .flat_map(|(_, diags)| diags.iter())
                .filter(|d| d.code == "RY070")
                .count()
        };

        let without = run(None);
        assert!(
            without > 0,
            "without rlang the predicate cannot narrow x; RY070 must fire for x()"
        );

        let mut with = ry_workspace::WorkspaceContext::default();
        with.attached_packages.insert("rlang".to_string());
        assert_eq!(
            run(Some(with)),
            0,
            "with rlang attached the predicate narrows x; no RY070 may survive"
        );
    }

    #[test]
    fn check_project_cross_file_resolution() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file_a = parser.parse("a.R", "shared_fn <- function(x) x\n").unwrap();
        let file_b = parser.parse("b.R", "shared_fn(42)\n").unwrap();
        let input = CheckInput {
            files: vec![
                ("a.R".to_string(), 0, Arc::new(file_a)),
                ("b.R".to_string(), 0, Arc::new(file_b)),
            ],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: None,
        };
        let output = check_project(input);
        // shared_fn is defined in a.R and called in b.R — should resolve.
        let b_diags: usize = output
            .diagnostics
            .iter()
            .find(|(p, _)| p == "b.R")
            .map(|(_, d)| d.len())
            .unwrap_or(0);
        assert_eq!(b_diags, 0, "cross-file function call should resolve");
    }

    #[test]
    fn check_project_with_scope_capture_returns_records_per_file() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse("a.R", "f <- function(x = 1L) { y <- x\n y }\n")
            .unwrap();
        let records = check_project_with_scope_capture(CheckInput {
            files: vec![("a.R".to_string(), 0, Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: None,
        });
        assert_eq!(records.len(), 1);
        let (path, file_records) = &records[0];
        assert_eq!(path, "a.R");
        // Exactly the top scope and the one function scope.
        assert_eq!(file_records.len(), 2, "{file_records:?}");
        let function = file_records
            .iter()
            .find(|r| r.kind == ry_checker::ScopeRecordKind::Function)
            .expect("function scope recorded");
        assert_eq!(function.name.as_deref(), Some("f"));
        assert_eq!(function.params.len(), 1);
    }
}
