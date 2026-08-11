//! Unified analysis host for ry.
//!
//! Plan 38-W4: This crate provides a single deep analysis interface consumed
//! by CLI, LSP, and tests. Its first implementation wraps the current parser,
//! Project, workspace resolver, and typeshed loader.
//!
//! The host owns open-over-disk precedence and per-folder routing. Callers
//! supply changes, not cache mutations. All queries read one immutable revision.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

pub mod snapshot;
pub mod symbols;

pub use snapshot::{AnalysisSnapshot, QueryResult, SnapshotDiagnostic};
pub mod catalog;
pub mod interactive;
pub use catalog::{
    BindingEffect, DefusingKind, Dispatch, Evaluation, FlowEffect, FunctionSemantics,
    InMemoryCatalog, ParameterSpec, PredicateTarget, ReturnRule, SelectKind, SemanticCatalog,
};
pub use interactive::{
    CompletionItem, CompletionKind, HoverInfo, InlayHint, InlayHintKind, SignatureInfo,
};

pub use symbols::{
    DefinitionSite, ReferenceSite, SymbolId, SymbolIndex, SymbolKind, build_index_from_file,
    merge_indices,
};

// == Stable identities ==

/// Identifier for a workspace root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspaceId(pub usize);

/// Identifier for a file, independent of its display path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

/// Monotonically increasing revision number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Revision(pub u64);

/// Client-side document version (from LSP `didOpen`/`didChange`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct DocumentVersion(pub i32);

// == Change operations ==

/// Atomic change operation applied to the analysis host.
#[derive(Debug, Clone)]
pub enum Change {
    /// Add or update a workspace root.
    AddRoot { path: PathBuf },
    /// Remove a workspace root.
    RemoveRoot { path: PathBuf },
    /// Add or update a disk file.
    SetDiskFile { path: PathBuf, content: String },
    /// Remove a disk file.
    RemoveDiskFile { path: PathBuf },
    /// Open or update a buffer overlay (takes precedence over disk).
    SetOpenFile {
        path: PathBuf,
        version: DocumentVersion,
        content: String,
    },
    /// Close a buffer overlay (revert to disk if present).
    CloseFile { path: PathBuf },
    /// Set the analysis configuration.
    SetConfig(Box<ry_config::Config>),
}

// == AnalysisHost ==

/// The analysis host owns all semantic inputs and provides query access.
///
/// Callers apply changes via `apply()` and query results via `snapshot()`.
/// The host owns open-over-disk precedence and per-folder routing.
#[derive(Default)]
pub struct AnalysisHost {
    revision: Revision,
    roots: Vec<PathBuf>,
    open_files: std::collections::HashMap<PathBuf, (DocumentVersion, String)>,
    disk_files: std::collections::HashMap<PathBuf, String>,
    config: ry_config::Config,
}

impl AnalysisHost {
    /// Create a new empty host.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a batch of changes, returning the new revision.
    ///
    /// Either all changes install one coherent revision, or the last valid
    /// revision is retained.
    pub fn apply(&mut self, changes: impl IntoIterator<Item = Change>) -> Revision {
        for change in changes {
            match change {
                Change::AddRoot { path } => {
                    if !self.roots.contains(&path) {
                        self.roots.push(path);
                    }
                }
                Change::RemoveRoot { path } => {
                    self.roots.retain(|r| r != &path);
                }
                Change::SetDiskFile { path, content } => {
                    self.disk_files.insert(path, content);
                }
                Change::RemoveDiskFile { path } => {
                    self.disk_files.remove(&path);
                }
                Change::SetOpenFile {
                    path,
                    version,
                    content,
                } => {
                    self.open_files.insert(path, (version, content));
                }
                Change::CloseFile { path } => {
                    self.open_files.remove(&path);
                }
                Change::SetConfig(config) => {
                    self.config = *config;
                }
            }
        }
        self.revision.0 += 1;
        self.revision
    }

    /// Get the current revision.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Get the effective content for a file path.
    /// Open buffers take precedence over disk files.
    pub fn file_content(&self, path: &Path) -> Option<&str> {
        self.open_files
            .get(path)
            .map(|(_, content)| content.as_str())
            .or_else(|| self.disk_files.get(path).map(|s| s.as_str()))
    }

    /// Get all known file paths (open + disk).
    pub fn all_files(&self) -> impl Iterator<Item = &Path> {
        self.open_files
            .keys()
            .chain(self.disk_files.keys())
            .map(PathBuf::as_path)
    }

    /// Get the workspace roots.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Number of workspace roots.
    pub fn roots_count(&self) -> usize {
        self.roots.len()
    }

    /// Take an immutable snapshot of the current analysis state.
    ///
    /// The snapshot captures all file contents at the current revision.
    /// Diagnostic data must be supplied by the caller (e.g., from a checker
    /// run); future workstreams will integrate the checker into the host.
    pub fn snapshot(&self) -> AnalysisSnapshot {
        AnalysisSnapshot::from_host(self, std::collections::HashMap::new())
    }

    /// Take a snapshot with diagnostic data attached.
    ///
    /// This is the primary entry point for production use: the caller runs
    /// the checker, converts results to `SnapshotDiagnostic`, and attaches
    /// them to the snapshot.
    pub fn snapshot_with_diagnostics(
        &self,
        diagnostics: std::collections::HashMap<String, Vec<SnapshotDiagnostic>>,
    ) -> AnalysisSnapshot {
        AnalysisSnapshot::from_host(self, diagnostics)
    }

    /// Get the current configuration.
    pub fn config(&self) -> &ry_config::Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn apply_increments_revision() {
        let mut host = AnalysisHost::new();
        assert_eq!(host.revision(), Revision(0));
        let rev = host.apply(vec![Change::AddRoot {
            path: PathBuf::from("/tmp"),
        }]);
        assert_eq!(rev, Revision(1));
        assert_eq!(host.revision(), Revision(1));
    }

    #[test]
    fn open_file_shadows_disk() {
        let mut host = AnalysisHost::new();
        host.apply([
            Change::SetDiskFile {
                path: PathBuf::from("a.R"),
                content: "disk <- 1\n".to_string(),
            },
            Change::SetOpenFile {
                path: PathBuf::from("a.R"),
                version: DocumentVersion(1),
                content: "open <- 2\n".to_string(),
            },
        ]);
        assert_eq!(host.file_content(Path::new("a.R")), Some("open <- 2\n"));
    }

    #[test]
    fn close_file_reverts_to_disk() {
        let mut host = AnalysisHost::new();
        host.apply([
            Change::SetDiskFile {
                path: PathBuf::from("a.R"),
                content: "disk <- 1\n".to_string(),
            },
            Change::SetOpenFile {
                path: PathBuf::from("a.R"),
                version: DocumentVersion(1),
                content: "open <- 2\n".to_string(),
            },
        ]);
        host.apply([Change::CloseFile {
            path: PathBuf::from("a.R"),
        }]);
        assert_eq!(host.file_content(Path::new("a.R")), Some("disk <- 1\n"));
    }

    #[test]
    fn batch_apply_is_atomic_revision() {
        let mut host = AnalysisHost::new();
        let changes = vec![
            Change::AddRoot {
                path: PathBuf::from("/r1"),
            },
            Change::SetDiskFile {
                path: PathBuf::from("/r1/a.R"),
                content: "x <- 1\n".to_string(),
            },
            Change::SetConfig(Box::new(ry_config::Config::default())),
        ];
        let rev = host.apply(changes);
        assert_eq!(rev, Revision(1));
        assert_eq!(host.roots().len(), 1);
    }
}
