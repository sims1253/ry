//! Inputs the CLI and the language server prepare identically for a
//! [`Project`]: the resolved workspace context and adopted typehint records.

use crate::{Diagnostic, Project, Severity};
use ry_core::SourceFile;
use ry_core::declarations::DeclarationRecord;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

/// Install one package group's resolved workspace context.
pub fn set_workspace_context(project: &mut Project, workspace: ry_workspace::WorkspaceContext) {
    project.set_loaded(workspace.attached_packages);
    project.set_bare_loaded(workspace.bare_bindings);
    project.set_external_bindings(workspace.external_bindings);
    project.set_imported_from(workspace.imported_bindings);
    project.set_external_s3_methods(workspace.s3_methods);
    project.set_load_bindings(workspace.load_bindings);
}

/// Typehint records adopted from a file set, plus one RY117 per display
/// path whose contracts were declined.
#[derive(Default)]
pub struct AdoptedRecords {
    pub records: Vec<DeclarationRecord>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Adopt typehint contracts in `config`'s scope. Attachment uses the display
/// path, so a file is adopted only when its native path is known and that
/// display path names exactly one native source; otherwise its contracts are
/// declined and the loss is reported once per display path.
pub fn adopted_records(files: &[Arc<SourceFile>], config: &ry_config::Config) -> AdoptedRecords {
    let Some(scope) = config.annotations.typehint.adopted_scope() else {
        return AdoptedRecords::default();
    };
    let mut display_counts = HashMap::new();
    for file in files {
        *display_counts.entry(file.path.as_str()).or_insert(0usize) += 1;
    }
    let mut result = AdoptedRecords::default();
    let mut ambiguous = BTreeSet::new();
    for file in files {
        let records = crate::typehint::read_records_at(file, file.native_or_display_path(), &scope);
        let unambiguous = display_counts[file.path.as_str()] == 1
            && file.native_path.as_deref().is_some_and(|native| {
                ry_config::unambiguous_native_display_path(native, &file.path)
            });
        if unambiguous {
            result.records.extend(records);
        } else if !records.is_empty() {
            ambiguous.insert(file.path.as_str());
        }
    }
    result.diagnostics = ambiguous
        .into_iter()
        .map(ambiguous_typehint_identity)
        .collect();
    result
}

/// RY117 for a display path whose native source identity is ambiguous.
pub fn ambiguous_typehint_identity(path: &str) -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        ry_core::Span::new(0, 1, 0, 0),
        path,
        "RY117",
        "Native source paths share one display name; typehint attachment is ambiguous and was skipped.",
    )
}
