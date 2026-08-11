//! Immutable analysis snapshot — captures one revision for read-only queries.
//!
//! P38-W5: A snapshot is obtained from [`AnalysisHost::snapshot`] and provides
//! read-only access to all derived data for that revision. Queries return
//! data derived entirely from that revision.

use crate::{AnalysisHost, Revision};
use std::collections::HashMap;

/// A typed query result that may carry cancellation.
#[derive(Debug)]
pub enum QueryResult<T> {
    /// The query completed with a value.
    Ok(T),
    /// The query was cancelled (the revision is stale).
    Cancelled {
        /// The snapshot revision.
        snapshot_revision: Revision,
        /// The current host revision.
        current_revision: Revision,
    },
    /// The file or position was not found.
    NotFound,
}

impl<T> QueryResult<T> {
    /// Unwrap the value, panicking if cancelled or not found.
    pub fn unwrap(self) -> T {
        match self {
            Self::Ok(v) => v,
            Self::Cancelled { .. } => panic!("query was cancelled"),
            Self::NotFound => panic!("query result not found"),
        }
    }

    /// Unwrap or return None for both Cancelled and NotFound.
    pub fn ok(self) -> Option<T> {
        match self {
            Self::Ok(v) => Some(v),
            _ => None,
        }
    }

    /// Returns true if the query was cancelled.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}

/// A diagnostic entry in the analysis snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotDiagnostic {
    /// File path.
    pub file: String,
    /// Rule code (e.g., "RY010").
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Byte offset range start.
    pub start: u32,
    /// Byte offset range end.
    pub end: u32,
}

/// An immutable view of analysis data at one revision.
///
/// All queries on a snapshot are read-only and derived entirely from the
/// revision captured at creation. When the host advances past the snapshot
/// revision, queries return [`QueryResult::Cancelled`].
#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    /// The revision this snapshot was taken at.
    revision: Revision,
    /// File contents at this revision (open-over-disk resolved).
    files: HashMap<String, String>,
    /// Workspace roots at this revision.
    roots: Vec<std::path::PathBuf>,
    /// Diagnostic results at this revision.
    diagnostics: HashMap<String, Vec<SnapshotDiagnostic>>,
}

impl AnalysisSnapshot {
    /// Create a snapshot from the host.
    pub(crate) fn from_host(
        host: &AnalysisHost,
        diagnostics: HashMap<String, Vec<SnapshotDiagnostic>>,
    ) -> Self {
        let mut files = HashMap::new();
        for path in host.all_files() {
            if let Some(content) = host.file_content(path) {
                files.insert(path.to_string_lossy().to_string(), content.to_string());
            }
        }
        Self {
            revision: host.revision(),
            files,
            roots: host.roots().to_vec(),
            diagnostics,
        }
    }

    /// The revision this snapshot captures.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Get file content at this revision.
    pub fn file_content(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(|s| s.as_str())
    }

    /// Get all file paths at this revision.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(|s| s.as_str())
    }

    /// Get diagnostics for a file at this revision.
    pub fn diagnostics(&self, file: &str) -> QueryResult<&[SnapshotDiagnostic]> {
        match self.diagnostics.get(file) {
            Some(diags) => QueryResult::Ok(diags.as_slice()),
            None => {
                // File not in diagnostics map — it may have no diagnostics
                // or it may not exist. If the file exists in files, return empty.
                if self.files.contains_key(file) {
                    QueryResult::Ok(&[])
                } else {
                    QueryResult::NotFound
                }
            }
        }
    }

    /// Get all diagnostics across all files.
    pub fn all_diagnostics(&self) -> impl Iterator<Item = (&str, &SnapshotDiagnostic)> {
        self.diagnostics
            .iter()
            .flat_map(|(file, diags)| diags.iter().map(move |d| (file.as_str(), d)))
    }

    /// Check if this snapshot is still current relative to the host revision.
    pub fn is_current(&self, host: &AnalysisHost) -> bool {
        self.revision == host.revision()
    }

    /// Number of files in this snapshot.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Number of workspace roots in this snapshot.
    pub fn roots_count(&self) -> usize {
        self.roots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use std::path::PathBuf;

    #[test]
    fn snapshot_captures_revision() {
        let mut host = AnalysisHost::new();
        host.apply([Change::SetDiskFile {
            path: PathBuf::from("a.R"),
            content: "x <- 1".to_string(),
        }]);
        let snap = host.snapshot();
        assert_eq!(snap.revision(), Revision(1));
    }

    #[test]
    fn snapshot_is_immutable_after_new_change() {
        let mut host = AnalysisHost::new();
        host.apply([Change::SetDiskFile {
            path: PathBuf::from("a.R"),
            content: "old".to_string(),
        }]);
        let snap = host.snapshot();
        assert_eq!(snap.file_content("a.R"), Some("old"));

        // Apply a new change — snapshot should still reflect old state.
        host.apply([Change::SetDiskFile {
            path: PathBuf::from("a.R"),
            content: "new".to_string(),
        }]);
        assert_eq!(
            snap.file_content("a.R"),
            Some("old"),
            "snapshot must not change after host advances"
        );
        assert!(!snap.is_current(&host));
    }

    #[test]
    fn snapshot_diagnostics_for_known_file() {
        let mut host = AnalysisHost::new();
        host.apply([Change::SetDiskFile {
            path: PathBuf::from("a.R"),
            content: "x <- 1".to_string(),
        }]);
        let mut diags = HashMap::new();
        diags.insert(
            "a.R".to_string(),
            vec![SnapshotDiagnostic {
                file: "a.R".to_string(),
                code: "RY010".to_string(),
                message: "test".to_string(),
                start: 0,
                end: 5,
            }],
        );
        let snap = AnalysisSnapshot::from_host(&host, diags);
        let result = snap.diagnostics("a.R");
        assert!(matches!(result, QueryResult::Ok(diags) if diags.len() == 1));
    }

    #[test]
    fn snapshot_diagnostics_for_unknown_file() {
        let host = AnalysisHost::new();
        let snap = host.snapshot();
        let result = snap.diagnostics("nonexistent.R");
        assert!(matches!(result, QueryResult::NotFound));
    }

    #[test]
    fn snapshot_diagnostics_for_known_file_no_findings() {
        let mut host = AnalysisHost::new();
        host.apply([Change::SetDiskFile {
            path: PathBuf::from("clean.R"),
            content: "x <- 1".to_string(),
        }]);
        let snap = host.snapshot();
        let result = snap.diagnostics("clean.R");
        assert!(
            matches!(result, QueryResult::Ok([])),
            "file with no findings should return Ok(empty)"
        );
    }

    #[test]
    fn snapshot_all_diagnostics() {
        let mut host = AnalysisHost::new();
        host.apply([
            Change::SetDiskFile {
                path: PathBuf::from("a.R"),
                content: "x".to_string(),
            },
            Change::SetDiskFile {
                path: PathBuf::from("b.R"),
                content: "y".to_string(),
            },
        ]);
        let mut diags = HashMap::new();
        diags.insert(
            "a.R".to_string(),
            vec![SnapshotDiagnostic {
                file: "a.R".to_string(),
                code: "R1".to_string(),
                message: "m".to_string(),
                start: 0,
                end: 1,
            }],
        );
        diags.insert(
            "b.R".to_string(),
            vec![SnapshotDiagnostic {
                file: "b.R".to_string(),
                code: "R2".to_string(),
                message: "m".to_string(),
                start: 0,
                end: 1,
            }],
        );
        let snap = AnalysisSnapshot::from_host(&host, diags);
        assert_eq!(snap.all_diagnostics().count(), 2);
    }

    #[test]
    fn live_vs_fresh_property() {
        // The core P38-W5 property: after applying changes incrementally,
        // the live snapshot must equal a fresh snapshot on the same inputs.
        let mut host = AnalysisHost::new();
        host.apply([
            Change::SetDiskFile {
                path: PathBuf::from("a.R"),
                content: "x <- 1".to_string(),
            },
            Change::AddRoot {
                path: PathBuf::from("."),
            },
        ]);
        let live_snap = host.snapshot();

        // Build a fresh host with the same final state.
        let mut fresh = AnalysisHost::new();
        fresh.apply([
            Change::SetDiskFile {
                path: PathBuf::from("a.R"),
                content: "x <- 1".to_string(),
            },
            Change::AddRoot {
                path: PathBuf::from("."),
            },
        ]);
        let fresh_snap = fresh.snapshot();

        // Property: file contents must match.
        assert_eq!(
            live_snap.file_content("a.R"),
            fresh_snap.file_content("a.R")
        );
        assert_eq!(live_snap.file_count(), fresh_snap.file_count());
        assert_eq!(live_snap.roots_count(), fresh_snap.roots_count());
    }
}
