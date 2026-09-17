//! #490: deterministic cross-file definition shadowing in the LSP.
//!
//! `Project` resolves a top-level name defined in several files by merge
//! order (the later file wins). The LSP used to assemble that order from
//! unsorted HashMaps and to re-append a closed-then-reopened file at the
//! end, so the winning definition depended on the process's hash seed
//! and flipped after close/reopen of an unchanged file. These tests pin
//! the canonical contract: disk entries sorted by path, then open
//! documents sorted by path — the CLI's sorted discovery order, with the
//! editor's open buffers layered last so they shadow same-named
//! definitions from indexed files.
//!
//! `a.R` and `z.R` both define `f`; `use.R` calls `f()`. When `z.R`'s
//! integer `f` wins, `f() + 1L` is clean; when `a.R`'s character `f`
//! wins, `use.R` carries RY040 — the observable that flips.

mod harness;

use harness::{file_uri, join_session, normalize_diagnostics, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

/// `f <- function() "str"` — the character variant of `f`.
const A_CHAR: &str = "f <- function() \"str\"\n";
/// `f <- function() 1L` — the integer variant of `f`.
const F_INT: &str = "f <- function() 1L\n";
/// Calls the shadowed `f` in a shape that only mismatches for the
/// character variant.
const USE: &str = "x <- f() + 1L\n";

fn has_ry040(publish: &Value) -> bool {
    normalize_diagnostics(publish)
        .iter()
        .any(|diagnostic| diagnostic["code"] == json!("RY040"))
}

/// Close and reopen of an unchanged `a.R` must not flip which `f` wins.
#[test]
fn close_reopen_does_not_flip_the_winning_definition() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(close_reopen_shadowing());
}

async fn close_reopen_shadowing() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("z.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let z_uri = file_uri(&fixture.path("z.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;

    // All three open: canonical order is [a.R, use.R, z.R], so z.R wins.
    let mark = session.publication_mark();
    session.open(&a_uri, 1, A_CHAR).await.unwrap();
    session.open(&z_uri, 1, F_INT).await.unwrap();
    session.open(&use_uri, 1, USE).await.unwrap();
    let first = session
        .published_diagnostics_after(&use_uri, mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&first),
        "z.R's integer f must win while all files are open"
    );

    // did_close: a.R returns via the disk index. Canonical order is
    // [a.R (disk), use.R, z.R] — z.R still wins.
    let close_mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": a_uri}}),
        )
        .await
        .unwrap();
    let closed = session
        .published_diagnostics_after(&use_uri, close_mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&closed),
        "closing a.R must not change the winner while z.R defines f"
    );

    // Reopen unchanged: without canonical ordering the re-added a.R
    // lands at the end of the merge order and its character f wins.
    let reopen_mark = session.publication_mark();
    session.open(&a_uri, 2, A_CHAR).await.unwrap();
    let reopened = session
        .published_diagnostics_after(&use_uri, reopen_mark)
        .await
        .unwrap();
    assert!(
        !has_ry040(&reopened),
        "reopening unchanged a.R must not flip the winning f to its character variant"
    );

    join_session(session, server).await;
}

/// An open document's buffer is authoritative: with `a.R` open (edited
/// to the character `f`) and `z.R`'s integer `f` only on disk, the
/// buffer's definition wins — open documents are layered after the
/// sorted disk entries.
#[test]
fn open_buffer_shadows_disk_definitions() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(open_buffer_authority());
}

async fn open_buffer_authority() {
    let fixture = FixtureProject::empty().unwrap();
    // On disk both definitions return integer; only the open buffer of
    // a.R carries the character variant.
    fixture.write_file("a.R", F_INT).unwrap();
    fixture.write_file("z.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let use_uri = file_uri(&fixture.path("use.R"));
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, &use_uri).await;

    let mark = session.publication_mark();
    session.open(&a_uri, 1, A_CHAR).await.unwrap();
    let publish = session
        .published_diagnostics_after(&use_uri, mark)
        .await
        .unwrap();
    assert!(
        has_ry040(&publish),
        "the open buffer's character f must win over z.R's on-disk integer f"
    );

    join_session(session, server).await;
}

/// Cold-session determinism: two fresh sessions that open the same
/// documents in different orders must agree on which `f` wins.
#[test]
fn open_order_does_not_change_the_selected_definition() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(open_order_independence());
}

/// Open the defining documents (in the given order), then `use.R`, and
/// return the normalized diagnostics published for `use.R`.
async fn observe_use_diagnostics(
    fixture: &FixtureProject,
    defining: &[(&str, &str)],
    use_uri: &str,
) -> Vec<Value> {
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    sync_barrier(&mut session, use_uri).await;
    let mark = session.publication_mark();
    for (uri, text) in defining {
        session.open(uri, 1, text).await.unwrap();
    }
    session.open(use_uri, 1, USE).await.unwrap();
    let publish = session
        .published_diagnostics_after(use_uri, mark)
        .await
        .unwrap();
    join_session(session, server).await;
    normalize_diagnostics(&publish)
}

async fn open_order_independence() {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("a.R", A_CHAR).unwrap();
    fixture.write_file("z.R", F_INT).unwrap();
    fixture.write_file("use.R", USE).unwrap();
    let a_uri = file_uri(&fixture.path("a.R"));
    let z_uri = file_uri(&fixture.path("z.R"));
    let use_uri = file_uri(&fixture.path("use.R"));

    let forward =
        observe_use_diagnostics(&fixture, &[(&a_uri, A_CHAR), (&z_uri, F_INT)], &use_uri).await;
    let reverse =
        observe_use_diagnostics(&fixture, &[(&z_uri, F_INT), (&a_uri, A_CHAR)], &use_uri).await;
    assert_eq!(
        forward, reverse,
        "didOpen order must not influence which definition wins"
    );
    assert!(
        !forward.iter().any(|d| d["code"] == json!("RY040")),
        "z.R's integer f must win in canonical order: {forward:?}"
    );
}
