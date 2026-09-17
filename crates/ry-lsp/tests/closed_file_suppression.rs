//! #516: inline suppression must hold for files the editor never
//! opened and for files after they are closed, exactly as it does
//! while the file is open.
//!
//! Both closed-file publication paths run through one seam: a debounced
//! `publish_diagnostics` run assembles the project from the background
//! disk index plus the open buffers and post-processes every checked
//! file — disk-indexed ones included — through the shared
//! `ry_checker::post_process` pipeline, whose suppression stage reads
//! the parsed comments of the checked file. The reported symptom did
//! not reproduce against main, v0.10.0, or v0.9.2 when these tests
//! were written; they pin the behavior so a future refactor of the
//! publication path cannot silently drop it.
//!
//! `a.R` carries two RY040 (invalid-arithmetic) triggers: line 1 with
//! the reporter's directive (` # ry: ignore[RY040]`), line 3 without.
//! While suppression holds, exactly one RY040 — the line-3 one, LSP
//! line 2 — survives publication for `a.R`. `other.R` exists only to
//! keep a document open, and that is a requirement, not a convenience:
//! the server publishes closed-file diagnostics only from a publish
//! triggered by an open document (initialize and the watched-file
//! reload both republish open documents only, and nothing publishes a
//! closed file on its own), so without `other.R` the never-opened and
//! post-close publications under test would never happen at all.

mod harness;

use harness::{file_uri, join_session, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

/// Two RY040 triggers; the first carries the inline directive, the
/// second is the unsuppressed control that proves the publication
/// happened and suppression dropped only its target.
const SUPPRESSED_SRC: &str = "x <- \"a\" * 3  # ry: ignore[RY040]\ny <- 2L\nz <- \"b\" * 4\n";

/// A clean second document; opening it drives the project-wide publish.
const OTHER_SRC: &str = "w <- 1L\n";

/// LSP start lines of the RY040 diagnostics in one publish's array,
/// sorted.
fn ry040_lines(diagnostics: &[Value]) -> Vec<u64> {
    let mut lines: Vec<u64> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["code"] == json!("RY040"))
        .filter_map(|diagnostic| {
            diagnostic
                .pointer("/range/start/line")
                .and_then(Value::as_u64)
        })
        .collect();
    lines.sort_unstable();
    lines
}

fn run<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(future)
}

/// A never-opened file's diagnostics arrive via the background disk
/// index: opening `other.R` triggers the project-wide publish — the
/// only trigger, since no publication covers a closed file while no
/// document is open — and the publication for `a.R` must respect the
/// inline directive: the suppressed line dropped, the control line
/// kept.
#[test]
fn never_opened_file_respects_inline_suppression() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", SUPPRESSED_SRC).unwrap();
        fixture.write_file("other.R", OTHER_SRC).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let other_uri = file_uri(&fixture.path("other.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &other_uri).await;

        let mark = session.publication_mark();
        session.open(&other_uri, 1, OTHER_SRC).await.unwrap();
        let publish = session
            .published_diagnostics_after(&a_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            ry040_lines(
                publish["params"]["diagnostics"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
            ),
            vec![2],
            "the background-index publication for never-opened a.R must drop the \
             suppressed line and keep the control line, got: {:?}",
            publish["params"]["diagnostics"].to_string()
        );

        join_session(session, server).await;
    })
}

/// The close-after-open path: while `a.R` is open the directive holds;
/// after `didClose` the file returns through the disk index and the
/// republish (driven by the still-open `other.R`) must keep the
/// directive holding — the diagnostics published for the now-closed
/// file are exactly the control line.
#[test]
fn closed_file_keeps_inline_suppression() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("a.R", SUPPRESSED_SRC).unwrap();
        fixture.write_file("other.R", OTHER_SRC).unwrap();
        let a_uri = file_uri(&fixture.path("a.R"));
        let other_uri = file_uri(&fixture.path("other.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &other_uri).await;

        // Open both; while open, the directive holds.
        let open_mark = session.publication_mark();
        session.open(&a_uri, 1, SUPPRESSED_SRC).await.unwrap();
        session.open(&other_uri, 1, OTHER_SRC).await.unwrap();
        let open_publish = session
            .published_diagnostics_after(&a_uri, open_mark)
            .await
            .unwrap();
        assert_eq!(
            ry040_lines(
                open_publish["params"]["diagnostics"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
            ),
            vec![2],
            "the open buffer's publication must respect the directive, got: {:?}",
            open_publish["params"]["diagnostics"].to_string()
        );

        // Close a.R. didClose first clears its diagnostics, then the
        // republish for the still-open other.R re-publishes a.R from
        // the disk index. quiesce collects publications until the
        // stream goes quiet; the last publication per URI wins in the
        // map, so a.R's entry is the post-close disk-state set — after
        // the clearing notification and any straggler pre-close
        // publication, both of which precede the 180ms debounce. The
        // idle window must exceed that debounce so the republish is
        // actually collected.
        let close_mark = session.publication_mark();
        session
            .notify(
                "textDocument/didClose",
                json!({"textDocument": {"uri": a_uri}}),
            )
            .await
            .unwrap();
        let burst = session
            .quiesce_diagnostics(&a_uri, close_mark, std::time::Duration::from_millis(500))
            .await
            .unwrap();
        let closed = burst
            .get(&a_uri)
            .expect("the republish must cover the closed a.R again");
        assert_eq!(
            ry040_lines(closed),
            vec![2],
            "the post-close disk-index publication for a.R must drop the \
             suppressed line and keep the control line, got: {closed:?}"
        );

        join_session(session, server).await;
    })
}
