//! Inputs the CLI and the language server prepare identically for a
//! [`Project`]: the resolved workspace context and adopted typehint records,
//! and the file-level diagnostics both report the same way.

use crate::{Diagnostic, Project, Severity};
use ry_core::SourceFile;
use ry_core::declarations::DeclarationRecord;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

/// Install one package group's resolved workspace context.
pub fn set_workspace_context(project: &mut Project, workspace: ry_workspace::WorkspaceContext) {
    project.set_loaded(workspace.attached_packages);
    project.set_bare_loaded(workspace.bare_bindings);
    project.set_external_bindings(workspace.external_bindings);
    project.set_package_facts(workspace.package_facts);
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
/// display path names exactly one native source; otherwise contracts in
/// scope under either path are declined and the loss is reported once per
/// display path.
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
        let native = file.native_path.as_deref().filter(|native| {
            display_counts[file.path.as_str()] == 1
                && ry_config::unambiguous_native_display_path(native, &file.path)
        });
        if let Some(native) = native {
            result
                .records
                .extend(crate::typehint::read_records_at(file, native, &scope));
        } else if [file.native_or_display_path(), Path::new(&file.path)]
            .into_iter()
            .any(|path| !crate::typehint::read_records_at(file, path, &scope).is_empty())
        {
            // A contract in scope under either spelling is lost; say so.
            ambiguous.insert(file.path.as_str());
        }
    }
    result.diagnostics = ambiguous
        .into_iter()
        .map(ambiguous_typehint_identity)
        .collect();
    result
}

/// The file-level RY000 for a file ry could not read or parse at all
/// (#743), at the parser's position in the rejected text when it has one.
/// Renderers convert the byte offsets against that text, as for any finding.
pub fn parse_failure(path: &str, error: &ry_workspace::SourceError) -> Diagnostic {
    let (span, message) = match error {
        ry_workspace::SourceError::Read(error) => (
            ry_core::Span::new(0, 1, 0, 0),
            format!("cannot read file: {error}; file not checked"),
        ),
        ry_workspace::SourceError::Parse {
            error: ry_core::ParseError::TreeSitter { line, col, message },
            text,
        } => {
            let (line, col) = (line.saturating_sub(1), col.saturating_sub(1));
            let line_start: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
            let mut start = (line_start + col).min(text.len());
            while !text.is_char_boundary(start) {
                start -= 1;
            }
            (
                ry_core::Span::new(start, start, line, col),
                format!("{message}; file not checked"),
            )
        }
    };
    Diagnostic::new(Severity::Error, span, path, "RY000", message)
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
