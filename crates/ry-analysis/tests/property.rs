//! P38-W5 property tests: live vs fresh equivalence after arbitrary change batches.
//!
//! The core invariant: after applying a batch of changes to the host,
//! a snapshot taken from the live host must match, file for file, a
//! snapshot taken from a fresh host fed the same final *contents*. The
//! host exposes no open-file queries, so the fresh host cannot replay
//! open/closed state or document versions — every file is installed as
//! a disk file and only content equivalence is asserted.

use proptest::prelude::*;
use ry_analysis::*;
use std::path::PathBuf;

/// Arbitrary file path generator.
fn arb_path() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("R/a.R".to_string()),
        Just("R/b.R".to_string()),
        Just("R/c.R".to_string()),
        Just("tests/test.R".to_string()),
        Just("NAMESPACE".to_string()),
    ]
}

/// Arbitrary file content generator.
fn arb_content() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("x <- 1\n".to_string()),
        Just("f <- function(x) x\n".to_string()),
        Just("library(dplyr)\n".to_string()),
        Just("y <- f(1)\n".to_string()),
        Just("".to_string()),
    ]
}

/// Arbitrary change.
fn arb_change() -> impl Strategy<Value = Change> {
    prop_oneof![
        (arb_path(), arb_content()).prop_map(|(p, c)| Change::SetDiskFile {
            path: PathBuf::from(p),
            content: c,
        }),
        arb_path().prop_map(|p| Change::RemoveDiskFile {
            path: PathBuf::from(p)
        }),
        (arb_path(), arb_content()).prop_map(|(p, c)| Change::SetOpenFile {
            path: PathBuf::from(p),
            version: DocumentVersion(1),
            content: c,
        }),
        arb_path().prop_map(|p| Change::CloseFile {
            path: PathBuf::from(p)
        }),
    ]
}

proptest! {
    /// Property: a live host after N changes produces the same file set
    /// and content as a fresh host fed the same final contents (as disk
    /// files — open state and versions are not reconstructable through
    /// the host's public API).
    #[test]
    fn live_equals_fresh_file_contents(changes in proptest::collection::vec(arb_change(), 1..20)) {
        // Apply all changes to a live host.
        let mut live = AnalysisHost::new();
        for change in &changes {
            live.apply([change.clone()]);
        }

        // Build a fresh host from the live host's final contents.
        let mut fresh = AnalysisHost::new();
        let live_files: Vec<_> = live.all_files().map(|p| p.to_path_buf()).collect();
        for path in &live_files {
            if let Some(content) = live.file_content(path) {
                fresh.apply([Change::SetDiskFile {
                    path: path.clone(),
                    content: content.to_string(),
                }]);
            }
        }

        // Property: every file in live is in fresh with the same content.
        let live_snap = live.snapshot();
        let fresh_snap = fresh.snapshot();

        prop_assert_eq!(live_snap.file_count(), fresh_snap.file_count(),
            "live and fresh must have same file count");

        for file in live_snap.files() {
            prop_assert_eq!(
                live_snap.file_content(file),
                fresh_snap.file_content(file),
                "file content mismatch for {}", file
            );
        }
    }

    /// Property: snapshot immutability — a snapshot does not change
    /// after further host mutations.
    #[test]
    fn snapshot_is_frozen(changes1 in proptest::collection::vec(arb_change(), 1..10),
                         changes2 in proptest::collection::vec(arb_change(), 1..10)) {
        let mut host = AnalysisHost::new();
        for change in &changes1 {
            host.apply([change.clone()]);
        }
        let snap = host.snapshot();
        let snap_revision = snap.revision();
        let snap_file_count = snap.file_count();

        // Apply more changes.
        for change in &changes2 {
            host.apply([change.clone()]);
        }

        // Snapshot must be unchanged.
        prop_assert_eq!(snap.revision(), snap_revision,
            "snapshot revision must not change");
        prop_assert_eq!(snap.file_count(), snap_file_count,
            "snapshot file count must not change");
        prop_assert!(!snap.is_current(&host),
            "snapshot should be stale after host advances");
    }

    /// Property: revision monotonicity — each apply increments the revision.
    #[test]
    fn revision_monotonically_increases(n in 1u32..50) {
        let mut host = AnalysisHost::new();
        let initial = host.revision();
        for i in 0..n {
            // Index, not the loop bound: reusing the bound wrote the same
            // bytes every iteration, so the sequence never exercised
            // successive different writes to one path.
            host.apply([Change::SetDiskFile {
                path: PathBuf::from("R/a.R"),
                content: format!("v{i}"),
            }]);
        }
        let final_rev = host.revision();
        prop_assert_eq!(final_rev.0, initial.0 + n as u64,
            "revision must increment by exactly the number of apply calls");
    }
}
