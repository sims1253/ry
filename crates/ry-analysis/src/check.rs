//! Unified diagnostics query — one entry point for CLI and LSP.
//!
//! P38-W8: This module encapsulates the Project coordination that was
//! previously duplicated between ry-cli and ry-lsp. Both callers supply
//! parsed files and workspace context; the module returns diagnostics.

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
/// Both CLI and LSP call this instead of coordinating Project setters.
pub fn check_project(input: CheckInput) -> CheckOutput {
    let mut project = ry_checker::Project::new();

    // Apply workspace metadata.
    let empty_workspace = ry_workspace::WorkspaceContext::default();
    let workspace = input.workspace.as_ref().unwrap_or(&empty_workspace);
    project.set_loaded(workspace.attached_packages.clone());
    project.set_bare_loaded(workspace.bare_bindings.clone());
    project.set_user_stubs(Arc::clone(&input.user_stubs));
    project.set_external_bindings(workspace.external_bindings.clone());
    project.set_imported_from(workspace.imported_bindings.clone());
    project.set_external_s3_methods(workspace.s3_methods.clone());
    project.set_load_bindings(workspace.load_bindings.clone());

    // Add all files.
    for (path, _, file) in &input.files {
        project.add_file(path.clone(), (**file).clone());
    }

    // Run the checker.
    let diagnostics = project.check();

    CheckOutput { diagnostics }
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
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("test.R", "dplyr_function(1)\n").unwrap();
        let mut workspace = ry_workspace::WorkspaceContext::default();
        workspace.attached_packages.insert("dplyr".to_string());
        let input = CheckInput {
            files: vec![("test.R".to_string(), 0, Arc::new(file))],
            user_stubs: Arc::new(BTreeMap::new()),
            workspace: Some(workspace),
        };
        let output = check_project(input);
        // Should complete without error even with workspace context.
        assert!(!output.diagnostics.is_empty());
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
}
